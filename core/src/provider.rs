use crate::{storage::Settings, vault};
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, Response, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Clone, Copy, PartialEq)]
enum Protocol {
    Chat,
    Anthropic,
    Responses,
}

pub struct Provider {
    pub client: Client,
    base: String,
    endpoint: String,
    protocol: Protocol,
    key: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub input: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}

impl Usage {
    pub fn add(&mut self, next: &Self) {
        self.input += next.input;
        self.output += next.output;
        self.cached += next.cached;
    }
}

#[derive(Default)]
pub struct Completion {
    pub text: String,
    pub calls: BTreeMap<u32, Call>,
    pub usage: Usage,
}

impl Completion {
    fn call(&mut self, index: u32) -> &mut Call {
        self.calls.entry(index).or_insert_with(|| Call {
            id: String::new(),
            name: String::new(),
            input: String::new(),
        })
    }

    pub fn message(&self) -> Value {
        let calls = self.calls.values().map(|call| json!({
            "id":call.id,"type":"function","function":{"name":call.name,"arguments":call.input}
        })).collect::<Vec<_>>();
        json!({"role":"assistant","content":self.text,"tool_calls":calls})
    }
}

pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(180))
        .user_agent("Sillune/0.1")
        .build()?)
}

pub fn trusted_endpoint(input: &str) -> Result<Url> {
    let url = Url::parse(input).context("Adresse HTTP invalide")?;
    let local = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain("localhost")) => true,
        _ => false,
    };
    if !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        bail!("Utilise une adresse HTTPS ou HTTP sur la boucle locale");
    }
    Ok(url)
}

impl Provider {
    pub fn new(settings: &Settings) -> Result<Self> {
        let key = vault::get(&settings.provider)?.unwrap_or_default();
        Self::with_key(settings, key)
    }

    pub fn with_key(settings: &Settings, key: String) -> Result<Self> {
        let model = settings.model.trim().to_owned();
        let (base, protocol) = match settings.provider.as_str() {
            "openrouter" => ("https://openrouter.ai/api/v1", Protocol::Chat),
            "openai" => ("https://api.openai.com/v1", Protocol::Responses),
            "anthropic" => ("https://api.anthropic.com/v1", Protocol::Anthropic),
            "mistral" => ("https://api.mistral.ai/v1", Protocol::Chat),
            "nous" => ("https://inference-api.nousresearch.com/v1", Protocol::Chat),
            "opencode" => {
                if model.starts_with("claude-")
                    || matches!(
                        model.as_str(),
                        "qwen3.8-flash" | "qwen3.6-plus" | "qwen3.5-plus"
                    )
                {
                    (
                        "https://opencode.ai/inference/anthropic/v1",
                        Protocol::Anthropic,
                    )
                } else if model.starts_with("gpt-") || model.starts_with("muse-spark") {
                    (
                        "https://opencode.ai/inference/openai/v1",
                        Protocol::Responses,
                    )
                } else {
                    ("https://opencode.ai/inference/openai/v1", Protocol::Chat)
                }
            }
            "compatible" => (settings.url.trim(), Protocol::Chat),
            _ => bail!("Fournisseur inconnu"),
        };
        let url = trusted_endpoint(base)?;
        if settings.provider != "compatible" && key.is_empty() {
            bail!("Ajoute la clé du fournisseur dans les réglages");
        }
        if model.is_empty() {
            bail!("Choisis un identifiant de modèle");
        }
        let base = url.as_str().trim_end_matches('/').to_owned();
        let suffix = match protocol {
            Protocol::Chat => "/chat/completions",
            Protocol::Anthropic => "/messages",
            Protocol::Responses => "/responses",
        };
        let endpoint = format!("{base}{suffix}");
        Ok(Self {
            client: client()?,
            base,
            endpoint,
            protocol,
            key,
            model,
        })
    }

    fn auth(&self, request: RequestBuilder) -> RequestBuilder {
        match self.protocol {
            Protocol::Anthropic => request
                .header("anthropic-version", "2023-06-01")
                .header("x-api-key", &self.key),
            _ if !self.key.is_empty() => request.bearer_auth(&self.key),
            _ => request,
        }
    }

    pub async fn models(&self) -> Result<Vec<String>> {
        let response = self
            .auth(self.client.get(format!("{}/models", self.base)))
            .send()
            .await?;
        let json = checked(response).await?.json::<Value>().await?;
        let mut ids = json["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    pub async fn complete(
        &self,
        system: &str,
        messages: &[Value],
        tools: &[Value],
        max: u32,
        cancel: &CancellationToken,
        mut delta: impl FnMut(&str) + Send,
    ) -> Result<Completion> {
        let body = self.body(system, messages, tools, max);
        let response = tokio::select! {
            r = self.auth(self.client.post(&self.endpoint)).json(&body).send() => checked(r?).await?,
            _ = cancel.cancelled() => bail!("Arrêté"),
        };
        let is_sse = response
            .headers()
            .get("content-type")
            .and_then(|s| s.to_str().ok())
            .is_some_and(|s| s.contains("text/event-stream"));
        let mut result = Completion::default();
        if !is_sse {
            let value = response.json::<Value>().await?;
            self.parse_non_stream(&value, &mut result)?;
            delta(&result.text);
            return Ok(result);
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(bytes) = tokio::select! {
            b = stream.next() => b,
            _ = cancel.cancelled() => bail!("Arrêté"),
        } {
            buffer.extend_from_slice(&bytes?);
            if buffer.len() > 2 * 1024 * 1024 {
                bail!("Événement du fournisseur trop volumineux");
            }
            let mut start = 0;
            while let Some(end) = buffer[start..]
                .iter()
                .position(|&b| b == b'\n')
                .map(|i| start + i)
            {
                let line = std::str::from_utf8(&buffer[start..end])?.trim_end();
                start = end + 1;
                let Some(payload) = line.strip_prefix("data:").map(str::trim) else {
                    continue;
                };
                if payload == "[DONE]" || payload.is_empty() {
                    continue;
                }
                let value = serde_json::from_str::<Value>(payload)
                    .context("Flux JSON incomplet du fournisseur")?;
                let prefix = result.text.len();
                self.parse_event(&value, &mut result)?;
                if result.text.len() > prefix {
                    delta(&result.text[prefix..]);
                }
            }
            buffer.drain(..start);
        }
        if let Ok(rest) = std::str::from_utf8(&buffer)
            && let Some(payload) = rest.trim().strip_prefix("data:").map(str::trim)
            && !payload.is_empty()
            && payload != "[DONE]"
        {
            let prefix = result.text.len();
            self.parse_event(&serde_json::from_str(payload)?, &mut result)?;
            if result.text.len() > prefix {
                delta(&result.text[prefix..]);
            }
        }
        if result.calls.is_empty() && result.text.trim().is_empty() {
            bail!("Le modèle a renvoyé un résultat vide");
        }
        Ok(result)
    }

    fn body(&self, system: &str, messages: &[Value], tools: &[Value], max: u32) -> Value {
        match self.protocol {
            Protocol::Chat => {
                let all = [
                    vec![json!({"role":"system","content":system})],
                    messages.to_vec(),
                ]
                .concat();
                let mut body =
                    json!({"model":self.model,"messages":all,"stream":true,"max_tokens":max});
                if self.base == "https://openrouter.ai/api/v1" {
                    body["stream_options"] = json!({"include_usage":true});
                }
                if !tools.is_empty() {
                    body["tools"] = json!(tools);
                }
                body
            }
            Protocol::Anthropic => {
                let mut all = Vec::new();
                for message in messages {
                    if message["role"] == "tool" {
                        all.push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":message["tool_call_id"],"content":message["content"]}]}));
                    } else if let Some(calls) = message["tool_calls"].as_array() {
                        let mut blocks = Vec::new();
                        if let Some(text) = message["content"].as_str().filter(|s| !s.is_empty()) {
                            blocks.push(json!({"type":"text","text":text}));
                        }
                        for call in calls {
                            let input = call["function"]["arguments"]
                                .as_str()
                                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                                .unwrap_or(json!({}));
                            blocks.push(json!({"type":"tool_use","id":call["id"],"name":call["function"]["name"],"input":input}));
                        }
                        all.push(json!({"role":"assistant","content":blocks}));
                    } else {
                        all.push(message.clone());
                    }
                }
                let mut body = json!({"model":self.model,"system":system,"messages":all,"stream":true,"max_tokens":max});
                if !tools.is_empty() {
                    body["tools"] = json!(tools.iter().map(|tool| json!({
                        "name":tool["function"]["name"],"description":tool["function"]["description"],"input_schema":tool["function"]["parameters"]
                    })).collect::<Vec<_>>());
                }
                body
            }
            Protocol::Responses => {
                let mut input = Vec::new();
                for message in messages {
                    if message["role"] == "tool" {
                        input.push(json!({"type":"function_call_output","call_id":message["tool_call_id"],"output":message["content"]}));
                    } else {
                        if message["content"].as_str().is_some_and(|s| !s.is_empty()) {
                            input
                                .push(json!({"role":message["role"],"content":message["content"]}));
                        }
                        if let Some(calls) = message["tool_calls"].as_array() {
                            input.extend(calls.iter().map(|call| json!({
                                "type":"function_call","name":call["function"]["name"],"arguments":call["function"]["arguments"],"call_id":call["id"]
                            })));
                        }
                    }
                }
                let mut body = json!({"model":self.model,"input":input,"instructions":system,"stream":true,"max_output_tokens":max,"store":false});
                if !tools.is_empty() {
                    body["tools"] = json!(tools.iter().map(|tool| json!({
                        "type":"function","name":tool["function"]["name"],"description":tool["function"]["description"],
                        "parameters":tool["function"]["parameters"],"strict":false
                    })).collect::<Vec<_>>());
                }
                body
            }
        }
    }

    fn parse_event(&self, event: &Value, result: &mut Completion) -> Result<()> {
        if event["error"].is_object()
            || matches!(event["type"].as_str(), Some("error" | "response.failed"))
        {
            bail!("Le fournisseur a interrompu le flux");
        }
        let string = |v: &Value| v.as_str().unwrap_or_default().to_owned();
        match self.protocol {
            Protocol::Chat => {
                set_usage(&mut result.usage, &event["usage"]);
                let delta = &event["choices"][0]["delta"];
                if let Some(text) = delta["content"].as_str() {
                    result.text.push_str(text);
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for value in calls {
                        let call = result.call(value["index"].as_u64().unwrap_or(0) as u32);
                        if let Some(id) = value["id"].as_str() {
                            call.id = id.into();
                        }
                        call.name.push_str(&string(&value["function"]["name"]));
                        call.input
                            .push_str(&string(&value["function"]["arguments"]));
                    }
                }
            }
            Protocol::Anthropic => {
                set_usage(&mut result.usage, &event["message"]["usage"]);
                set_usage(&mut result.usage, &event["usage"]);
                let index = event["index"].as_u64().unwrap_or(0) as u32;
                match event["type"].as_str() {
                    Some("content_block_start") if event["content_block"]["type"] == "tool_use" => {
                        let block = &event["content_block"];
                        let call = result.call(index);
                        call.id = string(&block["id"]);
                        call.name = string(&block["name"]);
                    }
                    Some("content_block_delta") => {
                        if let Some(text) = event["delta"]["text"].as_str() {
                            result.text.push_str(text);
                        }
                        if let Some(input) = event["delta"]["partial_json"].as_str() {
                            result.call(index).input.push_str(input);
                        }
                    }
                    _ => {}
                }
            }
            Protocol::Responses => {
                let index = event["output_index"].as_u64().unwrap_or(0) as u32;
                match event["type"].as_str() {
                    Some("response.output_text.delta") => {
                        result.text.push_str(&string(&event["delta"]))
                    }
                    Some("response.output_item.added" | "response.output_item.done")
                        if event["item"]["type"] == "function_call" =>
                    {
                        let item = &event["item"];
                        let call = result.call(index);
                        call.id = string(&item["call_id"]);
                        call.name = string(&item["name"]);
                        if let Some(args) = item["arguments"].as_str().filter(|s| !s.is_empty()) {
                            call.input = args.into();
                        }
                    }
                    Some("response.function_call_arguments.delta") => {
                        result.call(index).input.push_str(&string(&event["delta"]))
                    }
                    Some("response.completed" | "response.incomplete") => {
                        set_usage(&mut result.usage, &event["response"]["usage"])
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn parse_non_stream(&self, value: &Value, result: &mut Completion) -> Result<()> {
        set_usage(&mut result.usage, &value["usage"]);
        match self.protocol {
            Protocol::Chat => {
                let message = &value["choices"][0]["message"];
                self.parse_event(&json!({"choices":[{"delta":message}]}), result)?;
                if let Some(calls) = message["tool_calls"].as_array() {
                    result.calls.clear();
                    for (i, call) in calls.iter().enumerate() {
                        self.parse_event(
                            &json!({"choices":[{"delta":{"tool_calls":[{
                                "index":i,"id":call["id"],"function":call["function"]
                            }]}}]}),
                            result,
                        )?;
                    }
                }
            }
            Protocol::Anthropic => {
                for (i, block) in value["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    if let Some(text) = block["text"].as_str() {
                        result.text.push_str(text);
                    }
                    if block["type"] == "tool_use" {
                        self.parse_event(
                            &json!({"type":"content_block_start","index":i,"content_block":block}),
                            result,
                        )?;
                        result.call(i as u32).input = block["input"].to_string();
                    }
                }
            }
            Protocol::Responses => {
                for (i, item) in value["output"].as_array().into_iter().flatten().enumerate() {
                    for block in item["content"].as_array().into_iter().flatten() {
                        if let Some(text) = block["text"].as_str() {
                            result.text.push_str(text);
                        }
                    }
                    self.parse_event(
                        &json!({"type":"response.output_item.done","output_index":i,"item":item}),
                        result,
                    )?;
                }
            }
        }
        if result.calls.is_empty() && result.text.trim().is_empty() {
            bail!("Le modèle a renvoyé un résultat vide");
        }
        Ok(())
    }
}

fn set_usage(usage: &mut Usage, value: &Value) {
    if let Some(input) = value["prompt_tokens"]
        .as_u64()
        .or(value["input_tokens"].as_u64())
    {
        usage.input = input
            + value["cache_read_input_tokens"].as_u64().unwrap_or(0)
            + value["cache_creation_input_tokens"].as_u64().unwrap_or(0);
    }
    if let Some(output) = value["completion_tokens"]
        .as_u64()
        .or(value["output_tokens"].as_u64())
    {
        usage.output = output;
    }
    if let Some(cached) = value["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .or(value["input_tokens_details"]["cached_tokens"].as_u64())
        .or(value["cache_read_input_tokens"].as_u64())
    {
        usage.cached = cached;
    }
}

pub async fn checked(response: Response) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let message = match status.as_u16() {
        401 | 403 => "identifiants ou périmètre à vérifier",
        402 => "crédits insuffisants",
        404 => "adresse ou modèle introuvable",
        429 => "limite du fournisseur atteinte",
        _ => "requête refusée",
    };
    bail!("HTTP {status} · {message}");
}

use crate::{catalog, storage::Settings, vault};
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
    Gemini,
}

pub struct Provider {
    pub client: Client,
    base: String,
    endpoint: String,
    protocol: Protocol,
    key: String,
    pub model: String,
    account: String,
    effort: Option<String>,
    pub tools: Option<bool>,
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
    native: BTreeMap<u32, Value>,
    reasoning_details: Vec<Value>,
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
        json!({"role":"assistant","content":self.text,"tool_calls":calls,
            "native":self.native.values().collect::<Vec<_>>(),"reasoning_details":self.reasoning_details})
    }
}

pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(180))
        .user_agent(concat!("Sillune/", env!("CARGO_PKG_VERSION")))
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
            Protocol::Gemini => unreachable!(),
        };
        let endpoint = format!("{base}{suffix}");
        Ok(Self {
            client: client()?,
            base,
            endpoint,
            protocol,
            key,
            model,
            account: settings.provider.clone(),
            effort: None,
            tools: None,
        })
    }

    pub async fn resolve(settings: &Settings, level: Option<u8>) -> Result<Self> {
        let mut provider = Self::new(settings)?;
        let found = if settings.provider == "opencode"
            || level.is_some()
            || settings.provider == "openrouter"
        {
            match catalog::list(settings, false).await {
                Ok(models) => models.into_iter().find(|m| m.id == settings.model),
                Err(error) if settings.provider == "opencode" => return Err(error),
                Err(_) => None,
            }
        } else {
            None
        };
        if let Some(model) = found.as_ref() {
            provider.tools = model.tools;
            if let Some(route) = &model.route {
                provider.model = route.id.clone();
                provider.base = route.base.clone();
                provider.protocol = match route.protocol.as_str() {
                    "messages" => Protocol::Anthropic,
                    "responses" => Protocol::Responses,
                    "generate-content" => Protocol::Gemini,
                    _ => Protocol::Chat,
                };
                let suffix = match provider.protocol {
                    Protocol::Anthropic => "/messages".into(),
                    Protocol::Responses => "/responses".into(),
                    Protocol::Gemini => {
                        if !route
                            .id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                        {
                            bail!("Identifiant Gemini invalide");
                        }
                        format!("/models/{}:streamGenerateContent?alt=sse", route.id)
                    }
                    Protocol::Chat => "/chat/completions".into(),
                };
                provider.endpoint = format!("{}{suffix}", provider.base);
            }
        }
        if settings.provider == "opencode" && found.is_none() {
            bail!("Choisis un modèle disponible dans le catalogue OpenCode");
        }
        let efforts = found.map(|m| m.efforts).unwrap_or_else(|| {
            catalog::known_efforts(&settings.provider, &settings.model)
                .iter()
                .map(|x| (*x).into())
                .collect()
        });
        provider.effort = level.and_then(|level| catalog::effective(level, &efforts));
        Ok(provider)
    }

    pub fn effort(&self) -> Option<&str> {
        self.effort.as_deref()
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

    pub async fn complete(
        &self,
        system: &str,
        messages: &[Value],
        tools: &[Value],
        max: u32,
        cancel: &CancellationToken,
        mut delta: impl FnMut(&str) + Send,
    ) -> Result<Completion> {
        let tools = if self.tools == Some(false) {
            &[]
        } else {
            tools
        };
        let body = self.body(system, messages, tools, max);
        let response = tokio::select! {
            r = self.send(body, !tools.is_empty() && !messages.iter().any(|v|v["role"]=="tool"),
                || self.body(system,messages,&[],max)) => r?,
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
            normalize_calls(&mut result);
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
        normalize_calls(&mut result);
        Ok(result)
    }

    async fn send(
        &self,
        body: Value,
        can_retry: bool,
        plain: impl FnOnce() -> Value,
    ) -> Result<Response> {
        let post = |body: &Value| self.auth(self.client.post(&self.endpoint)).json(body);
        let response = post(&body).send().await?;
        if can_retry && matches!(response.status().as_u16(), 400 | 404 | 422) {
            let status = response.status();
            let detail = response
                .text()
                .await
                .unwrap_or_default()
                .to_ascii_lowercase();
            if [
                "does not support tools",
                "does not support tool",
                "doesn't support tool",
                "not support function calling",
                "no endpoints found that support tool",
                "tool use is not supported",
                "tool calling is not supported",
                "tools are not supported",
            ]
            .iter()
            .any(|needle| detail.contains(needle))
            {
                return checked(post(&plain()).send().await?).await;
            }
            bail!("HTTP {status} · requête refusée");
        }
        checked(response).await
    }

    fn body(&self, system: &str, messages: &[Value], tools: &[Value], max: u32) -> Value {
        let mut body = match self.protocol {
            Protocol::Chat => {
                let all = [
                    vec![json!({"role":"system","content":system})],
                    messages
                        .iter()
                        .map(|message| {
                            let mut output = message.clone();
                            if let Some(fields) = output.as_object_mut() {
                                fields.remove("native");
                                if self.account != "openrouter"
                                    || fields
                                        .get("reasoning_details")
                                        .and_then(Value::as_array)
                                        .is_some_and(Vec::is_empty)
                                {
                                    fields.remove("reasoning_details");
                                }
                            }
                            output
                        })
                        .collect(),
                ]
                .concat();
                let mut body =
                    json!({"model":self.model,"messages":all,"stream":true,"max_tokens":max});
                if self.base == "https://openrouter.ai/api/v1" {
                    body["stream_options"] = json!({"include_usage":true});
                }
                if !tools.is_empty() {
                    body["tools"] = json!(tools);
                    if self.account == "openrouter" {
                        body["provider"] = json!({"require_parameters":true});
                    }
                }
                body
            }
            Protocol::Anthropic => {
                let mut all = Vec::new();
                for message in messages {
                    if message["role"] == "tool" {
                        all.push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":message["tool_call_id"],"content":message["content"]}]}));
                    } else if let Some(calls) = message["tool_calls"].as_array() {
                        let mut blocks = message["native"].as_array().cloned().unwrap_or_default();
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
                    } else if let Some(native) =
                        message["native"].as_array().filter(|a| !a.is_empty())
                    {
                        input.extend(native.iter().cloned());
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
                if self.effort.is_some()
                    || self.model.starts_with("gpt-5")
                    || self.model.starts_with('o')
                {
                    body["include"] = json!(["reasoning.encrypted_content"]);
                }
                if !tools.is_empty() {
                    body["tools"] = json!(tools.iter().map(|tool| json!({
                        "type":"function","name":tool["function"]["name"],"description":tool["function"]["description"],
                        "parameters":tool["function"]["parameters"],"strict":false
                    })).collect::<Vec<_>>());
                }
                body
            }
            Protocol::Gemini => {
                let mut contents = Vec::new();
                let mut tool_names = BTreeMap::new();
                for message in messages {
                    if message["role"] == "tool" {
                        let id = message["tool_call_id"].as_str().unwrap_or_default();
                        let name = tool_names.get(id).copied().unwrap_or("tool");
                        contents.push(json!({"role":"user","parts":[{"functionResponse":{"name":name,"response":{"result":message["content"]}}}]}));
                    } else if let Some(native) =
                        message["native"].as_array().filter(|v| !v.is_empty())
                    {
                        for call in message["tool_calls"].as_array().into_iter().flatten() {
                            if let (Some(id), Some(name)) =
                                (call["id"].as_str(), call["function"]["name"].as_str())
                            {
                                tool_names.insert(id, name);
                            }
                        }
                        contents.push(json!({"role":"model","parts":native}));
                    } else {
                        let role = if message["role"] == "assistant" {
                            "model"
                        } else {
                            "user"
                        };
                        contents.push(json!({"role":role,"parts":[{"text":message["content"]}]}));
                    }
                }
                let mut body = json!({"systemInstruction":{"parts":[{"text":system}]},"contents":contents,
                    "generationConfig":{"maxOutputTokens":max}});
                if !tools.is_empty() {
                    body["tools"] = json!([{"functionDeclarations":tools.iter().map(|tool| json!({
                        "name":tool["function"]["name"],"description":tool["function"]["description"],
                        "parameters":tool["function"]["parameters"]
                    })).collect::<Vec<_>>()}]);
                }
                body
            }
        };
        let Some(effort) = self.effort.as_ref().filter(|_| max >= 5000) else {
            return body;
        };
        match self.protocol {
            Protocol::Responses => body["reasoning"] = json!({"effort":effort}),
            Protocol::Anthropic => {
                body["thinking"] = json!({"type":"adaptive","display":"omitted"});
                body["output_config"] = json!({"effort":effort});
            }
            Protocol::Gemini => {
                body["generationConfig"]["thinkingConfig"] =
                    json!({"thinkingLevel":effort.to_uppercase()})
            }
            Protocol::Chat if self.account == "openrouter" => {
                body["reasoning"] = json!({"effort":effort})
            }
            Protocol::Chat => body["reasoning_effort"] = json!(effort),
        }
        if matches!(self.protocol, Protocol::Responses) {
            let reserve = if matches!(effort.as_str(), "max" | "ultra") {
                32_768
            } else {
                16_384
            };
            body["max_output_tokens"] = json!(max.max(reserve));
        } else if matches!(self.protocol, Protocol::Gemini) {
            body["generationConfig"]["maxOutputTokens"] = json!(max.max(16_384));
        } else {
            body["max_tokens"] = json!(max.max(16_384));
        }
        body
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
                } else if let Some(parts) = delta["content"].as_array() {
                    for part in parts.iter().filter(|p| p["type"] == "text") {
                        if let Some(text) = part["text"].as_str() {
                            result.text.push_str(text);
                        }
                    }
                }
                if let Some(items) = delta["reasoning_details"].as_array() {
                    result.reasoning_details.extend(items.iter().cloned());
                }
                if let Some(old) = delta.get("function_call").filter(|v| v.is_object()) {
                    let call = result.call(0);
                    append_function(call, old);
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for (position, value) in calls.iter().enumerate() {
                        let call =
                            result.call(value["index"].as_u64().unwrap_or(position as u64) as u32);
                        if let Some(id) = value["id"].as_str() {
                            call.id = id.into();
                        }
                        append_function(call, &value["function"]);
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
                        if let Some(block) = result.native.get_mut(&index) {
                            for key in ["thinking", "signature"] {
                                if let Some(part) = event["delta"][key].as_str() {
                                    block[key] = json!(format!(
                                        "{}{part}",
                                        block[key].as_str().unwrap_or_default()
                                    ));
                                }
                            }
                        }
                    }
                    Some("content_block_start")
                        if matches!(
                            event["content_block"]["type"].as_str(),
                            Some("thinking" | "redacted_thinking")
                        ) =>
                    {
                        result.native.insert(index, event["content_block"].clone());
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
                    Some("response.function_call_arguments.done") => {
                        if let Some(args) =
                            event["arguments"].as_str().filter(|args| !args.is_empty())
                        {
                            result.call(index).input = args.to_owned();
                        }
                    }
                    Some("response.completed" | "response.incomplete") => {
                        set_usage(&mut result.usage, &event["response"]["usage"])
                    }
                    _ => {}
                }
                if event["type"] == "response.output_item.done" {
                    let item = &event["item"];
                    if matches!(
                        item["type"].as_str(),
                        Some("function_call" | "reasoning" | "message")
                    ) {
                        result.native.insert(index, item.clone());
                    }
                }
            }
            Protocol::Gemini => {
                let meta = &event["usageMetadata"];
                set_usage(
                    &mut result.usage,
                    &json!({"input_tokens":meta["promptTokenCount"],"output_tokens":meta["candidatesTokenCount"]}),
                );
                for part in event["candidates"][0]["content"]["parts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let index = result.native.len() as u32;
                    if let Some(call) = part.get("functionCall") {
                        let info = result.call(index);
                        info.id = call["id"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("call_{index}"));
                        info.name = string(&call["name"]);
                        info.input = call.get("args").cloned().unwrap_or(json!({})).to_string();
                    } else if part["thought"] != true
                        && let Some(content) = part["text"].as_str()
                    {
                        result.text.push_str(content);
                    }
                    result.native.insert(index, part.clone());
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
                    if matches!(
                        block["type"].as_str(),
                        Some("thinking" | "redacted_thinking")
                    ) {
                        result.native.insert(i as u32, block.clone());
                    }
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
            Protocol::Gemini => self.parse_event(value, result)?,
        }
        if result.calls.is_empty() && result.text.trim().is_empty() {
            bail!("Le modèle a renvoyé un résultat vide");
        }
        Ok(())
    }
}

fn append_function(call: &mut Call, function: &Value) {
    if let Some(name) = function["name"].as_str().filter(|name| *name != call.name) {
        call.name.push_str(name);
    }
    if let Some(part) = function["arguments"].as_str() {
        call.input.push_str(part);
    } else if function["arguments"].is_object() {
        call.input = function["arguments"].to_string();
    }
}

fn normalize_calls(result: &mut Completion) {
    result.calls.retain(|_, call| !call.name.is_empty());
    for call in result.calls.values_mut() {
        if call.id.is_empty() {
            call.id = format!("call_{}", uuid::Uuid::new_v4().simple());
        }
        if call.input.is_empty() {
            call.input = "{}".into();
        }
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

#[derive(Debug)]
struct HttpFailure(reqwest::StatusCode);

impl std::error::Error for HttpFailure {}

impl std::fmt::Display for HttpFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self.0.as_u16() {
            401 | 403 => "identifiants ou périmètre à vérifier",
            402 => "crédits insuffisants",
            404 => "adresse ou modèle introuvable",
            429 => "limite du fournisseur atteinte",
            _ => "requête refusée",
        };
        write!(f, "HTTP {} · {message}", self.0)
    }
}

pub fn auth_failed(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<HttpFailure>()
        .is_some_and(|e| e.0 == reqwest::StatusCode::UNAUTHORIZED)
}

pub fn label(account: &str) -> &'static str {
    match account {
        "openrouter" => "OpenRouter",
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "mistral" => "Mistral",
        "nous" => "Nous Portal",
        "opencode" => "OpenCode",
        _ => "L’API compatible",
    }
}

pub async fn checked(response: Response) -> Result<Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(HttpFailure(response.status()).into())
    }
}

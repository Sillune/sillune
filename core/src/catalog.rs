use crate::{
    provider::{checked, client, trusted_endpoint},
    storage::Settings,
    vault,
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
    pub tools: Option<bool>,
    #[serde(skip)]
    pub route: Option<Route>,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub id: String,
    pub base: String,
    pub protocol: String,
}

type Catalogs = HashMap<(String, String), (Instant, Vec<Model>)>;
static CACHE: LazyLock<Mutex<Catalogs>> = LazyLock::new(|| Mutex::new(HashMap::new()));
const EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"];

pub fn clear() {
    CACHE.lock().unwrap().clear();
}

pub async fn list(settings: &Settings, refresh: bool) -> Result<Vec<Model>> {
    let slot = (
        settings.provider.clone(),
        if settings.provider == "compatible" {
            settings.url.clone()
        } else {
            String::new()
        },
    );
    if !refresh
        && let Some((time, models)) = CACHE.lock().unwrap().get(&slot)
        && time.elapsed() < Duration::from_secs(300)
    {
        return Ok(models.clone());
    }
    let key = vault::get(&settings.provider)?.unwrap_or_default();
    let address = match settings.provider.as_str() {
        "openrouter" => "https://openrouter.ai/api/v1/models".into(),
        "openai" => "https://api.openai.com/v1/models".into(),
        "anthropic" => "https://api.anthropic.com/v1/models".into(),
        "mistral" => "https://api.mistral.ai/v1/models".into(),
        "nous" => "https://inference-api.nousresearch.com/v1/models".into(),
        "opencode" => "https://opencode.ai/console/api/v1/models".into(),
        "compatible" => format!(
            "{}/models",
            trusted_endpoint(settings.url.trim())?
                .as_str()
                .trim_end_matches('/')
        ),
        _ => bail!("Fournisseur inconnu"),
    };
    if !matches!(settings.provider.as_str(), "openrouter" | "compatible") && key.is_empty() {
        bail!("Enregistre la clé du fournisseur pour afficher ses modèles");
    }
    let client = client()?;
    let mut cursor = None;
    let mut models = Vec::new();
    for _ in 0..30 {
        let mut request = client.get(&address).timeout(Duration::from_secs(20));
        if settings.provider == "anthropic" {
            request = request
                .header("anthropic-version", "2023-06-01")
                .header("x-api-key", &key)
                .query(&[("limit", 100)]);
            if let Some(cursor) = cursor.as_deref() {
                request = request.query(&[("after_id", cursor)]);
            }
        } else if !key.is_empty() {
            request = request.bearer_auth(&key);
        }
        let payload = checked(request.send().await?)
            .await?
            .json::<Value>()
            .await?;
        if settings.provider == "opencode" {
            models = console_models(&payload);
            break;
        }
        let rows = payload["data"]
            .as_array()
            .or_else(|| payload["models"].as_array())
            .or_else(|| payload.as_array())
            .context("Le serveur a renvoyé un catalogue de modèles inconnu")?;
        models.extend(
            rows.iter()
                .filter_map(|row| standard_model(&settings.provider, row)),
        );
        if settings.provider != "anthropic" || payload["has_more"] != true {
            break;
        }
        let next = payload["last_id"]
            .as_str()
            .context("Page Anthropic incomplète")?
            .to_owned();
        if cursor.as_deref() == Some(next.as_str()) {
            break;
        }
        cursor = Some(next);
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    if models.is_empty() {
        bail!("Le fournisseur ne publie aucun modèle de conversation pour ce compte");
    }
    CACHE
        .lock()
        .unwrap()
        .insert(slot, (Instant::now(), models.clone()));
    Ok(models)
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn standard_model(provider: &str, row: &Value) -> Option<Model> {
    let id = row["id"]
        .as_str()
        .or_else(|| row["name"].as_str())
        .or_else(|| row.as_str())?;
    let outputs = strings(&row["architecture"]["output_modalities"]);
    if (!outputs.is_empty() && !outputs.iter().any(|o| o == "text"))
        || (provider == "openai"
            && [
                "embedding",
                "moderation",
                "whisper",
                "transcribe",
                "realtime",
                "tts",
                "image",
                "audio",
                "sora",
                "dall-e",
            ]
            .iter()
            .any(|kind| id.contains(kind)))
    {
        return None;
    }
    let mut efforts = strings(&row["reasoning"]["supported_efforts"]);
    if row["reasoning"]
        .get("supported_efforts")
        .is_some_and(Value::is_null)
    {
        efforts = ["minimal", "low", "medium", "high", "xhigh"]
            .map(str::to_owned)
            .to_vec();
    }
    if provider == "anthropic" {
        let effort = &row["capabilities"]["effort"];
        efforts = EFFORTS
            .iter()
            .filter(|level| {
                let value = &effort[**level];
                value["supported"] == true || value == true
            })
            .map(|v| (*v).into())
            .collect();
    }
    if efforts.is_empty() {
        efforts = known_efforts(provider, id)
            .iter()
            .map(|v| (*v).to_owned())
            .collect();
    }
    let tools = row["supported_parameters"]
        .as_array()
        .map(|p| p.iter().any(|name| name == "tools"))
        .or_else(|| row["capabilities"]["function_calling"].as_bool())
        .or_else(|| row["capabilities"]["tools"]["supported"].as_bool());
    Some(Model {
        id: id.into(),
        name: row["display_name"]
            .as_str()
            .or_else(|| row["name"].as_str())
            .unwrap_or(id)
            .into(),
        efforts,
        tools,
        route: None,
    })
}

fn console_models(data: &Value) -> Vec<Model> {
    let mut models = Vec::new();
    if let Some(groups) = data.as_object() {
        for (group, provider) in groups {
            let Some(rows) = provider["models"].as_object() else {
                continue;
            };
            for (id, row) in rows {
                let Some(api) = row["api"].as_object() else {
                    continue;
                };
                let Some((protocol, address)) = api.iter().find_map(|(p, v)| {
                    if !matches!(
                        p.as_str(),
                        "chat-completions" | "responses" | "messages" | "generate-content"
                    ) {
                        return None;
                    }
                    let url = trusted_endpoint(v["base_url"].as_str()?).ok()?;
                    (url.host_str() == Some("opencode.ai") && url.path().starts_with("/inference/"))
                        .then_some((p, url))
                }) else {
                    continue;
                };
                let name = row["name"].as_str().unwrap_or(id);
                let label = provider["name"].as_str().unwrap_or(group);
                models.push(Model {
                    id: if group == "opencode" {
                        id.clone()
                    } else {
                        format!("{group}/{id}")
                    },
                    name: if group == "opencode" {
                        name.into()
                    } else {
                        format!("{name} · {label}")
                    },
                    efforts: strings(&row["capabilities"]["reasoning"]["effort"]),
                    tools: row["capabilities"]["tools"]["supported"].as_bool(),
                    route: Some(Route {
                        id: id.clone(),
                        base: address.as_str().trim_end_matches('/').into(),
                        protocol: protocol.clone(),
                    }),
                });
            }
        }
    }
    models
}

pub fn known_efforts(provider: &str, id: &str) -> &'static [&'static str] {
    match provider {
        "openai" if id.starts_with("gpt-6") => &["low", "medium", "high", "xhigh", "max"],
        "openai"
            if id.starts_with("gpt-5.5")
                || id.starts_with("gpt-5.4")
                || id.starts_with("gpt-5.2") =>
        {
            &["low", "medium", "high", "xhigh"]
        }
        "openai"
            if id.starts_with("gpt-5-mini") || id.starts_with("gpt-5-nano") || id == "gpt-5" =>
        {
            &["minimal", "low", "medium", "high"]
        }
        "openai"
            if id.starts_with("gpt-5")
                || id.starts_with("o1")
                || id.starts_with("o3")
                || id.starts_with("o4-mini") =>
        {
            &["low", "medium", "high"]
        }
        "mistral" if id.starts_with("zai-glm-5-3") => &["low", "high", "max"],
        "mistral"
            if [
                "mistral-small-2603",
                "mistral-small-4",
                "mistral-medium-3-5",
                "mistral-large-4",
            ]
            .iter()
            .any(|name| id.starts_with(name)) =>
        {
            &["low", "medium", "high"]
        }
        _ => &[],
    }
}

pub async fn efforts(settings: &Settings) -> Vec<String> {
    if let Ok(models) = list(settings, false).await
        && let Some(model) = models.into_iter().find(|m| m.id == settings.model)
    {
        return model.efforts;
    }
    known_efforts(&settings.provider, &settings.model)
        .iter()
        .map(|level| (*level).into())
        .collect()
}

pub fn effective(level: u8, values: &[String]) -> Option<String> {
    let score = |value: &str| match value {
        "minimal" => Some(0),
        "low" => Some(1),
        "medium" => Some(2),
        "high" => Some(3),
        "xhigh" => Some(4),
        "max" => Some(5),
        "ultra" => Some(6),
        _ => None,
    };
    let wanted = [0, 2, 3, 4, 5, 6].get(level as usize).copied()?;
    values
        .iter()
        .filter_map(|v| score(v).map(|s| (s, v)))
        .min_by_key(|(s, _)| (*s > wanted, (*s as u8).abs_diff(wanted)))
        .map(|(_, v)| v.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_to_documented_efforts() {
        let model = standard_model(
            "openrouter",
            &json!({"id":"a/b","reasoning":{"supported_efforts":["low","high","xhigh"]}}),
        )
        .unwrap();
        assert_eq!(effective(5, &model.efforts).as_deref(), Some("xhigh"));
        assert_eq!(effective(0, &model.efforts).as_deref(), Some("low"));
        assert!(effective(4, &[]).is_none());
    }

    #[test]
    fn preserves_console_route_and_rejects_foreign_host() {
        let rows = console_models(&json!({"opencode-go":{"models":{
            "reasoner":{"api":{"messages":{"base_url":"https://opencode.ai/inference/go/anthropic/v1"}}},
            "bad":{"api":{"responses":{"base_url":"https://example.org/inference/x"}}}
        }}}));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "opencode-go/reasoner");
        assert_eq!(rows[0].route.as_ref().unwrap().protocol, "messages");
    }
}

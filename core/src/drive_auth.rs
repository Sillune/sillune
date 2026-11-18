use crate::{provider, vault};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const RECORD: &str = "drive-oauth";

struct Flow {
    listener: TcpListener,
    redirect: String,
    state: String,
    verifier: String,
    client: String,
    secret: String,
}

#[derive(Serialize, Deserialize)]
struct Session {
    client: String,
    secret: String,
    refresh: String,
    access: String,
    expires: u64,
}

pub async fn authorize(
    client: &str,
    secret: &str,
) -> Result<(String, impl Future<Output = Result<()>> + Send + 'static)> {
    if !client.ends_with(".apps.googleusercontent.com")
        || !client
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
    {
        bail!("Renseigne un ID client OAuth Google pour application de bureau");
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let redirect = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let state = Uuid::new_v4().to_string();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let client = client.to_owned();
    let secret = secret.to_owned();
    let mut target = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    target
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client)
        .append_pair("redirect_uri", &redirect)
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    let flow = Flow {
        listener,
        redirect,
        state,
        verifier,
        client,
        secret,
    };
    Ok((target.to_string(), finish(flow)))
}

pub async fn wait(
    flow: impl Future<Output = Result<()>>,
    cancel: &CancellationToken,
) -> Result<()> {
    tokio::select! {
        result=tokio::time::timeout(Duration::from_secs(240),flow)=>result.context("Délai de connexion Google dépassé")?,
        _=cancel.cancelled()=>bail!("Connexion Google annulée"),
    }
}

async fn finish(flow: Flow) -> Result<()> {
    let code = loop {
        let (mut socket, _) = flow.listener.accept().await?;
        let mut buffer = vec![0u8; 8192];
        let size = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut buffer)).await??;
        let request = String::from_utf8_lossy(&buffer[..size]);
        let path = request
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|line| line.split_whitespace().next());
        let params = path
            .and_then(|path| Url::parse(&format!("http://localhost{path}")).ok())
            .filter(|url| url.path() == "/callback")
            .map(|url| {
                url.query_pairs()
                    .into_owned()
                    .collect::<std::collections::BTreeMap<_, _>>()
            });
        let valid = params
            .as_ref()
            .is_some_and(|p| p.get("state") == Some(&flow.state));
        let message = if valid {
            "Tu peux revenir dans Sillune."
        } else {
            "Requête inconnue."
        };
        let status = if valid { "200 OK" } else { "400 Bad Request" };
        socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{message}",message.len()).as_bytes()).await?;
        if valid {
            break params
                .and_then(|mut p| p.remove("code"))
                .context("Google Drive a refusé le consentement")?;
        }
    };
    let token = exchange(&[
        ("grant_type", "authorization_code"),
        ("client_id", &flow.client),
        ("client_secret", &flow.secret),
        ("redirect_uri", &flow.redirect),
        ("code_verifier", &flow.verifier),
        ("code", &code),
    ])
    .await?;
    let state = Session {
        client: flow.client,
        secret: flow.secret,
        refresh: token["refresh_token"]
            .as_str()
            .context("Jeton de renouvellement Google manquant")?
            .into(),
        access: token["access_token"]
            .as_str()
            .context("Jeton Google manquant")?
            .into(),
        expires: now() + token["expires_in"].as_u64().unwrap_or(1800),
    };
    vault::set(RECORD, &serde_json::to_string(&state)?)?;
    vault::set("drive", "")?;
    Ok(())
}

pub fn connected() -> bool {
    ["drive", RECORD].iter().any(|key| {
        vault::get(key)
            .unwrap_or_default()
            .is_some_and(|v| !v.is_empty())
    })
}

pub async fn token() -> Result<String> {
    if let Some(token) = vault::get("drive")?.filter(|v| !v.is_empty()) {
        return Ok(token);
    }
    let raw = vault::get(RECORD)?.context("Connecte Google Drive dans les réglages")?;
    let mut session = serde_json::from_str::<Session>(&raw)?;
    if session.expires < now() + 90 {
        let value = exchange(&[
            ("grant_type", "refresh_token"),
            ("client_id", &session.client),
            ("client_secret", &session.secret),
            ("refresh_token", &session.refresh),
        ])
        .await?;
        session.access = value["access_token"]
            .as_str()
            .context("Jeton Google absent")?
            .into();
        session.expires = now() + value["expires_in"].as_u64().unwrap_or(1800);
        vault::set(RECORD, &serde_json::to_string(&session)?)?;
    }
    Ok(session.access)
}

pub fn disconnect() -> Result<()> {
    vault::set("drive", "")?;
    vault::set(RECORD, "")
}

async fn exchange(form: &[(&str, &str)]) -> Result<Value> {
    let filtered = form
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .copied()
        .collect::<Vec<_>>();
    let response = provider::client()?
        .post(TOKEN_ENDPOINT)
        .form(&filtered)
        .send()
        .await?;
    Ok(provider::checked(response).await?.json().await?)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

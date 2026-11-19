use crate::{
    provider,
    storage::{McpConfig, Mode, Settings, UndoEntry},
    vault,
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use cap_std::{ambient_authority, fs::Dir};
use futures_util::StreamExt;
use reqwest::{Client, redirect::Policy};
use scraper::{Html, Selector};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{IpAddr, SocketAddr},
    path::{Component, Path},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use url::Url;
use uuid::Uuid;

pub const MAX_FILE: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    NewFile,
    Write,
    External,
}

pub fn allowed(mode: Mode, tool: &str) -> bool {
    let read = matches!(
        tool,
        "list_files"
            | "read_file"
            | "attach_file"
            | "web_search"
            | "open_page"
            | "deliver_file"
            | "recall"
            | "github_read"
            | "drive_read"
            | "plugin_docs"
    );
    read || (matches!(mode, Mode::Agent | Mode::Bot)
        && matches!(
            tool,
            "write_file"
                | "remove_file"
                | "terminal"
                | "github_issue"
                | "drive_upload"
                | "mcp_tools"
                | "mcp_call"
                | "remember"
        ))
        || (mode == Mode::Plugin && tool == "install_plugin")
}

pub fn asks(profile: &str, effect: Effect) -> bool {
    match profile {
        "all" => false,
        "safe" => matches!(effect, Effect::Write | Effect::External),
        _ => true,
    }
}

fn schema(name: &str, description: &str, strings: &[&str], required: &[&str]) -> Value {
    let properties = strings
        .iter()
        .map(|f| (f.to_string(), json!({"type":"string"})))
        .collect::<serde_json::Map<_, _>>();
    json!({"type":"function","function":{"name":name,"description":description,
        "parameters":{"type":"object","properties":properties,"required":required}}})
}

pub fn catalog(
    mode: Mode,
    settings: &Settings,
    request: &str,
    github: bool,
    drive: bool,
) -> Vec<Value> {
    if mode == Mode::Plugin {
        return vec![
            schema(
                "plugin_docs",
                "Lire le README d’un plugin existant par son identifiant.",
                &["id"],
                &["id"],
            ),
            schema(
                "install_plugin",
                "Vérifier, prévisualiser et installer le plugin depuis son manifeste TOML. rust est un source Rust optionnel pour un comportement WebAssembly.",
                &["manifest", "readme", "rust"],
                &["manifest", "readme"],
            ),
        ];
    }
    let mut v = vec![
        schema(
            "web_search",
            "Rechercher des sources sur le web. Les résultats contiennent leur URL.",
            &["query"],
            &["query"],
        ),
        schema(
            "open_page",
            "Lire le texte d’une page web publique et vérifier la source.",
            &["url"],
            &["url"],
        ),
        schema(
            "deliver_file",
            "Livrer un fichier texte ou HTML dans le chat. Donne le contenu complet dans content.",
            &["name", "content"],
            &["name", "content"],
        ),
    ];
    if !settings.workspace.is_empty() {
        v.extend([
            schema("list_files", "Lister les entrées d’un dossier du projet.", &["path"], &[]),
            schema("read_file", "Lire le texte d’un fichier du projet. start et lines désignent les lignes à extraire.", &["path","start","lines"], &["path"]),
            schema("attach_file", "Joindre un fichier existant du projet dans la réponse, par exemple une vidéo ou un audio.", &["path"], &["path"]),
        ]);
    }
    let change = matches!(mode, Mode::Agent | Mode::Bot);
    if change && !settings.workspace.is_empty() {
        v.extend([
            schema("write_file", "Créer ou écrire un fichier du projet. Une sauvegarde permet d’annuler.", &["path","content"], &["path","content"]),
            schema("remove_file", "Supprimer un fichier du projet. Une sauvegarde permet d’annuler.", &["path"], &["path"]),
            schema("terminal", "Exécuter PowerShell pour un travail dans le dossier sélectionné. Les effets de commandes peuvent dépasser les fichiers suivis.", &["command"], &["command"]),
        ]);
    }
    if settings.memory {
        v.push(schema(
            "recall",
            "Retrouver un extrait pertinent d’une conversation antérieure.",
            &["query"],
            &["query"],
        ));
        if change {
            v.push(schema("remember", "Ajouter une note durable que l’utilisateur veut mémoriser. kind vaut preference, fact ou method. id est facultatif pour corriger une note.", &["kind","content","id"], &["kind","content"]));
        }
    }
    let prompt = request.to_lowercase();
    if github && (mode == Mode::Agent || prompt.contains("git") || prompt.contains("issue")) {
        v.push(schema(
            "github_read",
            "Lire un fichier, un dépôt ou une issue GitHub. repo doit être owner/repo.",
            &["repo", "path", "issue"],
            &["repo"],
        ));
        if change {
            v.push(schema(
                "github_issue",
                "Créer une issue GitHub dans owner/repo.",
                &["repo", "title", "body"],
                &["repo", "title"],
            ));
        }
    }
    if drive && (mode == Mode::Agent || prompt.contains("drive")) {
        v.push(schema(
            "drive_read",
            "Lister les fichiers Google Drive autorisés, ou lire le fichier demandé par son id.",
            &["id"],
            &[],
        ));
        if change {
            v.push(schema(
                "drive_upload",
                "Créer un document texte sur Google Drive.",
                &["name", "content"],
                &["name", "content"],
            ));
        }
    }
    if change && !settings.mcp.is_empty() {
        v.extend([
            schema("mcp_tools", "Lister les outils exposés par le serveur MCP nommé.", &["server"], &["server"]),
            schema("mcp_call", "Appeler un outil sur un serveur MCP configuré. arguments contient un objet JSON sérialisé.", &["server","tool","arguments"], &["server","tool","arguments"]),
        ]);
    }
    v
}

pub fn arg<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .with_context(|| format!("Paramètre {key} manquant"))
}

pub fn clipped(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let cutoff = value.floor_char_boundary(max);
    format!("{}\n[… extrait limité]", &value[..cutoff])
}

pub fn media_type(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "glb" => "model/gltf-binary",
        "pdf" => "application/pdf",
        "md" | "txt" | "json" | "csv" | "rs" | "js" | "css" | "svg" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn relative(value: &str, allow_root: bool) -> Result<&Path> {
    let path = Path::new(value);
    if (value.is_empty() && !allow_root)
        || value.contains(':')
        || value.contains('\0')
        || path.components().any(|part| match part {
            Component::Normal(segment) => {
                let name = segment
                    .to_string_lossy()
                    .trim_end_matches([' ', '.'])
                    .to_ascii_lowercase();
                name.is_empty()
                    || name == ".git"
                    || name == ".env"
                    || name.starts_with(".env.")
                    || matches!(
                        name.as_str(),
                        "con" | "nul" | "aux" | "prn" | "com1" | "lpt1"
                    )
            }
            Component::CurDir => !allow_root,
            _ => true,
        })
    {
        bail!("Chemin en dehors du dossier autorisé ou réservé");
    }
    Ok(path)
}

pub struct Workspace(Dir);

impl Workspace {
    pub fn open(root: &str) -> Result<Self> {
        if root.is_empty() || !Path::new(root).is_absolute() {
            bail!("Choisis un dossier de travail dans les réglages");
        }
        Ok(Self(
            Dir::open_ambient_dir(root, ambient_authority())
                .context("Dossier de travail inaccessible")?,
        ))
    }

    pub fn list(&self, path: &str) -> Result<Vec<String>> {
        let path = relative(path, true)?;
        let path = if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        };
        let mut entries = self
            .0
            .read_dir(path)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                relative(&name, true).ok()?;
                let suffix = if e.file_type().ok()?.is_dir() {
                    "/"
                } else {
                    ""
                };
                Some(format!("{name}{suffix}"))
            })
            .take(150)
            .collect::<Vec<_>>();
        entries.sort_unstable();
        Ok(entries)
    }

    pub fn read(&self, path: &str, limit: usize) -> Result<Vec<u8>> {
        let path = relative(path, false)?;
        let file = self.0.open(path)?;
        if !file.metadata()?.is_file() {
            bail!("Ce chemin ne désigne pas un fichier");
        }
        self.read_limited(file, limit)
    }

    fn read_limited(&self, file: cap_std::fs::File, limit: usize) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut data)?;
        if data.len() > limit {
            bail!("Fichier trop volumineux pour cette opération");
        }
        Ok(data)
    }

    pub fn current(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let path = relative(path, false)?;
        match self.0.open(path) {
            Ok(file) if file.metadata()?.is_file() => Ok(Some(self.read_limited(file, MAX_FILE)?)),
            Ok(_) => bail!("Ce chemin ne désigne pas un fichier"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn plan(&self, path: &str, after: Option<&[u8]>) -> Result<UndoEntry> {
        if after.is_some_and(|buf| buf.len() > MAX_FILE) {
            bail!("Écriture trop volumineuse");
        }
        Ok(UndoEntry {
            path: path.to_owned(),
            before: self.current(path)?,
            after: after.map(hash),
        })
    }

    pub fn apply(&self, plan: &UndoEntry, after: Option<&[u8]>) -> Result<()> {
        if self.current(&plan.path)? != plan.before {
            bail!("Le fichier a changé depuis la demande");
        }
        self.replace(&plan.path, after)
    }

    pub fn restore(&self, plan: &UndoEntry) -> Result<bool> {
        if self.current(&plan.path)?.as_deref().map(hash) != plan.after {
            return Ok(false);
        }
        self.replace(&plan.path, plan.before.as_deref())?;
        Ok(true)
    }

    fn replace(&self, path: &str, after: Option<&[u8]>) -> Result<()> {
        let path = relative(path, false)?;
        let Some(data) = after else {
            match self.0.remove_file(path) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.into()),
            }
        };
        let parent = path.parent().context("Chemin invalide")?;
        if !parent.as_os_str().is_empty() {
            self.0.create_dir_all(parent)?;
        }
        let temporary = parent.join(format!(".sillune-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut opts = cap_std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            let mut file = self.0.open_with(&temporary, &opts)?;
            file.write_all(data)?;
            file.sync_all()?;
            drop(file);
            self.0.rename(&temporary, &self.0, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = self.0.remove_file(&temporary);
        }
        result
    }
}

pub fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

pub async fn search(settings: &Settings, query: &str) -> Result<String> {
    let client = provider::client()?;
    let query = clipped(query, 500);
    if settings.search == "searx" {
        let base = provider::trusted_endpoint(&settings.searx_url)?;
        let endpoint = if base.path().trim_end_matches('/').ends_with("/search") {
            base.to_string()
        } else {
            format!("{}/search", base.as_str().trim_end_matches('/'))
        };
        let data = provider::checked(
            client
                .get(endpoint)
                .query(&[("q", query.as_str()), ("format", "json")])
                .send()
                .await?,
        )
        .await?
        .json::<Value>()
        .await?;
        let results = data["results"]
            .as_array()
            .into_iter()
            .flatten()
            .take(6)
            .map(|r| {
                format!(
                    "{}\n{}\n{}",
                    r["title"].as_str().unwrap_or_default(),
                    r["url"].as_str().unwrap_or_default(),
                    clipped(r["content"].as_str().unwrap_or_default(), 300)
                )
            })
            .collect::<Vec<_>>();
        return Ok(if results.is_empty() {
            "Aucun résultat.".into()
        } else {
            results.join("\n\n")
        });
    }
    let key = vault::get("openrouter")?.filter(|key| !key.is_empty());
    if settings.search == "duckduckgo" || key.is_none() {
        return public_search(&client, &query).await;
    }
    let result = async {
        provider::checked(client.post("https://openrouter.ai/api/v1/chat/completions").bearer_auth(key.unwrap()).json(&json!({
        "model":"openrouter/auto", "max_tokens":1200, "plugins":[{"id":"web","max_results":5}],
        "messages":[{"role":"system","content":"Recherche ces termes. Donne les faits et les URL des sources web consultées."},{"role":"user","content":query}]
        })).timeout(Duration::from_secs(42)).send().await?).await?.json::<Value>().await.map_err(anyhow::Error::from)
    }.await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            return public_search(&client, &query)
                .await
                .with_context(|| format!("OpenRouter Web : {error}"));
        }
    };
    let message = &result["choices"][0]["message"];
    let mut text = message["content"].as_str().unwrap_or_default().to_owned();
    let mut count = 0;
    for entry in message["annotations"]
        .as_array()
        .into_iter()
        .flatten()
        .take(6)
    {
        let cite = &entry["url_citation"];
        if let Some(url) = cite["url"]
            .as_str()
            .filter(|url| Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https")))
        {
            count += 1;
            text.push_str(&format!(
                "\n{} · {url}",
                cite["title"].as_str().unwrap_or("Source")
            ));
        }
    }
    if count == 0 {
        return public_search(&client, &query).await;
    }
    Ok(clipped(&text, 5000))
}

async fn public_search(client: &Client, query: &str) -> Result<String> {
    let response = provider::checked(
        client
            .get("https://html.duckduckgo.com/html/")
            .query(&[("q", query)])
            .timeout(Duration::from_secs(18))
            .send()
            .await?,
    )
    .await?;
    let markup = response.text().await?;
    let html = Html::parse_document(&markup);
    let result = Selector::parse(".result__body").unwrap();
    let anchor = Selector::parse(".result__a").unwrap();
    let snippet = Selector::parse(".result__snippet").unwrap();
    let sources = html
        .select(&result)
        .filter_map(|row| {
            let link = row.select(&anchor).next()?;
            let href = link.value().attr("href")?;
            let url = if href.starts_with("//") && href.contains("duckduckgo.com/l/?") {
                Url::parse(&format!("https:{href}"))
                    .ok()?
                    .query_pairs()
                    .find(|(k, _)| k == "uddg")?
                    .1
                    .into_owned()
            } else {
                href.to_owned()
            };
            let parsed = Url::parse(&url).ok()?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return None;
            }
            let title = link.text().collect::<Vec<_>>().join(" ");
            let summary = row
                .select(&snippet)
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            Some(format!("{title}\n{url}\n{}", clipped(&summary, 320)))
        })
        .take(6)
        .collect::<Vec<_>>();
    if sources.is_empty() {
        bail!(
            "DuckDuckGo n’a renvoyé aucune source lisible. Tu peux choisir OpenRouter ou SearXNG dans Réglages → Connecteurs."
        );
    }
    Ok(sources.join("\n\n"))
}

pub fn explicit_search(request: &str) -> bool {
    use std::sync::LazyLock;
    static WEB: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r"(?i)^/web\s+\S|^(?:peux-tu\s+|tu\s+peux\s+)?(?:fais\s+(?:une|des)\s+recherches?\s+sur|recherche\s+(?:des|\d+|deux|trois)\s+sources|look\s+up)\b|\b(?:recherch(?:e|er|es)|cherch(?:e|er|es)|vérifi(?:e|er)|trouv(?:e|er)|regard(?:e|er)|search|browse|look\s+up|consulte)\b[^.!?\n]{0,60}\b(?:web|internet|en\s+ligne|online|sur\s+le\s+net)\b"
    ).unwrap()
    });
    static OPT_OUT: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r"(?i)\b(?:sans\s+(?:faire\s+)?(?:de\s+)?recherche\s+(?:web|en\s+ligne)|ne\s+(?:recherche|cherche|navigue)\s+pas|do\s+not\s+(?:search|browse))\b"
    ).unwrap()
    });
    WEB.is_match(request.trim()) && !OPT_OUT.is_match(request)
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || b == 0 || b == 2))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let segments = ip.segments();
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && segments[0] != 0x2002
        }
    }
}

async fn visible_page_client(url: &Url) -> Result<Client> {
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Seules les pages web publiques HTTP et HTTPS sont prises en charge");
    }
    let host = url
        .host_str()
        .context("Adresse sans hôte")?
        .trim_matches(['[', ']']);
    let port = url.port_or_known_default().context("Port inconnu")?;
    if !matches!(port, 80 | 443) {
        bail!("Ce port web est hors du périmètre de lecture");
    }
    let addresses: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host((host, port)).await?.collect()
    };
    if addresses.is_empty() || addresses.iter().any(|a| !public_ip(a.ip())) {
        bail!("L’adresse ne désigne pas une page publique");
    }
    Ok(Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .resolve_to_addrs(host, &addresses)
        .timeout(Duration::from_secs(20))
        .user_agent("Sillune/0.1")
        .build()?)
}

pub async fn open_page(address: &str) -> Result<String> {
    let mut url = Url::parse(address)?;
    for _ in 0..5 {
        let client = visible_page_client(&url).await?;
        let response = client.get(url.clone()).send().await?;
        if response.status().is_redirection() {
            let path = response
                .headers()
                .get("location")
                .context("Redirection sans adresse")?
                .to_str()?;
            url = url.join(path)?;
            continue;
        }
        let response = provider::checked(response).await?;
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        if !content_type.starts_with("text/") && !content_type.contains("json") {
            bail!("La page ne contient pas de texte lisible");
        }
        let markup = content_type.contains("html");
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk?);
            if bytes.len() > 768 * 1024 {
                break;
            }
        }
        let text = String::from_utf8_lossy(&bytes);
        if !markup {
            return Ok(format!("{url}\n{}", clipped(&text, 6800)));
        }
        let doc = Html::parse_document(&text);
        let selector = Selector::parse("title, h1, h2, h3, article p, main p, p, li").unwrap();
        let cleaned = doc
            .select(&selector)
            .flat_map(|e| e.text())
            .flat_map(|s| s.split_whitespace())
            .take(1800)
            .collect::<Vec<_>>()
            .join(" ");
        return Ok(format!("{url}\n{}", clipped(&cleaned, 6800)));
    }
    bail!("Trop de redirections")
}

pub fn quiet(command: &mut Command) -> &mut Command {
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command
}

pub async fn terminal(root: &str, script: &str) -> Result<String> {
    let mut command = Command::new("powershell.exe");
    quiet(&mut command).current_dir(root).args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]);
    let output = tokio::time::timeout(Duration::from_secs(65), command.output()).await??;
    Ok(format!(
        "Code {}\n{}\n{}",
        output.status,
        clipped(&String::from_utf8_lossy(&output.stdout), 7000),
        clipped(&String::from_utf8_lossy(&output.stderr), 2500)
    ))
}

pub async fn github(name: &str, args: &Value) -> Result<String> {
    let repo = arg(args, "repo")?;
    let parts = repo.split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || p.len() > 100
                || !p
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                || *p == "."
                || *p == ".."
        })
    {
        bail!("Le dépôt doit être owner/repo");
    }
    let key = vault::get("github")?.context("Connecte GitHub dans les réglages")?;
    let base = format!("https://api.github.com/repos/{repo}");
    let client = provider::client()?;
    let request = if name == "github_issue" {
        client.post(format!("{base}/issues")).json(
            &json!({"title":arg(args,"title")?, "body":args["body"].as_str().unwrap_or_default()}),
        )
    } else if let Some(issue) = args["issue"].as_str().filter(|s| !s.is_empty()) {
        client.get(format!("{base}/issues/{}", issue.parse::<u64>()?))
    } else if let Some(path) = args["path"].as_str().filter(|s| !s.is_empty()) {
        let mut url = Url::parse(&format!("{base}/contents/"))?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Chemin GitHub invalide"))?
            .pop_if_empty()
            .extend(path.split('/'));
        client.get(url)
    } else {
        client.get(base)
    };
    let value = provider::checked(
        request
            .bearer_auth(key)
            .header("accept", "application/vnd.github+json")
            .send()
            .await?,
    )
    .await?
    .json::<Value>()
    .await?;
    if let Some(content) = value["content"]
        .as_str()
        .filter(|_| value["encoding"] == "base64")
    {
        return Ok(clipped(
            &String::from_utf8_lossy(&STANDARD.decode(content.replace('\n', ""))?),
            7000,
        ));
    }
    Ok(clipped(&value.to_string(), 6000))
}

pub async fn drive(name: &str, args: &Value) -> Result<String> {
    let key = crate::drive_auth::token().await?;
    let client = provider::client()?;
    let request = if name == "drive_upload" {
        let boundary = format!("sillune-{}", Uuid::new_v4());
        let meta = json!({"name":arg(args,"name")?,"mimeType":"text/plain"});
        let body = format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{}\r\n--{boundary}--\r\n",
            arg(args, "content")?
        );
        client
            .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart")
            .header(
                "content-type",
                format!("multipart/related; boundary={boundary}"),
            )
            .body(body)
    } else if let Some(id) = args["id"].as_str().filter(|s| !s.is_empty()) {
        if !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            bail!("ID de fichier invalide");
        }
        let base = format!("https://www.googleapis.com/drive/v3/files/{id}");
        let meta = provider::checked(
            client
                .get(&base)
                .query(&[("fields", "mimeType")])
                .bearer_auth(&key)
                .send()
                .await?,
        )
        .await?
        .json::<Value>()
        .await?;
        if meta["mimeType"] == "application/vnd.google-apps.document" {
            client
                .get(format!("{base}/export"))
                .query(&[("mimeType", "text/plain")])
        } else {
            client.get(base).query(&[("alt", "media")])
        }
    } else {
        client
            .get("https://www.googleapis.com/drive/v3/files")
            .query(&[
                (
                    "fields",
                    "files(id,name,mimeType,webViewLink),nextPageToken",
                ),
                ("pageSize", "30"),
                ("q", "trashed = false"),
            ])
    };
    let text = provider::checked(request.bearer_auth(key).send().await?)
        .await?
        .text()
        .await?;
    Ok(clipped(&text, 7000))
}

pub async fn mcp(config: &McpConfig, method: &str, params: Value) -> Result<String> {
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"Sillune","version":"0.1.0"}}});
    let ready = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    let call = json!({"jsonrpc":"2.0","id":2,"method":method,"params":params});
    let result = if !config.url.is_empty() {
        let url = provider::trusted_endpoint(&config.url)?;
        let c = provider::client()?;
        let secret = vault::get(&format!("mcp-{}", config.name))?;
        let request = |value: &Value, info: Option<(&str, Option<&str>)>| {
            let mut r = c
                .post(url.clone())
                .header("accept", "application/json, text/event-stream")
                .json(value);
            if let Some(secret) = &secret {
                r = r.bearer_auth(secret);
            }
            if let Some((version, session)) = info {
                r = r.header("MCP-Protocol-Version", version);
                if let Some(session) = session {
                    r = r.header("Mcp-Session-Id", session);
                }
            }
            r
        };
        let response = provider::checked(request(&initialize, None).send().await?).await?;
        let session = response
            .headers()
            .get("mcp-session-id")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        let init = mcp_http_body(response, 1).await?;
        let version = init["result"]["protocolVersion"]
            .as_str()
            .context("Initialisation MCP refusée")?;
        let info = Some((version, session.as_deref()));
        provider::checked(request(&ready, info).send().await?).await?;
        mcp_http_body(
            provider::checked(request(&call, info).send().await?).await?,
            2,
        )
        .await?
    } else {
        if config.command.is_empty() {
            bail!("Commande serveur MCP absente");
        }
        let mut command = Command::new(&config.command);
        quiet(&mut command)
            .args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn()?;
        let mut input = child.stdin.take().context("Entrée MCP absente")?;
        let mut lines = BufReader::new(child.stdout.take().context("Sortie MCP absente")?).lines();
        input
            .write_all(format!("{initialize}\n").as_bytes())
            .await?;
        let init = mcp_stdio_read(&mut lines, 1).await?;
        if init["result"]["protocolVersion"].as_str().is_none() {
            bail!("Initialisation MCP refusée");
        }
        input
            .write_all(format!("{ready}\n{call}\n").as_bytes())
            .await?;
        mcp_stdio_read(&mut lines, 2).await?
    };
    if result.get("error").is_some() {
        bail!("Le serveur MCP a refusé la demande");
    }
    Ok(clipped(&result["result"].to_string(), 8000))
}

async fn mcp_stdio_read<R: tokio::io::AsyncBufRead + Unpin>(
    lines: &mut tokio::io::Lines<R>,
    id: u64,
) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await? {
            if let Ok(value) = serde_json::from_str::<Value>(&line)
                && value["id"].as_u64() == Some(id)
            {
                return Ok(value);
            }
        }
        bail!("Le serveur MCP a fermé le flux")
    })
    .await?
}

async fn mcp_http_body(response: reqwest::Response, id: u64) -> Result<Value> {
    if !response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"))
    {
        return Ok(response.json().await?);
    }
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    while let Some(part) = stream.next().await {
        buffer.extend_from_slice(&part?);
        if buffer.len() > 2 * 1024 * 1024 {
            bail!("Événement MCP trop volumineux");
        }
        while let Some(end) = buffer.iter().position(|&b| b == b'\n') {
            let line = std::str::from_utf8(&buffer[..end])?;
            if let Some(payload) = line.strip_prefix("data:")
                && let Ok(value) = serde_json::from_str::<Value>(payload.trim())
                && value["id"].as_u64() == Some(id)
            {
                return Ok(value);
            }
            buffer.drain(..=end);
        }
    }
    bail!("Réponse MCP absente")
}

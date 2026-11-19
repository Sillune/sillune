use crate::{
    mesh, provider,
    storage::Settings,
    tools::{clipped, quiet},
};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    sync::Mutex,
};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Deserialize)]
struct Asset {
    file: String,
    url: String,
    sha: String,
    size: u64,
}

#[derive(Deserialize)]
struct Pack {
    name: String,
    license: String,
    folder: String,
    binary: String,
    archive: Option<Asset>,
    files: Vec<Asset>,
}

struct Voice {
    child: Child,
    port: u16,
    key: String,
    executable: PathBuf,
    weights: PathBuf,
}

struct ImageServer {
    child: Child,
    port: u16,
    binary: PathBuf,
    weights: Vec<PathBuf>,
    log: tempfile::NamedTempFile,
}

#[derive(Clone)]
pub struct VoiceEndpoint {
    pub base: String,
    pub key: String,
    pub port: u16,
}

pub struct Engines {
    root: PathBuf,
    bundled: PathBuf,
    packs: BTreeMap<String, Pack>,
    voice: Mutex<Option<Voice>>,
    image: Mutex<Option<ImageServer>>,
    render: Mutex<()>,
    download: Mutex<()>,
}

impl Engines {
    pub fn new(profile: &Path) -> Result<Self> {
        let exe = std::env::current_exe()?;
        Ok(Self {
            root: profile.join("runtime"),
            bundled: exe
                .parent()
                .context("Dossier de Sillune inaccessible")?
                .join("Runtime"),
            packs: serde_json::from_str(include_str!("packs.json"))?,
            voice: Mutex::new(None),
            image: Mutex::new(None),
            render: Mutex::new(()),
            download: Mutex::new(()),
        })
    }

    fn roots(&self, settings: &Settings) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if !settings.engines.is_empty() {
            paths.push(PathBuf::from(&settings.engines));
        }
        paths.extend([self.root.clone(), self.bundled.clone()]);
        paths
    }

    fn binary(&self, settings: &Settings, pack: &Pack) -> Option<PathBuf> {
        self.roots(settings)
            .into_iter()
            .map(|r| r.join(&pack.folder).join(&pack.binary))
            .find(|p| p.is_file() && (pack.folder != "shape" || mesh::ready(p)))
    }

    fn model(&self, settings: &Settings, asset: &Asset) -> Option<PathBuf> {
        self.roots(settings)
            .into_iter()
            .map(|r| r.join("Models").join(&asset.file))
            .find(|path| asset_present(path, asset))
    }

    fn model_folder(&self, settings: &Settings, pack: &Pack) -> Option<PathBuf> {
        self.roots(settings)
            .into_iter()
            .map(|root| root.join("Models"))
            .find(|root| {
                pack.files
                    .iter()
                    .all(|asset| asset_present(&root.join(&asset.file), asset))
            })
    }

    fn missing<'a>(&self, settings: &Settings, pack: &'a Pack) -> Vec<&'a Asset> {
        if pack.folder == "shape" {
            if self.model_folder(settings, pack).is_some() {
                return Vec::new();
            }
            let local = self.root.join("Models");
            return pack
                .files
                .iter()
                .filter(|asset| !asset_present(&local.join(&asset.file), asset))
                .collect();
        }
        pack.files
            .iter()
            .filter(|asset| self.model(settings, asset).is_none())
            .collect()
    }

    pub fn status(&self, settings: &Settings) -> Value {
        let rows = self.packs.iter().map(|(id,p)| {
            let missing = self.missing(settings, p).iter().map(|a|a.size).sum::<u64>();
            let binary = self.binary(settings, p).is_some();
            (id.clone(),json!({"name":p.name,"license":p.license,"ready":missing == 0 && binary,
                "missing":missing + if binary { 0 } else { p.archive.as_ref().map_or(0, |a| a.size) },
                "files":p.files.iter().map(|a| json!({"name":a.file,"url":a.url,"sha":a.sha,"size":a.size})).collect::<Vec<_>>()}))
        }).collect::<serde_json::Map<_,_>>();
        Value::Object(rows)
    }

    pub async fn install(
        &self,
        settings: &Settings,
        id: &str,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64, u64) + Send,
    ) -> Result<()> {
        let _guard = self
            .download
            .try_lock()
            .map_err(|_| anyhow::anyhow!("Un pack est déjà en cours de téléchargement"))?;
        let pack = self.packs.get(id).context("Pack inconnu")?;
        fs::create_dir_all(self.root.join("Models"))?;
        let archive_needed = self.binary(settings, pack).is_none();
        let missing = self.missing(settings, pack);
        let total = missing.iter().map(|a| a.size).sum::<u64>()
            + if archive_needed {
                pack.archive.as_ref().map_or(0, |a| a.size)
            } else {
                0
            };
        let mut done = 0u64;
        progress(0, total);
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(1500))
            .build()?;
        if let Some(archive) = pack.archive.as_ref().filter(|_| archive_needed) {
            let target = self.root.join(&archive.file);
            download_asset(&client, archive, &target, cancel, |n| progress(n, total)).await?;
            let dest = self.root.join(&pack.folder);
            let dest_clone = dest.clone();
            tokio::task::spawn_blocking(move || extract(&target, &dest_clone)).await??;
            done += archive.size;
        } else if id == "shape" && archive_needed {
            mesh::install(&self.root.join(&pack.folder), cancel).await?;
        }
        for asset in missing {
            let target = self.root.join("Models").join(&asset.file);
            fs::create_dir_all(target.parent().context("Dossier de modèle manquant")?)?;
            download_asset(&client, asset, &target, cancel, |n| {
                progress(done + n, total)
            })
            .await?;
            done += asset.size;
        }
        progress(total, total);
        Ok(())
    }

    pub async fn image(
        &self,
        settings: &Settings,
        profile: &str,
        prompt: &str,
        seed: i64,
        width: u32,
        height: u32,
        cancel: &CancellationToken,
    ) -> Result<(Vec<u8>, Duration, bool)> {
        if prompt.trim().is_empty() || prompt.len() > 6000 {
            bail!("Décris l’image en moins de 6 000 caractères");
        }
        let steps = match profile {
            "image" => 1,
            "quality" => 4,
            "zimage" => 8,
            _ => bail!("Profil d’image inconnu"),
        };
        let quality = steps > 1;
        let dimensions: &[u32] = if quality {
            &[384, 512, 640, 768, 896, 1024]
        } else {
            &[384, 512, 640, 768]
        };
        if !dimensions.contains(&width) || !dimensions.contains(&height) {
            bail!("Dimensions incompatibles avec ce modèle");
        }
        let pack = self.packs.get(profile).context("Profil d’image inconnu")?;
        let binary = self
            .binary(settings, pack)
            .context("Installe le moteur d’image dans les réglages")?;
        let weights = pack
            .files
            .iter()
            .map(|file| {
                self.model(settings, file)
                    .context("Installe le pack Image local")
            })
            .collect::<Result<Vec<_>>>()?;
        let _render = tokio::select! {
            guard = self.render.lock() => guard,
            _ = cancel.cancelled() => bail!("Génération interrompue"),
        };
        let mut slot = self.image.lock().await;
        if quality {
            self.close_voice().await;
        }
        let client = provider::client()?;
        let start = Instant::now();
        let warm = match slot.as_mut() {
            Some(server) => {
                server.binary == binary
                    && server.weights == weights
                    && server.child.try_wait()?.is_none()
            }
            None => false,
        };
        if !warm {
            slot.take();
            let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
            let log = tempfile::NamedTempFile::new()?;
            let mut command = Command::new(&binary);
            quiet(&mut command)
                .current_dir(binary.parent().context("Dossier du moteur indisponible")?)
                .args([
                    "--verbose",
                    "--listen-ip",
                    "127.0.0.1",
                    "--listen-port",
                    &port.to_string(),
                    "--cfg-scale",
                    "1",
                ])
                .stdout(fs::OpenOptions::new().append(true).open(log.path())?)
                .stderr(fs::OpenOptions::new().append(true).open(log.path())?);
            if quality {
                command
                    .arg("--diffusion-model")
                    .arg(&weights[0])
                    .arg("--llm")
                    .arg(&weights[1])
                    .arg("--vae")
                    .arg(&weights[2])
                    .args(["--steps", &steps.to_string(), "--sampling-method", "euler"]);
                if profile == "zimage" {
                    command.args([
                        "--offload-to-cpu",
                        "--flash-attn",
                        "auto,diffusion=on",
                        "--vae-tiling",
                        "--vae-tile-size",
                        "512",
                        "--vae-tile-overlap",
                        "0.25",
                    ]);
                } else {
                    command.args(["--max-vram", "8.5"]);
                }
            } else {
                command.arg("-m").arg(&weights[0]).args(["--steps", "1"]);
            }
            let mut instance = ImageServer {
                child: command.spawn()?,
                port,
                binary,
                weights,
                log,
            };
            let until = Instant::now() + Duration::from_secs(55);
            loop {
                if let Some(code) = instance.child.try_wait()? {
                    let data = fs::read_to_string(instance.log.path()).unwrap_or_default();
                    bail!(
                        "Le moteur d’image s’est arrêté ({code}). {}",
                        clipped(&data, 800)
                    );
                }
                if cancel.is_cancelled() || Instant::now() > until {
                    bail!("Chargement du modèle interrompu ou trop long");
                }
                if client
                    .get(format!("http://127.0.0.1:{port}/"))
                    .timeout(Duration::from_secs(1))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    *slot = Some(instance);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(210)).await;
            }
        }
        let server = slot.as_ref().context("Moteur d’image indisponible")?;
        let endpoint = format!("http://127.0.0.1:{}/sdapi/v1/txt2img", server.port);
        let work = async {
            let result = provider::checked(
                client
                    .post(endpoint)
                    .json(&json!({
                        "prompt":prompt,"seed":seed,"width":width,"height":height,"steps":steps,
                        "cfg_scale":1,"sampler_name":if quality {"Euler"}else{"Euler a"},
                    }))
                    .send()
                    .await?,
            )
            .await?
            .json::<Value>()
            .await?;
            let encoded = result["images"][0]
                .as_str()
                .context("Le modèle n’a produit aucune image")?;
            Ok::<_, anyhow::Error>(
                STANDARD.decode(encoded.split_once(',').map(|(_, p)| p).unwrap_or(encoded))?,
            )
        };
        let result = tokio::select! {
            value=tokio::time::timeout(Duration::from_secs(240),work)=>value.context("La génération a dépassé son délai").and_then(|v|v),
            _=cancel.cancelled()=>Err(anyhow::anyhow!("Génération interrompue")),
        };
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(e) => {
                slot.take();
                return Err(e);
            }
        };
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            bail!("Image renvoyée invalide");
        }
        let vulkan = fs::read_to_string(slot.as_ref().unwrap().log.path()).is_ok_and(|log| {
            log.lines()
                .any(|line| line.contains("compute buffer") && line.contains("on Vulkan"))
        });
        Ok((bytes, start.elapsed(), vulkan))
    }

    pub async fn mesh(
        &self,
        settings: &Settings,
        prompt: &str,
        seed: u32,
        cancel: &CancellationToken,
        status: impl Fn(&str),
    ) -> Result<mesh::Generated> {
        let pack = self.packs.get("shape").context("Pack 3D indisponible")?;
        let python = self
            .binary(settings, pack)
            .context("Installe Shap-E dans Réglages → Modèles locaux")?;
        let root = self
            .model_folder(settings, pack)
            .context("Les fichiers Shap-E doivent être installés dans le même dossier")?;
        let _render = tokio::select! {
            guard = self.render.lock() => guard,
            _ = cancel.cancelled() => bail!("Génération interrompue"),
        };
        self.image.lock().await.take();
        self.close_voice().await;
        mesh::generate(&python, &root.join("shap-e"), prompt, seed, cancel, status).await
    }

    pub async fn voice_endpoint(
        &self,
        settings: &Settings,
        cancel: &CancellationToken,
    ) -> Result<VoiceEndpoint> {
        let pack = &self.packs["voice"];
        let binary = self
            .binary(settings, pack)
            .context("Installe le moteur vocal LFM3")?;
        let model = self
            .model(settings, &pack.files[0])
            .context("Installe le pack voix LFM3")?;
        let projector = self
            .model(settings, &pack.files[1])
            .context("Composant audio LFM3 absent")?;
        {
            let mut image = self.image.lock().await;
            if image
                .as_ref()
                .is_some_and(|server| server.weights.len() > 1)
            {
                image.take();
            }
        }
        let mut running = self.voice.lock().await;
        if let Some(current) = running.as_mut()
            && current.executable == binary
            && current.weights == model
            && current.child.try_wait()?.is_none()
        {
            return Ok(make_endpoint(current));
        }
        running.take();
        let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
        let key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let mut command = Command::new(&binary);
        let child = quiet(&mut command)
            .current_dir(
                binary
                    .parent()
                    .context("Dossier du moteur vocal inaccessible")?,
            )
            .arg("-m")
            .arg(&model)
            .arg("--mmproj")
            .arg(projector)
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--no-webui",
                "--alias",
                "sillune-voice",
                "--api-key",
                &key,
                "-c",
                "8192",
                "-ngl",
                "99",
                "--tts-lang",
                "fr",
                "-np",
                "1",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let mut voice = Voice {
            child,
            port,
            key,
            executable: binary,
            weights: model,
        };
        let endpoint = make_endpoint(&voice);
        let client = provider::client()?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(status) = voice.child.try_wait()? {
                bail!("Le moteur LFM3 s’est arrêté ({status})");
            }
            if cancel.is_cancelled() || Instant::now() >= deadline {
                bail!("Préparation de LFM3 interrompue ou trop longue");
            }
            if client
                .get(format!("http://127.0.0.1:{port}/health"))
                .bearer_auth(&voice.key)
                .timeout(Duration::from_secs(1))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                *running = Some(voice);
                return Ok(endpoint);
            }
            tokio::time::sleep(Duration::from_millis(225)).await;
        }
    }

    pub async fn close_voice(&self) {
        if let Some(mut voice) = self.voice.lock().await.take() {
            let _ = voice.child.start_kill();
        }
    }

    pub async fn close(&self) {
        if let Some(mut server) = self.image.lock().await.take() {
            let _ = server.child.start_kill();
        }
        self.close_voice().await;
    }

    pub async fn transcribe(
        endpoint: &VoiceEndpoint,
        pcm: &[u8],
        cancel: &CancellationToken,
    ) -> Result<String> {
        if pcm.len() > 16_000 * 2 * 41 || pcm.len() < 1600 || pcm.len() % 2 != 0 {
            bail!("Entrée audio PCM invalide");
        }
        let request =
            format!("ws://127.0.0.1:{}/v1/realtime", endpoint.port).into_client_request()?;
        let mut request = request;
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {}", endpoint.key).parse()?);
        let work = async {
            let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
            socket.send(Message::Text(json!({
                "type":"session.update","session":{"type":"transcription","audio":{"input":{
                    "format":{"type":"audio/pcm","rate":16000},"transcription":{"language":"fr"}
                }}}
            }).to_string().into())).await?;
            for samples in pcm.chunks(16_000) {
                socket.send(Message::Text(json!({"type":"input_audio_buffer.append","audio":STANDARD.encode(samples)}).to_string().into())).await?;
            }
            socket
                .send(Message::Text(
                    json!({"type":"input_audio_buffer.commit"})
                        .to_string()
                        .into(),
                ))
                .await?;
            let mut text = String::new();
            while let Some(message) = socket.next().await {
                let message = message?;
                let Message::Text(raw) = message else {
                    continue;
                };
                let value: Value = serde_json::from_str(&raw)?;
                match value["type"].as_str() {
                    Some("conversation.item.input_audio_transcription.delta") => {
                        text.push_str(value["delta"].as_str().unwrap_or_default())
                    }
                    Some("conversation.item.input_audio_transcription.completed") => {
                        if let Some(transcript) = value["transcript"].as_str() {
                            text = transcript.into();
                        }
                        break;
                    }
                    Some("error") => bail!(
                        "Transcription LFM3 refusée : {}",
                        value["error"]["message"]
                            .as_str()
                            .unwrap_or("erreur locale")
                    ),
                    _ => {}
                }
            }
            let text = text.trim();
            if text.is_empty() {
                bail!("LFM3 n’a pas reconnu de parole");
            }
            Ok(text.to_owned())
        };
        tokio::select! {
            value = tokio::time::timeout(Duration::from_secs(100), work) => value?,
            _=cancel.cancelled() => bail!("Transcription interrompue"),
        }
    }

    pub async fn speak(
        endpoint: &VoiceEndpoint,
        voice: &str,
        input: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>> {
        let client = provider::client()?;
        let request = client.post(format!("{}/audio/speech", endpoint.base)).bearer_auth(&endpoint.key)
            .json(&json!({"model":"sillune-voice","voice":voice,"lang":"fr","input":input.chars().take(700).collect::<String>(),
                "response_format":"wav"}));
        Self::speech_bytes(request, cancel).await
    }

    pub async fn cloud_speak(
        key: &str,
        voice: &str,
        input: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>> {
        let request = provider::client()?.post("https://api.openai.com/v1/audio/speech").bearer_auth(key)
            .json(&json!({"model":"gpt-4o-mini-tts","voice":voice,"input":input.chars().take(3000).collect::<String>(),
                "instructions":"Parle avec une voix naturelle et chaleureuse. Garde la langue du texte et une diction claire.",
                "response_format":"wav"}));
        Self::speech_bytes(request, cancel).await
    }

    async fn speech_bytes(
        request: reqwest::RequestBuilder,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>> {
        let work = async {
            let response = provider::checked(request.send().await?).await?;
            let raw = response.bytes().await?.to_vec();
            if !raw.starts_with(b"RIFF") || raw.len() < 500 {
                bail!("Le moteur vocal a renvoyé un fichier audio invalide");
            }
            Ok(raw)
        };
        tokio::select! {
            result = work => result,
            _=cancel.cancelled() => bail!("Voix interrompue"),
        }
    }
}

fn asset_present(path: &Path, asset: &Asset) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.len() == asset.size)
}

fn make_endpoint(voice: &Voice) -> VoiceEndpoint {
    VoiceEndpoint {
        base: format!("http://127.0.0.1:{}/v1", voice.port),
        key: voice.key.clone(),
        port: voice.port,
    }
}

async fn download_asset(
    client: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    cancel: &CancellationToken,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let temp = target.with_extension("part");
    let mut file = tokio::fs::File::create(&temp).await?;
    let response = tokio::select! {
        value = client.get(&asset.url).send() => provider::checked(value?).await?,
        _ = cancel.cancelled() => bail!("Téléchargement arrêté"),
    };
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut size = 0;
    loop {
        let bytes = tokio::select! {
            part = stream.next() => part,
            _ = cancel.cancelled() => bail!("Téléchargement arrêté"),
        };
        let Some(bytes) = bytes else {
            break;
        };
        let bytes = bytes?;
        size += bytes.len() as u64;
        if size > asset.size {
            bail!("La taille du téléchargement est inattendue");
        }
        hasher.update(&bytes);
        file.write_all(&bytes).await?;
        progress(size);
    }
    if size != asset.size || format!("{:x}", hasher.finalize()) != asset.sha {
        bail!("Vérification SHA-256 du téléchargement échouée");
    }
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(temp, target).await?;
    Ok(())
}

fn extract(archive: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let path = entry.enclosed_name().context("Chemin du pack invalide")?;
        if entry.is_dir() {
            continue;
        }
        let name = path.file_name().context("Entrée archive invalide")?;
        let mut target = fs::File::create(destination.join(name))?;
        std::io::copy(&mut entry.take(200 * 1024 * 1024), &mut target)?;
        target.flush()?;
    }
    Ok(())
}

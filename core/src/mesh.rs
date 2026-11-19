use crate::tools::{clipped, quiet};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};
use tokio_util::sync::CancellationToken;

const CHECK: &str =
    "import torch; from diffusers import ShapEPipeline; from skimage.measure import marching_cubes";
const VERSION: &str = "import sys; assert sys.version_info[:2] == (3, 12) and sys.maxsize > 2**32";
const UV_URL: &str =
    "https://github.com/astral-sh/uv/releases/download/0.12.17/uv-x86_64-pc-windows-msvc.zip";
const UV_SHA: &str = "a252121d5b59398fcb137c6ea448176459a44010f33f67e0072305a637119ca7";
const UV_SIZE: u64 = 17_906_210;

pub fn ready(python: &Path) -> bool {
    python.with_file_name("sillune-shape-v2").is_file()
        || python.with_file_name("sillune-shape-v1").is_file()
}

pub async fn install(root: &Path, cancel: &CancellationToken, status: impl Fn(&str)) -> Result<()> {
    tokio::fs::create_dir_all(root).await?;
    let venv = root.join("venv-v2");
    let python = venv.join("Scripts/python.exe");
    status("Préparation de Python 3.12");
    if run(Command::new(&python).args(["-c", VERSION]), cancel)
        .await
        .is_err()
    {
        prepare_python(root, &venv, cancel, &status).await?;
    }
    let mut check_torch = Command::new(&python);
    check_torch.args(["-c", "import torch"]);
    if run(&mut check_torch, cancel).await.is_err() && !cancel.is_cancelled() {
        status("Installation de PyTorch pour le CPU");
        let mut pip = Command::new(&python);
        pip.args([
            "-m",
            "pip",
            "install",
            "--no-input",
            "--no-cache-dir",
            "--disable-pip-version-check",
            "torch==2.8.0",
            "--index-url",
            "https://download.pytorch.org/whl/cpu",
        ]);
        run(&mut pip, cancel)
            .await
            .map_err(|e| anyhow::anyhow!("Installation de PyTorch · {e:#}"))?;
    }
    status("Installation des dépendances de Shap-E");
    let mut pip = Command::new(&python);
    pip.args([
        "-m",
        "pip",
        "install",
        "--no-input",
        "--no-cache-dir",
        "--disable-pip-version-check",
        "diffusers==0.35.2",
        "transformers==4.55.4",
        "accelerate==1.10.1",
        "scikit-image==0.25.2",
    ]);
    run(&mut pip, cancel)
        .await
        .map_err(|e| anyhow::anyhow!("Dépendances de Shap-E · {e:#}"))?;
    let mut check = Command::new(&python);
    check.args(["-c", CHECK]);
    status("Vérification du moteur Shap-E");
    run(&mut check, cancel)
        .await
        .map_err(|e| anyhow::anyhow!("Vérification du moteur · {e:#}"))?;
    tokio::fs::write(python.with_file_name("sillune-shape-v2"), b"ready").await?;
    Ok(())
}

async fn prepare_python(
    root: &Path,
    venv: &Path,
    cancel: &CancellationToken,
    status: &impl Fn(&str),
) -> Result<()> {
    let paths = [
        std::env::var_os("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("Programs/Python/Python312/python.exe")),
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Python312/python.exe")),
        Some(PathBuf::from("py")),
        Some(PathBuf::from("python")),
    ];
    for python in paths.into_iter().flatten() {
        if cancel.is_cancelled() {
            bail!("Installation arrêtée");
        }
        if python.is_absolute() && !python.is_file() {
            continue;
        }
        let mut check = Command::new(&python);
        let mut build = Command::new(&python);
        if python == Path::new("py") {
            check.arg("-3.12");
            build.arg("-3.12");
        }
        if run(check.args(["-c", VERSION]), cancel).await.is_ok()
            && run(
                build
                    .args(["-m", "venv", "--system-site-packages"])
                    .arg(venv),
                cancel,
            )
            .await
            .is_ok()
        {
            return Ok(());
        }
    }
    status("Téléchargement de Python 3.12 pour Sillune");
    let uv = download_uv(root, cancel).await?;
    let mut command = Command::new(uv);
    command
        .env("UV_PYTHON_INSTALL_DIR", root.join("python"))
        .env("UV_CACHE_DIR", root.join("uv-cache"))
        .env("UV_NO_PROGRESS", "1")
        .args(["venv", "--managed-python", "--python", "3.12", "--seed"])
        .arg(venv);
    run(&mut command, cancel)
        .await
        .map_err(|e| anyhow::anyhow!("Installation de Python 3.12 · {e:#}"))
}

async fn download_uv(root: &Path, cancel: &CancellationToken) -> Result<PathBuf> {
    let path = root.join("uv-0.12.17.exe");
    if path.is_file() {
        return Ok(path);
    }
    let fetch = async {
        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?
            .get(UV_URL)
            .send()
            .await?
            .error_for_status()?;
        if response
            .content_length()
            .is_some_and(|size| size != UV_SIZE)
        {
            bail!("Archive Python de taille inattendue");
        }
        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();
        let mut data = Vec::with_capacity(UV_SIZE as usize);
        while let Some(bytes) = stream.next().await {
            let bytes = bytes?;
            if data.len() + bytes.len() > UV_SIZE as usize {
                bail!("Archive Python de taille inattendue");
            }
            data.extend_from_slice(&bytes);
        }
        if data.len() != UV_SIZE as usize || format!("{:x}", Sha256::digest(&data)) != UV_SHA {
            bail!("Empreinte de l’archive Python invalide");
        }
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data))?;
        let index = (0..zip.len())
            .find(|i| {
                zip.by_index(*i).is_ok_and(|f| {
                    f.enclosed_name()
                        .is_some_and(|name| name.file_name().is_some_and(|n| n == "uv.exe"))
                        && f.size() <= 55 * 1024 * 1024
                })
            })
            .context("Programme uv absent de l’archive")?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        std::io::copy(&mut zip.by_index(index)?, &mut file)?;
        file.persist(&path)?;
        Ok(path)
    };
    tokio::select! {
        result = fetch => result,
        _ = cancel.cancelled() => bail!("Installation arrêtée"),
    }
}

async fn run(command: &mut Command, cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        bail!("Installation arrêtée");
    }
    let result = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(900), quiet(command).output()) => result.context("Une étape a dépassé 15 minutes")??,
        _ = cancel.cancelled() => bail!("Installation arrêtée"),
    };
    if !result.status.success() {
        let error = String::from_utf8_lossy(if result.stderr.is_empty() {
            &result.stdout
        } else {
            &result.stderr
        });
        let summary = error
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        bail!("{} · {}", result.status, clipped(summary.trim(), 1000));
    }
    Ok(())
}

pub struct Generated {
    pub bytes: Vec<u8>,
    pub device: String,
    pub triangles: u64,
}

pub async fn generate(
    python: &Path,
    model: &Path,
    prompt: &str,
    seed: u32,
    cancel: &CancellationToken,
    status: impl Fn(&str),
) -> Result<Generated> {
    if prompt.trim().is_empty() || prompt.len() > 2500 {
        bail!("Décris ton objet en moins de 2 500 caractères");
    }
    let temp = tempfile::tempdir()?;
    let script = temp.path().join("mesh_worker.py");
    tokio::fs::write(&script, include_bytes!("mesh_worker.py")).await?;
    let output = temp.path().join("model.glb");
    let mut xpu = true;
    loop {
        let mut command = Command::new(python);
        let mut child = quiet(&mut command)
            .arg(&script)
            .env("HF_HUB_OFFLINE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child
            .stdin
            .take()
            .context("Démarrage de Shap-E impossible")?;
        input
            .write_all(&serde_json::to_vec(&json!({
                "model":model, "output":output, "prompt":prompt, "steps":24, "seed":seed, "xpu":xpu,
            }))?)
            .await?;
        drop(input);
        let result = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(480), child.wait_with_output()) => {
                result.context("Shap-E a dépassé son délai de calcul")??
            },
            _ = cancel.cancelled() => bail!("Génération 3D arrêtée"),
        };
        if result.status.success() {
            let info: Value =
                serde_json::from_slice(&result.stdout).context("Rapport de Shap-E invalide")?;
            let bytes = tokio::fs::read(&output).await?;
            if !bytes.starts_with(b"glTF") || bytes.len() > 64 * 1024 * 1024 {
                bail!("Fichier GLB invalide");
            }
            return Ok(Generated {
                bytes,
                device: info["device"].as_str().unwrap_or("cpu").to_uppercase(),
                triangles: info["triangles"].as_u64().unwrap_or(0),
            });
        }
        let error = String::from_utf8_lossy(&result.stderr);
        if xpu && error.contains("SILLUNE_DEVICE=xpu") {
            status("Shap-E · Nouvelle tentative sur le CPU");
            xpu = false;
            continue;
        }
        let details = error.lines().rev().take(2).collect::<Vec<_>>().join(" ");
        bail!("Shap-E · {}", clipped(details.trim(), 650));
    }
}

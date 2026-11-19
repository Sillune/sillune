use crate::tools::{clipped, quiet};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};
use tokio_util::sync::CancellationToken;

const CHECK: &str =
    "import torch; from diffusers import ShapEPipeline; from skimage.measure import marching_cubes";

pub fn ready(python: &Path) -> bool {
    python.with_file_name("sillune-shape-v1").is_file()
}

pub async fn install(root: &Path, cancel: &CancellationToken) -> Result<()> {
    tokio::fs::create_dir_all(root).await?;
    let python = root.join("venv/Scripts/python.exe");
    if !python.is_file() {
        let mut launcher = Command::new("py");
        launcher
            .args(["-3.12", "-m", "venv", "--system-site-packages"])
            .arg(root.join("venv"));
        let mut fallback = Command::new("python");
        fallback
            .args(["-m", "venv", "--system-site-packages"])
            .arg(root.join("venv"));
        if run(&mut launcher, cancel).await.is_err() && !cancel.is_cancelled() {
            run(&mut fallback, cancel)
                .await
                .context("Installe Python 3.12 pour activer le pack Shap-E")?;
        }
        if cancel.is_cancelled() {
            bail!("Installation arrêtée");
        }
    }
    let mut check_torch = Command::new(&python);
    check_torch.args(["-c", "import torch"]);
    if run(&mut check_torch, cancel).await.is_err() && !cancel.is_cancelled() {
        let mut pip = Command::new(&python);
        pip.args([
            "-m",
            "pip",
            "install",
            "torch==2.8.0",
            "--index-url",
            "https://download.pytorch.org/whl/cpu",
        ]);
        run(&mut pip, cancel).await?;
    }
    let mut pip = Command::new(&python);
    pip.args([
        "-m",
        "pip",
        "install",
        "diffusers==0.35.2",
        "transformers==4.55.4",
        "accelerate==1.10.1",
        "scikit-image==0.25.2",
    ]);
    run(&mut pip, cancel).await?;
    let mut check = Command::new(&python);
    check.args(["-c", CHECK]);
    run(&mut check, cancel).await?;
    tokio::fs::write(python.with_file_name("sillune-shape-v1"), b"ready").await?;
    Ok(())
}

async fn run(command: &mut Command, cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        bail!("Installation arrêtée");
    }
    let result = tokio::select! {
        result = quiet(command).output() => result?,
        _ = cancel.cancelled() => bail!("Installation arrêtée"),
    };
    if !result.status.success() {
        let error = String::from_utf8_lossy(&result.stderr);
        bail!("{}", clipped(error.trim(), 750));
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

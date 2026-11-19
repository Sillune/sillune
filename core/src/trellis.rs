use crate::{
    mesh::Generated,
    tools::{clipped, quiet},
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{collections::VecDeque, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub async fn generate(
    binary: &Path,
    models: &Path,
    photo: &[u8],
    seed: u32,
    cancel: &CancellationToken,
    status: impl Fn(&str),
) -> Result<Generated> {
    if photo.len() > 24 * 1024 * 1024 {
        bail!("Choisis une image de moins de 24 Mio");
    }
    let input_name = if photo.starts_with(b"\x89PNG\r\n\x1a\n") {
        "source.png"
    } else if photo.starts_with(b"\xff\xd8\xff") {
        "source.jpg"
    } else {
        bail!("Le contenu du fichier ne correspond pas à une image PNG ou JPEG");
    };
    let directory = tempfile::tempdir()?;
    let input = directory.path().join(input_name);
    let output = directory.path().join("sillune.glb");
    tokio::fs::write(&input, photo).await?;

    status("TRELLIS.2 · Préparation du GPU");
    run(
        quiet(&mut Command::new(binary))
            .current_dir(directory.path())
            .arg(&input)
            .arg(&output)
            .arg("--models")
            .arg(models)
            .args([
                "--backend",
                "Vulkan",
                "--require-gpu",
                "--res",
                "512",
                "--steps",
                "12",
                "--atlas",
                "1024",
                "--seed",
                &seed.to_string(),
            ]),
        Duration::from_secs(1200),
        cancel,
        status,
    )
    .await
    .map_err(|err| anyhow::anyhow!("TRELLIS.2 · {err:#}"))?;
    let file = tokio::fs::metadata(&output)
        .await
        .context("Le moteur n’a pas produit de GLB")?;
    if file.len() > 64 * 1024 * 1024 {
        bail!("Le résultat 3D dépasse 64 Mio");
    }
    let bytes = tokio::fs::read(output).await?;
    let triangles = triangles(&bytes)?;
    Ok(Generated {
        bytes,
        device: "Vulkan".into(),
        triangles,
    })
}

async fn run(
    command: &mut Command,
    timeout: Duration,
    cancel: &CancellationToken,
    status: impl Fn(&str),
) -> Result<()> {
    let execution = async {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .context("Progression du moteur absente")?;
        let stderr = child.stderr.take().context("Journal du moteur absent")?;
        let progress = async {
            let mut lines = BufReader::new(stdout).split(b'\n');
            while let Some(line) = lines.next_segment().await? {
                let stage = match line.get(..3) {
                    Some(b"[1/") => "Détourage de l’objet",
                    Some(b"[2/") => "Analyse de l’image",
                    Some(b"[3/") => "Construction de la structure",
                    Some(b"[4/" | b"[5/") => "Construction du maillage",
                    Some(b"[6/") => "Génération des textures",
                    Some(b"[7/") => "Création du fichier GLB",
                    _ => continue,
                };
                status(&format!("TRELLIS.2 · {stage}"));
            }
            Ok::<_, std::io::Error>(())
        };
        let errors = async {
            let mut lines = BufReader::new(stderr).split(b'\n');
            let mut tail = VecDeque::with_capacity(3);
            while let Some(line) = lines.next_segment().await? {
                let decoded = String::from_utf8_lossy(&line);
                let line = decoded.trim();
                if !line.is_empty() {
                    if tail.len() == 3 {
                        tail.pop_front();
                    }
                    tail.push_back(clipped(line, 320));
                }
            }
            Ok::<_, std::io::Error>(tail)
        };
        let ((), mut tail) = tokio::try_join!(progress, errors)?;
        let exit = child.wait().await?;
        if !exit.success() {
            bail!("{exit} · {}", tail.make_contiguous().join("\n"));
        }
        Ok(())
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => bail!("Génération arrêtée"),
        result = tokio::time::timeout(timeout, execution) => result.context("Délai de calcul dépassé")?,
    }
}

fn triangles(data: &[u8]) -> Result<u64> {
    if data.len() < 20
        || !data.starts_with(b"glTF")
        || &data[4..8] != [2, 0, 0, 0]
        || u32::from_le_bytes(data[8..12].try_into()?) as usize != data.len()
    {
        bail!("Fichier GLB invalide");
    }
    let len = u32::from_le_bytes(data[12..16].try_into()?) as usize;
    if &data[16..20] != b"JSON" || len > data.len() - 20 {
        bail!("Métadonnées GLB invalides");
    }
    let graph: Value = serde_json::from_slice(&data[20..20 + len])?;
    let mut total = 0u64;
    for mesh in graph["meshes"].as_array().into_iter().flatten() {
        for primitive in mesh["primitives"].as_array().into_iter().flatten() {
            if primitive["mode"].as_u64().unwrap_or(4) != 4 {
                continue;
            }
            let index = primitive["indices"]
                .as_u64()
                .or_else(|| primitive["attributes"]["POSITION"].as_u64());
            if let Some(index) = index {
                let count = graph["accessors"][index as usize]["count"]
                    .as_u64()
                    .unwrap_or(0);
                total = total.saturating_add(count / 3);
            }
        }
    }
    if total == 0 {
        bail!("Le GLB ne contient aucun triangle");
    }
    Ok(total)
}

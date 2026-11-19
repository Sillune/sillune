#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod microphone;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sillune_core::app::App;
use std::{path::PathBuf, sync::Arc};
use tauri::Manager;

fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
async fn api(app: tauri::State<'_, Arc<App>>, op: String, data: Value) -> Result<Value, String> {
    app.inner().clone().dispatch(&op, data).await.map_err(error)
}

#[tauri::command]
async fn choose_folder() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .pick_folder()
            .map(|path| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(error)
}

#[tauri::command]
async fn export_file(app: tauri::State<'_, Arc<App>>, id: String) -> Result<bool, String> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (artifact, data) = app.store.artifact(&id).map_err(error)?;
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&artifact.name)
            .save_file()
        else {
            return Ok(false);
        };
        std::fs::write(path, data).map_err(error)?;
        Ok(true)
    })
    .await
    .map_err(error)?
}

#[tauri::command]
async fn import_file(app: tauri::State<'_, Arc<App>>) -> Result<Option<Value>, String> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return Ok(None);
        };
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("Nom de fichier invalide")?;
        if path.metadata().map_err(error)?.len() > 64 * 1024 * 1024 {
            return Err("La pièce jointe dépasse 64 Mio".to_owned());
        }
        let data = std::fs::read(&path).map_err(error)?;
        let media = sillune_core::tools::media_type(name);
        Ok(Some(json!(
            app.store.add_artifact(name, media, &data).map_err(error)?
        )))
    })
    .await
    .map_err(error)?
}

#[tauri::command]
async fn mic_start(mic: tauri::State<'_, microphone::Microphone>) -> Result<(), String> {
    mic.start().map_err(error)
}

#[tauri::command]
async fn mic_stop(mic: tauri::State<'_, microphone::Microphone>) -> Result<String, String> {
    Ok(STANDARD.encode(mic.stop().map_err(error)?))
}

#[tauri::command]
async fn choose_audio() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio PCM", &["wav"])
            .pick_file()
        else {
            return Ok(None);
        };
        Ok(Some(
            STANDARD.encode(microphone::read_wav(&path).map_err(error)?),
        ))
    })
    .await
    .map_err(error)?
}

#[tauri::command]
async fn open_link(url: String) -> Result<(), String> {
    let url = url::Url::parse(&url).map_err(error)?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Adresse web invalide".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(url.as_str())
        .spawn()
        .map_err(error)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let root = match std::env::var_os("SILLUNE_PROFILE") {
                Some(root) => PathBuf::from(root),
                None => app.path().app_local_data_dir()?,
            };
            let root = App::open_with_desktop(root, app.path().desktop_dir().ok())?;
            app.manage(root);
            app.manage(microphone::Microphone::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api,
            choose_folder,
            export_file,
            import_file,
            mic_start,
            mic_stop,
            choose_audio,
            open_link
        ])
        .build(tauri::generate_context!())
        .expect("Sillune n’a pas pu ouvrir sa fenêtre")
        .run(|handle, event| {
            if matches!(event, tauri::RunEvent::Exit)
                && let Some(app) = handle.try_state::<Arc<App>>()
            {
                tauri::async_runtime::block_on(app.close());
            }
        });
}

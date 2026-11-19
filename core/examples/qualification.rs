// Run with a disposable profile and workspace. The API key arrives over stdin.
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sillune_core::{app::App, storage::Settings, vault};
use std::{
    io::{self, BufRead},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

struct Credentials(Option<String>);
impl Drop for Credentials {
    fn drop(&mut self) {
        let _ = vault::set("openrouter", self.0.as_deref().unwrap_or_default());
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 {
        bail!("qualification <profil> <dossier Runtime> <PCM16 mono 16 kHz> [--benchmark]");
    }
    let profile = PathBuf::from(&args[0]);
    let workarea = profile.join("Dossier de test");
    std::fs::create_dir_all(&workarea)?;
    let app = App::open(profile.clone())?;
    let backup = Credentials(vault::get("openrouter")?);
    let mut key = String::new();
    io::stdin().lock().read_line(&mut key)?;
    if key.trim().is_empty() {
        bail!("Clé de qualification absente");
    }
    vault::set("openrouter", key.trim())?;
    key.clear();
    let settings = Settings {
        provider: "openrouter".into(),
        model: "openai/gpt-4.1-mini".into(),
        theme: "dark".into(),
        workspace: workarea.to_string_lossy().into_owned(),
        engines: args[1].clone(),
        ..Settings::default()
    };
    app.store.save_settings(&settings)?;
    if args.iter().any(|a| a == "--benchmark") {
        benchmark(&app).await?;
        return Ok(());
    }

    let chat = session(&app, "chat", "Chat · Sources et page HTML").await?;
    let result=turn(&app,&chat,json!({"text":
        "Cherche sur le web les documentations officielles de Tauri et Leptos. Utilise web_search une fois, et vérifie les deux URL. Livre un fichier comparatif.html avec le titre « Deux outils Rust », un tableau simple, et deux liens sourcés. Réponds brièvement."}),true).await?;
    check_file(&app, &result, "text/html")?;
    let (_, contents) = app
        .store
        .artifact(result["files"][0]["id"].as_str().unwrap())?;
    let html = String::from_utf8(contents)?;
    if !html.contains("tauri.app") || !html.contains("leptos") {
        bail!("Le comparatif n’inclut pas les deux sources");
    }
    if std::fs::read_dir(&workarea)?.next().is_some() {
        bail!("Écriture inattendue en Chat");
    }
    println!("Chat · réponses OpenRouter, deux sources, HTML joint, lecture seule");

    let agent = session(&app, "agent", "Agent · Autorisations et retour arrière").await?;
    let message = json!({"text":"Avec write_file, crée un fichier dossier-essai.txt qui contient exactement le texte fictif « Projet Lunaire ». Utilise ce fichier unique."});
    let denied = turn(&app, &agent, message.clone(), false).await?;
    if denied["approved"].as_u64() == Some(0) || workarea.join("dossier-essai.txt").exists() {
        bail!("Refus agent invalide");
    }
    let yes = turn(&app, &agent, message, true).await?;
    if yes["approved"].as_u64() == Some(0) || !workarea.join("dossier-essai.txt").is_file() {
        bail!("Écriture en Agent absente");
    }
    let restored = app.dispatch("undo", json!({"session":agent})).await?;
    if restored["restored"] != json!(["dossier-essai.txt"]) {
        bail!("Restauration agent non confirmée");
    }
    println!("Agent · refus, écriture autorisée et restauration confirmés");

    let plugin = session(&app, "plugin", "Plugin · Minuteur et onglet Notes").await?;
    let timer = "Crée un minuteur de dix minutes avec l’identifiant minuteur. Affiche-le en haut à droite. Utilise install_plugin et ajoute un README expliquant le test.";
    let notes = "Crée un nouvel onglet intitulé Notes, identifiant notes, avec la phrase « Carnet des idées ». Utilise install_plugin et ajoute un README explicatif.";
    for message in [timer, notes] {
        turn(&app, &plugin, json!({"text":message}), true).await?;
    }
    let entries = app.plugins.list();
    if !entries
        .iter()
        .any(|p| p.manifest.id == "minuteur" && p.active && p.manifest.seconds == 600)
        || !entries
            .iter()
            .any(|p| p.manifest.id == "notes" && p.active && p.manifest.kind == "tab")
    {
        bail!("Les deux extensions ne sont pas actives");
    }
    for plugin in ["minuteur", "notes"] {
        app.plugins.docs(plugin)?;
        assert_eq!(
            app.plugins
                .guide()?
                .matches(&format!("plugins/{plugin}/"))
                .count(),
            1
        );
    }
    println!("Plugin · deux créations par le modèle, manifestes, README et index vérifiés");

    let image = session(&app, "image", "Image · Observatoire lunaire").await?;
    for seed in [431, 432, 433, 434, 435, 436] {
        let result=turn(&app,&image,json!({"text":"A small solar observatory on a misty green hillside under the crescent moon, cinematic film photograph","seed":seed}),true).await?;
        check_file(&app, &result, "image/png")?;
        println!("Image · {}", result["text"].as_str().unwrap_or_default());
    }
    let bot = session(&app, "bot", "Bot · LFM3 en français").await?;
    let pcm = STANDARD.encode(std::fs::read(&args[2])?);
    let voice = turn(&app, &bot, json!({"pcm":pcm,"speak":true}), false).await?;
    if voice["voice"] != true || !voice["files"].as_array().is_some_and(Vec::is_empty) {
        bail!("Le Bot doit produire une réponse audio temporaire");
    }
    let audio = app.dispatch("voice_audio", json!({"session":bot})).await?;
    if !audio
        .as_str()
        .is_some_and(|v| v.starts_with("data:audio/wav;base64,UklGR"))
    {
        bail!("L’audio temporaire du Bot est absent");
    }
    if !voice["events"].as_array().is_some_and(|rows| {
        rows.iter()
            .any(|r| r.as_str().unwrap_or_default().starts_with("Voix reconnue"))
    }) {
        bail!("La transcription LFM3 n’est pas confirmée");
    }
    println!("Bot · transcription et WAV généré par LFM3");
    app.close().await;
    drop(backup);
    Ok(())
}

async fn session(app: &Arc<App>, mode: &str, title: &str) -> Result<String> {
    Ok(app
        .dispatch("session_new", json!({"mode":mode,"title":title}))
        .await?["id"]
        .as_str()
        .context("Session absente")?
        .to_owned())
}

async fn turn(app: &Arc<App>, id: &str, mut data: Value, allow: bool) -> Result<Value> {
    data["session"] = json!(id);
    app.dispatch("start", data).await?;
    tokio::time::timeout(Duration::from_secs(170), async {
        let mut approvals = 0u64;
        let mut previous = String::new();
        loop {
            let mut view = app.dispatch("poll", json!({"session":id})).await?;
            if let Some(approval) = view["approval"]["id"].as_str().filter(|a| *a != previous) {
                previous = approval.into();
                approvals += 1;
                app.dispatch("approve", json!({"id":approval,"allow":allow}))
                    .await?;
            }
            if view["done"] == true {
                if let Some(error) = view["error"].as_str().filter(|s| !s.is_empty()) {
                    bail!("{error}");
                }
                view["approved"] = json!(approvals);
                return Ok(view);
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
    })
    .await?
}

fn check_file(app: &Arc<App>, view: &Value, kind: &str) -> Result<()> {
    let id = view["files"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["media"] == kind)
        .and_then(|a| a["id"].as_str())
        .with_context(|| format!("Pièce jointe {kind} absente"))?;
    let (_, data) = app.store.artifact(id)?;
    if data.len() < 40 {
        bail!("Pièce jointe trop courte");
    }
    Ok(())
}

async fn benchmark(app: &Arc<App>) -> Result<()> {
    let mut totals = [0u64; 2];
    for (index, full) in [true, false].into_iter().enumerate() {
        let chat = session(
            app,
            "chat",
            if full {
                "Mesure · Historique intégral"
            } else {
                "Mesure · Contexte optimisé"
            },
        )
        .await?;
        for i in 0..10 {
            let context=(0..16).map(|j|format!("Lot {}.{}: la navette fictive planifie une livraison de matériel et son contrôle de qualité lors du prochain trimestre.",i+1,j+1)).collect::<Vec<_>>().join(" ");
            let text = format!(
                "Voici le registre fictif du lot {}. {context} Réponds exactement « Lot {} validé. » sans créer de fichier.",
                i + 1,
                i + 1
            );
            let result = turn(app, &chat, json!({"text":text,"full_context":full}), true).await?;
            if !result["text"]
                .as_str()
                .is_some_and(|s| s.contains(&format!("Lot {} validé", i + 1)))
            {
                bail!("Résultat divergent lors du test de contexte");
            }
            totals[index] += result["usage"]["input"].as_u64().unwrap_or(0);
        }
    }
    let delta = 100.0 * (1.0 - totals[1] as f64 / totals[0].max(1) as f64);
    println!(
        "10 tours à réponses équivalentes : {} → {} tokens entrants ({delta:.1} % économisés)",
        totals[0], totals[1]
    );
    Ok(())
}

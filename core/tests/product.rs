use serde_json::{Value, json};
use sillune_core::{
    app::App,
    plugins::Registry,
    storage::{Mode, Settings},
    tools::{Workspace, allowed},
};
use std::{io::Read, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn fixture() -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let _ = answer(socket).await;
            });
        }
    });
    format!("http://{address}/v1")
}

async fn answer(mut socket: TcpStream) -> anyhow::Result<()> {
    let mut request = Vec::new();
    let (header, start) = loop {
        let mut chunk = [0u8; 4096];
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&chunk[..n]);
        if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            break (String::from_utf8(request[..pos].to_vec())?, pos + 4);
        }
    };
    let bytes = header
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        })
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0usize);
    while request.len() < start + bytes {
        let mut chunk = [0u8; 4096];
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&chunk[..n]);
    }
    let target = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default();
    let response = if target == "/v1/models" {
        Some(
            json!({"data":[{"id":"fixture-model","display_name":"Modèle de validation",
            "reasoning":{"supported_efforts":["minimal","medium","high","xhigh"]},
            "supported_parameters":[]}]}),
        )
    } else if target.starts_with("/v1/search?") {
        Some(
            json!({"results":[{"title":"Source des essais","url":"https://example.org/robot",
            "content":"Donnée contrôlée SILLUNE_WEB_QUALIFICATION"}]}),
        )
    } else {
        None
    };
    if let Some(response) = response {
        let payload = response.to_string();
        socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}", payload.len()).as_bytes()).await?;
        return Ok(());
    }
    let body: Value = serde_json::from_slice(&request[start..start + bytes]).unwrap_or(Value::Null);
    if body["model"] == "fixture-refused" {
        socket
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            )
            .await?;
        return Ok(());
    }
    let messages = body["messages"].as_array();
    let latest = messages
        .and_then(|m| m.last())
        .cloned()
        .unwrap_or(Value::Null);
    let text = latest["content"].as_str().unwrap_or_default();
    let tool = |index: u32, name: &str, args: Value| json!({"index":index,"id":format!("job-{index}"),"type":"function","function":{"name":name,"arguments":args.to_string()}});
    let event = if body["model"] == "fixture-model" {
        let works = text.contains("SILLUNE_WEB_QUALIFICATION")
            && text.contains("https://example.org/robot")
            && body["reasoning_effort"] == "xhigh"
            && body["tools"].is_null();
        json!({"choices":[{"delta":{"content":if works {"Recherche, modèle et effort vérifiés."} else {"Données manquantes."}}}]})
    } else if text.contains("secours.txt") {
        let content = if body["messages"][0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("uniquement avec")
            && body["tools"].is_null()
        {
            json!({"tool":"write_file","arguments":{"path":"secours.txt","content":"preuve"}})
                .to_string()
        } else {
            anyhow::ensure!(
                !body["tools"]
                    .as_array()
                    .unwrap_or(&Vec::new())
                    .iter()
                    .any(|tool| tool["function"]["name"] == "web_search")
            );
            "Je vais créer le fichier.".to_owned()
        };
        json!({"choices":[{"delta":{"content":content}}]})
    } else if text.contains("palette encre") {
        let manifest = json!({"id":"encre","name":"Encre","scheme":"dark","colors":{
            "background":"#101723","sidebar":"#131d2c","surface":"#1b2a40","text":"#f4f7ff",
            "muted":"#b4c2db","accent":"#90b9ff","border":"#3a5173"
        }});
        json!({"choices":[{"delta":{"tool_calls":[{"id":"theme-0","function":{
            "name":"install_theme","arguments":{"manifest":manifest}
        }}]}}]})
    } else if text.contains("ancienne syntaxe") {
        json!({"choices":[{"delta":{"function_call":{"name":"deliver_file","arguments":"{\"name\":\"ancien.txt\",\"content\":\"Pièce jointe\"}"}}}]})
    } else if latest["role"] == "tool" {
        json!({"choices":[{"delta":{"content":if text.starts_with("Action non") {"L’action a été refusée."} else {"Terminé."}}}]})
    } else if text.contains("comparatif") {
        json!({"choices":[{"delta":{"tool_calls":[
            tool(0,"write_file",json!({"path":"interdit.txt","content":"inattendu"})),
            tool(1,"deliver_file",json!({"name":"comparatif.html","content":"<h1>Comparatif</h1><p>Deux notes de test.</p>"}))
        ]}}]})
    } else if text.contains("minuteur") || text.contains("onglet") {
        let timer = text.contains("minuteur");
        let spec = if timer {
            "id='minuteur'\ntitle='Minuteur'\ndescription='Compte à rebours de dix minutes'\nkind='timer'\nversion='1.0.0'\nseconds=600"
        } else {
            "id='notes'\ntitle='Notes'\ndescription='Onglet de rappels'\nkind='tab'\nversion='1.0.0'\ntext='Mes notes'"
        };
        json!({"choices":[{"delta":{"tool_calls":[tool(0,"install_plugin",json!({"manifest":spec,"readme":"# Extension\n\nVérifier le composant dans la fenêtre."}))]}}]})
    } else if text.contains("écrit") {
        json!({"choices":[{"delta":{"tool_calls":[tool(0,"write_file",json!({"path":"essai.txt","content":"Texte créé."}))]}}]})
    } else {
        json!({"choices":[{"delta":{"content":"Bonjour, je suis Sillune. Je t’écoute."}}]})
    };
    let payload = format!(
        "data: {event}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"usage":{"prompt_tokens":120,"completion_tokens":18}})
    );
    socket.write_all(format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
        payload.len()).as_bytes()).await?;
    Ok(())
}

async fn session(app: &Arc<App>, mode: &str) -> String {
    app.dispatch("session_new", json!({"mode":mode}))
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

async fn poll(app: &Arc<App>, id: &str, condition: impl Fn(&Value) -> bool) -> Value {
    poll_for(app, id, 140, condition).await
}

async fn poll_for(
    app: &Arc<App>,
    id: &str,
    seconds: u64,
    condition: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(seconds), async {
        loop {
            let value = app.dispatch("poll", json!({"session":id})).await.unwrap();
            if condition(&value) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("Tour bloqué")
}

fn artifact_id(poll: &Value) -> &str {
    poll["files"][0]["id"].as_str().expect("pièce jointe")
}

#[tokio::test]
async fn creates_empty_desktop_file_with_permission_and_no_provider() {
    let root = tempfile::tempdir().unwrap();
    let desktop = root.path().join("Bureau");
    std::fs::create_dir(&desktop).unwrap();
    let app = App::open_with_desktop(root.path().join("profil"), Some(desktop.clone())).unwrap();
    app.store
        .save_settings(&Settings {
            provider: "compatible".into(),
            ..Settings::default()
        })
        .unwrap();
    let agent = session(&app, "agent").await;
    let prompt = "Crée un fichier texte vide nommé « Fichier vide.txt » sur mon bureau.";
    app.dispatch("start", json!({"session":agent,"text":prompt}))
        .await
        .unwrap();
    let pending = poll(&app, &agent, |v| {
        !v["approval"].is_null() || v["done"] == true
    })
    .await;
    assert!(
        pending["approval"]["detail"]
            .as_str()
            .unwrap()
            .contains("Bureau → Fichier vide.txt")
    );
    assert!(!desktop.join("Fichier vide.txt").exists());
    app.dispatch(
        "approve",
        json!({"id":pending["approval"]["id"],"allow":true}),
    )
    .await
    .unwrap();
    let done = poll(&app, &agent, |v| v["done"] == true).await;
    assert_eq!(done["error"], "");
    assert_eq!(
        done["text"],
        "Fichier créé sur le Bureau · Fichier vide.txt"
    );
    assert_eq!(
        std::fs::read(desktop.join("Fichier vide.txt")).unwrap(),
        b""
    );
    app.dispatch(
        "start",
        json!({"session":agent,"text":"Fait un fichier test.md sur mon bureau"}),
    )
    .await
    .unwrap();
    let pending = poll(&app, &agent, |v| v["approval"].is_object()).await;
    assert!(
        pending["approval"]["detail"]
            .as_str()
            .unwrap()
            .contains("Bureau → test.md")
    );
    app.dispatch(
        "approve",
        json!({"id":pending["approval"]["id"],"allow":true}),
    )
    .await
    .unwrap();
    let created = poll(&app, &agent, |v| v["done"] == true).await;
    assert_eq!(created["error"], "");
    assert_eq!(std::fs::read(desktop.join("test.md")).unwrap(), b"");
    app.dispatch("start", json!({"session":agent,"text":prompt}))
        .await
        .unwrap();
    assert!(
        poll(&app, &agent, |v| v["done"] == true).await["error"]
            .as_str()
            .unwrap()
            .contains("existe déjà")
    );
}

#[tokio::test]
async fn structured_recovery_and_theme_install_follow_mode() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings {
        provider: "compatible".into(),
        url: fixture().await,
        model: "fixture".into(),
        permission: "all".into(),
        workspace: root.path().display().to_string(),
        ..Settings::default()
    };
    let app = App::open(root.path().join("profil")).unwrap();
    app.store.save_settings(&settings).unwrap();
    let agent = session(&app, "agent").await;
    app.dispatch(
        "start",
        json!({"session":agent,"text":"Crée le fichier secours.txt dans le dossier de travail."}),
    )
    .await
    .unwrap();
    let response = poll(&app, &agent, |v| v["done"] == true).await;
    assert_eq!(response["error"], "");
    assert_eq!(
        std::fs::read_to_string(root.path().join("secours.txt")).unwrap(),
        "preuve"
    );
    assert!(!response["text"].as_str().unwrap().contains("Je vais"));

    let theme = session(&app, "themes").await;
    app.dispatch(
        "start",
        json!({"session":theme,"text":"Crée une palette encre."}),
    )
    .await
    .unwrap();
    let response = poll(&app, &theme, |v| v["done"] == true).await;
    assert_eq!(response["error"], "");
    let status = app.dispatch("snapshot", json!({})).await.unwrap();
    assert_eq!(status["settings"]["theme"], "encre");
    assert_eq!(
        status["themes"]["encre"]["colors"]["accent_text"],
        "#000000"
    );
    assert!(status["skills"].as_array().unwrap().len() >= 15);
}

#[tokio::test]
async fn discovers_models_runs_web_without_native_tools_and_keeps_chat_preferences() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::open(tmp.path().join("profile")).unwrap();
    let url = fixture().await;
    app.store
        .save_settings(&Settings {
            provider: "compatible".into(),
            model: "fixture".into(),
            search: "searx".into(),
            searx_url: url.clone(),
            url,
            ..Settings::default()
        })
        .unwrap();
    let snapshot = app.dispatch("snapshot", json!({})).await.unwrap();
    for pack in ["image", "quality", "zimage", "shape", "trellis", "voice"] {
        assert_eq!(snapshot["packs"][pack]["ready"], false);
    }
    assert!(!tmp.path().join("profile/runtime").exists());
    let rows = app
        .dispatch("models", json!({"provider":"compatible","refresh":true}))
        .await
        .unwrap();
    assert_eq!(rows[0]["id"], "fixture-model");
    let chat = session(&app, "chat").await;
    app.dispatch(
        "chat_preferences",
        json!({"session":chat,"provider":"compatible","model":"fixture-model","effort":5}),
    )
    .await
    .unwrap();
    app.dispatch(
        "start",
        json!({"session":chat,"text":"Cherche sur le web des informations sur les robots."}),
    )
    .await
    .unwrap();
    let result = poll(&app, &chat, |v| v["done"] == true).await;
    assert_eq!(result["error"], "");
    assert_eq!(result["text"], "Recherche, modèle et effort vérifiés.");
    assert!(
        result["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains("Recherche web exécutée"))
    );
    let history = app
        .dispatch("messages", json!({"session":chat}))
        .await
        .unwrap();
    assert_eq!(history["preferences"]["effort"], 5);
    assert_eq!(app.store.settings().unwrap().model, "fixture");
    app.dispatch(
        "start",
        json!({"session":chat,
        "text":"Cherche des sources en ligne sur les robots.",
        "selection":{"provider":"compatible","model":"fixture-refused"}}),
    )
    .await
    .unwrap();
    let refused = poll(&app, &chat, |v| v["done"] == true).await;
    assert_eq!(refused["credentials"], true);
    assert!(refused["error"].as_str().unwrap().contains("HTTP 401"));
    assert!(
        refused["text"]
            .as_str()
            .unwrap()
            .contains("<https://example.org/robot>")
    );
    let independent = session(&app, "chat").await;
    app.dispatch(
        "start",
        json!({"session":independent,"text":"Essaie l’ancienne syntaxe."}),
    )
    .await
    .unwrap();
    let result = poll(&app, &independent, |v| v["done"] == true).await;
    assert_eq!(result["error"], "");
    assert_eq!(result["files"][0]["name"], "ancien.txt");
    app.dispatch("session_delete", json!({"session":chat}))
        .await
        .unwrap();
    assert!(
        app.store
            .kv(&format!("chat-model/{chat}"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn filesystem_scope_and_restore_conflict() {
    let root = tempfile::tempdir().unwrap();
    let ws = Workspace::open(root.path().to_str().unwrap()).unwrap();
    assert!(!allowed(Mode::Chat, "write_file"));
    assert!(ws.read("../hors-du-projet", 100).is_err());
    assert!(ws.read(".env", 100).is_err());
    assert!(ws.read("a.txt:flux", 100).is_err());
    std::fs::write(root.path().join("note.txt"), b"original").unwrap();
    let edit = ws.plan("note.txt", Some(b"modifie")).unwrap();
    ws.apply(&edit, Some(b"modifie")).unwrap();
    std::fs::write(root.path().join("note.txt"), b"retouche").unwrap();
    assert!(!ws.restore(&edit).unwrap());
    assert_eq!(ws.read("note.txt", 500).unwrap(), b"retouche");
}

#[tokio::test]
#[ignore = "Nécessite la cible Rust wasm32-unknown-unknown"]
async fn rust_widget_compiles_and_executes() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("plugins");
    let plugins = Registry::open(root.clone()).unwrap();
    let description = "id='calcul-lunaire'\ntitle='Calcul lunaire'\ndescription='Calcule une valeur dans un onglet'\nkind='wasm'\nversion='1.0.0'";
    let manifest = Registry::validate(description).unwrap();
    assert!(
        plugins
            .install(
                manifest.clone(),
                "## Vérifier",
                "const TEST: &str = include_str!(\"local.txt\");"
            )
            .await
            .is_err()
    );
    plugins.install(
        manifest, "## Vérifier\nEntrer 7 et exécuter le calcul.",
        "#[unsafe(no_mangle)] pub extern \"C\" fn sillune_tick(elapsed: i32, value: i32) -> i32 { elapsed + value * 3 }",
    ).await.unwrap();
    assert_eq!(plugins.tick("calcul-lunaire", 5, 7).unwrap(), 26);
    let reopened = Registry::open(root).unwrap();
    assert_eq!(reopened.tick("calcul-lunaire", 1, 10).unwrap(), 31);
}

#[tokio::test]
async fn chat_agent_plugin_memory() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let profile = tmp.path().join("profile");
    let app = App::open(profile.clone()).unwrap();
    let mut settings = Settings {
        provider: "compatible".into(),
        url: fixture().await,
        model: "fixture".into(),
        workspace: workspace.to_str().unwrap().into(),
        ..Settings::default()
    };
    app.store.save_settings(&settings).unwrap();

    let chat = session(&app, "chat").await;
    app.dispatch(
        "start",
        json!({"session":chat,"text":"Fais le comparatif des deux notes."}),
    )
    .await
    .unwrap();
    let result = poll(&app, &chat, |v| v["done"] == true).await;
    assert_eq!(result["error"], "");
    let preview = app
        .dispatch("artifact", json!({"id":artifact_id(&result)}))
        .await
        .unwrap();
    assert!(
        preview["preview"]
            .as_str()
            .unwrap()
            .contains("<h1>Comparatif</h1>")
    );
    assert!(std::fs::read_dir(&workspace).unwrap().next().is_none());

    let agent = session(&app, "agent").await;
    for allow in [false, true] {
        app.dispatch(
            "start",
            json!({"session":agent,"text":"Vérifie et écrit le fichier essai."}),
        )
        .await
        .unwrap();
        let pending = poll(&app, &agent, |v| {
            v["approval"].is_object() || v["done"] == true
        })
        .await;
        assert_eq!(pending["approval"]["label"], "Écrire dans le dossier");
        app.dispatch(
            "approve",
            json!({"id":pending["approval"]["id"],"allow":allow}),
        )
        .await
        .unwrap();
        let done = poll(&app, &agent, |v| v["done"] == true).await;
        assert_eq!(done["error"], "");
        assert_eq!(workspace.join("essai.txt").exists(), allow);
    }
    let undo = app
        .dispatch("undo", json!({"session":agent}))
        .await
        .unwrap();
    assert_eq!(undo["restored"], json!(["essai.txt"]));
    assert!(!workspace.join("essai.txt").exists());
    settings.permission = "safe".into();
    app.store.save_settings(&settings).unwrap();
    app.dispatch(
        "start",
        json!({"session":agent,"text":"Vérifie et écrit le fichier essai."}),
    )
    .await
    .unwrap();
    assert_eq!(poll(&app, &agent, |v| v["done"] == true).await["error"], "");
    assert!(workspace.join("essai.txt").is_file());
    app.dispatch(
        "start",
        json!({"session":agent,"text":"Vérifie et écrit le fichier essai."}),
    )
    .await
    .unwrap();
    let pending = poll(&app, &agent, |v| {
        v["approval"].is_object() || v["done"] == true
    })
    .await;
    assert!(pending["approval"].is_object());
    app.dispatch(
        "approve",
        json!({"id":pending["approval"]["id"],"allow":false}),
    )
    .await
    .unwrap();
    poll(&app, &agent, |v| v["done"] == true).await;

    settings.permission = "all".into();
    settings.memory = true;
    app.store.save_settings(&settings).unwrap();
    let plugin = session(&app, "plugin").await;
    for request in ["Crée le minuteur de dix minutes.", "Crée un onglet Notes."] {
        app.dispatch("start", json!({"session":plugin,"text":request}))
            .await
            .unwrap();
        assert_eq!(
            poll(&app, &plugin, |v| v["done"] == true).await["error"],
            ""
        );
    }
    let restarted = App::open(profile.clone()).unwrap();
    let status = restarted.dispatch("snapshot", json!({})).await.unwrap();
    assert!(
        status["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "minuteur" && p["seconds"] == 600)
    );
    let skill = std::fs::read_to_string(profile.join("plugins").join("SKILL.md")).unwrap();
    assert_eq!(skill.matches("plugins/minuteur/").count(), 1);
    assert_eq!(skill.matches("plugins/notes/").count(), 1);
    assert!(profile.join("plugins/notes/README.md").is_file());
    restarted
        .dispatch("plugin_active", json!({"id":"notes","active":false}))
        .await
        .unwrap();
    assert!(!restarted.plugins.enabled("notes"));
    restarted
        .dispatch("plugin_remove", json!({"id":"minuteur"}))
        .await
        .unwrap();
    assert!(
        !restarted
            .plugins
            .guide()
            .unwrap()
            .contains("plugins/minuteur/")
    );

    restarted
        .dispatch(
            "note_save",
            json!({"kind":"preference","content":"La couleur favorite est violette."}),
        )
        .await
        .unwrap();
    let id = restarted.store.notes().unwrap()[0].id.clone();
    restarted
        .dispatch(
            "note_save",
            json!({"id":id,"kind":"preference","content":"La couleur favorite est verte."}),
        )
        .await
        .unwrap();
    assert_eq!(restarted.store.notes().unwrap().len(), 1);
    assert!(
        restarted.store.notes().unwrap()[0]
            .content
            .contains("verte")
    );
    assert!(
        !restarted
            .store
            .recall("comparatif", "nouveau")
            .unwrap()
            .is_empty()
    );
    restarted
        .dispatch("note_forget", json!({"id":id}))
        .await
        .unwrap();
    assert!(restarted.store.notes().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "Nécessite les moteurs Vulkan et LFM3 installés"]
async fn local_image_and_french_voice() {
    let root = tempfile::tempdir().unwrap();
    let app = App::open(root.path().into()).unwrap();
    let runtime = std::env::var("SILLUNE_RUNTIME_TEST").expect("dossier Runtime");
    let settings = Settings {
        provider: "compatible".into(),
        url: fixture().await,
        model: "fixture".into(),
        engines: runtime,
        ..Settings::default()
    };
    app.store.save_settings(&settings).unwrap();
    let image = session(&app, "image").await;
    app.dispatch(
        "start",
        json!({"session":image,"text":"A tiny moonlit library, cinematic pastel colors","seed":42}),
    )
    .await
    .unwrap();
    let result = poll(&app, &image, |v| v["done"] == true).await;
    assert_eq!(result["error"], "");
    let (_, data) = app.store.artifact(artifact_id(&result)).unwrap();
    assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
    assert!(result["text"].as_str().unwrap().contains("Vulkan"));
    println!("Image: {}", result["text"].as_str().unwrap());

    let bot = session(&app, "bot").await;
    let audio = std::env::var_os("SILLUNE_AUDIO_TEST");
    let mut body = json!({"session":bot,"text":"Bonjour","speak":true});
    if let Some(pcm) = audio {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let mut buf = Vec::new();
        std::fs::File::open(PathBuf::from(pcm))
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        body["text"] = json!("");
        body["pcm"] = json!(STANDARD.encode(buf));
    }
    app.dispatch("start", body).await.unwrap();
    let result = poll(&app, &bot, |v| v["done"] == true).await;
    assert_eq!(result["error"], "");
    assert_eq!(result["voice"], true);
    assert!(result["files"].as_array().unwrap().is_empty());
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let playback = app
        .dispatch("voice_audio", json!({"session":bot}))
        .await
        .unwrap();
    let wav = STANDARD
        .decode(playback.as_str().unwrap().split_once(',').unwrap().1)
        .unwrap();
    assert_eq!(&wav[..4], b"RIFF");
    let reopened = App::open(root.path().into()).unwrap();
    assert!(
        reopened
            .dispatch("voice_audio", json!({"session":bot}))
            .await
            .is_err()
    );
    println!(
        "Bot LFM3: {} octets WAV, {} événements",
        wav.len(),
        result["events"].as_array().unwrap().len()
    );
    app.engines.close_voice().await;
}

#[tokio::test]
#[ignore = "Nécessite les packs Z-Image et Shap-E installés"]
async fn optional_zimage_and_3d() {
    let root = tempfile::tempdir().unwrap();
    let app = App::open(root.path().into()).unwrap();
    app.store
        .save_settings(&Settings {
            engines: std::env::var("SILLUNE_RUNTIME_TEST").expect("dossier Runtime"),
            ..Settings::default()
        })
        .unwrap();
    let image = session(&app, "image").await;
    for side in [768, 768, 1024] {
        app.dispatch("start", json!({"session":image, "image_profile":"zimage",
            "text":"A red toy robot in a moonlit workshop, copper details, soft blue eye lights, studio photograph",
            "width":side,"height":side,"seed":42})).await.unwrap();
        let result = poll_for(&app, &image, 320, |v| v["done"] == true).await;
        assert_eq!(result["error"], "");
        let (_, data) = app.store.artifact(artifact_id(&result)).unwrap();
        assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(data[16..20].try_into().unwrap()), side);
        println!("Z-Image: {}", result["text"].as_str().unwrap());
    }
    let mesh = session(&app, "3d").await;
    app.dispatch(
        "start",
        json!({"session":mesh,
        "text":"A small red robot with a round head and two blue eyes","seed":42,"mesh_profile":"shape"}),
    )
    .await
    .unwrap();
    let result = poll_for(&app, &mesh, 1000, |v| v["done"] == true).await;
    assert_eq!(result["error"], "");
    let (file, data) = app.store.artifact(artifact_id(&result)).unwrap();
    assert_eq!(file.media, "model/gltf-binary");
    assert_eq!(&data[..4], b"glTF");
    assert_eq!(
        u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize,
        data.len()
    );
    let end = 20 + u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    let metadata: Value = serde_json::from_slice(&data[20..end]).unwrap();
    let attributes = &metadata["meshes"][0]["primitives"][0]["attributes"];
    assert!(attributes["POSITION"].is_number());
    assert!(attributes["COLOR_0"].is_number());
    println!("Shap-E: {}", result["text"].as_str().unwrap());
    app.engines.close().await;
}

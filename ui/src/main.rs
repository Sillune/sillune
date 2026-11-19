mod client;
mod mesh_view;
mod model_picker;
mod settings;

use client::{
    MODES, Ui, browser_now, clock, copy_text, field, invoke, link_click, list, markdown,
    mode_label, safe_document,
};
use leptos::{prelude::*, task::spawn_local};
use serde_json::{Value, json};

fn main() {
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let ui = Ui::new();
    let now = RwSignal::new(browser_now());
    spawn_local(async move {
        ui.refresh().await;
    });
    ui.start_polling();
    clock(move || now.set(browser_now()), 250);
    view! {
        <div class="app" data-theme=move || ui.setting("theme") style=move || theme_style(ui)>
            <Sidebar ui=ui/>
            <main class="main">
                <Header ui=ui now=now/>
                {move || match ui.panel.get().as_str() {
                    "settings"=>view!{<settings::SettingsPage ui=ui/>}.into_any(),
                    "prompts"=>view!{<PromptPage ui=ui/>}.into_any(),
                    "conversation"=>view!{<Conversation ui=ui/>}.into_any(),
                    _=>view!{<ExtensionPage ui=ui/>}.into_any(),
                }}
            </main>
            <Preview ui=ui/>
            <Show when=move || !ui.notice.get().is_empty()>
                <div class="toast" role="status">
                    <span>{move || ui.notice.get()}</span>
                    <button class="iconbutton" on:click=move |_|ui.notice.set(String::new()) aria-label="Fermer le message">"×"</button>
                </div>
            </Show>
        </div>
    }
}

fn theme_style(ui: Ui) -> String {
    ui.snapshot.with(|snapshot| {
        let id = snapshot["settings"]["theme"].as_str().unwrap_or_default();
        let theme = &snapshot["themes"][id];
        let Some(scheme) = theme["scheme"].as_str() else { return String::new(); };
        let colors = &theme["colors"];
        let mut style = format!("color-scheme:{scheme};");
        for (key, vars) in [
            ("background", &["canvas"][..]), ("sidebar", &["side"]), ("surface", &["card"]),
            ("text", &["text"]), ("muted", &["muted","hint"]), ("accent", &["accent","accent-solid"]),
            ("accent_text", &["accent-ink"]), ("border", &["edge"]), ("danger", &["danger"]), ("success", &["success"]),
        ] {
            if let Some(value) = colors[key].as_str() {
                for name in vars { style.push_str(&format!("--{name}:{value};")); }
            }
        }
        style.push_str("--hover:color-mix(in srgb,var(--text) 8%,var(--card));--accent-surface:color-mix(in srgb,var(--accent) 20%,var(--card));");
        style
    })
}

#[component]
fn Sidebar(ui: Ui) -> impl IntoView {
    let expanded = RwSignal::new(false);
    view! {
        <aside class="sidebar" class:nav-open=move || ui.nav.get()>
            <div class="wordmark">
                <div class="mark"><i></i></div>
                <span>"Sillune"</span>
                <button class="iconbutton nav-close" aria-label="Fermer la navigation" on:click=move |_|ui.nav.set(false)>"×"</button>
            </div>
            <button class="new-button" on:click=move |_|ui.new_chat(&ui.mode.get_untracked())>
                <span class="nav-icon">"+"</span>"Nouvelle conversation"
            </button>
            <div class="nav-group">
                {MODES.iter().enumerate().map(|(index,(mode,label,icon))| {
                    let (mode,label,icon)=(*mode,*label,*icon);
                    view! {
                        <button class="nav-row" hidden=move || { !ui.mode_available(mode) ||
                            index>=2 && (ui.setting("mode_navigation")=="header" || !expanded.get()) }
                            class:selected=move || ui.panel.get()=="conversation" && ui.mode.get()==mode
                            on:click=move |_| ui.select_mode(mode)>
                            <span class="nav-icon" aria-hidden="true">{icon}</span><span>{label}</span>
                        </button>
                    }
                }).collect_view()}
                <Show when=move || ui.setting("mode_navigation")!="header">
                    <button class="nav-row more-modes" aria-expanded=move ||expanded.get().to_string()
                        on:click=move |_|expanded.update(|value|*value=!*value)>
                        <span class="nav-icon" aria-hidden="true"><span class="chevron"></span></span>
                        {move || if expanded.get() {"Réduire les modes"} else {"Autres modes"}}
                    </button>
                </Show>
            </div>
            <div class="nav-group minor">
                <button class="nav-row" class:selected=move || ui.panel.get()=="prompts" on:click=move |_|{ui.panel.set("prompts".into());ui.nav.set(false);}>
                    <span class="nav-icon">"▤"</span>"Prompt"
                </button>
                <button class="nav-row" class:selected=move || ui.panel.get()=="settings" on:click=move |_|ui.settings("general")>
                    <span class="nav-icon">"⚙"</span>"Réglages"
                </button>
            </div>
            <For each={move || ui.snapshot.with(|s|list(s,"plugins")).into_iter()
                    .filter(|p|p["active"]==true && matches!(p["kind"].as_str(),Some("tab"|"wasm"))).collect::<Vec<_>>()}
                key={|p: &Value|format!("{}@{}",p["id"],p["version"])}
                children={move |p: Value| {
                    let id=field(&p,"id");
                    let current=id.clone();
                    view!{<button class="nav-row addon-row" class:selected=move || ui.panel.get()==current on:click=move |_|{ui.panel.set(id.clone());ui.nav.set(false);}>
                        <span class="nav-icon">"◇"</span><span>{field(&p,"title")}</span>
                    </button>}
                }}/>
            <div class="history">
                <div class="history-label">"Conversations"</div>
                <For each={move || ui.snapshot.with(|s|list(s,"sessions"))}
                    key={|r: &Value|field(r,"id")}
                    children={move |r: Value| {
                        let id=field(&r,"id");
                        let title=field(&r,"title");
                        let read_id=id.clone();
                        let preview_title=title.clone();
                        let delete=move |_| {
                            if ui.active(){ui.tell("Arrête d’abord le tour en cours.");return;}
                            let id=id.clone();
                            let confirmed=web_sys::window().and_then(|w|w.confirm_with_message("Supprimer cette conversation et ses pièces jointes ?").ok()).unwrap_or(false);
                            if !confirmed{return;}
                            spawn_local(async move {
                                match invoke("session_delete",json!({"session":id})).await{
                                    Ok(_)=>{
                                        if ui.session.get_untracked()==id{ui.new_chat(&ui.mode.get_untracked());}
                                        ui.refresh().await;
                                    }
                                    Err(e)=>ui.tell(e),
                                }
                            });
                        };
                        view!{
                            <div class="history-row" class:selected=move || ui.session.get()==read_id>
                                <button class="history-open" title=preview_title on:click=move |_|ui.open_chat(r.clone())>{title}</button>
                                <button class="iconbutton history-delete" aria-label="Supprimer la conversation" on:click=delete>"×"</button>
                            </div>
                        }
                    }}/>
            </div>
            <button class="workspace" title=move || ui.setting("workspace") on:click=move |_|ui.settings("general")>
                <span class="workspace-icon">"▱"</span>
                <span>{move || {
                    let path=ui.setting("workspace");
                    if path.is_empty(){"Choisir un dossier".into()}else{path.trim_end_matches(['\\','/']).rsplit(['\\','/']).next().unwrap_or(&path).to_owned()}
                }}</span><span class="tail">"›"</span>
            </button>
        </aside>
    }
}

#[component]
fn Header(ui: Ui, now: RwSignal<f64>) -> impl IntoView {
    view! {
        <header class="header">
            <button class="iconbutton nav-toggle" aria-label="Ouvrir la navigation" on:click=move |_|ui.nav.update(|v|*v=!*v)>"☰"</button>
            <Show when=move ||ui.panel.get()=="conversation" && ui.setting("mode_navigation")=="header"
                  fallback=move ||view!{<div class="header-name">{move || match ui.panel.get().as_str() {
                "settings"=>"Réglages".to_owned(),
                "prompts"=>"Prompt".to_owned(),
                "conversation"=>mode_label(&ui.mode.get()).to_owned(),
                id=>ui.snapshot.with(|s|list(s,"plugins").iter().find(|p|p["id"]==id).map(|p|field(p,"title")).unwrap_or_default()),
            }}</div>}>
                <label class="mode-picker">
                    <span>"Mode"</span>
                    <span class="select-chevron"><select aria-label="Sélecteur de mode" prop:value=move ||ui.mode.get()
                      on:change=move |ev| {
                        let chosen = event_target_value(&ev);
                        if !ui.active() { ui.select_mode(&chosen); }
                      } disabled=move ||ui.active()>
                        {MODES.iter().map(|(mode,label,_)|view!{<option hidden=move ||!ui.mode_available(mode) value=*mode>{*label}</option>}).collect_view()}
                    </select><span class="chevron" aria-hidden="true"></span></span>
                </label>
            </Show>
            <Show when=move || ui.panel.get()=="conversation">
                <button class="model-pill" on:click=move |_|{
                    if matches!(ui.mode.get_untracked().as_str(), "image" | "3d") { ui.settings("models"); }
                    else { ui.picker.set(if ui.picker.get_untracked()=="models" {""} else {"models"}); }
                }>
                    <i class="status-dot" class:connected=move || ui.snapshot.with(|s| {
                        let provider=ui.selected("provider");
                        if ui.mode.get()=="image" {s["packs"][ui.image_profile.get()]["ready"]==true}
                        else if ui.mode.get()=="3d" {s["packs"][ui.mesh_profile.get()]["ready"]==true}
                        else {s["secrets"][provider]==true || ui.selected("provider")=="compatible"}
                    })></i>
                    <span>{move || match ui.mode.get().as_str() {
                        "image"=>match ui.image_profile.get().as_str() {"quality"=>"FLUX.2 · Local".into(),"zimage"=>"Z-Image Turbo · Local".into(),_=>"SDXS · Local".into()},
                        "3d"=>if ui.mesh_profile.get()=="shape" {"Shap-E · Local"} else {"TRELLIS.2 · Local"}.into(),
                        _=>{
                            let model=ui.selected("model");
                            if model.is_empty(){"Choisir un modèle".into()}else{model}
                        }
                    }}</span><span class="chevron" aria-hidden="true"></span>
                </button>
            </Show>
            <div class="top-actions">
                <For each={move || ui.snapshot.with(|s|list(s,"plugins")).into_iter()
                    .filter(|p|p["active"]==true && p["kind"]=="timer").collect::<Vec<_>>()}
                    key={|p: &Value|format!("{}@{}:{}",p["id"],p["version"],p["seconds"])}
                    children={move |p: Value|view!{<Timer title={field(&p,"title")} duration={p["seconds"].as_u64().unwrap_or(600)} now=now/>}}
                />
                <button class="iconbutton" title="Réglages" aria-label="Réglages" on:click=move |_|ui.settings("general")>"⚙"</button>
            </div>
        </header>
    }
}

#[component]
fn Timer(title: String, duration: u64, now: RwSignal<f64>) -> impl IntoView {
    let end = RwSignal::new(None::<f64>);
    let bank = RwSignal::new(duration);
    let remaining = move || match end.get() {
        Some(end) => ((end - now.get()).max(0.0) / 1000.0).ceil() as u64,
        None => bank.get(),
    };
    let toggle = move |_| {
        if end.get_untracked().is_some() && remaining() > 0 {
            bank.set(remaining());
            end.set(None);
        } else {
            let amount = if remaining() == 0 {
                duration
            } else {
                remaining()
            };
            bank.set(amount);
            end.set(Some(browser_now() + amount as f64 * 1000.0));
        }
    };
    view! {
        <div class="timer" title=title>
            <button class="timer-toggle" on:click=toggle aria-label={move || if end.get().is_some() && remaining()>0{"Mettre le minuteur en pause"}else{"Démarrer le minuteur"}}>
                <span>{move || if end.get().is_some() && remaining()>0{"Ⅱ"}else{"▷"}}</span>
                <strong>{move || format!("{:02}:{:02}",remaining()/60,remaining()%60)}</strong>
            </button>
            <button class="iconbutton" aria-label="Réinitialiser le minuteur" on:click=move |_|{end.set(None);bank.set(duration);}>"↺"</button>
        </div>
    }
}

#[component]
fn Conversation(ui: Ui) -> impl IntoView {
    view! {
        <div class="conversation">
            <div id="thread" class="thread" aria-label="Conversation">
                <div class="thread-content">
                    <Show when=move || ui.mode.get()=="bot"><Mascot ui=ui/></Show>
                    <Show when=move || ui.messages.with(|m|m.is_empty()) && !ui.active()><Welcome ui=ui/></Show>
                    {move || ui.messages.get().into_iter().map(|message|view!{<MessageRow ui=ui message=message/>}).collect_view()}
                    <Show when=move || ui.mode.get()=="bot" && ui.voice_ready.get() && !ui.active()>
                        <button class="button compact voice-replay" on:click=move |_| {
                            if ui.playing.get_untracked() { client::stop_voice(); } else { ui.replay(); }
                        }>{move || if ui.playing.get() {"■ Arrêter la voix"} else {"↻ Réécouter la dernière réponse"}}</button>
                    </Show>
                    <Show when=move || ui.active()>
                        <div class="message assistant stream">
                            <div class="message-badge"><span class="mini-mark">"◕"</span>"Sillune"</div>
                            <div class="markdown" on:click=link_click inner_html=move || markdown(&ui.job.with(|j|field(j,"text")))></div>
                            <p class="working" role="status"><span class="pulse"></span>
                                {move || {
                                    let value=ui.job.with(|j|field(j,"detail"));
                                    if value.is_empty(){"Préparation…".into()}else{value}
                                }}
                            </p>
                        </div>
                    </Show>
                    <Show when=move || ui.job.with(|j|!j["approval"].is_null())>
                        <div class="approval" role="group" aria-label="Autorisation requise">
                            <div class="approval-title"><span>"◇"</span>{move || ui.job.with(|j|field(&j["approval"],"label"))}</div>
                            <pre>{move || ui.job.with(|j|field(&j["approval"],"detail"))}</pre>
                            <div class="button-row">
                                <button class="button" on:click=move |_|ui.approval(false)>"Refuser"</button>
                                <button class="button primary" on:click=move |_|ui.approval(true)>"Autoriser"</button>
                            </div>
                        </div>
                    </Show>
                    <Show when=move || ui.job.with(|j|j["done"]==true && j["error"].as_str().is_some_and(|s|!s.is_empty()))>
                        <aside class="turn-error" role="alert">
                            <p>{move ||ui.job.with(|j|field(j,"error"))}</p>
                            <Show when=move ||ui.job.with(|j|j["credentials"]==true)>
                                <button class="button compact" on:click=move |_|ui.settings("models")>"Ouvrir les réglages du fournisseur"</button>
                            </Show>
                        </aside>
                    </Show>
                    <Show when={move || ui.job.with(|j|!list(j,"events").is_empty() || j["usage"]["input"].as_u64().unwrap_or(0)>0)}>
                        <details class="events">
                            <summary>"Détail du tour" <span>{move || ui.job.with(|j| {
                                let count=j["usage"]["input"].as_u64().unwrap_or(0)+j["usage"]["output"].as_u64().unwrap_or(0);
                                if count==0 {String::new()} else {format!("{count} tokens")}
                            })}</span></summary>
                            {move || ui.job.with(|j|list(j,"events")).into_iter().map(|event|view!{<p>{event.as_str().unwrap_or_default().to_owned()}</p>}).collect_view()}
                            <p>{move || ui.job.with(|j|format!("Entrée {} · Sortie {} · Cache {}",j["usage"]["input"].as_u64().unwrap_or(0),j["usage"]["output"].as_u64().unwrap_or(0),j["usage"]["cached"].as_u64().unwrap_or(0)))}</p>
                        </details>
                    </Show>
                    <Show when=move || ui.undo.get() && !ui.active()>
                        <button class="button undo" on:click=move |_|ui.revert()>"↶  Annuler les modifications du tour"</button>
                    </Show>
                </div>
            </div>
            <Composer ui=ui/>
        </div>
    }
}

#[component]
fn Welcome(ui: Ui) -> impl IntoView {
    let content = move || match ui.mode.get().as_str() {
        "agent" => (
            "Qu’est-ce qu’on fait ?",
            "Confie une tâche à Sillune dans ton dossier.",
            "Examiner ce dossier",
            "Lis le contenu du dossier et explique son organisation.",
            "Créer un fichier",
            "Crée une courte liste de tâches dans notes.md.",
        ),
        "image" => (
            "Imagine une image.",
            "Décris le sujet, le style et la lumière. L’image est générée sur ton PC.",
            "Paysage de nuit",
            "Wide cinematic photo of a small observatory under a starry sky, purple light",
            "Illustration",
            "Pastel illustration of a moon-shaped bookshop, amber windows, light snow",
        ),
        "bot" => (
            "Je t’écoute.",
            "Parle au micro ou écris à Sillune.",
            "Parler",
            "",
            "Lire un WAV",
            "",
        ),
        "3d" => (
            "Crée un objet en 3D.",
            "Décris un objet ou joins une image. Le résultat s’ouvre en GLB.",
            "Petit robot",
            "A small red toy robot with a round head and two blue eyes",
            "Objet stylisé",
            "A stylized green cactus in a terracotta pot, simple and colorful",
        ),
        "plugin" => (
            "Étends ton Sillune.",
            "Décris un plugin. Sillune l’installe dans ton interface.",
            "Minuteur",
            "Crée un minuteur de 10 minutes en haut à droite de l’application.",
            "Onglet Notes",
            "Crée un nouvel onglet Notes dans Sillune avec une courte liste de rappels.",
        ),
        "themes" => (
            "Donne une couleur à Sillune.",
            "Décris une palette. Sillune te propose de l’appliquer et l’ajoute à tes réglages.",
            "Encre bleue",
            "Crée un thème sombre Encre bleue, avec un fond bleu de nuit et des accents bleus glacés.",
            "Papier ivoire",
            "Crée un thème clair Papier ivoire, avec des couleurs chaudes et des accents verts.",
        ),
        _ => (
            "Par quoi on commence ?",
            "Écris, explore le web ou ouvre un fichier de travail.",
            "Rechercher sur le web",
            "Recherche deux sources et résume les différences entre ",
            "Créer une page HTML",
            "Prépare une page HTML autonome qui présente ",
        ),
    };
    view! {
        <div class="welcome" class:bot-welcome=move || ui.mode.get()=="bot">
            <Show when=move || ui.mode.get()!="bot"><div class="hero-mark"><i></i><b></b></div></Show>
            <h1>{move || content().0}</h1>
            <p>{move || content().1}</p>
            <div class="suggestions">
                <button class="suggestion" on:click=move |_|if ui.mode.get_untracked()=="bot"{ui.microphone()}else{ui.composer.set(content().3.into());}>
                    <span class="suggestion-icon">"↗"</span>{move || content().2}
                </button>
                <button class="suggestion" on:click=move |_|if ui.mode.get_untracked()=="bot"{ui.import_audio()}else{ui.composer.set(content().5.into());}>
                    <span class="suggestion-icon">"+" </span>{move || content().4}
                </button>
            </div>
        </div>
    }
}

#[component]
fn Mascot(ui: Ui) -> impl IntoView {
    let phase = move || {
        if ui.mic.get() {
            "listening".to_owned()
        } else if ui.playing.get() {
            "speaking".to_owned()
        } else {
            ui.job.with(|job| field(job, "phase"))
        }
    };
    view! {
        <div class="mascot-stage" data-state=phase>
            <div class="star star-a"></div><div class="star star-b"></div><div class="star star-c"></div>
            <div class="halo halo-outer"></div><div class="halo halo-inner"></div>
            <div class="mascot">
                <div class="ear left"></div><div class="ear right"></div>
                <div class="face">
                    <div class="eyes"><i></i><i></i></div>
                    <div class="blush left"></div><div class="blush right"></div><div class="mouth"></div>
                </div>
            </div>
            <div class="mascot-status">
                <i></i>
                {move || match phase().as_str() {"listening"=>"À l’écoute","thinking"|"working"=>"Réfléchit","speaking"=>"Parle","approval"=>"Attend ta décision",_=>"Sillune"}}
            </div>
        </div>
    }
}

#[component]
fn Composer(ui: Ui) -> impl IntoView {
    view! {
        <div class="composer-area">
            <div class="composer">
                <Show when=move || !ui.attachments.with(|a|a.is_empty())>
                    <div class="composer-files">{move || ui.attachments.get().into_iter().map(|file| {
                        let id=field(&file,"id");
                        view!{<button class="file-chip" on:click=move |_|ui.attachments.update(|files|files.retain(|f|f["id"]!=id))>{field(&file,"name")} " ×"</button>}
                    }).collect_view()}</div>
                </Show>
                <textarea rows="2" aria-label="Message"
                    placeholder=move || match ui.mode.get().as_str() {"image"=>"Décris une image…","3d"=>"Décris ton objet 3D…","plugin"=>"Décris un plugin…","themes"=>"Décris le thème que tu veux…","bot"=>"Parle ou écris à Sillune…",_=>"Écrire à Sillune…"}
                    prop:value=move || ui.composer.get()
                    on:input=move |ev|ui.composer.set(event_target_value(&ev))
                    on:keydown=move |ev:web_sys::KeyboardEvent|if ev.key()=="Enter" && !ev.shift_key() && !ev.is_composing(){ev.prevent_default();ui.send(None);}>
                </textarea>
                <div class="composer-tools">
                    <Show when=move || ui.mode.get()!="image" && (ui.mode.get()!="3d" || ui.mesh_profile.get()=="trellis")>
                        <button class="iconbutton" aria-label=move || if ui.mode.get()=="3d" {"Joindre une image PNG ou JPEG"} else {"Joindre un fichier"}
                            title=move || if ui.mode.get()=="3d" {"Joindre une image PNG ou JPEG"} else {"Joindre un fichier"}
                            on:click=move |_|ui.attach()>"＋"</button>
                    </Show>
                    <Show when=move || matches!(ui.mode.get().as_str(),"chat"|"agent"|"bot")>
                        <button class="picker-button web-toggle" class:current=move ||ui.web.get()
                            aria-pressed=move ||ui.web.get().to_string() title="Lancer une recherche web pour ce message"
                            on:click=move |_|ui.web.update(|enabled|*enabled=!*enabled)>"◎ Web"</button>
                    </Show>
                    <Show when=move || matches!(ui.mode.get().as_str(),"agent"|"bot"|"plugin"|"themes")>
                        <select class="flat-select" aria-label="Niveau de permission" prop:value=move || ui.setting("permission") on:change=move |ev|{
                            let mut settings=ui.snapshot.with_untracked(|s|s["settings"].clone());
                            settings["permission"]=json!(event_target_value(&ev));
                            client::commit_settings(ui,settings);
                        }>
                            <option value="ask">"Toujours demander"</option>
                            <option value="safe">"Faible risque"</option>
                            <option value="all">"Tout approuver"</option>
                        </select>
                    </Show>
                    <Show when=move || ui.mode.get()=="image">
                        <span class="select-chevron"><select class="flat-select" aria-label="Modèle d’image" prop:value=move ||ui.image_profile.get() on:change=move |ev|ui.image_profile.set(event_target_value(&ev))>
                            <option value="image">"Rapide · SDXS"</option><option value="quality">"Qualité · FLUX.2"</option>
                            <option value="zimage">"Z-Image Turbo"</option>
                        </select><span class="chevron" aria-hidden="true"></span></span>
                        <select class="flat-select" aria-label="Format de l’image" prop:value=move ||ui.size.get() on:change=move |ev|ui.size.set(event_target_value(&ev))>
                            <option value="square">"Carré"</option><option value="wide">"Paysage"</option><option value="tall">"Portrait"</option>
                            <option value="medium_square">"Carré 768 px"</option>
                            <option value="large" disabled=move ||ui.image_profile.get()=="image">"Carré 1024 px"</option>
                        </select>
                    </Show>
                    <Show when=move || ui.mode.get()=="3d">
                        <span class="select-chevron"><select class="flat-select" aria-label="Modèle 3D" prop:value=move ||ui.mesh_profile.get()
                            on:change=move |ev| {
                                let model=event_target_value(&ev);
                                if model=="shape" {ui.attachments.set(Vec::new());}
                                ui.mesh_profile.set(model);
                            }>
                            <option value="trellis">"TRELLIS.2"</option>
                            <option value="shape">"Shap-E"</option>
                        </select><span class="chevron" aria-hidden="true"></span></span>
                    </Show>
                    <Show when=move || matches!(ui.mode.get().as_str(),"image"|"3d")>
                        <input class="seed" type="number" min="0" placeholder="Graine" aria-label="Graine" prop:value=move || ui.seed.get() on:input=move |ev|ui.seed.set(event_target_value(&ev))/>
                        <button class="button compact" on:click=move |_|ui.stop_render()>"Arrêter le modèle"</button>
                    </Show>
                    <Show when=move || ui.mode.get()=="bot">
                        <button class="button mic-button" class:listening=move || ui.mic.get() on:click=move |_|ui.microphone()>
                            {move || if ui.mic.get(){"■ Envoyer ma voix"}else{"◉ Micro"}}
                        </button>
                        <button class="iconbutton" aria-label="Lire un fichier WAV" title="Lire un fichier WAV" on:click=move |_|ui.import_audio()>"♫"</button>
                        <button class="iconbutton" aria-label="Couper ou rétablir la voix" on:click=move |_|{
                            ui.quiet.update(|v|*v=!*v);if ui.quiet.get_untracked(){client::stop_voice();}
                        }>{move || if ui.quiet.get(){"◌"}else{"◖))"}}</button>
                    </Show>
                    <div class="spacer"></div>
                    <Show when=move || !matches!(ui.mode.get().as_str(),"image"|"3d")>
                        <model_picker::ModelPicker ui=ui/>
                    </Show>
                    <Show when=move || ui.active() || ui.playing.get() fallback=move || view!{
                        <button class="send" aria-label="Envoyer" disabled=move || ui.composer.with(|s|s.trim().is_empty())
                            && !(ui.mode.get()=="3d" && ui.mesh_profile.get()=="trellis" && !ui.attachments.with(Vec::is_empty))
                            on:click=move |_|ui.send(None)>"↑"</button>
                    }>
                        <button class="send stop" aria-label="Arrêter" on:click=move |_|ui.cancel()>"■"</button>
                    </Show>
                </div>
            </div>
        </div>
    }
}

#[component]
fn MessageRow(ui: Ui, message: Value) -> impl IntoView {
    let user = message["role"] == "user";
    let text = field(&message, "text");
    let files = list(&message, "files");
    view! {
        <article class=if user {"message user"} else {"message assistant"}>
            {if user {view!{<div class="user-bubble">{text.clone()}</div>}.into_any()} else {view!{
                <div class="message-badge"><span class="mini-mark">"◕"</span>"Sillune"</div>
                <div class="markdown" on:click=link_click inner_html=markdown(&text)></div>
            }.into_any()}}
            <div class="files">{files.into_iter().map(|file| {
                if !user && file["name"]=="sillune-voix.wav" && file["media"]=="audio/wav" {
                    let id=field(&file,"id");
                    view!{<button class="button compact" on:click=move |_|{
                        let id=id.clone();
                        spawn_local(async move {
                            if let Ok(value)=invoke("artifact",json!({"id":id})).await {
                                let _=wasm_bindgen_futures::JsFuture::from(client::play_voice(&field(&value,"preview"))).await;
                            }
                        });
                    }>"▶ Réécouter la réponse"</button>}.into_any()
                } else { view!{<Attachment ui=ui file=file/>}.into_any() }
            }).collect_view()}</div>
        </article>
    }
}

#[component]
fn Attachment(ui: Ui, file: Value) -> impl IntoView {
    let id = field(&file, "id");
    let show = id.clone();
    let export = id.clone();
    let name = field(&file, "name");
    let kind = field(&file, "media");
    let bitmap = RwSignal::new(String::new());
    if kind.starts_with("image/") {
        spawn_local(async move {
            if let Ok(v) = invoke("artifact", json!({"id":id})).await {
                bitmap.set(field(&v, "preview"));
            }
        });
    }
    view! {
        <div class="attachment">
            <Show when=move || !bitmap.get().is_empty()>
                <img class="attachment-image" src=move || bitmap.get() alt="Image générée"/>
            </Show>
            <div class="attachment-bar">
                <span class="file-symbol">"▧"</span><span class="filename">{name}</span>
                <button class="button compact" on:click=move |_|ui.show_artifact(show.clone())>"Aperçu"</button>
                <button class="iconbutton" aria-label="Enregistrer sous" title="Enregistrer sous" on:click=move |_|ui.export(export.clone())>"↓"</button>
            </div>
        </div>
    }
}

#[component]
fn Preview(ui: Ui) -> impl IntoView {
    view! {
        <Show when=move || ui.preview.with(|v|!v.is_null())>
            <div class="modal-shade" on:click=move |_|ui.preview.set(Value::Null)>
                <section class="modal" role="dialog" aria-modal="true" aria-label="Aperçu de la pièce jointe" on:click=move |ev|ev.stop_propagation()>
                    <div class="modal-head">
                        <h2>{move || ui.preview.with(|v|field(&v["file"],"name"))}</h2>
                        <button class="button" on:click=move |_|ui.export(ui.preview.with_untracked(|v|field(&v["file"],"id")))>"Enregistrer sous"</button>
                        <button class="iconbutton" aria-label="Fermer l’aperçu" on:click=move |_|ui.preview.set(Value::Null)>"×"</button>
                    </div>
                    <div class="modal-body">
                        {move || ui.preview.with(|value| {
                            let media=field(&value["file"],"media");let source=field(value,"preview");
                            match media.as_str() {
                                "text/html"=>view!{<iframe title="Aperçu HTML isolé" sandbox="" srcdoc=safe_document(&source)></iframe>}.into_any(),
                                "model/gltf-binary"=>view!{<mesh_view::MeshView source=source/>}.into_any(),
                                x if x.starts_with("image/")=>view!{<img class="preview-img" src=source alt="Aperçu de l’image"/>}.into_any(),
                                x if x.starts_with("audio/")=>view!{<audio controls src=source></audio>}.into_any(),
                                x if x.starts_with("video/")=>view!{<video controls src=source></video>}.into_any(),
                                x if x.starts_with("text/")=>view!{<pre>{source}</pre>}.into_any(),
                                _=>view!{<p>"Enregistre ce fichier pour l’ouvrir dans une application compatible."</p>}.into_any(),
                            }
                        })}
                    </div>
                </section>
            </div>
        </Show>
    }
}

#[component]
fn ExtensionPage(ui: Ui) -> impl IntoView {
    let value = RwSignal::new("0".to_owned());
    let result = RwSignal::new(String::new());
    view! {
        <section class="page">
            {move || {
                let id=ui.panel.get();
                let entry=ui.snapshot.with(|s|list(s,"plugins").into_iter().find(|p|p["id"]==id && p["active"]==true));
                let Some(entry)=entry else{return view!{<p>"Extension inactive."</p>}.into_any();};
                let body=field(&entry,"html");
                let kind=field(&entry,"kind");
                view! {
                    <h1>{field(&entry,"title")}</h1>
                    <p class="extension-text">{field(&entry,"text")}</p>
                    {if body.is_empty(){().into_any()}else{view!{<iframe class="extension-frame" sandbox="" title="Contenu du plugin" srcdoc=safe_document(&body)></iframe>}.into_any()}}
                    {if kind=="wasm" {
                        view!{<div class="wasm-controls">
                            <label>"Valeur"<input type="number" prop:value=move ||value.get() on:input=move |ev|value.set(event_target_value(&ev))/></label>
                            <button class="button primary" on:click=move |_|{
                                let id=id.clone();
                                spawn_local(async move{
                                    match invoke("plugin_tick",json!({"id":id,"elapsed":0,"value":value.get_untracked().parse::<i32>().unwrap_or(0)})).await{
                                        Ok(v)=>result.set(v.to_string()),Err(e)=>ui.tell(e),
                                    }
                                });
                            }>"Exécuter"</button>
                            <output>{move ||result.get()}</output>
                        </div>}.into_any()
                    }else{().into_any()}}
                }.into_any()
            }}
        </section>
    }
}

#[component]
fn PromptPage(ui: Ui) -> impl IntoView {
    let index = RwSignal::new(None::<usize>);
    let show = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let text = RwSignal::new(String::new());
    let submit = move |_| {
        let mut prompts = ui.snapshot.with_untracked(|s| list(s, "prompts"));
        let item = json!({"name":name.get_untracked().trim(),"text":text.get_untracked()});
        if let Some(id) = index.get_untracked() {
            if let Some(row) = prompts.get_mut(id) {
                *row = item;
            }
        } else {
            prompts.push(item);
        }
        spawn_local(async move {
            match invoke("prompts_save", json!({"prompts":prompts})).await {
                Ok(v) => {
                    ui.snapshot.set(v);
                    show.set(false);
                }
                Err(e) => ui.tell(e),
            }
        });
    };
    view! {
        <section class="page">
            <div class="section-head">
                <h1>"Prompt"</h1>
                <button class="button primary" on:click=move |_|{index.set(None);name.set(String::new());text.set(String::new());show.set(true);}>"＋ Ajouter un prompt"</button>
            </div>
            <Show when=move ||show.get()>
                <div class="form-card">
                    <label>"Nom"<input prop:value=move ||name.get() on:input=move |ev|name.set(event_target_value(&ev)) placeholder="Nom du prompt"/></label>
                    <label>"Texte"<textarea rows="7" prop:value=move ||text.get() on:input=move |ev|text.set(event_target_value(&ev)) placeholder="Écris le prompt…"></textarea></label>
                    <div class="button-row"><button class="button" on:click=move |_|show.set(false)>"Annuler"</button><button class="button primary" on:click=submit>"Enregistrer"</button></div>
                </div>
            </Show>
            <Show when=move ||ui.snapshot.with(|s|list(s,"prompts").is_empty()) && !show.get()>
                <p class="empty-page">"Tes prompts enregistrés apparaîtront ici."</p>
            </Show>
            {move ||ui.snapshot.with(|s|list(s,"prompts")).into_iter().enumerate().map(|(i,item)|{
                let item_name=field(&item,"name");
                let item_text=field(&item,"text");
                let copy=item_text.clone();
                let edit=item_text.clone();
                let edit_name=item_name.clone();
                view!{
                    <article class="form-card prompt-item">
                        <h2>{item_name}</h2><p>{item_text}</p>
                        <div class="button-row">
                            <button class="button" on:click=move |_|{copy_text(copy.clone());ui.tell("Prompt copié.");}>"Copier"</button>
                            <button class="button" on:click=move |_|{index.set(Some(i));name.set(edit_name.clone());text.set(edit.clone());show.set(true);}>"Modifier"</button>
                            <button class="button danger" on:click=move |_|{
                                let mut rows=ui.snapshot.with_untracked(|s|list(s,"prompts"));
                                if i<rows.len(){rows.remove(i);}
                                spawn_local(async move {match invoke("prompts_save",json!({"prompts":rows})).await{Ok(v)=>ui.snapshot.set(v),Err(e)=>ui.tell(e)}});
                            }>"Supprimer"</button>
                        </div>
                    </article>
                }
            }).collect_view()}
        </section>
    }
}

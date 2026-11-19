use crate::client::{Ui, commit_settings, field, invoke, list, native, sleep_for};
use leptos::{prelude::*, task::spawn_local};
use serde_json::{Value, json};
use wasm_bindgen_futures::JsFuture;

pub const PROVIDERS: &[(&str, &str, &str)] = &[
    ("openrouter", "OpenRouter", "openrouter/auto"),
    ("openai", "OpenAI", "gpt-5-mini"),
    ("anthropic", "Anthropic", "claude-sonnet-4-6"),
    ("mistral", "Mistral", "mistral-small-latest"),
    ("nous", "Nous Portal", "hermes-4-70b"),
    ("opencode", "OpenCode Console", "deepseek-v4-pro"),
    ("compatible", "API compatible ou locale", ""),
];

#[component]
pub fn SettingsPage(ui: Ui) -> impl IntoView {
    let draft = RwSignal::new(Value::Null);
    let mcp_json = RwSignal::new("[]".to_owned());
    Effect::new(move |_| {
        if draft.with_untracked(Value::is_null)
            && let Some(settings) = ui.snapshot.with(|state| {
                state["settings"]
                    .as_object()
                    .map(|_| state["settings"].clone())
            })
        {
            mcp_json.set(
                serde_json::to_string_pretty(
                    &settings["mcp"].as_array().cloned().unwrap_or_default(),
                )
                .unwrap_or_else(|_| "[]".into()),
            );
            draft.set(settings);
        }
    });
    let save = move |_| {
        let mut settings = draft.get_untracked();
        match serde_json::from_str::<Value>(&mcp_json.get_untracked()) {
            Ok(mcp) if mcp.is_array() => settings["mcp"] = mcp,
            _ => {
                ui.tell("Fournis un tableau JSON valide pour les serveurs MCP.");
                return;
            }
        }
        commit_settings(ui, settings);
    };
    let tabs = [
        ("general", "Général"),
        ("models", "Modèles"),
        ("connectors", "Connecteurs"),
        ("memory", "Mémoire"),
        ("plugins", "Plugins"),
    ];
    view! {
        <section class="page settings-page">
            <div class="section-head"><h1>"Réglages"</h1><button class="button primary" disabled=move ||draft.with(Value::is_null) on:click=save>"Enregistrer"</button></div>
            <nav class="settings-tabs" aria-label="Rubriques de réglage">
                {tabs.into_iter().map(|(id,title)|view!{
                    <button class="tab-button" class:selected=move ||ui.settings_tab.get()==id on:click=move |_|ui.settings_tab.set(id.into())>{title}</button>
                }).collect_view()}
            </nav>
            <Show when=move ||draft.with(Value::is_object) fallback=||view!{<p role="status">"Chargement des réglages…"</p>}>
            {move || match ui.settings_tab.get().as_str() {
                "models"=>view!{<Models ui=ui draft=draft/>}.into_any(),
                "connectors"=>view!{<Connectors ui=ui draft=draft mcp_json=mcp_json/>}.into_any(),
                "memory"=>view!{<Memory ui=ui draft=draft/>}.into_any(),
                "plugins"=>view!{<Plugins ui=ui/>}.into_any(),
                _=>view!{<General ui=ui draft=draft/>}.into_any(),
            }}
            </Show>
        </section>
    }
}

fn set(draft: RwSignal<Value>, key: &str, value: impl Into<Value>) {
    draft.update(|s| s[key] = value.into());
}

#[component]
fn General(ui: Ui, draft: RwSignal<Value>) -> impl IntoView {
    view! {
        <div class="form-card">
            <h2>"Espace de travail"</h2>
            <p class="subtle">"Dossier utilisé lorsque Sillune lit ou modifie tes fichiers."</p>
            <div class="form-line">
                <input aria-label="Dossier de travail" placeholder="Dossier de travail" prop:value=move ||draft.with(|s|field(s,"workspace")) on:input=move |ev|set(draft,"workspace",event_target_value(&ev))/>
                <button class="button" on:click=move |_|choose_folder(ui,draft,"workspace")>"Parcourir"</button>
            </div>
            <label>"Permissions par défaut"
                <select prop:value=move ||draft.with(|s|field(s,"permission")) on:change=move |ev|set(draft,"permission",event_target_value(&ev))>
                    <option value="ask">"Toujours demander"</option><option value="safe">"Approuver les actions à faible risque"</option><option value="all">"Tout approuver"</option>
                </select>
            </label>
        </div>
        <div class="form-card">
            <h2>"Apparence"</h2>
            <label>"Thème"
                <select prop:value=move ||draft.with(|s|field(s,"theme")) on:change=move |ev|set(draft,"theme",event_target_value(&ev))>
                    <option value="system">"Système"</option><option value="dark">"Sombre"</option><option value="light">"Clair"</option>
                    <option value="cobalt">"Cobalt"</option>
                    {move || ui.snapshot.with(|s| s["themes"].as_object().into_iter().flat_map(|themes|themes.values().cloned()).collect::<Vec<_>>())
                         .into_iter().map(|theme|view!{<option value={field(&theme,"id")}>{field(&theme,"name")}</option>}).collect_view()}
                </select>
            </label>
            <label>"Sélecteur de modes"
                <select prop:value=move ||draft.with(|s|field(s,"mode_navigation"))
                    on:change=move |ev|set(draft,"mode_navigation",event_target_value(&ev))>
                    <option value="sidebar">"Barre latérale · Chat, Agent et autres modes"</option>
                    <option value="header">"En haut · Sélecteur de mode"</option>
                </select>
            </label>
        </div>
        <div class="form-card">
            <h2>"Instructions personnelles"</h2>
            <label>"Instructions pour Sillune"
                <textarea rows="7" placeholder="Explique ta façon de travailler…" prop:value=move ||draft.with(|s|field(s,"instructions")) on:input=move |ev|set(draft,"instructions",event_target_value(&ev))></textarea>
            </label>
        </div>
    }
}

#[component]
fn Models(ui: Ui, draft: RwSignal<Value>) -> impl IntoView {
    let available = RwSignal::new(Vec::<Value>::new());
    let provider = Signal::derive(move || draft.with(|s| field(s, "provider")));
    view! {
        <div class="form-card">
            <h2>"Fournisseur et modèle"</h2>
            <div class="fields">
                <label>"Fournisseur"
                    <select prop:value=move ||provider.get() on:change=move |ev|{
                        let value=event_target_value(&ev);
                        let fallback=PROVIDERS.iter().find(|(id,_,_)|*id==value).map(|(_,_,model)|*model).unwrap_or_default();
                        set(draft,"provider",value);set(draft,"model",fallback);available.set(Vec::new());
                    }>
                        {PROVIDERS.iter().map(|(id,name,_)|view!{<option value=*id>{*name}</option>}).collect_view()}
                    </select>
                </label>
                <label>"Identifiant du modèle"
                    <input list="model-list" prop:value=move ||draft.with(|s|field(s,"model")) on:input=move |ev|set(draft,"model",event_target_value(&ev))/>
                    <datalist id="model-list">{move ||available.get().into_iter().map(|m|view!{<option value={field(&m,"id")}>{field(&m,"name")}</option>}).collect_view()}</datalist>
                </label>
            </div>
            <Show when=move ||provider.get()=="compatible">
                <label>"URL d’API compatible"
                    <input placeholder="http://127.0.0.1:11434/v1" prop:value=move ||draft.with(|s|field(s,"url")) on:input=move |ev|set(draft,"url",event_target_value(&ev))/>
                </label>
            </Show>
            <SecretField ui=ui name=provider label="Clé API"/>
            <button class="button" on:click=move |_|{
                let settings=draft.get_untracked();
                spawn_local(async move {
                    match invoke("models",json!({"provider":settings["provider"],"url":settings["url"],"refresh":true})).await{
                        Ok(v)=>{available.set(v.as_array().cloned().unwrap_or_default());ui.tell(format!("{} modèles disponibles.",available.get_untracked().len()));}
                        Err(e)=>ui.tell(e),
                    }
                });
            }>"Récupérer la liste des modèles"</button>
        </div>
        <div class="form-card">
            <h2>"Voix de Sillune"</h2>
            <label>"Synthèse vocale du Bot"
                <select prop:value=move ||draft.with(|s|field(s,"voice")) on:change=move |ev|set(draft,"voice",event_target_value(&ev))>
                    <option value="local-female">"LFM3 locale · voix féminine"</option>
                    <option value="local-male">"LFM3 locale · voix masculine"</option>
                    <option value="openai-marin">"OpenAI · Marin"</option>
                    <option value="openai-cedar">"OpenAI · Cedar"</option>
                </select>
            </label>
            <p class="subtle">"Le modèle choisi dans le chat rédige la réponse. La voix choisie ici la lit. Le micro utilise le pack LFM3 pour la transcription locale."</p>
            <Show when=move ||draft.with(|s|field(s,"voice").starts_with("openai-"))>
                <p class="subtle">"La voix OpenAI envoie le texte de la réponse à OpenAI et utilise les crédits de ta clé."</p>
                <Show when=move ||provider.get()!="openai">
                    <SecretField ui=ui name={Signal::derive(||"openai".into())} label="Clé OpenAI pour la voix"/>
                </Show>
            </Show>
        </div>
        <div class="form-card">
            <h2>"Modèles locaux"</h2>
            <Pack ui=ui id="image" title="Image · SDXS-512-DreamShaper"/>
            <Pack ui=ui id="quality" title="Image · FLUX.2 klein 4B quantifié"/>
            <Pack ui=ui id="zimage" title="Image · Z-Image Turbo Q4"/>
            <Pack ui=ui id="shape" title="3D · Shap-E"/>
            <p class="subtle">"Sillune prépare Python et les dépendances de Shap-E lors de l’installation."</p>
            <Pack ui=ui id="voice" title="Bot · LFM3-Audio-2B français"/>
            <details class="advanced">
                <summary>"Dossier des moteurs et modèles"</summary>
                <label>"Dossier Runtime"
                    <div class="form-line">
                        <input prop:value=move ||draft.with(|s|field(s,"engines")) placeholder="Dossier automatique" on:input=move |ev|set(draft,"engines",event_target_value(&ev))/>
                        <button class="button" on:click=move |_|choose_folder(ui,draft,"engines")>"Parcourir"</button>
                    </div>
                </label>
            </details>
        </div>
    }
}

fn choose_folder(ui: Ui, draft: RwSignal<Value>, key: &'static str) {
    spawn_local(async move {
        match native("choose_folder", json!({})).await {
            Ok(v) => {
                if let Some(path) = v.as_str() {
                    set(draft, key, path);
                }
            }
            Err(e) => ui.tell(e),
        }
    });
}

#[component]
fn SecretField(ui: Ui, name: Signal<String>, label: &'static str) -> impl IntoView {
    let pending = RwSignal::new(String::new());
    Effect::new(move |_| {
        let _ = name.get();
        pending.set(String::new());
    });
    let save = move |clear: bool| {
        let id = name.get_untracked();
        let value = if clear {
            String::new()
        } else {
            pending.get_untracked()
        };
        if !clear && value.trim().is_empty() {
            ui.tell("Saisis la clé avant de l’enregistrer.");
            return;
        }
        spawn_local(async move {
            match invoke("secret", json!({"name":id,"value":value})).await {
                Ok(v) => {
                    ui.snapshot.set(v);
                    pending.set(String::new());
                    ui.tell(if clear {
                        "Clé retirée."
                    } else {
                        "Clé enregistrée dans Windows."
                    });
                }
                Err(e) => ui.tell(e),
            }
        });
    };
    view! {
        <div class="secret-row">
            <label>{label}
                <div class="form-line">
                    <input type="password" autocomplete="off" aria-label=label placeholder=move ||if ui.snapshot.with(|s|s["secrets"][name.get()]==true){"Clé enregistrée"}else{"Saisir la clé"}
                        prop:value=move ||pending.get() on:input=move |ev|pending.set(event_target_value(&ev))/>
                    <button class="button" on:click=move |_|save(false)>"Enregistrer la clé"</button>
                    <button class="button" on:click=move |_|save(true)>"Retirer"</button>
                </div>
            </label>
        </div>
    }
}

#[component]
fn Pack(ui: Ui, id: &'static str, title: &'static str) -> impl IntoView {
    let status = RwSignal::new(Value::Null);
    let downloading = move || status.with(|v| !v.is_null() && v["done"] != true);
    let start = move |_| {
        status.set(json!({"done":false,"detail":"Préparation…","progress":0.0}));
        spawn_local(async move {
            let session = match invoke("download", json!({"pack":id})).await {
                Ok(v) => field(&v, "session"),
                Err(e) => {
                    ui.tell(e);
                    let _ = status.try_set(Value::Null);
                    return;
                }
            };
            loop {
                let value = invoke("poll", json!({"session":session})).await;
                match value {
                    Ok(v) => {
                        let finished = v["done"] == true;
                        let _ = status.try_set(v.clone());
                        if finished {
                            let error = field(&v, "error");
                            ui.tell(if error.is_empty() {
                                "Pack local prêt.".into()
                            } else {
                                error
                            });
                            ui.refresh().await;
                            return;
                        }
                    }
                    Err(e) => {
                        ui.tell(e);
                        let _ = status.try_set(Value::Null);
                        return;
                    }
                }
                let _ = JsFuture::from(sleep_for(300)).await;
            }
        });
    };
    view! {
        <article class="pack">
            <div class="pack-row">
                <div>
                    <h3>{title}</h3>
                    <span class="subtle">{move ||ui.snapshot.with(|s|{
                        if s["packs"][id]["ready"]==true {"Prêt sur ce PC".into()}
                        else{format!("{:.2} Go à télécharger",s["packs"][id]["missing"].as_u64().unwrap_or(0) as f64/1_000_000_000.0)}
                    })}</span>
                </div>
                <Show when=move ||ui.snapshot.with(|s|s["packs"][id]["ready"]!=true)>
                    <Show when=downloading fallback=move ||view!{<button class="button" on:click=start>"Installer le pack"</button>}>
                        <button class="button" on:click=move |_|spawn_local(async move {let _=invoke("cancel",json!({"session":format!("pack-{id}")})).await;})>"Arrêter"</button>
                    </Show>
                </Show>
            </div>
            <Show when=downloading>
                <progress max="1" prop:value=move ||status.with(|v|v["progress"].as_f64().unwrap_or(0.0))></progress>
                <p class="subtle">{move ||status.with(|v|field(v,"detail"))}</p>
            </Show>
            <details class="advanced">
                <summary>"Provenance et empreintes"</summary>
                <button class="text-link" on:click=move |_|{
                    let address=ui.snapshot.with_untracked(|s|field(&s["packs"][id],"license"));
                    spawn_local(async move {let _=native("open_link",json!({"url":address})).await;});
                }>"Voir la licence"</button>
                {move ||ui.snapshot.with(|s|list(&s["packs"][id],"files")).into_iter().map(|file|view!{
                    <p class="digest"><strong>{field(&file,"name")}</strong><br/>{field(&file,"sha")}</p>
                }).collect_view()}
            </details>
        </article>
    }
}

#[component]
fn Connectors(ui: Ui, draft: RwSignal<Value>, mcp_json: RwSignal<String>) -> impl IntoView {
    let mcp_name = RwSignal::new(String::new());
    view! {
        <div class="form-card">
            <h2>"Recherche web"</h2>
            <label>"Moteur"
                <select prop:value=move ||draft.with(|s|field(s,"search")) on:change=move |ev|set(draft,"search",event_target_value(&ev))>
                    <option value="openrouter">"OpenRouter Web"</option><option value="duckduckgo">"DuckDuckGo"</option><option value="searx">"SearXNG"</option>
                </select>
            </label>
            <Show when=move ||draft.with(|s|s["search"]=="searx") fallback=move ||view!{<p class="subtle">{move || if draft.with(|s|s["search"]=="openrouter") {"OpenRouter utilise ta clé quand elle est disponible. DuckDuckGo sert de solution de secours."} else {"DuckDuckGo ne nécessite pas de clé API."}}</p>}>
                <label>"Instance SearXNG"
                    <input placeholder="https://recherche.exemple" prop:value=move ||draft.with(|s|field(s,"searx_url")) on:input=move |ev|set(draft,"searx_url",event_target_value(&ev))/>
                </label>
            </Show>
        </div>
        <div class="form-card">
            <h2>"GitHub"</h2>
            <p class="subtle">"Jeton limité aux dépôts et aux actions que tu veux exposer."</p>
            <SecretField ui=ui name=Signal::derive(||"github".into()) label="Jeton GitHub"/>
        </div>
        <div class="form-card">
            <h2>"Google Drive"</h2>
            <DriveConnection ui=ui/>
        </div>
        <div class="form-card">
            <h2>"Serveurs MCP"</h2>
            <label>"Configuration JSON"
                <textarea class="mono" rows="7" prop:value=move ||mcp_json.get() on:input=move |ev|mcp_json.set(event_target_value(&ev))></textarea>
            </label>
            <details class="advanced"><summary>"Exemples de configuration"</summary>
                <pre>{r#"[{"name":"github","url":"https://api.githubcopilot.com/mcp/"}]
    [{"name":"local","command":"mon-serveur-mcp.exe","args":[]}]"#}</pre>
            </details>
            <label>"Nom du serveur pour l’authentification"
                <input placeholder="github" prop:value=move ||mcp_name.get() on:input=move |ev|mcp_name.set(event_target_value(&ev))/>
            </label>
            <Show when=move ||!mcp_name.get().trim().is_empty()>
                <SecretField ui=ui name=Signal::derive(move ||format!("mcp-{}",mcp_name.get().trim())) label="Jeton MCP"/>
            </Show>
        </div>
    }
}

#[component]
fn DriveConnection(ui: Ui) -> impl IntoView {
    let id = RwSignal::new(String::new());
    let secret = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let connect = move |_| {
        busy.set(true);
        let data =
            json!({"client":id.get_untracked().trim(),"secret":secret.get_untracked().trim()});
        secret.set(String::new());
        spawn_local(async move {
            let completed = async {
                let flow = invoke("drive_connect", data).await?;
                let url = field(&flow, "url");
                let session = field(&flow, "session");
                native("open_link", json!({"url":url})).await?;
                loop {
                    let job = invoke("poll", json!({"session":session})).await?;
                    if job["done"] == true {
                        let error = field(&job, "error");
                        if !error.is_empty() {
                            return Err(error);
                        }
                        ui.refresh().await;
                        return Ok(());
                    }
                    let _ = JsFuture::from(sleep_for(400)).await;
                }
            }
            .await;
            let _ = busy.try_set(false);
            match completed {
                Ok(_) => ui.tell("Google Drive connecté."),
                Err(e) => ui.tell(e),
            }
        });
    };
    view! {
        <p class="subtle">{move || if ui.snapshot.with(|s|s["drive_connected"]==true){"Compte connecté."}
            else {"Connecte un client OAuth Google de type « Application de bureau ». Sillune demande l’accès drive.file."}}</p>
        <label>"Identifiant client Google"
            <input prop:value=move ||id.get() on:input=move |ev|id.set(event_target_value(&ev)) placeholder="…apps.googleusercontent.com"/>
        </label>
        <label>"Secret du client Google"
            <input type="password" autocomplete="off" prop:value=move ||secret.get() on:input=move |ev|secret.set(event_target_value(&ev))/>
        </label>
        <div class="button-row">
            <button class="button" disabled=move || busy.get() || id.with(|s|s.trim().is_empty()) on:click=connect>"Connecter dans le navigateur"</button>
            <Show when=move ||busy.get()>
                <button class="button" on:click=move |_|spawn_local(async move {let _=invoke("cancel",json!({"session":"drive-oauth"})).await;})>"Annuler"</button>
            </Show>
            <Show when=move ||ui.snapshot.with(|s|s["drive_connected"]==true)>
                <button class="button" on:click=move |_|act(ui,"drive_disconnect",json!({}))>"Déconnecter localement"</button>
            </Show>
        </div>
        <details class="advanced">
            <summary>"Utiliser un jeton Drive existant"</summary>
            <SecretField ui=ui name=Signal::derive(||"drive".into()) label="Jeton Google Drive"/>
        </details>
    }
}

#[component]
fn Memory(ui: Ui, draft: RwSignal<Value>) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let kind = RwSignal::new("preference".to_owned());
    let id = RwSignal::new(None::<String>);
    view! {
        <div class="form-card">
            <h2>"Mémoire facultative"</h2>
            <label class="checkbox"><input type="checkbox" prop:checked=move ||draft.with(|s|s["memory"]==true) on:change=move |ev|set(draft,"memory",event_target_checked(&ev))/>"Activer les notes et le rappel des conversations"</label>
            <p class="subtle">"Sillune retient les préférences et les pratiques que tu confirmes. Les notes utiles entrent dans le contexte du fournisseur sélectionné."</p>
        </div>
        <div class="form-card">
            <h2>"Notes mémorisées"</h2>
            {move ||ui.snapshot.with(|s|list(s,"notes")).into_iter().map(|note| {
                let edit=note.clone();
                let note_id=field(&note,"id");
                view!{
                    <div class="note-row">
                        <p>{field(&note,"content")}</p>
                        <button class="button compact" on:click=move |_|{id.set(Some(field(&edit,"id")));text.set(field(&edit,"content"));kind.set(field(&edit,"kind"));}>"Modifier"</button>
                        <button class="button compact" on:click=move |_|act(ui,"note_forget",json!({"id":note_id}))>"Oublier"</button>
                    </div>
                }
            }).collect_view()}
            <label>"Type de note"
                <select prop:value=move ||kind.get() on:change=move |ev|kind.set(event_target_value(&ev))>
                    <option value="preference">"Préférence"</option><option value="fact">"Fait"</option><option value="method">"Méthode"</option>
                </select>
            </label>
            <label>"Contenu"<textarea rows="3" prop:value=move ||text.get() on:input=move |ev|text.set(event_target_value(&ev))></textarea></label>
            <div class="button-row">
                <button class="button primary" disabled=move ||text.with(|v|v.trim().is_empty()) on:click=move |_|{
                    act(ui,"note_save",json!({"id":id.get_untracked(),"kind":kind.get_untracked(),"content":text.get_untracked()}));
                    id.set(None);text.set(String::new());
                }>"Mémoriser"</button>
                <button class="button danger" on:click=move |_|{
                    if web_sys::window().and_then(|w|w.confirm_with_message("Effacer toutes les notes mémorisées ?").ok()).unwrap_or(false){
                        act(ui,"note_forget",json!({}));
                    }
                }>"Effacer les notes"</button>
            </div>
        </div>
        <Show when=move ||!ui.snapshot.with(|s|list(s,"suggestions").is_empty())>
            <div class="form-card"><h2>"Suggestions"</h2>
                {move ||ui.snapshot.with(|s|list(s,"suggestions")).into_iter().enumerate().map(|(i,note)|view!{
                    <div class="note-row">
                        <p>{field(&note,"content")}</p>
                        <button class="button compact" on:click=move |_|act(ui,"note_review",json!({"index":i,"save":true}))>"Mémoriser"</button>
                        <button class="button compact" on:click=move |_|act(ui,"note_review",json!({"index":i,"save":false}))>"Ignorer"</button>
                    </div>
                }).collect_view()}
            </div>
        </Show>
    }
}

#[component]
fn Plugins(ui: Ui) -> impl IntoView {
    view! {
        <div class="form-card">
            <div class="section-head"><h2>"Plugins disponibles"</h2>
                <button class="button primary" on:click=move |_|ui.select_mode("plugin")>"Créer avec Sillune"</button>
            </div>
            {move ||ui.snapshot.with(|s|list(s,"plugins")).into_iter().map(|entry| {
                let id=field(&entry,"id");
                let doc_id=id.clone();
                let remove_id=id.clone();
                let title=field(&entry,"title");
                let doc_title=title.clone();
                let active=entry["active"]==true;
                let bundled=entry["bundled"]==true;
                view!{
                    <article class="plugin-row">
                        <div><h3>{title}</h3><p>{field(&entry,"description")}</p>
                        <small>{format!("plugins/{}/ · {}",field(&entry,"id"),field(&entry,"version"))}</small></div>
                        <div class="button-row">
                            <button class="button compact" on:click=move |_|{
                                let id=doc_id.clone();let title=doc_title.clone();
                                spawn_local(async move {
                                    match invoke("plugin_docs",json!({"id":id})).await {
                                        Ok(content)=>ui.preview.set(json!({"file":{"name":format!("{title} · README.md"),"media":"text/plain","id":""},"preview":content})),
                                        Err(e)=>ui.tell(e),
                                    }
                                });
                            }>"README"</button>
                            {(!bundled).then(||view!{
                                <button class="button compact" on:click=move |_|act(ui,"plugin_active",json!({"id":id,"active":!active}))>{if active{"Désactiver"}else{"Activer"}}</button>
                                <button class="button compact danger" on:click=move |_|{
                                    if web_sys::window().and_then(|w|w.confirm_with_message("Désinstaller ce plugin ?").ok()).unwrap_or(false) {
                                        act(ui,"plugin_remove",json!({"id":remove_id}));
                                    }
                                }>"Désinstaller"</button>
                            })}
                        </div>
                    </article>
                }
            }).collect_view()}
        </div>
        <div class="form-card">
            <h2>"Skills fournis"</h2>
            <p class="subtle">"Ajoute un skill à un message pour charger son guide pendant ce tour."</p>
            {move ||ui.snapshot.with(|s|list(s,"skills")).into_iter().map(|skill| {
                let id=field(&skill,"id");
                let use_id=id.clone();
                let title=field(&skill,"name");
                let doc_title=title.clone();
                view!{
                    <article class="plugin-row">
                        <div><h3>{title}</h3><small>{field(&skill,"license")}</small></div>
                        <div class="button-row">
                            <button class="button compact" on:click=move |_|{
                                let id=id.clone();let name=doc_title.clone();
                                spawn_local(async move {
                                    match invoke("skill_doc",json!({"id":id})).await {
                                        Ok(content)=>ui.preview.set(json!({"file":{"name":name,"media":"text/plain","id":""},"preview":content})),
                                        Err(e)=>ui.tell(e),
                                    }
                                });
                            }>"Lire"</button>
                            <button class="button compact" on:click=move |_|{
                                if !ui.active(){ui.new_chat("agent");ui.composer.set(format!("/skill {use_id} "));}
                            }>"Utiliser"</button>
                        </div>
                    </article>
                }
            }).collect_view()}
        </div>
    }
}

fn act(ui: Ui, op: &'static str, data: Value) {
    spawn_local(async move {
        match invoke(op, data).await {
            Ok(v) => ui.snapshot.set(v),
            Err(e) => ui.tell(e),
        }
    });
}

use crate::client::{Ui, commit_settings, field, invoke, list, native, sleep_for};
use crate::locale::tr;
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

pub fn provider_label<'a>(id: &str, name: &'a str) -> &'a str {
    if id == "compatible" {
        tr("API compatible ou locale", "Compatible or local API")
    } else {
        name
    }
}

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
                ui.tell(tr(
                    "Fournis un tableau JSON valide pour les serveurs MCP.",
                    "Provide a valid JSON array for the MCP servers.",
                ));
                return;
            }
        }
        commit_settings(ui, settings);
    };
    let tabs = [
        ("general", tr("Général", "General")),
        ("models", tr("Modèles", "Models")),
        ("connectors", tr("Connecteurs", "Connectors")),
        ("memory", tr("Mémoire", "Memory")),
        ("plugins", "Plugins"),
    ];
    view! {
        <section class="page settings-page">
            <div class="section-head"><h1>{tr("Réglages", "Settings")}</h1><button class="button primary" disabled=move ||draft.with(Value::is_null) on:click=save>{tr("Enregistrer", "Save")}</button></div>
            <nav class="settings-tabs" aria-label={tr("Rubriques de réglage", "Settings sections")}>
                {tabs.into_iter().map(|(id,title)|view!{
                    <button class="tab-button" class:selected=move ||ui.settings_tab.get()==id on:click=move |_|ui.settings_tab.set(id.into())>{title}</button>
                }).collect_view()}
            </nav>
            <Show when=move ||draft.with(Value::is_object) fallback=||view!{<p role="status">{tr("Chargement des réglages…", "Loading settings…")}</p>}>
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
            <h2>{tr("Espace de travail", "Workspace")}</h2>
            <p class="subtle">{tr("Dossier utilisé lorsque Sillune lit ou modifie tes fichiers.", "Sillune reads and edits files in this folder.")}</p>
            <div class="form-line">
                <input aria-label={tr("Dossier de travail", "Workspace folder")} placeholder={tr("Dossier de travail", "Workspace folder")} prop:value=move ||draft.with(|s|field(s,"workspace")) on:input=move |ev|set(draft,"workspace",event_target_value(&ev))/>
                <button class="button" on:click=move |_|choose_folder(ui,draft,"workspace")>{tr("Parcourir", "Browse")}</button>
            </div>
            <label>{tr("Permissions par défaut", "Default permissions")}
                <select prop:value=move ||draft.with(|s|field(s,"permission")) on:change=move |ev|set(draft,"permission",event_target_value(&ev))>
                    <option value="ask">{tr("Toujours demander", "Always ask")}</option><option value="safe">{tr("Approuver les actions à faible risque", "Allow low-risk actions")}</option><option value="all">{tr("Tout approuver", "Allow everything")}</option>
                </select>
            </label>
        </div>
        <div class="form-card">
            <h2>{tr("Apparence", "Appearance")}</h2>
            <label>{tr("Langue", "Language")}
                <select aria-label={tr("Langue", "Language")} prop:value=move || draft.with(|s|field(s,"language"))
                    on:change=move |ev|set(draft,"language",event_target_value(&ev))>
                    <option value="en">"English"</option>
                    <option value="fr">"Français"</option>
                </select>
            </label>
            <label>{tr("Thème", "Theme")}
                <select prop:value=move ||draft.with(|s|field(s,"theme")) on:change=move |ev|set(draft,"theme",event_target_value(&ev))>
                    <option value="system">{tr("Système", "System")}</option><option value="dark">{tr("Sombre", "Dark")}</option><option value="light">{tr("Clair", "Light")}</option>
                    <option value="cobalt">"Cobalt"</option>
                    {move || ui.snapshot.with(|s| s["themes"].as_object().into_iter().flat_map(|themes|themes.values().cloned()).collect::<Vec<_>>())
                         .into_iter().map(|theme|view!{<option value={field(&theme,"id")}>{field(&theme,"name")}</option>}).collect_view()}
                </select>
            </label>
            <label>{tr("Sélecteur de modes", "Mode selector")}
                <select prop:value=move ||draft.with(|s|field(s,"mode_navigation"))
                    on:change=move |ev|set(draft,"mode_navigation",event_target_value(&ev))>
                    <option value="sidebar">{tr("Barre latérale · Chat, Agent et autres modes", "Sidebar · Chat, Agent and more")}</option>
                    <option value="header">{tr("En haut · Sélecteur de mode", "Top bar · Mode selector")}</option>
                </select>
            </label>
        </div>
        <div class="form-card">
            <h2>{tr("Instructions personnelles", "Custom instructions")}</h2>
            <label>{tr("Instructions pour Sillune", "Instructions for Sillune")}
                <textarea rows="7" placeholder={tr("Explique ta façon de travailler…", "Describe how you like to work…")} prop:value=move ||draft.with(|s|field(s,"instructions")) on:input=move |ev|set(draft,"instructions",event_target_value(&ev))></textarea>
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
            <h2>{tr("Fournisseur et modèle", "Provider and model")}</h2>
            <div class="fields">
                <label>{tr("Fournisseur", "Provider")}
                    <select prop:value=move ||provider.get() on:change=move |ev|{
                        let value=event_target_value(&ev);
                        let fallback=PROVIDERS.iter().find(|(id,_,_)|*id==value).map(|(_,_,model)|*model).unwrap_or_default();
                        set(draft,"provider",value);set(draft,"model",fallback);available.set(Vec::new());
                    }>
                        {PROVIDERS.iter().map(|(id,name,_)|view!{<option value=*id>{provider_label(id,name)}</option>}).collect_view()}
                    </select>
                </label>
                <label>{tr("Identifiant du modèle", "Model ID")}
                    <input list="model-list" prop:value=move ||draft.with(|s|field(s,"model")) on:input=move |ev|set(draft,"model",event_target_value(&ev))/>
                    <datalist id="model-list">{move ||available.get().into_iter().map(|m|view!{<option value={field(&m,"id")}>{field(&m,"name")}</option>}).collect_view()}</datalist>
                </label>
            </div>
            <Show when=move ||provider.get()=="compatible">
                <label>{tr("URL d’API compatible", "Compatible API URL")}
                    <input placeholder="http://127.0.0.1:11434/v1" prop:value=move ||draft.with(|s|field(s,"url")) on:input=move |ev|set(draft,"url",event_target_value(&ev))/>
                </label>
            </Show>
            <SecretField ui=ui name=provider label={tr("Clé API", "API key")}/>
            <button class="button" on:click=move |_|{
                let settings=draft.get_untracked();
                spawn_local(async move {
                    match invoke("models",json!({"provider":settings["provider"],"url":settings["url"],"refresh":true})).await{
                        Ok(v)=>{available.set(v.as_array().cloned().unwrap_or_default());ui.tell(crate::locale_format!("{} modèles disponibles.", "{} models available.",available.get_untracked().len()));}
                        Err(e)=>ui.tell(e),
                    }
                });
            }>{tr("Récupérer la liste des modèles", "Fetch models")}</button>
        </div>
        <div class="form-card">
            <h2>{tr("Voix de Sillune", "Sillune’s voice")}</h2>
            <label>{tr("Synthèse vocale du Bot", "Bot voice")}
                <select prop:value=move ||draft.with(|s|field(s,"voice")) on:change=move |ev|set(draft,"voice",event_target_value(&ev))>
                    <option value="local-female">{tr("LFM3 locale · voix féminine", "Local LFM3 · female voice")}</option>
                    <option value="local-male">{tr("LFM3 locale · voix masculine", "Local LFM3 · male voice")}</option>
                    <option value="openai-marin">"OpenAI · Marin"</option>
                    <option value="openai-cedar">"OpenAI · Cedar"</option>
                </select>
            </label>
            <p class="subtle">{tr("Le modèle choisi dans le chat rédige la réponse. La voix choisie ici la lit. Le micro utilise le pack LFM3 pour la transcription locale.", "Your chat model writes the reply. The voice you choose here reads it out. The microphone uses the LFM3 pack for local transcription.")}</p>
            <Show when=move ||draft.with(|s|field(s,"voice").starts_with("openai-"))>
                <p class="subtle">{tr("La voix OpenAI envoie le texte de la réponse à OpenAI et utilise les crédits de ta clé.", "OpenAI voices send reply text to OpenAI and use your API credits.")}</p>
                <Show when=move ||provider.get()!="openai">
                    <SecretField ui=ui name={Signal::derive(||"openai".into())} label={tr("Clé OpenAI pour la voix", "OpenAI API key for voice")}/>
                </Show>
            </Show>
        </div>
        <div class="form-card">
            <h2>{tr("Modèles locaux", "Local models")}</h2>
            <Pack ui=ui id="image" title="Image · SDXS-512-DreamShaper"/>
            <Pack ui=ui id="quality" title={tr("Image · FLUX.2 klein 4B quantifié", "Image · FLUX.2 klein 4B quantized")}/>
            <Pack ui=ui id="zimage" title="Image · Z-Image Turbo Q4"/>
            <Pack ui=ui id="trellis" title="3D · TRELLIS.2 4B Q8 Vulkan"/>
            <p class="subtle">{tr("Convertit une image PNG ou JPEG en GLB texturé. Pour partir d’un texte, installe aussi un modèle d’image, par exemple Z-Image Turbo.", "Convert a PNG or JPEG to a textured GLB. For text-to-3D, also install an image model such as Z-Image Turbo.")}</p>
            <Pack ui=ui id="shape" title={tr("3D · Shap-E (historique)", "3D · Shap-E (legacy)")}/>
            <Pack ui=ui id="voice" title={tr("Bot · LFM3-Audio-2B français", "Bot · LFM3-Audio-2B")}/>
            <details class="advanced">
                <summary>{tr("Dossier des moteurs et modèles", "Engine and model location")}</summary>
                <label>{tr("Dossier Runtime", "Runtime folder")}
                    <div class="form-line">
                        <input prop:value=move ||draft.with(|s|field(s,"engines")) placeholder={tr("Dossier automatique", "Default location")} on:input=move |ev|set(draft,"engines",event_target_value(&ev))/>
                        <button class="button" on:click=move |_|choose_folder(ui,draft,"engines")>{tr("Parcourir", "Browse")}</button>
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
            ui.tell(tr(
                "Saisis la clé avant de l’enregistrer.",
                "Enter a key before saving.",
            ));
            return;
        }
        spawn_local(async move {
            match invoke("secret", json!({"name":id,"value":value})).await {
                Ok(v) => {
                    ui.snapshot.set(v);
                    pending.set(String::new());
                    ui.tell(if clear {
                        tr("Clé retirée.", "Key removed.")
                    } else {
                        tr("Clé enregistrée dans Windows.", "Key stored in Windows.")
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
                    <input type="password" autocomplete="off" aria-label=label placeholder=move ||if ui.snapshot.with(|s|s["secrets"][name.get()]==true){tr("Clé enregistrée", "Key saved")}else{tr("Saisir la clé", "Enter API key")}
                        prop:value=move ||pending.get() on:input=move |ev|pending.set(event_target_value(&ev))/>
                    <button class="button" on:click=move |_|save(false)>{tr("Enregistrer la clé", "Save key")}</button>
                    <button class="button" on:click=move |_|save(true)>{tr("Retirer", "Remove")}</button>
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
        status.set(json!({"done":false,"detail":tr("Choisis le dossier du pack…", "Choose where to install the pack…"),"progress":0.0}));
        spawn_local(async move {
            let folder = match native("choose_folder", json!({})).await {
                Ok(value) if value.is_string() => value,
                Ok(_) => {
                    let _ = status.try_set(Value::Null);
                    return;
                }
                Err(e) => {
                    ui.tell(e);
                    let _ = status.try_set(Value::Null);
                    return;
                }
            };
            let session = match invoke("download", json!({"pack":id,"folder":folder})).await {
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
                                tr("Pack local prêt.", "Local pack ready.").into()
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
                        if s["packs"][id]["ready"]==true {tr("Prêt sur ce PC", "Ready on this PC").into()}
                        else{crate::locale_format!("{:.2} Go à télécharger", "{:.2} GB to download",s["packs"][id]["missing"].as_u64().unwrap_or(0) as f64/1_000_000_000.0)}
                    })}</span>
                </div>
                <Show when=move ||ui.snapshot.with(|s|s["packs"][id]["ready"]!=true)>
                    <Show when=downloading fallback=move ||view!{<button class="button" on:click=start>{tr("Installer le pack", "Install pack")}</button>}>
                        <button class="button" on:click=move |_|spawn_local(async move {let _=invoke("cancel",json!({"session":format!("pack-{id}")})).await;})>{tr("Arrêter", "Stop")}</button>
                    </Show>
                </Show>
            </div>
            <Show when=downloading>
                <progress max="1" prop:value=move ||status.with(|v|v["progress"].as_f64().unwrap_or(0.0))></progress>
                <p class="subtle">{move ||status.with(|v|crate::locale::job_text(field(v,"detail")))}</p>
            </Show>
            <Show when=move ||ui.snapshot.with(|s|s["packs"][id]["folder"].is_string())>
                <p class="digest">{move ||ui.snapshot.with(|s|field(&s["packs"][id],"folder"))}</p>
            </Show>
            <details class="advanced">
                <summary>{tr("Provenance et empreintes", "Sources and checksums")}</summary>
                <button class="text-link" on:click=move |_|{
                    let address=ui.snapshot.with_untracked(|s|field(&s["packs"][id],"license"));
                    spawn_local(async move {let _=native("open_link",json!({"url":address})).await;});
                }>{tr("Voir la licence", "View license")}</button>
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
            <h2>{tr("Recherche web", "Web search")}</h2>
            <label>"Moteur"
                <select prop:value=move ||draft.with(|s|field(s,"search")) on:change=move |ev|set(draft,"search",event_target_value(&ev))>
                    <option value="openrouter">"OpenRouter Web"</option><option value="duckduckgo">"DuckDuckGo"</option><option value="searx">"SearXNG"</option>
                </select>
            </label>
            <Show when=move ||draft.with(|s|s["search"]=="searx") fallback=move ||view!{<p class="subtle">{move || if draft.with(|s|s["search"]=="openrouter") {tr("OpenRouter utilise ta clé quand elle est disponible. DuckDuckGo sert de solution de secours.", "OpenRouter uses your key when available and falls back to DuckDuckGo.")} else {tr("DuckDuckGo ne nécessite pas de clé API.", "DuckDuckGo works without an API key.")}}</p>}>
                <label>{tr("Instance SearXNG", "SearXNG instance")}
                    <input placeholder="https://recherche.exemple" prop:value=move ||draft.with(|s|field(s,"searx_url")) on:input=move |ev|set(draft,"searx_url",event_target_value(&ev))/>
                </label>
            </Show>
        </div>
        <div class="form-card">
            <h2>"GitHub"</h2>
            <p class="subtle">{tr("Jeton limité aux dépôts et aux actions que tu veux exposer.", "Use a token scoped to the repositories and actions you want to allow.")}</p>
            <SecretField ui=ui name=Signal::derive(||"github".into()) label={tr("Jeton GitHub", "GitHub token")}/>
        </div>
        <div class="form-card">
            <h2>"Google Drive"</h2>
            <DriveConnection ui=ui/>
        </div>
        <div class="form-card">
            <h2>{tr("Serveurs MCP", "MCP servers")}</h2>
            <label>{tr("Configuration JSON", "JSON configuration")}
                <textarea class="mono" rows="7" prop:value=move ||mcp_json.get() on:input=move |ev|mcp_json.set(event_target_value(&ev))></textarea>
            </label>
            <details class="advanced"><summary>{tr("Exemples de configuration", "Configuration examples")}</summary>
                <pre>{r#"[{"name":"github","url":"https://api.githubcopilot.com/mcp/"}]
    [{"name":"local","command":"mon-serveur-mcp.exe","args":[]}]"#}</pre>
            </details>
            <label>{tr("Nom du serveur pour l’authentification", "Server name for authentication")}
                <input placeholder="github" prop:value=move ||mcp_name.get() on:input=move |ev|mcp_name.set(event_target_value(&ev))/>
            </label>
            <Show when=move ||!mcp_name.get().trim().is_empty()>
                <SecretField ui=ui name=Signal::derive(move ||format!("mcp-{}",mcp_name.get().trim())) label={tr("Jeton MCP", "MCP token")}/>
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
                Ok(_) => ui.tell(tr("Google Drive connecté.", "Google Drive connected.")),
                Err(e) => ui.tell(e),
            }
        });
    };
    view! {
        <p class="subtle">{move || if ui.snapshot.with(|s|s["drive_connected"]==true){tr("Compte connecté.", "Account connected.")}
            else {tr("Connecte un client OAuth Google de type « Application de bureau ». Sillune demande l’accès drive.file.", "Connect a Google OAuth Desktop app client. Sillune requests the drive.file scope.")}}</p>
        <label>{tr("Identifiant client Google", "Google client ID")}
            <input prop:value=move ||id.get() on:input=move |ev|id.set(event_target_value(&ev)) placeholder="…apps.googleusercontent.com"/>
        </label>
        <label>{tr("Secret du client Google", "Google client secret")}
            <input type="password" autocomplete="off" prop:value=move ||secret.get() on:input=move |ev|secret.set(event_target_value(&ev))/>
        </label>
        <div class="button-row">
            <button class="button" disabled=move || busy.get() || id.with(|s|s.trim().is_empty()) on:click=connect>{tr("Connecter dans le navigateur", "Connect in browser")}</button>
            <Show when=move ||busy.get()>
                <button class="button" on:click=move |_|spawn_local(async move {let _=invoke("cancel",json!({"session":"drive-oauth"})).await;})>{tr("Annuler", "Cancel")}</button>
            </Show>
            <Show when=move ||ui.snapshot.with(|s|s["drive_connected"]==true)>
                <button class="button" on:click=move |_|act(ui,"drive_disconnect",json!({}))>{tr("Déconnecter localement", "Disconnect locally")}</button>
            </Show>
        </div>
        <details class="advanced">
            <summary>{tr("Utiliser un jeton Drive existant", "Use an existing Drive token")}</summary>
            <SecretField ui=ui name=Signal::derive(||"drive".into()) label={tr("Jeton Google Drive", "Google Drive token")}/>
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
            <h2>{tr("Mémoire facultative", "Optional memory")}</h2>
            <label class="checkbox"><input type="checkbox" prop:checked=move ||draft.with(|s|s["memory"]==true) on:change=move |ev|set(draft,"memory",event_target_checked(&ev))/>{tr("Activer les notes et le rappel des conversations", "Enable notes and conversation recall")}</label>
            <p class="subtle">{tr("Sillune retient les préférences et les pratiques que tu confirmes. Les notes utiles entrent dans le contexte du fournisseur sélectionné.", "Sillune saves the preferences and practices you confirm and provides relevant notes to the selected model.")}</p>
        </div>
        <div class="form-card">
            <h2>{tr("Notes mémorisées", "Saved notes")}</h2>
            {move ||ui.snapshot.with(|s|list(s,"notes")).into_iter().map(|note| {
                let edit=note.clone();
                let note_id=field(&note,"id");
                view!{
                    <div class="note-row">
                        <p>{field(&note,"content")}</p>
                        <button class="button compact" on:click=move |_|{id.set(Some(field(&edit,"id")));text.set(field(&edit,"content"));kind.set(field(&edit,"kind"));}>{tr("Modifier", "Edit")}</button>
                        <button class="button compact" on:click=move |_|act(ui,"note_forget",json!({"id":note_id}))>{tr("Oublier", "Forget")}</button>
                    </div>
                }
            }).collect_view()}
            <label>{tr("Type de note", "Note type")}
                <select prop:value=move ||kind.get() on:change=move |ev|kind.set(event_target_value(&ev))>
                    <option value="preference">{tr("Préférence", "Preference")}</option><option value="fact">"Fait"</option><option value="method">{tr("Méthode", "Method")}</option>
                </select>
            </label>
            <label>{tr("Contenu", "Content")}<textarea rows="3" prop:value=move ||text.get() on:input=move |ev|text.set(event_target_value(&ev))></textarea></label>
            <div class="button-row">
                <button class="button primary" disabled=move ||text.with(|v|v.trim().is_empty()) on:click=move |_|{
                    act(ui,"note_save",json!({"id":id.get_untracked(),"kind":kind.get_untracked(),"content":text.get_untracked()}));
                    id.set(None);text.set(String::new());
                }>{tr("Mémoriser", "Remember")}</button>
                <button class="button danger" on:click=move |_|{
                    if web_sys::window().and_then(|w|w.confirm_with_message(tr("Effacer toutes les notes mémorisées ?", "Clear all saved notes?")).ok()).unwrap_or(false){
                        act(ui,"note_forget",json!({}));
                    }
                }>{tr("Effacer les notes", "Clear notes")}</button>
            </div>
        </div>
        <Show when=move ||!ui.snapshot.with(|s|list(s,"suggestions").is_empty())>
            <div class="form-card"><h2>"Suggestions"</h2>
                {move ||ui.snapshot.with(|s|list(s,"suggestions")).into_iter().enumerate().map(|(i,note)|view!{
                    <div class="note-row">
                        <p>{field(&note,"content")}</p>
                        <button class="button compact" on:click=move |_|act(ui,"note_review",json!({"index":i,"save":true}))>{tr("Mémoriser", "Remember")}</button>
                        <button class="button compact" on:click=move |_|act(ui,"note_review",json!({"index":i,"save":false}))>{tr("Ignorer", "Dismiss")}</button>
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
            <div class="section-head"><h2>{tr("Plugins disponibles", "Available plugins")}</h2>
                <button class="button primary" on:click=move |_|ui.select_mode("plugin")>{tr("Créer avec Sillune", "Create with Sillune")}</button>
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
                                <button class="button compact" on:click=move |_|act(ui,"plugin_active",json!({"id":id,"active":!active}))>{if active{tr("Désactiver", "Disable")}else{tr("Activer", "Enable")}}</button>
                                <button class="button compact danger" on:click=move |_|{
                                    if web_sys::window().and_then(|w|w.confirm_with_message(tr("Désinstaller ce plugin ?", "Uninstall this plugin?")).ok()).unwrap_or(false) {
                                        act(ui,"plugin_remove",json!({"id":remove_id}));
                                    }
                                }>{tr("Désinstaller", "Uninstall")}</button>
                            })}
                        </div>
                    </article>
                }
            }).collect_view()}
        </div>
        <div class="form-card">
            <h2>{tr("Skills fournis", "Built-in skills")}</h2>
            <p class="subtle">{tr("Ajoute un skill à un message pour charger son guide pendant ce tour.", "Add a skill to a message to load its guide for that turn.")}</p>
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
                            }>{tr("Utiliser", "Use")}</button>
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

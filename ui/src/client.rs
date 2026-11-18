use leptos::{prelude::*, task::spawn_local};
use serde_json::{Value, json};
use wasm_bindgen::{
    JsCast,
    prelude::{Closure, wasm_bindgen},
};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(inline_js = "
export async function nativeInvoke(command, payload) {
  return JSON.stringify(await window.__TAURI__.core.invoke(command, JSON.parse(payload)) ?? null)
}
let activeAudio
export async function playVoice(source) {
  stopVoice()
  activeAudio = new Audio(source)
  try { await activeAudio.play(); return true } catch { return false }
}
export function voicePlaying() { return activeAudio !== undefined && !activeAudio.paused && !activeAudio.ended }
export function stopVoice() { if (activeAudio) { activeAudio.pause(); activeAudio = undefined } }
export function scrollThread() {
  const thread = document.getElementById('thread')
  if (thread) requestAnimationFrame(() => thread.scrollTo(0, thread.scrollHeight))
}
export function sleepFor(ms) { return new Promise(resolve => setTimeout(resolve, ms)) }
")]
extern "C" {
    #[wasm_bindgen(js_name=nativeInvoke)]
    fn native_invoke(command: &str, payload: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name=playVoice)]
    pub fn play_voice(source: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name=voicePlaying)]
    pub fn voice_playing() -> bool;
    #[wasm_bindgen(js_name=stopVoice)]
    pub fn stop_voice();
    #[wasm_bindgen(js_name=scrollThread)]
    pub fn scroll_thread();
    #[wasm_bindgen(js_name=sleepFor)]
    pub fn sleep_for(ms: u32) -> js_sys::Promise;
}

pub async fn native(command: &str, payload: Value) -> Result<Value, String> {
    let value = JsFuture::from(native_invoke(command, &payload.to_string()))
        .await
        .map_err(|error| {
            error
                .as_string()
                .unwrap_or_else(|| "Communication avec Sillune interrompue".into())
        })?;
    serde_json::from_str(value.as_string().as_deref().unwrap_or("null")).map_err(|e| e.to_string())
}

pub async fn invoke(op: &str, data: Value) -> Result<Value, String> {
    native("api", json!({"op":op,"data":data})).await
}

pub fn clock(f: impl FnMut() + 'static, millis: i32) {
    let callback = Closure::wrap(Box::new(f) as Box<dyn FnMut()>);
    if let Some(window) = web_sys::window() {
        let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            millis,
        );
    }
    callback.forget();
}

pub fn browser_now() -> f64 {
    js_sys::Date::now()
}

pub fn field(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}
pub fn list(value: &Value, key: &str) -> Vec<Value> {
    value[key].as_array().cloned().unwrap_or_default()
}

pub const MODES: &[(&str, &str, &str)] = &[
    ("chat", "Chat", "◌"),
    ("agent", "Agent", "⌘"),
    ("image", "Image", "▧"),
    ("bot", "Bot", "◒"),
    ("plugin", "Plugin", "⊞"),
];

pub fn mode_label(mode: &str) -> &'static str {
    MODES
        .iter()
        .find(|m| m.0 == mode)
        .map(|m| m.1)
        .unwrap_or("Chat")
}

#[derive(Clone, Copy)]
pub struct Ui {
    pub snapshot: RwSignal<Value>,
    pub session: RwSignal<String>,
    pub mode: RwSignal<String>,
    pub panel: RwSignal<String>,
    pub settings_tab: RwSignal<String>,
    pub messages: RwSignal<Vec<Value>>,
    pub job: RwSignal<Value>,
    pub composer: RwSignal<String>,
    pub attachments: RwSignal<Vec<Value>>,
    pub preview: RwSignal<Value>,
    pub notice: RwSignal<String>,
    pub undo: RwSignal<bool>,
    pub mic: RwSignal<bool>,
    pub playing: RwSignal<bool>,
    pub quiet: RwSignal<bool>,
    pub size: RwSignal<String>,
    pub seed: RwSignal<String>,
    pub image_profile: RwSignal<String>,
    pub busy: RwSignal<bool>,
    pub nav: RwSignal<bool>,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            snapshot: RwSignal::new(Value::Null),
            session: RwSignal::new(String::new()),
            mode: RwSignal::new("chat".into()),
            panel: RwSignal::new("conversation".into()),
            settings_tab: RwSignal::new("general".into()),
            messages: RwSignal::new(Vec::new()),
            job: RwSignal::new(Value::Null),
            composer: RwSignal::new(String::new()),
            attachments: RwSignal::new(Vec::new()),
            preview: RwSignal::new(Value::Null),
            notice: RwSignal::new(String::new()),
            undo: RwSignal::new(false),
            mic: RwSignal::new(false),
            playing: RwSignal::new(false),
            quiet: RwSignal::new(false),
            size: RwSignal::new("square".into()),
            seed: RwSignal::new(String::new()),
            image_profile: RwSignal::new("image".into()),
            busy: RwSignal::new(false),
            nav: RwSignal::new(false),
        }
    }

    pub fn active(self) -> bool {
        self.busy.get()
            || self
                .job
                .with(|j| !j.is_null() && !j["done"].as_bool().unwrap_or(false))
    }

    pub fn tell(self, text: impl Into<String>) {
        self.notice.set(text.into());
    }

    pub async fn refresh(self) {
        match invoke("snapshot", json!({})).await {
            Ok(snapshot) => self.snapshot.set(snapshot),
            Err(e) => self.tell(e),
        }
    }

    pub fn setting(self, name: &str) -> String {
        self.snapshot.with(|s| field(&s["settings"], name))
    }

    pub fn settings(self, tab: &str) {
        self.settings_tab.set(tab.into());
        self.panel.set("settings".into());
        self.nav.set(false);
    }

    pub fn select_mode(self, mode: &str) {
        self.nav.set(false);
        if self.panel.get_untracked() == "conversation"
            && self.mode.get_untracked() == mode
            && !self.session.get_untracked().is_empty()
        {
            return;
        }
        self.new_chat(mode);
    }

    pub fn new_chat(self, mode: &str) {
        if self.active() {
            self.tell(
                "Arrête ou termine le tour en cours avant d’ouvrir une nouvelle conversation.",
            );
            return;
        }
        stop_voice();
        self.playing.set(false);
        self.mode.set(mode.into());
        self.panel.set("conversation".into());
        self.session.set(String::new());
        self.composer.set(String::new());
        self.messages.set(Vec::new());
        self.attachments.set(Vec::new());
        self.job.set(Value::Null);
        self.undo.set(false);
    }

    pub fn open_chat(self, row: Value) {
        if self.active() || self.mic.get_untracked() {
            self.tell("Termine le tour en cours pour changer de conversation.");
            return;
        }
        self.panel.set("conversation".into());
        self.mode.set(field(&row, "mode"));
        self.session.set(field(&row, "id"));
        self.job.set(Value::Null);
        self.messages.set(Vec::new());
        self.composer.set(String::new());
        self.attachments.set(Vec::new());
        self.nav.set(false);
        spawn_local(async move {
            self.load_chat(&field(&row, "id")).await;
        });
    }

    async fn load_chat(self, session: &str) {
        match invoke("messages", json!({"session":session})).await {
            Ok(result) if self.session.get_untracked() == session => {
                self.messages.set(list(&result, "messages"));
                self.undo.set(result["undo"].as_bool().unwrap_or(false));
                scroll_thread();
            }
            Ok(_) => {}
            Err(e) => self.tell(e),
        }
    }

    pub fn send(self, pcm: Option<String>) {
        if self.active() {
            return;
        }
        let text = self.composer.get_untracked().trim().to_owned();
        if text.is_empty() && pcm.is_none() {
            return;
        }
        let mode = self.mode.get_untracked();
        if matches!(mode.as_str(), "image" | "bot") {
            let pack = if mode == "image" {
                self.image_profile.get_untracked()
            } else {
                "voice".into()
            };
            let ready = self
                .snapshot
                .with_untracked(|s| s["packs"][&pack]["ready"].as_bool().unwrap_or(false));
            if !ready && (mode == "image" || !self.quiet.get_untracked() || pcm.is_some()) {
                self.tell("Installe d’abord le pack local depuis les réglages des modèles.");
                self.settings("models");
                return;
            }
        }
        self.busy.set(true);
        stop_voice();
        self.playing.set(false);
        spawn_local(async move {
            let mut session = self.session.get_untracked();
            if session.is_empty() {
                let title = if text.is_empty() {
                    "Conversation vocale".into()
                } else {
                    text.chars().take(58).collect::<String>()
                };
                match invoke("session_new", json!({"mode":mode,"title":title})).await {
                    Ok(value) => {
                        session = field(&value, "id");
                        self.session.set(session.clone());
                    }
                    Err(e) => {
                        self.tell(e);
                        self.busy.set(false);
                        return;
                    }
                }
            }
            let dims = match self.size.get_untracked().as_str() {
                "wide" => (768, 512),
                "tall" => (512, 768),
                _ => (512, 512),
            };
            let attachments = self
                .attachments
                .get_untracked()
                .iter()
                .map(|f| field(f, "id"))
                .collect::<Vec<_>>();
            let row = json!({"session":session,"text":text,"pcm":pcm,"width":dims.0,"height":dims.1,
                "seed":self.seed.get_untracked().parse::<i64>().ok(),"speak":!self.quiet.get_untracked(),"attachments":attachments,
                "image_profile":self.image_profile.get_untracked()});
            match invoke("start", row).await {
                Ok(job) => {
                    self.job.set(job);
                    self.composer.set(String::new());
                    self.attachments.set(Vec::new());
                    let user = if text.is_empty() {
                        "Message vocal".into()
                    } else {
                        text
                    };
                    self.messages
                        .update(|messages| messages.push(json!({"role":"user","text":user})));
                    scroll_thread();
                }
                Err(e) => self.tell(e),
            }
            self.busy.set(false);
        });
    }

    pub fn start_polling(self) {
        let in_flight = RwSignal::new(false);
        clock(
            move || {
                self.playing.set(voice_playing());
                if in_flight.get_untracked()
                    || self
                        .job
                        .with_untracked(|j| j.is_null() || j["done"].as_bool().unwrap_or(false))
                {
                    return;
                }
                let session = self.session.get_untracked();
                if session.is_empty() {
                    return;
                }
                in_flight.set(true);
                spawn_local(async move {
                    if let Ok(job) = invoke("poll", json!({"session":session})).await
                        && self.session.get_untracked() == session
                        && !job.is_null()
                    {
                        let done = job["done"].as_bool().unwrap_or(false);
                        if job["text"].as_str().map(str::len)
                            != self
                                .job
                                .with_untracked(|v| v["text"].as_str().map(str::len))
                        {
                            scroll_thread();
                        }
                        if done {
                            self.load_chat(&session).await;
                            self.refresh().await;
                            let error = field(&job, "error");
                            if !error.is_empty() {
                                self.tell(error);
                            }
                            if self.mode.get_untracked() == "bot"
                                && !self.quiet.get_untracked()
                                && let Some(voice) = list(&job, "files")
                                    .into_iter()
                                    .find(|f| f["media"] == "audio/wav")
                                && let Ok(value) =
                                    invoke("artifact", json!({"id":voice["id"]})).await
                            {
                                let _ = JsFuture::from(play_voice(&field(&value, "preview"))).await;
                            }
                        }
                        self.job.set(job);
                    }
                    in_flight.set(false);
                });
            },
            180,
        );
    }

    pub fn cancel(self) {
        stop_voice();
        self.playing.set(false);
        if self.mic.get_untracked() {
            self.mic.set(false);
            spawn_local(async move {
                let _ = native("mic_stop", json!({})).await;
            });
        }
        let session = self.session.get_untracked();
        if !session.is_empty() && self.active() {
            spawn_local(async move {
                let _ = invoke("cancel", json!({"session":session})).await;
            });
        }
    }

    pub fn microphone(self) {
        if self.active() {
            return;
        }
        let mic = self.mic.get_untracked();
        if !mic
            && !self
                .snapshot
                .with_untracked(|s| s["packs"]["voice"]["ready"].as_bool().unwrap_or(false))
        {
            self.settings("models");
            return;
        }
        stop_voice();
        spawn_local(async move {
            if mic {
                self.mic.set(false);
                match native("mic_stop", json!({})).await {
                    Ok(pcm) => self.send(pcm.as_str().map(str::to_owned)),
                    Err(e) => self.tell(e),
                }
            } else {
                match native("mic_start", json!({})).await {
                    Ok(_) => self.mic.set(true),
                    Err(e) => self.tell(e),
                }
            }
        });
    }

    pub fn import_audio(self) {
        spawn_local(async move {
            match native("choose_audio", json!({})).await {
                Ok(v) => {
                    if let Some(pcm) = v.as_str() {
                        self.send(Some(pcm.into()));
                    }
                }
                Err(e) => self.tell(e),
            }
        });
    }

    pub fn attach(self) {
        spawn_local(async move {
            match native("import_file", json!({})).await {
                Ok(file) if !file.is_null() => self.attachments.update(|items| {
                    if items.len() < 6 {
                        items.push(file)
                    }
                }),
                Ok(_) => {}
                Err(e) => self.tell(e),
            }
        });
    }

    pub fn show_artifact(self, id: String) {
        spawn_local(async move {
            match invoke("artifact", json!({"id":id})).await {
                Ok(value) => self.preview.set(value),
                Err(e) => self.tell(e),
            }
        });
    }

    pub fn export(self, id: String) {
        spawn_local(async move {
            match native("export_file", json!({"id":id})).await {
                Ok(v) if v == true => self.tell("Fichier enregistré."),
                Ok(_) => {}
                Err(e) => self.tell(e),
            }
        });
    }

    pub fn approval(self, allow: bool) {
        let id = self.job.with_untracked(|job| field(&job["approval"], "id"));
        if id.is_empty() {
            return;
        }
        spawn_local(async move {
            match invoke("approve", json!({"id":id,"allow":allow})).await {
                Ok(_) => self.job.update(|j| j["approval"] = Value::Null),
                Err(e) => self.tell(e),
            }
        });
    }

    pub fn revert(self) {
        let id = self.session.get_untracked();
        spawn_local(async move {
            match invoke("undo", json!({"session":id})).await {
                Ok(result) => {
                    let restored = list(&result, "restored").len();
                    let conflicts = list(&result, "conflicts");
                    self.tell(format!(
                        "{restored} fichier(s) restauré(s) · {} conflit(s).",
                        conflicts.len()
                    ));
                    if let Some(file) = conflicts.first().and_then(|c| c.get("file")) {
                        self.show_artifact(field(file, "id"));
                    }
                    self.load_chat(&id).await;
                }
                Err(e) => self.tell(e),
            }
        });
    }
}

pub fn link_click(ev: web_sys::MouseEvent) {
    let Some(target) = ev
        .target()
        .and_then(|e| e.dyn_into::<web_sys::Element>().ok())
    else {
        return;
    };
    if let Ok(Some(a)) = target.closest("a[data-web]")
        && let Some(url) = a.get_attribute("href")
    {
        ev.prevent_default();
        spawn_local(async move {
            let _ = native("open_link", json!({"url":url})).await;
        });
    }
}

pub fn safe_document(contents: &str) -> String {
    format!(
        "<meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; img-src data: blob:; style-src 'unsafe-inline'; media-src data: blob:; base-uri 'none'; form-action 'none'\">{contents}"
    )
}

pub fn copy_text(text: String) {
    if let Some(window) = web_sys::window() {
        spawn_local(async move {
            let _ = JsFuture::from(window.navigator().clipboard().write_text(&text)).await;
        });
    }
}

pub fn markdown(text: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
    let events = Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH).map(
        |event| match event {
            Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
            Event::Start(Tag::Link { dest_url, .. })
                if dest_url.starts_with("https://") || dest_url.starts_with("http://") =>
            {
                let safe = dest_url
                    .replace('&', "&amp;")
                    .replace('"', "&quot;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                Event::Html(format!("<a data-web href=\"{safe}\">").into())
            }
            Event::Start(Tag::Link { .. }) => Event::Html("<a>".into()),
            Event::End(TagEnd::Link) => Event::Html("</a>".into()),
            Event::Start(Tag::Image { title, .. }) => Event::Text(title),
            Event::End(TagEnd::Image) => Event::Text("".into()),
            other => other,
        },
    );
    let mut result = String::new();
    html::push_html(&mut result, events);
    result
}

pub fn commit_settings(ui: Ui, settings: Value) {
    spawn_local(async move {
        match invoke("settings_save", json!({"settings":settings})).await {
            Ok(result) => {
                ui.snapshot.set(result);
                ui.tell("Réglages enregistrés.");
            }
            Err(e) => ui.tell(e),
        }
    });
}

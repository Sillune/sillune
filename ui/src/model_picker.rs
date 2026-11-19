use crate::locale::tr;
use crate::{
    client::{Ui, field, invoke},
    settings::PROVIDERS,
};
use leptos::{prelude::*, task::spawn_local};
use serde_json::{Value, json};

fn level_name(index: usize) -> Option<&'static str> {
    let levels = if crate::locale::is_french() {
        ["Minimal", "Moyen", "Élevé", "Très élevé", "Max", "Ultra"]
    } else {
        ["Minimal", "Medium", "High", "Very high", "Max", "Ultra"]
    };
    levels.get(index).copied()
}

#[component]
pub fn ModelPicker(ui: Ui) -> impl IntoView {
    let provider = RwSignal::new(String::new());
    let rows = RwSignal::new(Vec::<Value>::new());
    let search = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let version = RwSignal::new(0u32);
    let selected_provider = Memo::new(move |_| ui.selected("provider"));
    let selected_model = Memo::new(move |_| ui.selected("model"));
    let reasoning = RwSignal::new(Value::Null);
    let pending_reasoning = RwSignal::new(false);
    let reasoning_version = RwSignal::new(0u64);
    let drag_value = RwSignal::new(None::<f64>);
    let fetch = move |id: String, refresh: bool| {
        let ticket = version.get_untracked().wrapping_add(1);
        version.set(ticket);
        loading.set(true);
        error.set(String::new());
        rows.set(Vec::new());
        let url = ui.snapshot.with_untracked(|v| field(&v["settings"], "url"));
        spawn_local(async move {
            let result = invoke("models", json!({"provider":id,"url":url,"refresh":refresh})).await;
            if version.try_get_untracked() == Some(ticket) {
                loading.set(false);
                match result {
                    Ok(value) => rows.set(value.as_array().cloned().unwrap_or_default()),
                    Err(message) => error.set(message),
                }
            }
        });
    };
    Effect::new(move |_| {
        let open = ui.picker.get();
        if open == "models" {
            let id = selected_provider.get();
            if !id.is_empty() {
                provider.set(id.clone());
                search.set(String::new());
                fetch(id, false);
            }
        }
    });
    Effect::new(move |_| {
        if ui.picker.get() != "effort" {
            return;
        }
        let provider = selected_provider.get();
        let model = selected_model.get();
        drag_value.set(None);
        let id = reasoning_version.get_untracked().wrapping_add(1);
        reasoning_version.set(id);
        pending_reasoning.set(true);
        reasoning.set(Value::Null);
        spawn_local(async move {
            let result = invoke(
                "reasoning_options",
                json!({"provider":provider,"model":model}),
            )
            .await;
            if reasoning_version.try_get_untracked() == Some(id) {
                pending_reasoning.set(false);
                reasoning.set(result.unwrap_or_default());
            }
        });
    });
    let choose = move |id: String| {
        if !id.is_empty() && id.len() <= 300 {
            let chosen = provider.get_untracked();
            if ui.selected("provider") != chosen || ui.selected("model") != id {
                ui.choose(json!({"provider":chosen,"model":id,"effort":null}));
            }
            ui.picker.set("");
        }
    };
    let toggle = move |panel| {
        ui.picker.set(if ui.picker.get_untracked() == panel {
            ""
        } else {
            panel
        });
    };
    let saved_level = move || ui.selection.with(|s| s["effort"].as_u64());
    let position = move || {
        drag_value
            .get()
            .unwrap_or_else(|| saved_level().unwrap_or(0) as f64)
    };
    let current_level = move || {
        drag_value
            .get()
            .map(|value| value.round() as u64)
            .or_else(saved_level)
    };
    let level = move || current_level().and_then(|n| level_name(n as usize));
    let save_effort = move |effort: Option<u64>| {
        let model = ui.selected("model");
        if model.is_empty() {
            drag_value.set(None);
            ui.tell(tr(
                "Sélectionne d’abord un modèle.",
                "Choose a model first.",
            ));
            return;
        }
        if saved_level() != effort {
            ui.choose(json!({"provider":ui.selected("provider"),"model":model,"effort":effort}));
        }
        drag_value.set(None);
    };
    view! {
        <div class="composer-picker" on:keydown=move |event:web_sys::KeyboardEvent| {
            if event.key()=="Escape" { ui.picker.set(""); }
        }>
            <Show when=move || !ui.picker.get().is_empty()>
                <div class="picker-dismiss" on:click=move |_|ui.picker.set("")></div>
            </Show>
            <button class="picker-button" aria-label={tr("Sélectionner un modèle", "Choose a model")} aria-expanded=move || (ui.picker.get()=="models").to_string()
                title=move || ui.selected("model") on:click=move |_|toggle("models")>
                <span class="picker-icon" aria-hidden="true">"◇"</span>
                <span class="picker-model">{move || {
                    let id = ui.selected("model");
                    if id.is_empty() { tr("Modèle", "Model").into() } else { id }
                }}</span>
                <span class="chevron" aria-hidden="true"></span>
            </button>
            <button class="picker-button effort-button" aria-label={tr("Effort de raisonnement", "Reasoning effort")} aria-expanded=move || (ui.picker.get()=="effort").to_string()
                on:click=move |_|toggle("effort")>
                <span>{move || saved_level().and_then(|n|level_name(n as usize)).unwrap_or("Effort")}</span>
                <span class="chevron" aria-hidden="true"></span>
            </button>
            <Show when=move || ui.picker.get()=="models">
                <section class="picker-popover" aria-label={tr("Catalogue de modèles", "Model catalog")}>
                    <div class="picker-heading">
                        <label>
                            {tr("Fournisseur", "Provider")}
                            <span class="select-chevron"><select aria-label={tr("Fournisseur de modèles", "Model provider")} prop:value=move ||provider.get() on:change=move |ev|{
                                let id=event_target_value(&ev);
                                provider.set(id.clone());
                                search.set(String::new());
                                fetch(id,false);
                            }>
                                {PROVIDERS.iter().map(|(id,name,_)|view!{<option value=*id>{crate::settings::provider_label(id,name)}</option>}).collect_view()}
                            </select><span class="chevron" aria-hidden="true"></span></span>
                        </label>
                        <button class="iconbutton" aria-label={tr("Actualiser les modèles", "Refresh models")} title={tr("Actualiser les modèles", "Refresh models")}
                            disabled=move || loading.get() on:click=move |_|fetch(provider.get_untracked(),true)>"↻"</button>
                    </div>
                    <input class="picker-search" type="search" placeholder={tr("Filtrer par nom ou identifiant", "Filter by name or ID")}
                        aria-label={tr("Filtrer les modèles", "Filter models")} prop:value=move ||search.get() on:input=move |ev|search.set(event_target_value(&ev))/>
                    <div class="picker-list">
                        <Show when=move || loading.get()><p class="picker-status" role="status">{tr("Catalogue en cours de chargement…", "Loading models…")}</p></Show>
                        <Show when=move || !error.with(String::is_empty)>
                            <p class="picker-status" role="status">{move ||error.get()}</p>
                            <button class="picker-row" on:click=move |_|ui.settings("models")>{tr("Ouvrir les réglages du fournisseur ↗", "Open provider settings ↗")}</button>
                        </Show>
                        {move || {
                            let filter=search.get().trim().to_lowercase();
                            let selected=ui.selected("model");
                            let live=provider.get()==ui.selected("provider");
                            let options=rows.with(|rows| rows.iter().filter(|row| {
                                field(row,"name").to_lowercase().contains(&filter) || field(row,"id").to_lowercase().contains(&filter)
                            }).cloned().collect::<Vec<_>>());
                            if options.is_empty() && !loading.get() && error.with(String::is_empty) {
                                return view! { <p class="picker-status">{tr("Aucun modèle correspondant.", "No matching models.")}</p> }.into_any();
                            }
                            options.into_iter().map(|model| {
                                let id=field(&model,"id");
                                let name=field(&model,"name");
                                let active=live && id==selected;
                                let subtitle=(id!=name).then(||id.clone());
                                view!{
                                    <button class="picker-row" class:current=active aria-pressed=active.to_string() on:click=move |_|choose(id.clone())>
                                        <span><strong>{name}</strong>{subtitle.map(|s|view!{<small>{s}</small>})}</span>
                                        {active.then(||view!{<b aria-label={tr("Sélectionné", "Selected")}>"✓"</b>})}
                                    </button>
                                }
                            }).collect_view().into_any()
                        }}
                    </div>
                    <Show when=move || provider.get()=="compatible" || !error.with(String::is_empty)>
                        <form class="picker-manual" on:submit=move |ev|{ev.prevent_default();choose(search.get_untracked().trim().into());}>
                            <button class="button compact" type="submit" disabled=move ||search.with(|s|s.trim().is_empty() || s.len()>300)>{tr("Utiliser cet identifiant", "Use this ID")}</button>
                        </form>
                    </Show>
                </section>
            </Show>
            <Show when=move || ui.picker.get()=="effort">
                <section class="picker-popover effort-popover" aria-label={tr("Réglage du raisonnement", "Reasoning settings")}
                    class:ultra=move ||current_level()==Some(5)>
                    <div class="effort-heading">
                        <div><strong>{move ||level().unwrap_or(tr("Par défaut", "Default"))}</strong><small>{move ||ui.selected("model")}</small></div>
                        <button class="iconbutton" aria-label={tr("Rétablir l’effort par défaut", "Reset reasoning effort")} title={tr("Rétablir l’effort par défaut", "Reset reasoning effort")}
                            on:click=move |_|save_effort(None)>"↺"</button>
                    </div>
                    <div class="effort-steps" style=move ||format!("--progress:{}",position()/5.0)>
                        <div class="effort-track" aria-hidden="true">{(0..6).map(|_|view!{<i></i>}).collect_view()}</div>
                        <input type="range" min="0" max="5" step="any" aria-label={tr("Niveau de raisonnement", "Reasoning level")}
                            aria-valuetext=move ||level().unwrap_or(tr("Par défaut", "Default"))
                            prop:value=move ||position().to_string()
                            on:input=move |ev|drag_value.set(event_target_value(&ev).parse::<f64>().ok())
                            on:click=move |ev| {
                                if saved_level().is_none() && let Ok(value)=event_target_value(&ev).parse::<f64>() {
                                    save_effort(Some(value.round() as u64));
                                }
                            }
                            on:change=move |ev| {
                                if let Ok(value)=event_target_value(&ev).parse::<f64>() {
                                    save_effort(Some(value.round() as u64));
                                }
                            }
                            on:keydown=move |ev:web_sys::KeyboardEvent| {
                                let current=position().round() as u64;
                                let next=match ev.key().as_str() {
                                    "ArrowLeft"|"ArrowDown"=>current.saturating_sub(1),
                                    "ArrowRight"|"ArrowUp"=>(current+1).min(5),
                                    "Home"=>0, "End"=>5,
                                    _=>return,
                                };
                                ev.prevent_default();
                                save_effort(Some(next));
                            }/>
                    </div>
                    <p class="picker-status">{move || {
                        if pending_reasoning.get() { return tr("Vérification du modèle…", "Checking model…").to_owned(); }
                        let effective=current_level().and_then(|n|reasoning.with(|r|r["effective"][n as usize].as_str().map(str::to_owned)));
                        if let Some(effective)=effective {
                            crate::locale_format!("Niveau transmis · {}", "Sent to the model · {}", match effective.as_str() {
                                "minimal"=>"Minimal", "low"=>"Faible", "medium"=>tr("Moyen", "Medium"), "high"=>tr("Élevé", "High"),
                                "xhigh"=>tr("Très élevé", "Very high"), "max"=>"Max", "ultra"=>"Ultra", _=>&effective,
                            })
                        } else if current_level().is_some() { tr("Ce modèle gère son propre effort.", "This model manages its own reasoning effort.").into() }
                        else { String::new() }
                    }}</p>
                </section>
            </Show>
        </div>
    }
}

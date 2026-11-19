use crate::{
    client::{Ui, field, invoke},
    settings::PROVIDERS,
};
use leptos::{prelude::*, task::spawn_local};
use serde_json::{Value, json};

const LEVELS: [&str; 6] = ["Minimal", "Moyen", "Élevé", "Très élevé", "Max", "Ultra"];

#[component]
pub fn ModelPicker(ui: Ui) -> impl IntoView {
    let provider = RwSignal::new(String::new());
    let rows = RwSignal::new(Vec::<Value>::new());
    let search = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let version = RwSignal::new(0u32);
    let selected_provider = Memo::new(move |_| ui.selected("provider"));
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
        if !open.is_empty() {
            let id = selected_provider.get();
            if !id.is_empty() {
                provider.set(id.clone());
                search.set(String::new());
                fetch(id, false);
            }
        }
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
    let level = move || {
        ui.selection.with(|s| {
            s["effort"]
                .as_u64()
                .and_then(|n| LEVELS.get(n as usize))
                .copied()
        })
    };
    let supported = move || {
        let id = ui.selected("model");
        rows.with(|items| {
            items
                .iter()
                .find(|row| row["id"] == id)
                .and_then(|row| row["efforts"].as_array())
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        })
    };
    view! {
        <div class="composer-picker" on:keydown=move |event:web_sys::KeyboardEvent| {
            if event.key()=="Escape" { ui.picker.set(""); }
        }>
            <Show when=move || !ui.picker.get().is_empty()>
                <div class="picker-dismiss" on:click=move |_|ui.picker.set("")></div>
            </Show>
            <button class="picker-button" aria-label="Sélectionner un modèle" aria-expanded=move || (ui.picker.get()=="models").to_string()
                title=move || ui.selected("model") on:click=move |_|toggle("models")>
                <span class="picker-icon" aria-hidden="true">"◇"</span>
                <span class="picker-model">{move || {
                    let id = ui.selected("model");
                    if id.is_empty() { "Modèle".into() } else { id }
                }}</span>
                <span aria-hidden="true">"⌄"</span>
            </button>
            <button class="picker-button effort-button" aria-label="Effort de raisonnement" aria-expanded=move || (ui.picker.get()=="effort").to_string()
                on:click=move |_|toggle("effort")>
                <span class="picker-icon" aria-hidden="true">"ϟ"</span>
                <span>{move || level().unwrap_or("Effort")}</span>
            </button>
            <Show when=move || ui.picker.get()=="models">
                <section class="picker-popover" aria-label="Catalogue de modèles">
                    <div class="picker-heading">
                        <label>
                            "Fournisseur"
                            <select aria-label="Fournisseur de modèles" prop:value=move ||provider.get() on:change=move |ev|{
                                let id=event_target_value(&ev);
                                provider.set(id.clone());
                                search.set(String::new());
                                fetch(id,false);
                            }>
                                {PROVIDERS.iter().map(|(id,name,_)|view!{<option value=*id>{*name}</option>}).collect_view()}
                            </select>
                        </label>
                        <button class="iconbutton" aria-label="Actualiser les modèles" title="Actualiser les modèles"
                            disabled=move || loading.get() on:click=move |_|fetch(provider.get_untracked(),true)>"↻"</button>
                    </div>
                    <input class="picker-search" type="search" placeholder="Filtrer par nom ou identifiant"
                        aria-label="Filtrer les modèles" prop:value=move ||search.get() on:input=move |ev|search.set(event_target_value(&ev))/>
                    <div class="picker-list">
                        <Show when=move || loading.get()><p class="picker-status" role="status">"Catalogue en cours de chargement…"</p></Show>
                        <Show when=move || !error.with(String::is_empty)>
                            <p class="picker-status" role="status">{move ||error.get()}</p>
                            <button class="picker-row" on:click=move |_|ui.settings("models")>"Ouvrir les réglages du fournisseur ↗"</button>
                        </Show>
                        {move || {
                            let filter=search.get().trim().to_lowercase();
                            let selected=ui.selected("model");
                            let live=provider.get()==ui.selected("provider");
                            let options=rows.with(|rows| rows.iter().filter(|row| {
                                field(row,"name").to_lowercase().contains(&filter) || field(row,"id").to_lowercase().contains(&filter)
                            }).cloned().collect::<Vec<_>>());
                            if options.is_empty() && !loading.get() && error.with(String::is_empty) {
                                return view! { <p class="picker-status">"Aucun modèle correspondant."</p> }.into_any();
                            }
                            options.into_iter().map(|model| {
                                let id=field(&model,"id");
                                let name=field(&model,"name");
                                let active=live && id==selected;
                                let subtitle=(id!=name).then(||id.clone());
                                view!{
                                    <button class="picker-row" class:current=active aria-pressed=active.to_string() on:click=move |_|choose(id.clone())>
                                        <span><strong>{name}</strong>{subtitle.map(|s|view!{<small>{s}</small>})}</span>
                                        {active.then(||view!{<b aria-label="Sélectionné">"✓"</b>})}
                                    </button>
                                }
                            }).collect_view().into_any()
                        }}
                    </div>
                    <Show when=move || provider.get()=="compatible" || !error.with(String::is_empty)>
                        <form class="picker-manual" on:submit=move |ev|{ev.prevent_default();choose(search.get_untracked().trim().into());}>
                            <button class="button compact" type="submit" disabled=move ||search.with(|s|s.trim().is_empty() || s.len()>300)>"Utiliser cet identifiant"</button>
                        </form>
                    </Show>
                </section>
            </Show>
            <Show when=move || ui.picker.get()=="effort">
                <section class="picker-popover effort-popover" aria-label="Réglage du raisonnement">
                    <div class="effort-heading">
                        <div><small>"Raisonnement"</small><strong>{move ||level().unwrap_or("Par défaut")}</strong></div>
                        <button class="iconbutton" aria-label="Rétablir l’effort par défaut" title="Rétablir l’effort par défaut"
                            on:click=move |_|ui.choose(json!({"provider":ui.selected("provider"),"model":ui.selected("model"),"effort":null}))>"↺"</button>
                    </div>
                    <div class="effort-steps" style=move ||format!("--progress:{}%",ui.selection.with(|s|s["effort"].as_u64().map_or(0,|n|n*20)))>
                        <div class="effort-track" aria-hidden="true">{(0..6).map(|_|view!{<i></i>}).collect_view()}</div>
                        <input type="range" min="0" max="5" step="1" aria-label="Niveau de raisonnement"
                            aria-valuetext=move ||level().unwrap_or("Par défaut") disabled=move ||supported().is_empty()
                            prop:value=move ||ui.selection.with(|s|s["effort"].as_u64().unwrap_or(0).to_string())
                            on:click=move |ev| {
                                if ui.selection.with_untracked(|s|s["effort"].is_null())
                                    && let Ok(index)=event_target_value(&ev).parse::<u64>() {
                                    ui.choose(json!({"provider":ui.selected("provider"),"model":ui.selected("model"),"effort":index}));
                                }
                            }
                            on:change=move |ev| {
                                if let Ok(index)=event_target_value(&ev).parse::<u64>() {
                                    ui.choose(json!({"provider":ui.selected("provider"),"model":ui.selected("model"),"effort":index}));
                                }
                            }/>
                    </div>
                    <div class="effort-endpoints"><span>"Minimal"</span><span>"Ultra"</span></div>
                    <p class="picker-status">{move || {
                        let list=supported();
                        if loading.get() { "Vérification des niveaux du modèle…".into() }
                        else if list.is_empty() { "Effort géré par le modèle.".into() }
                        else { format!("Niveaux acceptés par le modèle · {list}") }
                    }}</p>
                </section>
            </Show>
        </div>
    }
}

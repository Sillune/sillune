use leptos::{html, prelude::*, task::spawn_local};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/mesh_view.js")]
extern "C" {
    #[wasm_bindgen(js_name = showMesh)]
    fn show_mesh(canvas: &JsValue, source: &str) -> js_sys::Promise;
}

#[component]
pub fn MeshView(source: String) -> impl IntoView {
    let canvas = NodeRef::<html::Canvas>::new();
    let error = RwSignal::new(false);
    Effect::new(move |_| {
        if let Some(element) = canvas.get() {
            let render = show_mesh(element.as_ref(), &source);
            spawn_local(async move {
                let failed = JsFuture::from(render).await.is_err();
                error.try_set(failed);
            });
        }
    });
    view! {
        <canvas class="mesh-canvas" node_ref=canvas tabindex="0" aria-label="Aperçu interactif du maillage 3D"></canvas>
        <p class="mesh-caption">{move || if error.get() {
            "L’aperçu 3D est indisponible. Le fichier GLB reste exportable."
        } else {
            "Glisse ou utilise les flèches pour tourner. Molette, + ou − pour zoomer."
        }}</p>
    }
}

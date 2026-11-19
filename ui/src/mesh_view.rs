use leptos::{html, prelude::*};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/mesh_view.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = showMesh)]
    fn show_mesh(canvas: &JsValue, source: &str) -> Result<(), JsValue>;
}

#[component]
pub fn MeshView(source: String) -> impl IntoView {
    let canvas = NodeRef::<html::Canvas>::new();
    let error = RwSignal::new(false);
    Effect::new(move |_| {
        if let Some(element) = canvas.get() {
            error.set(show_mesh(element.as_ref(), &source).is_err());
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

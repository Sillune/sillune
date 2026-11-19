use std::cell::Cell;

thread_local! {
    static FRENCH: Cell<bool> = const { Cell::new(false) };
}

pub fn set_language(language: &str) {
    FRENCH.set(language == "fr");
    if let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        let _ = root.set_attribute("lang", if is_french() { "fr" } else { "en" });
    }
}

pub fn is_french() -> bool {
    FRENCH.get()
}

pub fn tr<'a>(french: &'a str, english: &'a str) -> &'a str {
    if is_french() { french } else { english }
}

pub fn job_text(text: String) -> String {
    if is_french() {
        return text;
    }
    const MESSAGES: &[(&str, &str)] = &[
        ("Génération locale", "Local generation"),
        ("Génération du maillage", "Mesh generation"),
        (
            "Création de l’image de référence",
            "Creating the reference image",
        ),
        ("Préparation du GPU", "Preparing the GPU"),
        ("Détourage de l’objet", "Removing the background"),
        ("Analyse de l’image", "Analyzing the image"),
        ("Construction de la structure", "Building the structure"),
        ("Construction du maillage", "Building the mesh"),
        ("Génération des textures", "Generating textures"),
        ("Création du fichier GLB", "Exporting the GLB"),
        ("Préparation de la voix LFM3", "Preparing the LFM3 voice"),
        (
            "Sillune prépare la réponse vocale",
            "Preparing the spoken reply",
        ),
        ("Transcription locale", "Local transcription"),
        ("Voix reconnue", "Voice recognized"),
        ("Réponse en cours", "Generating a reply"),
        ("Prévisions météo", "Weather forecast"),
        ("Recherche web exécutée", "Web search completed"),
        ("Recherche web", "Web search"),
        ("Opération arrêtée", "Operation stopped"),
        (
            "Choisis un dossier accessible",
            "Choose an accessible folder",
        ),
        (
            "Installe TRELLIS.2 dans Réglages → Modèles locaux",
            "Install TRELLIS.2 in Settings → Local models",
        ),
        (
            "Installe le pack Image local",
            "Install the local Image pack",
        ),
        (
            "Installe le moteur d’image dans les réglages",
            "Install the image engine in Settings",
        ),
        (
            "Installe le moteur vocal LFM3",
            "Install the LFM3 voice engine",
        ),
        ("Installe le pack voix LFM3", "Install the LFM3 voice pack"),
        ("Refus de l’utilisateur", "Denied by the user"),
        ("Écrire un fichier", "Write a file"),
        ("Lire un fichier", "Read a file"),
        ("Lister le dossier", "List a folder"),
        ("Lire une page", "Read a page"),
        ("Fichier créé", "File created"),
        ("Fichier modifié", "File edited"),
    ];
    for (source, translated) in MESSAGES {
        if text == *source {
            return (*translated).to_owned();
        }
    }
    let Some((label, detail)) = text.split_once(" · ") else {
        return text;
    };
    if let Some((_, translated)) = MESSAGES.iter().find(|(source, _)| *source == label) {
        return format!("{translated} · {detail}");
    }
    if let Some((_, translated)) = MESSAGES.iter().find(|(source, _)| *source == detail) {
        return format!("{label} · {translated}");
    }
    text
}

#[macro_export]
macro_rules! locale_format {
    ($fr:literal, $en:literal $(, $args:expr)* $(,)?) => {
        if $crate::locale::is_french() {
            format!($fr $(, $args)*)
        } else {
            format!($en $(, $args)*)
        }
    };
}

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const GUIDE: &str = include_str!("../../plugins/themes/SKILL.md");

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub scheme: String,
    pub colors: BTreeMap<String, String>,
}

pub fn builtin(id: &str) -> bool {
    matches!(id, "system" | "light" | "dark" | "cobalt")
}

pub fn parse(manifest: &str) -> Result<Theme> {
    if manifest.len() > 4500 {
        bail!("Le manifeste du thème est trop long");
    }
    let mut theme: Theme =
        serde_json::from_str(manifest).context("Manifeste JSON du thème invalide")?;
    if builtin(&theme.id)
        || !(2..=36).contains(&theme.id.len())
        || !theme
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || !theme.id.as_bytes()[0].is_ascii_lowercase()
        || theme.name.trim().is_empty()
        || theme.name.chars().count() > 48
        || theme.name.chars().any(char::is_control)
        || !matches!(theme.scheme.as_str(), "light" | "dark")
    {
        bail!("Identifiant, nom ou type de thème invalide");
    }
    for (key, color) in &theme.colors {
        if !matches!(
            key.as_str(),
            "background"
                | "sidebar"
                | "surface"
                | "text"
                | "muted"
                | "accent"
                | "accent_text"
                | "border"
                | "danger"
                | "success"
        ) || luminance(color).is_none()
        {
            bail!("Couleur invalide · {key}. Utilise #RRGGBB");
        }
    }
    for key in [
        "background",
        "sidebar",
        "surface",
        "text",
        "muted",
        "accent",
    ] {
        if !theme.colors.contains_key(key) {
            bail!("Couleur requise · {key}");
        }
    }
    for (foreground, background) in [
        ("text", "background"),
        ("text", "surface"),
        ("text", "sidebar"),
        ("muted", "background"),
        ("muted", "surface"),
        ("muted", "sidebar"),
    ] {
        if contrast(&theme.colors[foreground], &theme.colors[background]) < 4.5 {
            bail!("Contraste inférieur à 4,5:1 · {foreground} / {background}");
        }
    }
    if let Some(ink) = theme.colors.get("accent_text")
        && contrast(ink, &theme.colors["accent"]) < 4.5
    {
        bail!("Contraste inférieur à 4,5:1 · accent_text / accent");
    }
    if !theme.colors.contains_key("accent_text") {
        let accent = &theme.colors["accent"];
        let ink = if contrast("#000000", accent) >= contrast("#ffffff", accent) {
            "#000000"
        } else {
            "#ffffff"
        };
        theme.colors.insert("accent_text".into(), ink.into());
    }
    Ok(theme)
}

fn luminance(hex: &str) -> Option<f64> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |offset| {
        let value = f64::from(u8::from_str_radix(&hex[offset..offset + 2], 16).ok()?) / 255.0;
        Some(if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        })
    };
    Some(0.2126 * channel(0)? + 0.7152 * channel(2)? + 0.0722 * channel(4)?)
}

fn contrast(first: &str, second: &str) -> f64 {
    let (first, second) = (luminance(first).unwrap(), luminance(second).unwrap());
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

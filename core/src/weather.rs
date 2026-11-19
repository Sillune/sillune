use crate::provider;
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::Value;
use std::{sync::LazyLock, time::Duration};

pub fn is_weather(text: &str) -> bool {
    static WEATHER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:m[ée]t[ée]o|weather|pleut|pleuvra|pleuvoir|neigera)\b").unwrap()
    });
    WEATHER.is_match(text)
}

pub fn location(text: &str) -> Option<(&str, usize)> {
    static LOCATION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"(?i)(?:\bà|\ba|\bin|\bpour)\s+([\p{L}\d][\p{L}\d’' ,\-]*?)(?:\s+(?:aujourd'hui|aujourd’hui|demain|après-demain|tomorrow|today|ce soir)\b|[?!.]|$)"
    ).unwrap()
    });
    let lower = text.to_lowercase();
    if !is_weather(text)
        || [
            " hier",
            "yesterday",
            "semaine prochaine",
            "next week",
            " en général",
        ]
        .iter()
        .any(|word| lower.contains(word))
    {
        return None;
    }
    let target = LOCATION
        .captures(text)?
        .get(1)?
        .as_str()
        .trim()
        .trim_end_matches(',')
        .trim();
    if target.len() > 100 || target.split_whitespace().count() > 7 {
        return None;
    }
    let day = if lower.contains("après-demain") {
        2
    } else if lower.contains("demain") || lower.contains("tomorrow") {
        1
    } else {
        0
    };
    Some((target, day))
}

pub async fn forecast(city: &str, day: usize, language: &str) -> Result<String> {
    let client = provider::client()?;
    let places: Value = client
        .get("https://geocoding-api.open-meteo.com/v1/search")
        .query(&[("name", city), ("count", "5"), ("language", language)])
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let candidates = places["results"]
        .as_array()
        .context("Ville introuvable dans Open-Meteo")?;
    let sample = city
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let place = candidates
        .iter()
        .find(|place| {
            place["name"]
                .as_str()
                .is_some_and(|name| name.to_lowercase() == sample)
        })
        .or_else(|| candidates.first())
        .context("Ville introuvable dans Open-Meteo")?;
    let lat = place["latitude"]
        .as_f64()
        .context("Latitude absente")?
        .to_string();
    let lon = place["longitude"]
        .as_f64()
        .context("Longitude absente")?
        .to_string();
    let days = (day + 1).to_string();
    let url = reqwest::Url::parse_with_params(
        "https://api.open-meteo.com/v1/forecast",
        [
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            ("timezone", "auto"),
            ("forecast_days", days.as_str()),
            (
                "daily",
                "weather_code,temperature_2m_min,temperature_2m_max,precipitation_probability_max",
            ),
        ],
    )?;
    let result: Value = client
        .get(url.clone())
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let date = result["daily"]["time"][day]
        .as_str()
        .context("Date de prévision absente")?;
    let min = result["daily"]["temperature_2m_min"][day]
        .as_f64()
        .context("Température absente")?;
    let max = result["daily"]["temperature_2m_max"][day]
        .as_f64()
        .context("Température absente")?;
    let precipitation = result["daily"]["precipitation_probability_max"][day]
        .as_u64()
        .context("Probabilité de précipitations absente")?;
    let conditions = match result["daily"]["weather_code"][day].as_u64() {
        Some(0) => ("ciel dégagé", "clear skies"),
        Some(1..=3) => ("nuages", "clouds"),
        Some(45 | 48) => ("brouillard", "fog"),
        Some(51..=67 | 80..=82) => ("pluie ou averses", "rain or showers"),
        Some(71..=77 | 85 | 86) => ("neige", "snow"),
        Some(95..=99) => ("orage", "thunderstorms"),
        Some(_) => ("conditions variables", "mixed conditions"),
        None => bail!("Conditions météo absentes"),
    };
    let name = place["name"].as_str().unwrap_or(city);
    let country = place["country"].as_str().unwrap_or_default();
    let (french, english) = conditions;
    let summary = if language == "fr" {
        format!(
            "Prévision de {french}, avec des températures de {min:.0} à {max:.0} °C. La probabilité maximale de précipitations est de **{precipitation} %**.\n\n[Source Open-Meteo]({url})"
        )
    } else {
        format!(
            "Expect {english}, with temperatures from {min:.0} to {max:.0} °C. The highest chance of precipitation is **{precipitation}%**.\n\n[Open-Meteo forecast]({url})"
        )
    };
    Ok(format!("**{name} ({country}) · {date}**\n\n{summary}"))
}

#[cfg(test)]
mod tests {
    use super::location;

    #[test]
    fn recognizes_implicit_french_forecast() {
        assert_eq!(
            location("Il pleut demain à Soissons ?"),
            Some(("Soissons", 1))
        );
        assert_eq!(location("Météo à Paris après-demain"), Some(("Paris", 2)));
        assert_eq!(location("Il pleut à Soissons en général ?"), None);
    }
}

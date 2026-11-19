use serde_json::{Value, json};

struct Skill {
    id: &'static str,
    name: &'static str,
    content: &'static str,
    source: &'static str,
    license: &'static str,
    resources: &'static [(&'static str, &'static str)],
}

const SKILLS: &[Skill] = include!(concat!(env!("OUT_DIR"), "/skills.rs"));

pub fn list() -> Vec<Value> {
    SKILLS.iter().map(|skill| json!({"id":skill.id,"name":skill.name,"license":skill.license,"source":skill.source.trim()})).collect()
}

pub fn documentation(id: &str) -> Option<&'static str> {
    SKILLS
        .iter()
        .find(|skill| skill.id == id)
        .map(|skill| skill.content)
}

pub fn selected(request: &str) -> Option<&'static str> {
    let mut words = request.split_whitespace();
    while let Some(word) = words.next() {
        if word == "/skill" {
            return documentation(words.next()?);
        }
    }
    None
}

pub fn guide() -> String {
    SKILLS
        .iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn load(id: &str, resource: Option<&str>, line: usize) -> Option<String> {
    let entry = SKILLS.iter().find(|entry| entry.id == id)?;
    let source = match resource.filter(|path| !path.is_empty() && *path != "SKILL.md") {
        Some(path) => entry.resources.iter().find(|(name, _)| *name == path)?.1,
        None => entry.content,
    };
    let mut lines = source.lines().skip(line.saturating_sub(1)).peekable();
    let mut result = String::new();
    let mut count = 0;
    while let Some(next) = lines.peek() {
        if result.len() + next.len() > 11_700 {
            if count == 0 {
                result.push_str(&crate::tools::clipped(lines.next().unwrap(), 11_700));
                count += 1;
            }
            break;
        }
        result.push_str(lines.next().unwrap());
        result.push('\n');
        count += 1;
    }
    if lines.peek().is_some() {
        result.push_str(&format!(
            "\nSuite disponible avec line={}.",
            line.max(1).saturating_add(count)
        ));
    }
    if line <= 1 && resource.is_none_or(str::is_empty) && !entry.resources.is_empty() {
        result
            .push_str("\nRessources de cette fiche, disponibles via load_skill avec resource :\n");
        result.push_str(
            &entry
                .resources
                .iter()
                .map(|(path, _)| *path)
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    Some(result)
}

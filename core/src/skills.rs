use serde_json::{Value, json};

struct Skill {
    id: &'static str,
    name: &'static str,
    content: &'static str,
    source: &'static str,
    license: &'static str,
}

const SKILLS: &[Skill] = &[
    Skill {
        id: "frontend-design",
        name: "Conception d’interface · Anthropic",
        content: include_str!("../../skills/frontend-design/SKILL.md"),
        source: include_str!("../../skills/frontend-design/SOURCE.txt"),
        license: "Apache-2.0",
    },
    Skill {
        id: "test-driven-development",
        name: "Développement piloté par les tests · Hermes Agent",
        content: include_str!("../../skills/test-driven-development/SKILL.md"),
        source: include_str!("../../skills/test-driven-development/SOURCE.txt"),
        license: "MIT",
    },
];

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

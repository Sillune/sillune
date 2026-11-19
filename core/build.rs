use std::{fmt::Write, fs, path::PathBuf};

fn main() {
    let directory =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../skills");
    let manifest = directory.join("catalog.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let catalog: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    let mut generated = String::from("&[\n");
    for skill in catalog.as_array().unwrap() {
        let id = skill["id"].as_str().unwrap();
        let name = skill["name"].as_str().unwrap();
        let license = skill["license"].as_str().unwrap();
        assert!(
            id.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        );
        let include = |relative: &str| {
            assert!(relative.split('/').all(|part| {
                !part.is_empty()
                    && part != ".."
                    && part
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            }));
            println!(
                "cargo:rerun-if-changed={}",
                directory.join(id).join(relative).display()
            );
            format!(
                "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {:?}))",
                format!("/../skills/{id}/{relative}")
            )
        };
        writeln!(generated, "Skill {{ id:{id:?}, name:{name:?}, license:{license:?}, content:{}, source:{}, resources:&[", include("SKILL.md"), include("SOURCE.txt")).unwrap();
        for file in skill["resources"].as_array().unwrap() {
            let file = file.as_str().unwrap();
            writeln!(generated, "({file:?},{}),", include(file)).unwrap();
        }
        writeln!(generated, "]}},").unwrap();
    }
    generated.push_str("]\n");
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("skills.rs"),
        generated,
    )
    .unwrap();
}

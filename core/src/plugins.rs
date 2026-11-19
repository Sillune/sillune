use crate::tools::{clipped, quiet};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tempfile::NamedTempFile;
use tokio::process::Command;
use uuid::Uuid;
use wasmi::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

const GUIDE: &str = include_str!("../../plugins/SKILL.md");
const BUILTINS: &[(&str, &str)] = &[
    (
        include_str!("../../plugins/chat/plugin.toml"),
        include_str!("../../plugins/chat/README.md"),
    ),
    (
        include_str!("../../plugins/agent/plugin.toml"),
        include_str!("../../plugins/agent/README.md"),
    ),
    (
        include_str!("../../plugins/image/plugin.toml"),
        include_str!("../../plugins/image/README.md"),
    ),
    (
        include_str!("../../plugins/3d/plugin.toml"),
        include_str!("../../plugins/3d/README.md"),
    ),
    (
        include_str!("../../plugins/bot/plugin.toml"),
        include_str!("../../plugins/bot/README.md"),
    ),
    (
        include_str!("../../plugins/plugin/plugin.toml"),
        include_str!("../../plugins/plugin/README.md"),
    ),
    (
        include_str!("../../plugins/providers/plugin.toml"),
        include_str!("../../plugins/providers/README.md"),
    ),
    (
        include_str!("../../plugins/connectors/plugin.toml"),
        include_str!("../../plugins/connectors/README.md"),
    ),
    (
        include_str!("../../plugins/prompts/plugin.toml"),
        include_str!("../../plugins/prompts/README.md"),
    ),
    (
        include_str!("../../plugins/memory/plugin.toml"),
        include_str!("../../plugins/memory/README.md"),
    ),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub version: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub seconds: u32,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct Entry {
    #[serde(flatten)]
    pub manifest: Manifest,
    pub active: bool,
    pub bundled: bool,
}

pub struct Registry {
    root: PathBuf,
    entries: Mutex<BTreeMap<String, Entry>>,
}

impl Registry {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let config = root.join("active.json");
        let active = if config.is_file() {
            serde_json::from_slice::<BTreeMap<String, bool>>(&fs::read(config)?)?
        } else {
            BTreeMap::new()
        };
        let mut entries = BTreeMap::new();
        for (spec, readme) in BUILTINS {
            let manifest: Manifest = toml::from_str(spec)?;
            let path = root.join(&manifest.id);
            fs::create_dir_all(&path)?;
            write_file(&path.join("plugin.toml"), spec.as_bytes())?;
            write_file(&path.join("README.md"), readme.as_bytes())?;
            entries.insert(
                manifest.id.clone(),
                Entry {
                    manifest,
                    bundled: true,
                    active: true,
                },
            );
        }
        for dir in fs::read_dir(&root)? {
            let dir = dir?;
            if !dir.file_type()?.is_dir() || !is_id(&dir.file_name().to_string_lossy()) {
                continue;
            }
            let Ok(text) = fs::read_to_string(dir.path().join("plugin.toml")) else {
                continue;
            };
            let Ok(manifest) = Self::validate(&text) else {
                continue;
            };
            if dir.file_name().to_string_lossy() != manifest.id
                || entries.contains_key(&manifest.id)
            {
                continue;
            }
            if !dir.path().join("README.md").is_file() {
                continue;
            }
            let enabled = *active.get(&manifest.id).unwrap_or(&true);
            entries.insert(
                manifest.id.clone(),
                Entry {
                    manifest,
                    active: enabled,
                    bundled: false,
                },
            );
        }
        let registry = Self {
            root,
            entries: Mutex::new(entries),
        };
        registry.persist(&registry.entries.lock().unwrap())?;
        Ok(registry)
    }

    pub fn list(&self) -> Vec<Entry> {
        self.entries.lock().unwrap().values().cloned().collect()
    }

    pub fn enabled(&self, id: &str) -> bool {
        self.entries
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|entry| entry.active)
    }

    pub fn guide(&self) -> Result<String> {
        Ok(fs::read_to_string(self.root.join("SKILL.md"))?)
    }

    pub fn docs(&self, id: &str) -> Result<String> {
        if !is_id(id) || !self.entries.lock().unwrap().contains_key(id) {
            bail!("Plugin inconnu");
        }
        Ok(clipped(
            &fs::read_to_string(self.root.join(id).join("README.md"))?,
            6000,
        ))
    }

    pub fn validate(raw: &str) -> Result<Manifest> {
        if raw.len() > 80_000 {
            bail!("Le manifeste est trop volumineux");
        }
        let mut manifest: Manifest = toml::from_str(raw).context("Manifeste TOML invalide")?;
        manifest.title = manifest.title.trim().to_owned();
        manifest.description = manifest
            .description
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !is_id(&manifest.id)
            || !(1..=48).contains(&manifest.title.chars().count())
            || !(1..=145).contains(&manifest.description.chars().count())
            || manifest.title.chars().any(char::is_control)
            || !manifest
                .version
                .split('.')
                .all(|p| p.parse::<u32>().is_ok())
            || manifest.version.split('.').count() != 3
        {
            bail!("Identifiant, nom, description ou version invalide");
        }
        if !manifest.permissions.is_empty() {
            bail!("Ce SDK de widget ne donne pas de permission externe");
        }
        if !matches!(manifest.kind.as_str(), "tab" | "timer" | "wasm") {
            bail!("Type de plugin inconnu");
        }
        if manifest.kind == "timer" && !(1..=86400).contains(&manifest.seconds) {
            bail!("Durée de minuteur invalide");
        }
        if manifest.text.len() > 20_000 || manifest.html.len() > 60_000 {
            bail!("La vue du plugin dépasse sa taille maximale");
        }
        Ok(manifest)
    }

    pub async fn install(&self, manifest: Manifest, readme: &str, rust: &str) -> Result<()> {
        if readme.trim().is_empty() || readme.len() > 24_000 {
            bail!("Un README.md court est requis");
        }
        if self
            .entries
            .lock()
            .unwrap()
            .get(&manifest.id)
            .is_some_and(|p| p.bundled)
        {
            bail!("Cet identifiant appartient à un plugin fourni");
        }
        let stage = tempfile::Builder::new()
            .prefix(".stage-")
            .tempdir_in(&self.root)?;
        fs::write(
            stage.path().join("plugin.toml"),
            toml::to_string_pretty(&manifest)?,
        )?;
        fs::write(stage.path().join("README.md"), readme)?;
        if manifest.kind == "wasm" {
            if rust.len() > 16000 || rust.trim().is_empty() {
                bail!("Source Rust WebAssembly absente ou trop longue");
            }
            validate_rust(rust)?;
            let source = stage.path().join("lib.rs");
            let output = stage.path().join("plugin.wasm");
            fs::write(&source, rust)?;
            let mut command = Command::new("rustc");
            #[cfg(windows)]
            command.arg("+stable-x86_64-pc-windows-msvc");
            let result = tokio::time::timeout(
                Duration::from_secs(45),
                quiet(&mut command)
                    .current_dir(stage.path())
                    .args([
                        "--target",
                        "wasm32-unknown-unknown",
                        "--edition",
                        "2024",
                        "--crate-type",
                        "cdylib",
                        "-C",
                        "opt-level=s",
                        "-C",
                        "panic=abort",
                    ])
                    .arg(&source)
                    .arg("-o")
                    .arg(&output)
                    .output(),
            )
            .await??;
            if !result.status.success() {
                bail!(
                    "Compilation du plugin impossible\n{}",
                    clipped(&String::from_utf8_lossy(&result.stderr), 3000)
                );
            }
            evaluate(&fs::read(output)?, 0, 0)?;
        }
        let mut entries = self.entries.lock().unwrap();
        let current = self.root.join(&manifest.id);
        let backup = self.root.join(format!(".rollback-{}", Uuid::new_v4()));
        let old = entries.get(&manifest.id).cloned();
        if current.exists() {
            fs::rename(&current, &backup)?;
        }
        if let Err(error) = fs::rename(stage.path(), &current) {
            if backup.exists() {
                let _ = fs::rename(&backup, &current);
            }
            return Err(error.into());
        }
        let id = manifest.id.clone();
        entries.insert(
            id.clone(),
            Entry {
                manifest,
                active: true,
                bundled: false,
            },
        );
        if let Err(error) = self.persist(&entries) {
            let _ = fs::remove_dir_all(&current);
            if backup.exists() {
                let _ = fs::rename(&backup, &current);
            }
            if let Some(old) = old {
                entries.insert(id.clone(), old);
            } else {
                entries.remove(&id);
            }
            let _ = self.persist(&entries);
            return Err(error);
        }
        if backup.exists() {
            let _ = fs::remove_dir_all(&backup);
        }
        Ok(())
    }

    pub fn activate(&self, id: &str, active: bool) -> Result<()> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.get_mut(id).context("Plugin inconnu")?;
        if entry.bundled {
            bail!("Le plugin fait partie de l’application");
        }
        let before = entry.active;
        entry.active = active;
        if let Err(error) = self.persist(&entries) {
            if let Some(entry) = entries.get_mut(id) {
                entry.active = before;
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.get(id).context("Plugin inconnu")?;
        if entry.bundled {
            bail!("Le plugin fait partie de l’application");
        }
        let current = self.root.join(id);
        let backup = self.root.join(format!(".removed-{}", Uuid::new_v4()));
        fs::rename(&current, &backup)?;
        let previous = entries.remove(id).unwrap();
        if let Err(error) = self.persist(&entries) {
            let _ = fs::rename(&backup, &current);
            entries.insert(id.into(), previous);
            let _ = self.persist(&entries);
            return Err(error);
        }
        fs::remove_dir_all(backup)?;
        Ok(())
    }

    pub fn tick(&self, id: &str, elapsed: i32, value: i32) -> Result<i32> {
        let entries = self.entries.lock().unwrap();
        let entry = entries
            .get(id)
            .filter(|entry| entry.active && entry.manifest.kind == "wasm")
            .context("Plugin WebAssembly inactif")?;
        let bytes = fs::read(self.root.join(&entry.manifest.id).join("plugin.wasm"))?;
        evaluate(&bytes, elapsed, value)
    }

    fn persist(&self, entries: &BTreeMap<String, Entry>) -> Result<()> {
        let mut index = GUIDE.trim_end().to_owned();
        index.push_str("\n\n");
        let active: BTreeMap<&str, bool> = entries
            .iter()
            .map(|(id, p)| (id.as_str(), p.active))
            .collect();
        for p in entries.values() {
            let m = &p.manifest;
            index.push_str(&format!(
                "- {} — {} — `plugins/{}/`\n",
                m.title, m.description, m.id
            ));
        }
        write_file(&self.root.join("SKILL.md"), index.as_bytes())?;
        write_file(
            &self.root.join("active.json"),
            &serde_json::to_vec_pretty(&active)?,
        )
    }
}

fn validate_rust(source: &str) -> Result<()> {
    use syn::visit::Visit;
    struct Guest(bool);
    impl<'a> Visit<'a> for Guest {
        fn visit_macro(&mut self, _: &'a syn::Macro) {
            self.0 = false;
        }
        fn visit_attribute(&mut self, a: &'a syn::Attribute) {
            if !(a.path().is_ident("unsafe")
                && a.meta
                    .require_list()
                    .is_ok_and(|list| list.tokens.to_string() == "no_mangle"))
            {
                self.0 = false;
            }
        }
        fn visit_item_mod(&mut self, _: &'a syn::ItemMod) {
            self.0 = false;
        }
        fn visit_item_extern_crate(&mut self, _: &'a syn::ItemExternCrate) {
            self.0 = false;
        }
        fn visit_item_foreign_mod(&mut self, _: &'a syn::ItemForeignMod) {
            self.0 = false;
        }
    }
    let ast = syn::parse_file(source).context("Source Rust invalide")?;
    let mut guest = Guest(true);
    guest.visit_file(&ast);
    if !guest.0 {
        bail!(
            "Le SDK attend un seul fichier Rust autonome avec l’export sillune_tick, sans macro, module externe ni attribut autre que no_mangle"
        );
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut temp = NamedTempFile::new_in(path.parent().context("Dossier du plugin inaccessible")?)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}

fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 45
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !matches!(id, "con" | "nul" | "aux" | "prn")
}

fn evaluate(bytes: &[u8], elapsed: i32, value: i32) -> Result<i32> {
    if bytes.len() > 2 * 1024 * 1024 {
        bail!("Module WebAssembly trop volumineux");
    }
    let engine = Engine::new(Config::default().consume_fuel(true));
    let module = Module::new(&engine, bytes)?;
    let limits = StoreLimitsBuilder::new()
        .memory_size(4 * 1024 * 1024)
        .instances(1)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|s: &mut StoreLimits| s);
    store.set_fuel(150_000)?;
    let instance = Linker::new(&engine)
        .instantiate(&mut store, &module)?
        .start(&mut store)?;
    let run = instance.get_typed_func::<(i32, i32), i32>(&store, "sillune_tick")?;
    Ok(run.call(&mut store, (elapsed, value))?)
}

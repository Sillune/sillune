use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path, sync::Mutex};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub provider: String,
    pub model: String,
    pub url: String,
    pub workspace: String,
    pub instructions: String,
    pub search: String,
    pub searx_url: String,
    pub theme: String,
    pub permission: String,
    pub memory: bool,
    pub mcp: Vec<McpConfig>,
    pub engines: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    pub name: String,
    pub url: String,
    pub command: String,
    pub args: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "openrouter".into(),
            model: "openrouter/auto".into(),
            url: String::new(),
            workspace: String::new(),
            instructions: String::new(),
            search: "openrouter".into(),
            searx_url: String::new(),
            theme: "system".into(),
            permission: "ask".into(),
            memory: false,
            mcp: Vec::new(),
            engines: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Chat,
    Agent,
    Image,
    Bot,
    Plugin,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Agent => "agent",
            Self::Image => "image",
            Self::Bot => "bot",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub mode: Mode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub files: Vec<Artifact>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub name: String,
    pub media: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub kind: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UndoEntry {
    pub path: String,
    pub before: Option<Vec<u8>>,
    pub after: Option<String>,
}

pub struct Store(Mutex<Connection>);

impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let db = Connection::open(root.join("sillune.sqlite"))?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON;
             CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK (id=1), data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL, mode TEXT NOT NULL, updated INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS messages (id INTEGER PRIMARY KEY, session TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, role TEXT NOT NULL, text TEXT NOT NULL, files TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS message_session ON messages(session,id);
             CREATE VIRTUAL TABLE IF NOT EXISTS recall USING fts5(session UNINDEXED, text, tokenize='unicode61 remove_diacritics 2');
             CREATE TABLE IF NOT EXISTS artifacts (id TEXT PRIMARY KEY, name TEXT NOT NULL, media TEXT NOT NULL, data BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS memory (id TEXT PRIMARY KEY, kind TEXT NOT NULL, content TEXT UNIQUE NOT NULL);
             CREATE TABLE IF NOT EXISTS summaries (session TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE, through INTEGER NOT NULL, body TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS changes (id INTEGER PRIMARY KEY, session TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, turn TEXT NOT NULL, root TEXT NOT NULL, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS kv (name TEXT PRIMARY KEY, data TEXT NOT NULL);",
        )?;
        Ok(Self(Mutex::new(db)))
    }

    pub fn settings(&self) -> Result<Settings> {
        let db = self.0.lock().unwrap();
        let row = db
            .query_row("SELECT data FROM settings WHERE id=1", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(row
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default())
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO settings VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            [serde_json::to_string(settings)?],
        )?;
        Ok(())
    }

    pub fn sessions(&self) -> Result<Vec<Session>> {
        let db = self.0.lock().unwrap();
        let mut query = db.prepare(
            "SELECT id,title,mode FROM sessions ORDER BY updated DESC,rowid DESC LIMIT 250",
        )?;
        let rows = query.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, title, mode) = row?;
            Ok(Session {
                id,
                title,
                mode: serde_json::from_str(&mode)?,
            })
        })
        .collect()
    }

    pub fn create_session(&self, mode: Mode, title: &str) -> Result<Session> {
        let session = Session {
            id: Uuid::new_v4().to_string(),
            title: title.chars().take(58).collect(),
            mode,
        };
        self.0.lock().unwrap().execute(
            "INSERT INTO sessions VALUES(?1,?2,?3,unixepoch())",
            params![session.id, session.title, serde_json::to_string(&mode)?],
        )?;
        Ok(session)
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        let mut db = self.0.lock().unwrap();
        let tx = db.transaction()?;
        {
            let mut select = tx.prepare("SELECT files FROM messages WHERE session=?1")?;
            let rows = select.query_map([id], |r| r.get::<_, String>(0))?;
            for row in rows {
                for file in serde_json::from_str::<Vec<Artifact>>(&row?)? {
                    tx.execute("DELETE FROM artifacts WHERE id=?1", [file.id])?;
                }
            }
        }
        tx.execute("DELETE FROM recall WHERE session=?1", [id])?;
        tx.execute("DELETE FROM sessions WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn append(&self, session: &str, message: &Message) -> Result<()> {
        let mut db = self.0.lock().unwrap();
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO messages(session,role,text,files) VALUES(?1,?2,?3,?4)",
            params![
                session,
                message.role,
                message.text,
                serde_json::to_string(&message.files)?
            ],
        )?;
        if !message.text.is_empty() && !contains_secret(&message.text) {
            tx.execute(
                "INSERT INTO recall(session,text) VALUES(?1,?2)",
                params![session, message.text],
            )?;
        }
        tx.execute(
            "UPDATE sessions SET updated=unixepoch() WHERE id=?1",
            [session],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn messages(&self, session: &str) -> Result<Vec<(i64, Message)>> {
        let db = self.0.lock().unwrap();
        let mut query =
            db.prepare("SELECT id,role,text,files FROM messages WHERE session=?1 ORDER BY id")?;
        let rows = query.query_map([session], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (id, role, text, file_json) = row?;
            Ok((
                id,
                Message {
                    role,
                    text,
                    files: serde_json::from_str(&file_json)?,
                },
            ))
        })
        .collect()
    }

    pub fn summary(&self, session: &str) -> Result<(i64, String)> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT through,body FROM summaries WHERE session=?1",
                [session],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn set_summary(&self, session: &str, through: i64, summary: &str) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO summaries VALUES(?1,?2,?3) ON CONFLICT(session) DO UPDATE SET through=excluded.through,body=excluded.body",
            params![session, through, summary],
        )?;
        Ok(())
    }

    pub fn recall(&self, query: &str, current: &str) -> Result<Vec<String>> {
        let phrase = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.chars().count() > 2)
            .take(12)
            .map(|word| format!("\"{word}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        if phrase.is_empty() {
            return Ok(Vec::new());
        }
        let db = self.0.lock().unwrap();
        let mut statement = db.prepare(
            "SELECT snippet(recall,1,'','',' … ',24) FROM recall WHERE recall MATCH ?1 AND session!=?2 ORDER BY rank LIMIT 4",
        )?;
        Ok(statement
            .query_map(params![phrase, current], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn notes(&self) -> Result<Vec<Note>> {
        let db = self.0.lock().unwrap();
        let mut query =
            db.prepare("SELECT id,kind,content FROM memory ORDER BY rowid DESC LIMIT 50")?;
        Ok(query
            .query_map([], |r| {
                Ok(Note {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    content: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_note(&self, id: Option<&str>, kind: &str, content: &str) -> Result<()> {
        if !matches!(kind, "preference" | "fact" | "method")
            || !(1..=550).contains(&content.chars().count())
            || contains_secret(content)
        {
            bail!("Note vide, trop longue ou contenant une donnée sensible");
        }
        let id = id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.0.lock().unwrap().execute(
            "INSERT INTO memory VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET kind=excluded.kind,content=excluded.content ON CONFLICT(content) DO NOTHING",
            params![id, kind, content.trim()],
        )?;
        Ok(())
    }

    pub fn forget(&self, id: Option<&str>) -> Result<()> {
        let db = self.0.lock().unwrap();
        match id {
            Some(id) => {
                db.execute("DELETE FROM memory WHERE id=?1", [id])?;
            }
            None => {
                db.execute("DELETE FROM memory", [])?;
            }
        }
        Ok(())
    }

    pub fn add_artifact(&self, name: &str, media: &str, data: &[u8]) -> Result<Artifact> {
        if data.len() > 64 * 1024 * 1024 {
            bail!("La pièce jointe dépasse 64 Mio");
        }
        let name = name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("fichier")
            .chars()
            .filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
            .take(160)
            .collect::<String>();
        let name = if name.trim_matches([' ', '.']).is_empty() {
            "fichier".into()
        } else {
            name
        };
        let artifact = Artifact {
            id: Uuid::new_v4().to_string(),
            name,
            media: media.to_owned(),
        };
        self.0.lock().unwrap().execute(
            "INSERT INTO artifacts VALUES(?1,?2,?3,?4)",
            params![artifact.id, artifact.name, artifact.media, data],
        )?;
        Ok(artifact)
    }

    pub fn artifact(&self, id: &str) -> Result<(Artifact, Vec<u8>)> {
        Ok(self.0.lock().unwrap().query_row(
            "SELECT name,media,data FROM artifacts WHERE id=?1",
            [id],
            |r| {
                Ok((
                    Artifact {
                        id: id.into(),
                        name: r.get(0)?,
                        media: r.get(1)?,
                    },
                    r.get(2)?,
                ))
            },
        )?)
    }

    pub fn add_change(
        &self,
        session: &str,
        turn: &str,
        root: &str,
        edit: &UndoEntry,
    ) -> Result<i64> {
        let db = self.0.lock().unwrap();
        db.execute(
            "INSERT INTO changes(session,turn,root,data) VALUES(?1,?2,?3,?4)",
            params![session, turn, root, serde_json::to_string(edit)?],
        )?;
        Ok(db.last_insert_rowid())
    }

    pub fn last_changes(&self, session: &str) -> Result<Vec<(i64, String, UndoEntry)>> {
        let db = self.0.lock().unwrap();
        let mut query = db.prepare(
            "SELECT id,root,data FROM changes WHERE session=?1 AND turn=(SELECT turn FROM changes WHERE session=?1 ORDER BY id DESC LIMIT 1) ORDER BY id DESC",
        )?;
        let rows = query.query_map([session], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, root, edit) = row?;
            Ok((id, root, serde_json::from_str(&edit)?))
        })
        .collect()
    }

    pub fn clear_change(&self, id: i64) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("DELETE FROM changes WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn kv(&self, key: &str) -> Result<Option<Value>> {
        let value = self
            .0
            .lock()
            .unwrap()
            .query_row("SELECT data FROM kv WHERE name=?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        Ok(value.map(|data| serde_json::from_str(&data)).transpose()?)
    }

    pub fn set_kv(&self, key: &str, value: &Value) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO kv VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET data=excluded.data",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }
}

pub fn contains_secret(text: &str) -> bool {
    use std::sync::LazyLock;
    static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r"(?i)\b(?:sk-(?:or-v\d+-|proj-)?[a-z0-9_-]{20,}|gh[pousr]_[a-z0-9]{18,}|github_pat_[a-z0-9_]{18,}|AIza[a-z0-9_-]{30,}|ya29\.[a-z0-9_-]{20,}|Bearer\s+[a-z0-9_.-]{20,})\b",
    ).unwrap()
    });
    PATTERN.is_match(text) || text.contains("PRIVATE KEY-----")
}

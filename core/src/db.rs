//! Everything farol keeps, in one SQLite file: settings, aliases, usage
//! (for ranking), quicklinks, hotkeys, timers and the clipboard history.
//!
//! The file is `$FAROL_DB`, else the platform's data dir (`farol/farol.db`
//! under `$XDG_DATA_HOME`, `~/Library/Application Support` or `%APPDATA%`).
//! Queries are small and indexed: they run on the service thread.

use std::path::PathBuf;
use std::rc::Rc;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

const SCHEMA: &[&str] = &[
    // 1
    "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
     CREATE TABLE aliases (alias TEXT PRIMARY KEY, target TEXT NOT NULL);
     CREATE TABLE usage (id TEXT PRIMARY KEY, count INTEGER NOT NULL, last INTEGER NOT NULL);
     CREATE TABLE quicklinks (name TEXT PRIMARY KEY, url TEXT NOT NULL, keyword TEXT NOT NULL DEFAULT '');
     CREATE TABLE hotkeys (keys TEXT PRIMARY KEY, query TEXT NOT NULL);
     CREATE TABLE timers (
        id INTEGER PRIMARY KEY, label TEXT NOT NULL,
        duration INTEGER NOT NULL, ends INTEGER NOT NULL, left INTEGER NOT NULL,
        paused INTEGER NOT NULL DEFAULT 0);
     CREATE TABLE clipboard (
        id INTEGER PRIMARY KEY, hash INTEGER NOT NULL UNIQUE, mime TEXT NOT NULL,
        text TEXT NOT NULL, data BLOB, at INTEGER NOT NULL, pinned INTEGER NOT NULL DEFAULT 0);
     CREATE INDEX clipboard_at ON clipboard(at);",
];

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("FAROL_DB") {
        return p.into();
    }
    crate::platform::data_dir().join("farol").join("farol.db")
}

pub type Db = Rc<Store>;

pub struct Store {
    conn: Connection,
}

/// Milliseconds since the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Store {
    pub fn open() -> rusqlite::Result<Db> {
        let path = path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::with(Connection::open(path)?)
    }

    pub fn memory() -> Db {
        Self::with(Connection::open_in_memory().expect("in-memory sqlite")).expect("schema")
    }

    fn with(conn: Connection) -> rusqlite::Result<Db> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in SCHEMA.iter().enumerate().skip(version as usize) {
            conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;", i + 1))?;
        }
        Ok(Rc::new(Self { conn }))
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ── settings ────────────────────────────────────────────────────────

    /// A setting as JSON, `None` when unset.
    pub fn setting(&self, key: &str) -> Option<Value> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get::<_, String>(0))
            .optional()
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.setting(key)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or(default)
    }

    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> rusqlite::Result<()> {
        let text = serde_json::to_string(value).unwrap_or_else(|_| "null".into());
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, text],
        )?;
        Ok(())
    }

    pub fn unset(&self, key: &str) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }

    pub fn settings(&self) -> Vec<(String, Value)> {
        self.rows("SELECT key, value FROM settings ORDER BY key", [], |r| {
            let v: String = r.get(1)?;
            Ok((r.get(0)?, serde_json::from_str(&v).unwrap_or(Value::Null)))
        })
    }

    // ── aliases ─────────────────────────────────────────────────────────

    /// `alias` → an item id (`apps:helium.desktop`) or a search text.
    pub fn set_alias(&self, alias: &str, target: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO aliases (alias, target) VALUES (?1, ?2)
             ON CONFLICT(alias) DO UPDATE SET target = excluded.target",
            params![alias.trim().to_lowercase(), target.trim()],
        )?;
        Ok(())
    }

    pub fn remove_alias(&self, alias: &str) -> rusqlite::Result<bool> {
        Ok(self.conn.execute("DELETE FROM aliases WHERE alias = ?1", [alias.trim().to_lowercase()])? > 0)
    }

    pub fn aliases(&self) -> Vec<(String, String)> {
        self.rows("SELECT alias, target FROM aliases ORDER BY alias", [], |r| Ok((r.get(0)?, r.get(1)?)))
    }

    // ── usage ───────────────────────────────────────────────────────────

    pub fn used(&self, id: &str) {
        let _ = self.conn.execute(
            "INSERT INTO usage (id, count, last) VALUES (?1, 1, ?2)
             ON CONFLICT(id) DO UPDATE SET count = count + 1, last = excluded.last",
            params![id, now()],
        );
    }

    /// Every id with its (count, last), for ranking.
    pub fn usage(&self) -> Vec<(String, i64, i64)> {
        self.rows("SELECT id, count, last FROM usage", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
    }

    // ── quicklinks and hotkeys ──────────────────────────────────────────

    /// `url` may hold `{query}` (filled with what follows the name).
    pub fn set_quicklink(&self, name: &str, url: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO quicklinks (name, url) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET url = excluded.url",
            params![name.trim(), url.trim()],
        )?;
        Ok(())
    }

    pub fn remove_quicklink(&self, name: &str) -> rusqlite::Result<bool> {
        Ok(self.conn.execute("DELETE FROM quicklinks WHERE name = ?1", [name.trim()])? > 0)
    }

    pub fn quicklinks(&self) -> Vec<(String, String)> {
        self.rows("SELECT name, url FROM quicklinks ORDER BY name", [], |r| Ok((r.get(0)?, r.get(1)?)))
    }

    /// `keys` like `super+alt+space`; `query` is what the launcher opens
    /// with (`""`, `"="`, `">"`), or `!run <item id>` to run without opening.
    pub fn set_hotkey(&self, keys: &str, query: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO hotkeys (keys, query) VALUES (?1, ?2)
             ON CONFLICT(keys) DO UPDATE SET query = excluded.query",
            params![normalize_keys(keys), query],
        )?;
        Ok(())
    }

    pub fn remove_hotkey(&self, keys: &str) -> rusqlite::Result<bool> {
        Ok(self.conn.execute("DELETE FROM hotkeys WHERE keys = ?1", [normalize_keys(keys)])? > 0)
    }

    pub fn hotkeys(&self) -> Vec<(String, String)> {
        self.rows("SELECT keys, query FROM hotkeys ORDER BY keys", [], |r| Ok((r.get(0)?, r.get(1)?)))
    }

    pub fn rows<T, P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
        f: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> Vec<T> {
        let Ok(mut stmt) = self.conn.prepare_cached(sql) else {
            return Vec::new();
        };
        stmt.query_map(params, f)
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }
}

/// `Super + Alt + Space` → `super+alt+space`, modifiers in a fixed order.
pub fn normalize_keys(keys: &str) -> String {
    let mut parts: Vec<String> = keys
        .split('+')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .map(|p| match p.as_str() {
            "mod" | "win" | "cmd" | "meta" | "logo" | "mod4" => "super".into(),
            "control" => "ctrl".into(),
            "option" | "opt" => "alt".into(),
            _ => p,
        })
        .collect();
    let rank = |p: &str| ["super", "ctrl", "alt", "shift"].iter().position(|m| *m == p).unwrap_or(9);
    let key = parts.iter().position(|p| rank(p) == 9).map(|i| parts.remove(i));
    parts.sort_by_key(|p| rank(p));
    parts.dedup();
    parts.extend(key);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let db = Store::memory();
        assert_eq!(db.get("terminal", String::from("x")), "x");
        db.set("terminal", &"ghostty").unwrap();
        assert_eq!(db.get("terminal", String::new()), "ghostty");
        db.unset("terminal").unwrap();
        assert!(db.setting("terminal").is_none());
    }

    #[test]
    fn aliases_are_lowercase_and_replace() {
        let db = Store::memory();
        db.set_alias("Zap", "whatsapp").unwrap();
        db.set_alias("zap", "apps:whatsapp.desktop").unwrap();
        assert_eq!(db.aliases(), vec![("zap".into(), "apps:whatsapp.desktop".into())]);
        assert!(db.remove_alias("ZAP").unwrap());
        assert!(db.aliases().is_empty());
    }

    #[test]
    fn usage_counts() {
        let db = Store::memory();
        db.used("a");
        db.used("a");
        let u = db.usage();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].1, 2);
    }

    #[test]
    fn keys_normalize() {
        assert_eq!(normalize_keys("Space + Alt + SUPER"), "super+alt+space");
        assert_eq!(normalize_keys("cmd+shift+v"), "super+shift+v");
    }
}

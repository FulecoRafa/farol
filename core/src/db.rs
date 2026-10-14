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
    // 2: image dimensions, read once when recorded (rows never load blobs).
    "ALTER TABLE clipboard ADD COLUMN width INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE clipboard ADD COLUMN height INTEGER NOT NULL DEFAULT 0;",
];

/// A clipboard entry as listed: no blob, the text cut to its start.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: i64,
    pub hash: i64,
    pub mime: String,
    /// The first [`CLIP_HEAD`] characters (empty for images).
    pub text: String,
    /// Bytes of the whole entry.
    pub size: i64,
    pub width: i64,
    pub height: i64,
    pub at: i64,
    pub pinned: bool,
}

/// Characters of text a listed [`Clip`] carries.
pub const CLIP_HEAD: usize = 400;

/// What a recorded selection was: text, or an image's bytes.
pub enum ClipData<'a> {
    Text(&'a str),
    Image { mime: &'a str, data: &'a [u8], width: i64, height: i64 },
}

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

    // ── clipboard ───────────────────────────────────────────────────────

    /// Record a selection at `at`: a copy already kept moves to the top
    /// (same hash), then the oldest unpinned go until at most `limit` are
    /// left (pinned ones always stay). Returns the hashes evicted.
    pub fn clip_record(&self, hash: i64, clip: ClipData, at: i64, limit: usize) -> rusqlite::Result<Vec<i64>> {
        let (mime, text, data, w, h) = match clip {
            ClipData::Text(t) => ("text/plain", t, None, 0, 0),
            ClipData::Image { mime, data, width, height } => (mime, "", Some(data), width, height),
        };
        self.conn.execute(
            "INSERT INTO clipboard (hash, mime, text, data, at, width, height) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(hash) DO UPDATE SET at = excluded.at",
            params![hash, mime, text, data, at, w, h],
        )?;
        self.clip_trim(limit)
    }

    /// Drop the oldest unpinned entries beyond `limit` (counting pinned).
    pub fn clip_trim(&self, limit: usize) -> rusqlite::Result<Vec<i64>> {
        let mut stmt = self.conn.prepare_cached(
            "DELETE FROM clipboard WHERE pinned = 0 AND id NOT IN (
                SELECT id FROM clipboard WHERE pinned = 0 ORDER BY at DESC, id DESC
                LIMIT max(?1 - (SELECT count(*) FROM clipboard WHERE pinned = 1), 0))
             RETURNING hash",
        )?;
        let rows = stmt.query_map([limit as i64], |r| r.get(0))?;
        rows.collect()
    }

    /// Entries without their blobs: pinned first, then newest first.
    pub fn clips(&self, limit: usize) -> Vec<Clip> {
        self.rows(
            "SELECT id, hash, mime, substr(text, 1, ?1), length(CAST(coalesce(data, text) AS BLOB)),
                    width, height, at, pinned
             FROM clipboard ORDER BY pinned DESC, at DESC, id DESC LIMIT ?2",
            params![CLIP_HEAD as i64, limit as i64],
            |r| {
                Ok(Clip {
                    id: r.get(0)?,
                    hash: r.get(1)?,
                    mime: r.get(2)?,
                    text: r.get(3)?,
                    size: r.get(4)?,
                    width: r.get(5)?,
                    height: r.get(6)?,
                    at: r.get(7)?,
                    pinned: r.get(8)?,
                })
            },
        )
    }

    /// One entry whole: its listing, the full text and the blob.
    pub fn clip(&self, id: i64) -> Option<(Clip, String, Option<Vec<u8>>)> {
        self.conn
            .query_row(
                "SELECT id, hash, mime, text, length(CAST(coalesce(data, text) AS BLOB)),
                        width, height, at, pinned, data
                 FROM clipboard WHERE id = ?1",
                [id],
                |r| {
                    let text: String = r.get(3)?;
                    let clip = Clip {
                        id: r.get(0)?,
                        hash: r.get(1)?,
                        mime: r.get(2)?,
                        text: text.chars().take(CLIP_HEAD).collect(),
                        size: r.get(4)?,
                        width: r.get(5)?,
                        height: r.get(6)?,
                        at: r.get(7)?,
                        pinned: r.get(8)?,
                    };
                    Ok((clip, text, r.get(9)?))
                },
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Forget an entry; its hash, if it existed.
    pub fn clip_delete(&self, id: i64) -> Option<i64> {
        self.conn
            .query_row("DELETE FROM clipboard WHERE id = ?1 RETURNING hash", [id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    /// Flip an entry's pin; the new state.
    pub fn clip_pin(&self, id: i64) -> Option<bool> {
        self.conn
            .query_row("UPDATE clipboard SET pinned = 1 - pinned WHERE id = ?1 RETURNING pinned", [id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    /// Move an entry to the top (it was copied again).
    pub fn clip_touch(&self, id: i64, at: i64) {
        let _ = self.conn.execute("UPDATE clipboard SET at = ?2 WHERE id = ?1", params![id, at]);
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

    fn texts(db: &Store) -> Vec<String> {
        db.clips(100).into_iter().map(|c| c.text).collect()
    }

    #[test]
    fn clipboard_dedupes_to_the_top() {
        let db = Store::memory();
        db.clip_record(1, ClipData::Text("a"), 10, 10).unwrap();
        db.clip_record(2, ClipData::Text("b"), 20, 10).unwrap();
        assert_eq!(texts(&db), ["b", "a"]);
        // Copied again: one row, now the newest, same id.
        let id = db.clips(10)[1].id;
        db.clip_record(1, ClipData::Text("a"), 30, 10).unwrap();
        assert_eq!(texts(&db), ["a", "b"]);
        assert_eq!(db.clips(10)[0].id, id);
    }

    #[test]
    fn clipboard_limit_spares_pinned() {
        let db = Store::memory();
        for i in 0..5 {
            db.clip_record(i, ClipData::Text(&i.to_string()), i * 10, 10).unwrap();
        }
        let oldest = db.clips(10).last().unwrap().id;
        assert_eq!(db.clip_pin(oldest), Some(true));
        // Limit 3 with one pinned: the pinned "0" plus the two newest.
        let evicted = db.clip_record(5, ClipData::Text("5"), 60, 3).unwrap();
        assert_eq!(texts(&db), ["0", "5", "4"]);
        assert_eq!(evicted.len(), 3);
        // Pinned survive even a limit below their count.
        db.clip_trim(0).unwrap();
        assert_eq!(texts(&db), ["0"]);
        assert_eq!(db.clip_pin(oldest), Some(false));
        db.clip_trim(0).unwrap();
        assert!(texts(&db).is_empty());
    }

    #[test]
    fn clipboard_images_and_delete() {
        let db = Store::memory();
        let png = [1u8, 2, 3];
        db.clip_record(7, ClipData::Image { mime: "image/png", data: &png, width: 4, height: 2 }, 1, 10).unwrap();
        let c = &db.clips(10)[0];
        assert_eq!((c.mime.as_str(), c.size, c.width, c.height), ("image/png", 3, 4, 2));
        let (_, _, data) = db.clip(c.id).unwrap();
        assert_eq!(data.as_deref(), Some(&png[..]));
        assert_eq!(db.clip_delete(c.id), Some(7));
        assert_eq!(db.clip_delete(c.id), None);
        // Text size is bytes, not characters.
        db.clip_record(8, ClipData::Text("ção"), 2, 10).unwrap();
        assert_eq!(db.clips(10)[0].size, 5);
    }

    #[test]
    fn keys_normalize() {
        assert_eq!(normalize_keys("Space + Alt + SUPER"), "super+alt+space");
        assert_eq!(normalize_keys("cmd+shift+v"), "super+shift+v");
    }
}

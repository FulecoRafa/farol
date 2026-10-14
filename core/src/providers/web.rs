//! The web: quicklinks (`link gh https://github.com/search?q={query}`, then
//! `gh tessela`), the browser's bookmarks (Helium by default, any Chromium
//! profile through `web.bookmarks`), and a web search for any text.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use serde_json::Value;

use crate::db::Db;
use crate::engine::{Provider, Query};
use crate::item::{Action, Item, Outcome};
use crate::platform::{self, home};

pub struct Web {
    db: Db,
    /// Bookmark files with their stamps, the quicklinks, and their rows.
    marks: RefCell<(Vec<(PathBuf, Option<SystemTime>)>, Vec<(String, String)>, Rc<Vec<Item>>)>,
    /// URL of the row `generate` produced last.
    pending: RefCell<Option<String>>,
}

/// Helium's bookmarks file on this OS.
fn helium_bookmarks() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/net.imput.helium/Default/Bookmarks")
    } else if cfg!(windows) {
        platform::cache_dir().join("imput/Helium/User Data/Default/Bookmarks")
    } else {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
        config.join("net.imput.helium/Default/Bookmarks")
    }
}

/// Percent-encoding for a query string value.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `https://github.com/x` → `github.com`.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split(['/', '?', '#']).next().unwrap_or(rest).trim_start_matches("www.")
}

/// Every URL in a Chromium bookmarks file, with its folder path.
fn read_bookmarks(path: &PathBuf) -> Vec<Item> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let Ok(json) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    let mut out = Vec::new();
    fn walk(node: &Value, folder: &str, out: &mut Vec<Item>) {
        match node.get("type").and_then(Value::as_str) {
            Some("url") => {
                let url = node.get("url").and_then(Value::as_str).unwrap_or_default();
                let name = node.get("name").and_then(Value::as_str).unwrap_or(url);
                out.push(Item {
                    id: format!("web:{url}"),
                    title: name.to_string(),
                    subtitle: if folder.is_empty() { host(url).to_string() } else { format!("{} · {folder}", host(url)) },
                    icon: "ph:star".into(),
                    keywords: url.to_string(),
                    actions: vec![Action::new("open", "abrir", "enter"), Action::new("copy", "copiar link", "ctrl+c")],
                    ..Item::default()
                });
            }
            _ => {
                let name = node.get("name").and_then(Value::as_str).unwrap_or_default();
                let inner = if folder.is_empty() { name.to_string() } else { format!("{folder} › {name}") };
                for child in node.get("children").and_then(Value::as_array).into_iter().flatten() {
                    walk(child, &inner, out);
                }
            }
        }
    }
    for root in json.get("roots").and_then(Value::as_object).into_iter().flat_map(|o| o.values()) {
        // The roots ("Bookmarks bar", "Other") are not worth showing.
        for child in root.get("children").and_then(Value::as_array).into_iter().flatten() {
            walk(child, "", &mut out);
        }
    }
    out
}

impl Web {
    pub fn new(db: Db) -> Self {
        let me = Self { db, marks: RefCell::new((Vec::new(), Vec::new(), Rc::default())), pending: RefCell::new(None) };
        me.reload();
        me
    }

    fn files(&self) -> Vec<PathBuf> {
        let custom: Vec<String> = self.db.get("web.bookmarks", Vec::new());
        if custom.is_empty() { vec![helium_bookmarks()] } else { custom.into_iter().map(PathBuf::from).collect() }
    }

    /// Re-read the bookmarks when a file changed (a stat per open) or the
    /// quicklinks did.
    fn reload(&self) {
        let stamps: Vec<(PathBuf, Option<SystemTime>)> =
            self.files().into_iter().map(|f| { let m = f.metadata().and_then(|m| m.modified()).ok(); (f, m) }).collect();
        let quick = self.db.quicklinks();
        if self.marks.borrow().0 == stamps && self.marks.borrow().1 == quick {
            return;
        }
        let mut items: Vec<Item> = stamps.iter().flat_map(|(f, _)| read_bookmarks(f)).collect();
        items.extend(quick.iter().cloned().filter(|(_, u)| !u.contains("{query}")).map(|(name, url)| Item {
            id: format!("web:{url}"),
            title: name.clone(),
            subtitle: format!("link rápido · {}", host(&url)),
            icon: "ph:link".into(),
            keywords: url.clone(),
            actions: vec![Action::new("open", "abrir", "enter"), Action::new("copy", "copiar link", "ctrl+c")],
            ..Item::default()
        }));
        *self.marks.borrow_mut() = (stamps, quick, Rc::new(items));
    }

    fn open(&self, url: &str) -> std::io::Result<()> {
        let browser: Vec<String> = self.db.get("browser", Vec::new());
        if browser.is_empty() {
            platform::open(url)
        } else {
            let mut argv = browser;
            argv.push(url.to_string());
            platform::spawn(&argv)
        }
    }

    /// "buscar … na web" with the search engine from `web.search`.
    fn search_row(&self, text: &str, id: &str) -> Item {
        let engine: String = self.db.get("web.search", "https://duckduckgo.com/?q={query}".to_string());
        self.row(id, format!("buscar “{text}” na web"), host(&engine).to_string(), "ph:magnifying-glass", engine.replace("{query}", &encode(text)))
    }

    fn row(&self, id: &str, title: String, subtitle: String, icon: &str, url: String) -> Item {
        *self.pending.borrow_mut() = Some(url);
        Item {
            id: id.into(),
            title,
            subtitle,
            icon: icon.into(),
            actions: vec![Action::new("open", "abrir", "enter"), Action::new("copy", "copiar link", "ctrl+c")],
            ..Item::default()
        }
    }
}

impl Provider for Web {
    fn id(&self) -> &'static str {
        "web"
    }
    fn title(&self) -> &'static str {
        "web"
    }
    fn prefix(&self) -> Option<&'static str> {
        Some("?")
    }
    fn mixed(&self) -> usize {
        4
    }
    fn opened(&self) {
        self.reload();
    }
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        // Quicklinks were added or removed by text since the last open.
        self.reload();
        Some(self.marks.borrow().2.clone())
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        let text = q.text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        // `gh tessela`: a quicklink with {query}.
        if let Some((name, rest)) = text.split_once(char::is_whitespace) {
            if let Some((_, url)) = self.db.quicklinks().into_iter().find(|(n, u)| n == name && u.contains("{query}")) {
                let rest = rest.trim();
                return vec![self.row(
                    "web:quicklink",
                    format!("{rest} — {name}"),
                    format!("buscar em {}", host(&url)),
                    "ph:link",
                    url.replace("{query}", &encode(rest)),
                )];
            }
        }
        // Something that is already an address.
        let bare = !text.contains(char::is_whitespace) && text.contains('.') && !text.starts_with('.');
        if text.starts_with("http://") || text.starts_with("https://") || (bare && text.rsplit('.').next().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(char::is_alphabetic))) {
            let url = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
            return vec![self.row("web:address", text.to_string(), "abrir endereço".into(), "ph:globe", url)];
        }
        // `? text`: search the web (the mixed list has [`WebSearch`] last).
        if q.scoped {
            return vec![self.search_row(text, "web:search")];
        }
        Vec::new()
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        let rest = id.strip_prefix("web:").unwrap_or_default();
        let url = match rest {
            "quicklink" | "address" | "search" => self.pending.borrow().clone().unwrap_or_default(),
            url => url.to_string(),
        };
        let result = if action == "copy" { platform::copy(&url).map(|_| ()) } else { self.open(&url) };
        match (result, action) {
            (Err(e), _) => Outcome::toast(e.to_string()),
            (Ok(()), "copy") => Outcome::close_with("link copiado"),
            _ => Outcome::close(),
        }
    }
}

/// The web search row of the mixed list, a section of its own so it comes
/// after everything else.
pub struct WebSearch(pub Rc<Web>);

impl Provider for WebSearch {
    fn id(&self) -> &'static str {
        "search"
    }
    fn title(&self) -> &'static str {
        "buscar"
    }
    fn mixed(&self) -> usize {
        0
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        let text = q.text.trim();
        if q.scoped || text.chars().count() < 3 || !self.0.db.get("web.fallback", true) {
            return Vec::new();
        }
        vec![self.0.search_row(text, "search:web")]
    }
    fn activate(&self, _id: &str, action: &str) -> Outcome {
        self.0.activate("web:search", action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    #[test]
    fn bookmarks_and_quicklinks() {
        let dir = std::env::temp_dir().join(format!("farol-web-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Bookmarks");
        std::fs::write(
            &file,
            r#"{"roots":{"bookmark_bar":{"children":[{"type":"folder","name":"dev","children":[{"type":"url","name":"Tessela","url":"https://github.com/x/tessela"}]}]}}}"#,
        )
        .unwrap();
        let db = Store::memory();
        db.set("web.bookmarks", &vec![file.display().to_string()]).unwrap();
        db.set_quicklink("gh", "https://github.com/search?q={query}").unwrap();
        let web = Web::new(db);
        let items = web.items().unwrap();
        assert_eq!(items[0].title, "Tessela");
        assert_eq!(items[0].subtitle, "github.com · dev");
        let rows = web.generate(&Query { text: "gh rust lang", scoped: false });
        assert_eq!(rows[0].title, "rust lang — gh");
        assert_eq!(web.pending.borrow().as_deref(), Some("https://github.com/search?q=rust+lang"));
        assert_eq!(web.generate(&Query { text: "example.com", scoped: false })[0].id, "web:address");
        assert!(web.generate(&Query { text: "como fazer pão", scoped: false }).is_empty());
        assert_eq!(web.generate(&Query { text: "como fazer pão", scoped: true })[0].id, "web:search");
        let search = WebSearch(Rc::new(web));
        assert_eq!(search.generate(&Query { text: "como fazer pão", scoped: false })[0].id, "search:web");
        assert!(search.generate(&Query { text: "ab", scoped: false }).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

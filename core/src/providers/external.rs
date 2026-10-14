//! Plugins outside the core: any process serving a tessela service socket
//! (the Rust SDK, Python, anything that speaks one JSON object per line)
//! with two methods:
//!
//! ```text
//! query    {text, scoped}      → [Item, ...]           rows for the text
//! activate {id, action}        → Outcome               {close, query, toast}
//! ```
//!
//! Declared in the `plugins` setting, and started like any tessela plugin
//! (`[plugins.x]` in tessela.toml, or by hand):
//!
//! ```json
//! [{"name": "gh", "title": "github", "prefix": "gh:", "mixed": 3}]
//! ```
//!
//! `service` defaults to the name; the socket is `<name>.sock` beside
//! farol's. Rows come back asynchronously and refresh the list in place;
//! ids are kept as `ext.<name>:<their id>`.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tessela_service::{spawn, unblock};

use crate::engine::{self, Provider, Query};
use crate::item::{Item, Outcome};

#[derive(Clone, Debug, Deserialize)]
pub struct Spec {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub prefix: Option<String>,
    /// Rows in the mixed list; 0 = only behind the prefix.
    #[serde(default)]
    pub mixed: usize,
    #[serde(default)]
    pub service: Option<String>,
}

pub struct External {
    spec: Spec,
    id: &'static str,
    title: &'static str,
    prefix: Option<&'static str>,
    socket: PathBuf,
    /// The last answered (text, scoped) and its rows, shared with the task.
    last: Rc<RefCell<(Option<(String, bool)>, Vec<Item>)>>,
    asked: RefCell<Option<(String, bool)>>,
}

/// One call on a service socket, blocking (run it off the service thread).
pub fn call(socket: &PathBuf, method: &str, args: Value) -> Result<Value, String> {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let stream = UnixStream::connect(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        let line = json!({ "id": 1, "op": "call", "method": method, "args": args }).to_string();
        (&stream).write_all(format!("{line}\n").as_bytes()).map_err(|e| e.to_string())?;
        let mut answer = String::new();
        BufReader::new(&stream).read_line(&mut answer).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
        if let Some(e) = v.get("error") {
            return Err(e.as_str().unwrap_or_default().to_string());
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }
    #[cfg(not(unix))]
    {
        let _ = (socket, method, args);
        Err("plugins need Unix sockets (not on this OS yet)".into())
    }
}

/// Provider ids and titles are `&'static str`: a plugin lives as long as the
/// service, so leaking its few strings once is fine.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

impl External {
    pub fn new(spec: Spec, dir: &std::path::Path) -> Self {
        let service = spec.service.clone().unwrap_or_else(|| spec.name.clone());
        Self {
            id: leak(format!("ext.{}", spec.name)),
            title: leak(if spec.title.is_empty() { spec.name.clone() } else { spec.title.clone() }),
            prefix: spec.prefix.clone().map(leak),
            socket: dir.join(format!("{service}.sock")),
            spec,
            last: Rc::default(),
            asked: RefCell::new(None),
        }
    }

    /// Every plugin in the `plugins` setting, sockets beside farol's.
    pub fn all(db: &crate::db::Db) -> Vec<Self> {
        let specs: Vec<Spec> = db.get("plugins", Vec::new());
        let dir = crate::client::socket().parent().map(PathBuf::from).unwrap_or_default();
        specs.into_iter().map(|s| Self::new(s, &dir)).collect()
    }
}

impl Provider for External {
    fn id(&self) -> &'static str {
        self.id
    }
    fn title(&self) -> &'static str {
        self.title
    }
    fn prefix(&self) -> Option<&'static str> {
        self.prefix
    }
    fn mixed(&self) -> usize {
        0
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        if !q.scoped && (self.spec.mixed == 0 || q.text.is_empty()) {
            return Vec::new();
        }
        let key = (q.text.to_string(), q.scoped);
        {
            let last = self.last.borrow();
            if last.0.as_ref() == Some(&key) {
                let n = if q.scoped { usize::MAX } else { self.spec.mixed };
                return last.1.iter().take(n).cloned().collect();
            }
        }
        if self.asked.borrow().as_ref() == Some(&key) {
            return Vec::new();
        }
        *self.asked.borrow_mut() = Some(key.clone());
        let (socket, last, prefix) = (self.socket.clone(), self.last.clone(), self.id);
        let args = json!({ "text": key.0, "scoped": key.1 });
        spawn(async move {
            let answer = unblock(move || call(&socket, "query", args)).await;
            let items: Vec<Item> = match answer {
                Ok(v) => serde_json::from_value(v).unwrap_or_default(),
                Err(e) => {
                    eprintln!("farol: plugin {prefix}: {e}");
                    Vec::new()
                }
            };
            let items = items.into_iter().map(|mut i| {
                i.id = format!("{prefix}:{}", i.id);
                i
            });
            *last.borrow_mut() = (Some(key), items.collect());
            engine::refresh();
        })
        .detach();
        Vec::new()
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        let theirs = id.strip_prefix(self.id).and_then(|r| r.strip_prefix(':')).unwrap_or(id);
        // Quick for a well-behaved plugin; the launcher waits for the answer.
        match call(&self.socket, "activate", json!({ "id": theirs, "action": action })) {
            Ok(v) => serde_json::from_value(v).unwrap_or_default(),
            Err(e) => Outcome::toast(format!("{}: {e}", self.title)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talks_to_a_service() {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("farol-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("hello.sock");
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let req: Value = serde_json::from_str(&line).unwrap();
                let result = match req["method"].as_str() {
                    Some("query") => json!([{ "id": "1", "title": format!("olá {}", req["args"]["text"].as_str().unwrap()) }]),
                    _ => json!({ "close": true, "toast": req["args"]["id"] }),
                };
                (&stream).write_all(format!("{}\n", json!({ "id": 1, "result": result })).as_bytes()).unwrap();
            }
        });
        let spec: Spec = serde_json::from_value(json!({ "name": "hello", "prefix": "hi:" })).unwrap();
        let ext = External::new(spec, &dir);
        assert_eq!((ext.id(), ext.prefix()), ("ext.hello", Some("hi:")));
        let rows: Vec<Item> = serde_json::from_value(call(&ext.socket, "query", json!({ "text": "mundo" })).unwrap()).unwrap();
        assert_eq!(rows[0].title, "olá mundo");
        let out = ext.activate("ext.hello:1", "");
        assert!(out.close);
        assert_eq!(out.toast, "1");
        std::fs::remove_dir_all(&dir).ok();
    }
}

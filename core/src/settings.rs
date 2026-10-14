//! What the settings window reads and edits, on the same service:
//!
//! topic    config   aliases, shortcuts (farol's and the desktop's), links,
//!                   plugins and the options, as the window shows them
//! methods  status → diagnostics (versions, database, timings per provider)
//!          alias_add {name, target} · alias_remove {name}
//!          hotkey_add {keys, target} · hotkey_remove {keys}
//!          link_add {name, url} · link_remove {name}
//!          option_set {key, value} (`autostart` starts farol at login)
//!
//! Every edit republishes `config`.

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tessela_service::{Service, Topic};

use crate::db::{Db, normalize_keys};
use crate::engine::Engine;
use crate::providers::{farol::Farol, hotkeys};

/// Options the window edits, with their defaults (shown when unset).
pub fn options() -> Vec<(&'static str, Value)> {
    let terminal = |v: Vec<String>| json!(v);
    vec![
        ("terminal", terminal(if cfg!(target_os = "linux") { vec!["ghostty".into(), "-e".into()] } else { vec![] })),
        ("terminal.here", json!(crate::platform::default_terminal_here())),
        ("editor", json!(crate::platform::default_editor())),
        ("browser", json!([])),
        ("web.search", json!("https://duckduckgo.com/?q={query}")),
        ("web.fallback", json!(true)),
        ("files.mixed", json!(true)),
        ("files.hidden", json!(false)),
        ("clipboard.limit", json!(500)),
    ]
}

#[derive(Clone, Default, Serialize)]
pub struct Config {
    aliases: Vec<Value>,
    hotkeys: Vec<Value>,
    desktop: Vec<Value>,
    links: Vec<Value>,
    plugins: Vec<Value>,
    options: serde_json::Map<String, Value>,
    autostart: bool,
}

pub struct Settings {
    db: Db,
    engine: Rc<Engine>,
    farol: Rc<Farol>,
    hotkeys: Rc<hotkeys::Hotkeys>,
    topic: Topic<Config>,
}

#[derive(Deserialize)]
struct Pair {
    #[serde(default)]
    name: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    keys: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: Value,
}

impl Settings {
    fn row(&self, id: &str) -> (String, String, String) {
        match self.engine.item(id) {
            Some(i) => (i.title, i.subtitle, if i.glyph.is_empty() { i.icon } else { i.glyph }),
            None => (id.to_string(), String::new(), String::new()),
        }
    }

    pub fn publish(&self) {
        let aliases = self
            .db
            .aliases()
            .into_iter()
            .map(|(name, target)| {
                let (title, subtitle, icon) = self.row(&target);
                json!({ "name": name, "target": target, "title": title, "subtitle": subtitle, "icon": icon })
            })
            .collect();
        let replaced: std::collections::BTreeMap<String, crate::platform::DesktopBind> =
            self.db.get("hotkeys.replaced", Default::default());
        let hotkeys = self
            .db
            .hotkeys()
            .into_iter()
            .map(|(keys, query)| {
                let (title, subtitle, icon) = match query.strip_prefix('@') {
                    Some(id) => self.row(id),
                    None if query.is_empty() => ("abrir o farol".into(), String::new(), "ph:lighthouse".into()),
                    None => (format!("abrir o farol em “{query}”"), String::new(), "ph:lighthouse".into()),
                };
                let instead = replaced.get(&keys).map(hotkeys::describe_bind).unwrap_or_default();
                json!({ "keys": keys, "query": query, "title": title, "subtitle": subtitle, "icon": icon, "replaced": instead })
            })
            .collect();
        let desktop = crate::platform::desktop_binds()
            .into_iter()
            .filter(|b| !b.arg.contains("FAROL_SOCKET="))
            .map(|b| json!({ "keys": b.keys, "title": hotkeys::describe_bind(&b) }))
            .collect();
        let links = self.db.quicklinks().into_iter().map(|(n, u)| json!({ "name": n, "url": u })).collect();
        let dir = crate::client::socket().parent().map(std::path::PathBuf::from).unwrap_or_default();
        let plugins = self
            .db
            .get::<Vec<Value>>("plugins", Vec::new())
            .into_iter()
            .map(|mut p| {
                let service = p.get("service").or(p.get("name")).and_then(Value::as_str).unwrap_or_default().to_string();
                p["online"] = json!(dir.join(format!("{service}.sock")).exists());
                p
            })
            .collect();
        let options = options().into_iter().map(|(k, d)| (k.to_string(), self.db.setting(k).unwrap_or(d))).collect();
        self.topic.publish(Config {
            aliases,
            hotkeys,
            desktop,
            links,
            plugins,
            options,
            autostart: crate::launch::autostart_enabled(),
        });
    }

    fn edit(&self, a: Pair, method: &str) -> Result<Value, String> {
        let err = |e: rusqlite::Error| e.to_string();
        match method {
            "alias_add" => {
                let name = a.name.trim().to_lowercase();
                if name.is_empty() || name.contains(char::is_whitespace) {
                    return Err("o nome é uma palavra".into());
                }
                let item = self.farol.resolve(a.target.trim()).ok_or("nada encontrado com esse destino")?;
                self.db.set_alias(&name, &item.id).map_err(err)?;
            }
            "alias_remove" => {
                self.db.remove_alias(&a.name).map_err(err)?;
            }
            "hotkey_add" => {
                let keys = normalize_keys(&a.keys);
                if !hotkeys::is_combo(&keys) {
                    return Err(format!("“{}” não é uma combinação de teclas", a.keys));
                }
                // The target as typed: nothing opens farol, a prefix opens
                // it there, anything else names a row ("whatsapp").
                let t = a.target.trim();
                let query = if t.is_empty() {
                    a.query.trim().to_string()
                } else if t.starts_with(['=', ':', '/', '>', '!', '?', '@']) {
                    t.to_string()
                } else {
                    format!("@{}", self.farol.resolve(t).ok_or("nada encontrado com esse destino")?.id)
                };
                self.db.set_hotkey(&keys, &query).map_err(err)?;
                self.hotkeys.apply();
            }
            "hotkey_remove" => {
                self.hotkeys.remove(&a.keys).map_err(|e| e.to_string())?;
            }
            "link_add" => {
                if !(a.url.starts_with("http://") || a.url.starts_with("https://")) || a.name.trim().is_empty() {
                    return Err("um nome e um endereço http(s)".into());
                }
                self.db.set_quicklink(&a.name, &a.url).map_err(err)?;
            }
            "link_remove" => {
                self.db.remove_quicklink(&a.name).map_err(err)?;
            }
            "option_set" if a.key == "autostart" => {
                crate::launch::set_autostart(a.value.as_bool().unwrap_or(false))?;
            }
            "option_set" => {
                if !options().iter().any(|(k, _)| *k == a.key) {
                    return Err(format!("opção desconhecida: {}", a.key));
                }
                self.db.set(&a.key, &a.value).map_err(err)?;
            }
            _ => return Err("?".into()),
        }
        self.publish();
        Ok(Value::Null)
    }
}

/// Process memory in KB (Linux; 0 elsewhere).
fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()))
        .unwrap_or(0)
}

/// Diagnostics: what `farol doctor` and the window's last page show.
pub fn status(db: &Db, engine: &Engine) -> Value {
    let count = |table: &str| db.rows(&format!("SELECT count(*) FROM {table}"), [], |r| r.get::<_, i64>(0)).first().copied().unwrap_or(0);
    let path = crate::db::path();
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "database": {
            "path": path.display().to_string(),
            "bytes": path.metadata().map(|m| m.len()).unwrap_or(0),
            "aliases": count("aliases"), "hotkeys": count("hotkeys"), "links": count("quicklinks"),
            "clipboard": count("clipboard"), "timers": count("timers"), "usage": count("usage"),
        },
        "socket": crate::client::socket().display().to_string(),
        "project": crate::launch::project_dir().map(|p| p.display().to_string()),
        "tessela": crate::launch::tessela().display().to_string(),
        "hotkeys": crate::platform::has_hotkeys(),
        "windows": crate::platform::has_windows(),
        "autostart": crate::launch::autostart_enabled(),
        "memory_kb": rss_kb(),
        "timings": engine.timings().into_iter().map(|(p, t)| json!({
            "provider": p, "last": t.last, "max": t.max, "avg": if t.count > 0 { t.total / t.count as f64 } else { 0.0 }, "count": t.count,
        })).collect::<Vec<_>>(),
    })
}

/// Add the topic and methods to the service. Returns what republishes
/// `config` (the window reads it fresh when it opens).
pub fn register(
    svc: &mut Service,
    db: Db,
    engine: Rc<Engine>,
    farol: Rc<Farol>,
    hotkeys: Rc<hotkeys::Hotkeys>,
) -> Rc<Settings> {
    let topic = svc.topic("config", Config::default());
    let s = Rc::new(Settings { db: db.clone(), engine: engine.clone(), farol, hotkeys, topic });
    for method in ["alias_add", "alias_remove", "hotkey_add", "hotkey_remove", "link_add", "link_remove", "option_set"] {
        let s = s.clone();
        svc.method_sync(method, move |a: Pair| s.edit(a, method).map_err(tessela_service::Error::msg));
    }
    svc.method_sync("status", move |_: Value| Ok(status(&db, &engine)));
    s
}

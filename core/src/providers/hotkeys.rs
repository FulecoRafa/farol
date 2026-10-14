//! Keyboard shortcuts, next to the ones the desktop already has.
//!
//!   atalho super+alt+space          opens farol
//!   atalho super+v >                opens it on the clipboard
//!   atalho super+shift+w @apps:x    runs a row (Ctrl+S on any row fills it)
//!
//! On Hyprland they become runtime binds calling `farol toggle`/`farol run`
//! (bound again whenever the config reloads); a bind they take over is
//! remembered and put back when the shortcut is removed. The desktop's own
//! binds are rows too: searchable by what they do, Enter does it.

use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use crate::db::{Db, normalize_keys};
use crate::engine::{Engine, Provider, Query};
use crate::item::{Action, Item, Outcome};
use crate::platform::{self, DesktopBind};

pub struct Hotkeys {
    db: Db,
    engine: OnceCell<Weak<Engine>>,
    /// The shell command that reaches this farol (binary and socket).
    me: String,
    pending: RefCell<Option<(String, String)>>,
    desktop: RefCell<Vec<DesktopBind>>,
}

/// `'it''s'` for sh.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `super+alt+space` → `super + alt + space`.
fn pretty(keys: &str) -> String {
    keys.replace('+', " + ")
}

/// What a desktop bind does, in words: its description, else a few known
/// dispatchers, else the command without store paths.
pub(crate) fn describe_bind(b: &DesktopBind) -> String {
    if !b.description.is_empty() {
        return b.description.clone();
    }
    let arg = b.arg.trim();
    match b.dispatcher.as_str() {
        "exec" | "execr" => arg
            .split_whitespace()
            .map(|w| if w.starts_with('/') { w.rsplit('/').next().unwrap_or(w) } else { w })
            .collect::<Vec<_>>()
            .join(" "),
        "killactive" => "fechar janela".into(),
        "forcekillactive" => "forçar encerrar janela".into(),
        "togglefloating" => "alternar janela flutuante".into(),
        "fullscreen" => "tela cheia".into(),
        "pin" => "fixar janela".into(),
        "workspace" => format!("ir para o workspace {arg}"),
        "movetoworkspace" => format!("mover janela para o workspace {arg}"),
        "movetoworkspacesilent" => format!("mandar janela para o workspace {arg}"),
        "movefocus" => format!("mover foco {}", arrow(arg)),
        "movewindow" => format!("mover janela {}", arrow(arg)),
        "exit" => "sair do Hyprland".into(),
        d if arg.is_empty() => d.to_string(),
        d => format!("{d} {arg}"),
    }
}

fn arrow(dir: &str) -> &str {
    match dir {
        "l" => "←",
        "r" => "→",
        "u" => "↑",
        "d" => "↓",
        other => other,
    }
}

impl Hotkeys {
    pub fn new(db: Db) -> Self {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "farol".into());
        let me = format!("FAROL_SOCKET={} {}", quote(&crate::client::socket().display().to_string()), quote(&exe));
        Self { db, engine: OnceCell::new(), me, pending: RefCell::new(None), desktop: RefCell::default() }
    }

    pub fn attach(&self, engine: &Rc<Engine>) {
        let _ = self.engine.set(Rc::downgrade(engine));
    }

    /// The command a shortcut runs: `@id` runs a row, anything else opens
    /// farol with that text.
    fn command(&self, query: &str) -> String {
        match query.strip_prefix('@') {
            Some(id) => format!("{} run {}", self.me, quote(id)),
            None => format!("{} toggle {}", self.me, quote(query)),
        }
    }

    /// In words: "abrir o farol", "abrir em >", "WhatsApp".
    fn meaning(&self, query: &str) -> String {
        if let Some(id) = query.strip_prefix('@') {
            let name = self.engine.get().and_then(Weak::upgrade).and_then(|e| e.item(id)).map(|i| i.title);
            return name.unwrap_or_else(|| id.to_string());
        }
        if query.is_empty() { "abrir o farol".into() } else { format!("abrir o farol em “{query}”") }
    }

    fn replaced(&self) -> BTreeMap<String, DesktopBind> {
        self.db.get("hotkeys.replaced", BTreeMap::new())
    }

    /// (Re)bind every shortcut. At start, on config reloads, after edits.
    pub fn apply(&self) {
        if !platform::has_hotkeys() {
            return;
        }
        let mut replaced = self.replaced();
        for (keys, query) in self.db.hotkeys() {
            match platform::bind_hotkey(&keys, &self.command(&query)) {
                // Remember the desktop's own bind, not a previous farol one.
                Ok(Some(prev)) if !prev.arg.contains("FAROL_SOCKET=") => {
                    replaced.entry(keys).or_insert(prev);
                }
                Ok(_) => {}
                Err(e) => eprintln!("farol: atalho {keys}: {e}"),
            }
        }
        let _ = self.db.set("hotkeys.replaced", &replaced);
        self.reload_desktop();
    }

    fn reload_desktop(&self) {
        *self.desktop.borrow_mut() = platform::desktop_binds();
    }

    fn remove(&self, keys: &str) -> std::io::Result<()> {
        let mut replaced = self.replaced();
        let restore = replaced.remove(keys);
        let _ = self.db.remove_hotkey(keys);
        let _ = self.db.set("hotkeys.replaced", &replaced);
        let r = if platform::has_hotkeys() { platform::unbind_hotkey(keys, restore.as_ref()) } else { Ok(()) };
        self.reload_desktop();
        r
    }

    fn rows(&self) -> Vec<Item> {
        let replaced = self.replaced();
        let mut items: Vec<Item> = self
            .db
            .hotkeys()
            .into_iter()
            .map(|(keys, query)| Item {
                id: format!("hotkeys:farol:{keys}"),
                title: self.meaning(&query),
                subtitle: match replaced.get(&keys) {
                    Some(b) => format!("{} · no lugar de “{}”", pretty(&keys), describe_bind(b)),
                    None => pretty(&keys),
                },
                icon: "ph:keyboard".into(),
                keywords: format!("atalho {keys}"),
                hint: keys.clone(),
                actions: vec![Action::new("run", "executar", "enter"), Action::new("remove", "apagar atalho", "ctrl+d")],
                ..Item::default()
            })
            .collect();
        items.extend(
            self.desktop
                .borrow()
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.arg.contains("FAROL_SOCKET="))
                .map(|(i, b)| Item {
                    id: format!("hotkeys:desktop:{i}"),
                    title: describe_bind(b),
                    subtitle: format!("atalho do desktop · {}", pretty(&b.keys)),
                    icon: "ph:keyboard".into(),
                    keywords: format!("atalho {} {} {}", b.keys, b.dispatcher, b.arg),
                    hint: b.keys.clone(),
                    actions: vec![Action::new("run", "executar", "enter")],
                    ..Item::default()
                }),
        );
        items
    }
}

impl Provider for Hotkeys {
    fn id(&self) -> &'static str {
        "hotkeys"
    }
    fn title(&self) -> &'static str {
        "atalhos"
    }
    fn mixed(&self) -> usize {
        3
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn opened(&self) {
        self.reload_desktop();
    }
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        Some(Rc::new(self.rows()))
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        let text = q.text.trim();
        let (word, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        if !matches!(word, "atalho" | "atalhos" | "hotkey" | "hotkeys") {
            return Vec::new();
        }
        let rest = rest.trim();
        if rest.is_empty() {
            return self.rows();
        }
        // `atalho @id keys` (from Ctrl+S) or `atalho keys [text]`.
        let (keys, query) = match rest.strip_prefix('@') {
            Some(at) => match at.split_once(char::is_whitespace) {
                Some((id, keys)) => (keys.trim().to_string(), format!("@{id}")),
                None => return Vec::new(),
            },
            None => {
                let (keys, query) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                (keys.to_string(), query.trim().to_string())
            }
        };
        let keys = normalize_keys(&keys);
        let has_key = keys.split('+').any(|p| !["super", "ctrl", "alt", "shift"].contains(&p) && !p.is_empty());
        if !has_key {
            return Vec::new();
        }
        let taken = self.desktop.borrow().iter().find(|b| b.keys == keys).map(describe_bind);
        let subtitle = match (&taken, platform::has_hotkeys()) {
            (_, false) => "atalhos globais ainda não existem neste sistema".to_string(),
            (Some(what), true) => format!("substitui “{what}” enquanto existir"),
            (None, true) => "criar atalho".to_string(),
        };
        let title = format!("{} → {}", pretty(&keys), self.meaning(&query));
        *self.pending.borrow_mut() = Some((keys, query));
        vec![Item {
            id: "hotkeys:new".into(),
            title,
            subtitle,
            icon: "ph:keyboard".into(),
            actions: vec![Action::new("create", "criar", "enter")],
            ..Item::default()
        }]
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        let rest = id.strip_prefix("hotkeys:").unwrap_or_default();
        if rest == "new" {
            let Some((keys, query)) = self.pending.borrow_mut().take() else { return Outcome::default() };
            if let Err(e) = self.db.set_hotkey(&keys, &query) {
                return Outcome::toast(e.to_string());
            }
            self.apply();
            return Outcome { query: Some(String::new()), toast: format!("{} criado", pretty(&keys)), ..Outcome::default() };
        }
        if let Some(keys) = rest.strip_prefix("farol:") {
            if action == "remove" {
                return match self.remove(keys) {
                    Ok(()) => Outcome::refresh_with(format!("{} apagado", pretty(keys))),
                    Err(e) => Outcome::toast(e.to_string()),
                };
            }
            let query = self.db.hotkeys().into_iter().find(|(k, _)| k == keys).map(|(_, q)| q).unwrap_or_default();
            return match query.strip_prefix('@') {
                Some(target) => match self.engine.get().and_then(Weak::upgrade) {
                    Some(e) => e.activate(target, ""),
                    None => Outcome::default(),
                },
                None => Outcome::query(query),
            };
        }
        if let Some(i) = rest.strip_prefix("desktop:").and_then(|i| i.parse::<usize>().ok()) {
            let bind = self.desktop.borrow().get(i).cloned();
            if let Some(b) = bind {
                return match platform::run_desktop_bind(&b) {
                    Ok(()) => Outcome::close(),
                    Err(e) => Outcome::toast(e.to_string()),
                };
            }
        }
        Outcome::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    fn bind(dispatcher: &str, arg: &str) -> DesktopBind {
        DesktopBind { keys: "super+q".into(), description: String::new(), dispatcher: dispatcher.into(), arg: arg.into() }
    }

    #[test]
    fn describes_binds() {
        assert_eq!(describe_bind(&bind("killactive", "")), "fechar janela");
        assert_eq!(describe_bind(&bind("workspace", "3")), "ir para o workspace 3");
        assert_eq!(
            describe_bind(&bind("exec", "/nix/store/abc-quickshell/bin/quickshell -c fuleco ipc call launcher toggle")),
            "quickshell -c fuleco ipc call launcher toggle"
        );
    }

    #[test]
    fn commands_and_phrases() {
        let h = Hotkeys::new(Store::memory());
        assert!(h.command("").ends_with(" toggle ''"));
        assert!(h.command(">").ends_with(" toggle '>'"));
        assert!(h.command("@apps:x.desktop").ends_with(" run 'apps:x.desktop'"));
        assert!(h.command("it's").ends_with(" toggle 'it'\\''s'"));
        let row = h.generate(&Query { text: "atalho Super + Alt + Space", scoped: false });
        assert_eq!(row.len(), 0, "spaces inside the keys split them");
        let row = h.generate(&Query { text: "atalho super+alt+space", scoped: false });
        assert_eq!(row[0].title, "super + alt + space → abrir o farol");
        assert_eq!(h.pending.borrow().clone(), Some(("super+alt+space".into(), String::new())));
        let row = h.generate(&Query { text: "atalho super+v >", scoped: false });
        assert_eq!(row[0].title, "super + v → abrir o farol em “>”");
        assert!(h.generate(&Query { text: "atalho super+alt", scoped: false }).is_empty());
    }
}

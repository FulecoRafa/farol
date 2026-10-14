//! Windows: the open ones in the mixed list (Enter goes to it), `!` to close
//! or kill them, and commands for the focused window: float, fullscreen,
//! pin, center, `ws 3` (move it there), `ir 3` (go to workspace 3).
//!
//! The launcher takes the keyboard but not the focus from the window below,
//! so "the focused window" is the one the user was in.

use std::cell::RefCell;
use std::rc::Rc;

use crate::engine::{Provider, Query};
use crate::item::{Action, Item, Outcome};
use crate::platform::{self, Window, WindowOp};
use crate::providers::apps::Apps;

/// The open windows, read when the launcher opens.
#[derive(Default)]
pub struct Shared {
    windows: RefCell<Vec<Window>>,
    /// Names and icons of windows come from their apps.
    apps: Option<Rc<Apps>>,
}

impl Shared {
    fn reload(&self) {
        *self.windows.borrow_mut() = platform::windows();
    }

    fn find(&self, id: &str) -> Option<Window> {
        self.windows.borrow().iter().find(|w| w.id == id).cloned()
    }
}

/// Commands on the focused window: (key, title, subtitle, icon, op).
const COMMANDS: &[(&str, &str, &str, &str, WindowOp)] = &[
    ("close", "fechar janela em foco", "a janela onde você estava", "x-circle", WindowOp::Close),
    ("kill", "forçar encerrar janela em foco", "sigkill no processo", "skull", WindowOp::Kill),
    ("float", "alternar janela flutuante", "flutuar ou voltar ao lado das outras", "app-window", WindowOp::Float),
    ("fullscreen", "tela cheia", "na janela em foco", "corners-out", WindowOp::Fullscreen),
    ("pin", "fixar janela em todos os workspaces", "só janelas flutuantes", "push-pin", WindowOp::Pin),
    ("center", "centralizar janela", "só janelas flutuantes", "crosshair", WindowOp::Center),
];

pub struct Windows {
    shared: Rc<Shared>,
    /// `!` mode: Enter closes instead of focusing.
    kill: bool,
    catalog: RefCell<Rc<Vec<Item>>>,
    /// The workspace the last generated row moves to or goes to.
    pending: RefCell<Option<WindowOp>>,
}

impl Windows {
    /// The switcher (mixed list) and the `!` closer, over the same windows.
    pub fn pair(apps: Rc<Apps>) -> (Self, Self) {
        let shared = Rc::new(Shared { windows: RefCell::default(), apps: Some(apps) });
        let make = |kill| Self { shared: shared.clone(), kill, catalog: RefCell::default(), pending: RefCell::new(None) };
        (make(false), make(true))
    }

    fn rebuild(&self) {
        let prefix = self.id();
        let mut items: Vec<Item> = self
            .shared
            .windows
            .borrow()
            .iter()
            .map(|w| {
                let (name, icon) = self
                    .shared
                    .apps
                    .as_ref()
                    .and_then(|a| a.for_window(&w.app))
                    .unwrap_or_else(|| (w.app.clone(), format!("icon:{}", w.app.to_lowercase())));
                let actions = if self.kill {
                    vec![
                        Action::new("close", "fechar janela", "enter"),
                        Action::new("kill", "forçar encerrar (kill -9)", "ctrl+x"),
                        Action::new("focus", "ir para a janela", "ctrl+f"),
                    ]
                } else {
                    vec![
                        Action::new("focus", "ir para a janela", "enter"),
                        Action::new("close", "fechar janela", "ctrl+w"),
                        Action::new("kill", "forçar encerrar (kill -9)", "ctrl+x"),
                    ]
                };
                Item {
                    id: format!("{prefix}:{}", w.id),
                    title: if w.title.is_empty() { w.app.clone() } else { w.title.clone() },
                    subtitle: format!("{name} · workspace {}", w.workspace),
                    icon,
                    keywords: format!("{name} {}", w.app),
                    actions,
                    ..Item::default()
                }
            })
            .collect();
        if !self.kill && platform::has_windows() {
            items.extend(COMMANDS.iter().map(|(key, title, subtitle, icon, _)| Item {
                id: format!("windows:cmd:{key}"),
                title: (*title).into(),
                subtitle: (*subtitle).into(),
                icon: format!("ph:{icon}"),
                keywords: "janela window".into(),
                actions: vec![Action::new("run", "executar", "enter")],
                ..Item::default()
            }));
        }
        *self.catalog.borrow_mut() = Rc::new(items);
    }
}

/// `ws 3` / `mover 3` → move there; `ir 3` → go there.
fn workspace_phrase(text: &str) -> Option<(WindowOp, String, String)> {
    let mut words = text.split_whitespace();
    let verb = words.next()?.to_lowercase();
    let n: u32 = words.next()?.parse().ok()?;
    if words.next().is_some() || n == 0 || n > 99 {
        return None;
    }
    match verb.as_str() {
        "ws" | "mover" | "mv" => Some((
            WindowOp::MoveTo(n),
            format!("mover janela para o workspace {n}"),
            format!("e ir junto · “ir {n}” só troca de workspace"),
        )),
        "ir" | "go" => Some((WindowOp::GoTo(n), format!("ir para o workspace {n}"), String::new())),
        _ => None,
    }
}

impl Provider for Windows {
    fn id(&self) -> &'static str {
        if self.kill { "kill" } else { "windows" }
    }
    fn title(&self) -> &'static str {
        if self.kill { "encerrar janelas" } else { "janelas" }
    }
    fn prefix(&self) -> Option<&'static str> {
        self.kill.then_some("!")
    }
    fn mixed(&self) -> usize {
        if self.kill { 0 } else { 4 }
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn opened(&self) {
        // Both share the list: the switcher reads, both rebuild their rows.
        if !self.kill {
            self.shared.reload();
        }
        self.rebuild();
    }
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        Some(self.catalog.borrow().clone())
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        if self.kill || !platform::has_windows() {
            return Vec::new();
        }
        let Some((op, title, subtitle)) = workspace_phrase(q.text) else { return Vec::new() };
        *self.pending.borrow_mut() = Some(op);
        vec![Item {
            id: "windows:workspace".into(),
            title,
            subtitle,
            icon: "ph:squares-four".into(),
            actions: vec![Action::new("run", "executar", "enter")],
            ..Item::default()
        }]
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        let rest = id.split_once(':').map(|(_, r)| r).unwrap_or_default();
        let result = if rest == "workspace" {
            match *self.pending.borrow() {
                Some(op) => platform::window_op(None, op),
                None => Ok(()),
            }
        } else if let Some(key) = rest.strip_prefix("cmd:") {
            match COMMANDS.iter().find(|c| c.0 == key) {
                Some(c) => platform::window_op(None, c.4),
                None => Ok(()),
            }
        } else {
            let Some(w) = self.shared.find(rest) else {
                return Outcome::toast("a janela já fechou");
            };
            let op = match action {
                "close" => WindowOp::Close,
                "kill" => WindowOp::Kill,
                "focus" => WindowOp::Focus,
                _ if self.kill => WindowOp::Close,
                _ => WindowOp::Focus,
            };
            let r = platform::window_op(Some(&w), op);
            // Closing from `!` keeps the list open, minus that window.
            if r.is_ok() && self.kill && op != WindowOp::Focus {
                self.shared.windows.borrow_mut().retain(|x| x.id != w.id);
                self.rebuild();
                return Outcome { query: Some("!".into()), toast: format!("{} fechada", w.app), ..Outcome::default() };
            }
            r
        };
        match result {
            Ok(()) => Outcome::close(),
            Err(e) => Outcome::toast(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases() {
        assert_eq!(workspace_phrase("ws 3").map(|p| p.0), Some(WindowOp::MoveTo(3)));
        assert_eq!(workspace_phrase("ir 10").map(|p| p.0), Some(WindowOp::GoTo(10)));
        assert!(workspace_phrase("ws").is_none());
        assert!(workspace_phrase("ws 3 4").is_none());
        assert!(workspace_phrase("wsx 3").is_none());
    }
}

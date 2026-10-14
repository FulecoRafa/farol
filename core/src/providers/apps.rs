//! Installed applications: desktop entries, `.app` bundles, Start menu.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::SystemTime;

use crate::db::Db;
use crate::engine::Provider;
use crate::item::{Action, Item, Outcome, Preview};
use crate::platform::{self, App};

pub struct Apps {
    db: Db,
    state: RefCell<State>,
}

#[derive(Default)]
struct State {
    stamp: Option<SystemTime>,
    items: Rc<Vec<Item>>,
    apps: HashMap<String, App>,
}

impl Apps {
    pub fn new(db: Db) -> Self {
        let me = Self { db, state: RefCell::default() };
        me.scan();
        me
    }

    fn scan(&self) {
        let stamp = platform::apps_stamp();
        if self.state.borrow().stamp == Some(stamp) {
            return;
        }
        let apps = platform::apps();
        let items = apps
            .iter()
            .map(|a| Item {
                id: format!("apps:{}", a.id),
                title: a.name.clone(),
                subtitle: a.comment.clone(),
                icon: a.icon.clone(),
                keywords: a.keywords.clone(),
                actions: vec![
                    Action::new("open", "abrir", "enter"),
                    Action::new("alias", "criar pseudônimo", "ctrl+a"),
                    Action::new("copy", "copiar comando", "ctrl+c"),
                ],
                ..Item::default()
            })
            .collect();
        *self.state.borrow_mut() = State {
            stamp: Some(stamp),
            items: Rc::new(items),
            apps: apps.into_iter().map(|a| (a.id.clone(), a)).collect(),
        };
    }

    /// The app a window belongs to, by its class: the declared
    /// `StartupWMClass`, else an entry id or reverse-DNS tail equal to it
    /// (`dev.zed.Zed` → `dev.zed.Zed.desktop`, `firefox` → `firefox.desktop`).
    pub fn for_window(&self, class: &str) -> Option<(String, String)> {
        let class = class.to_lowercase();
        let state = self.state.borrow();
        let stem = |id: &str| id.trim_end_matches(".desktop").to_lowercase();
        let tail = |id: &str| stem(id).rsplit('.').next().unwrap_or_default().to_string();
        let found = state
            .apps
            .values()
            .find(|a| a.wm_class.to_lowercase() == class)
            .or_else(|| state.apps.values().find(|a| stem(&a.id) == class))
            .or_else(|| state.apps.values().find(|a| tail(&a.id) == class || stem(&a.id) == tail(&class)))?;
        Some((found.name.clone(), found.icon.clone()))
    }

    fn app(&self, id: &str) -> Option<App> {
        self.state.borrow().apps.get(id.strip_prefix("apps:")?).cloned()
    }

    /// The terminal command apps with `Terminal=true` run in.
    fn terminal(&self) -> Vec<String> {
        let default: Vec<String> = if cfg!(target_os = "linux") {
            vec!["ghostty".into(), "-e".into()]
        } else {
            Vec::new()
        };
        self.db.get("terminal", default)
    }
}

impl Provider for Apps {
    fn id(&self) -> &'static str {
        "apps"
    }
    fn title(&self) -> &'static str {
        "aplicativos"
    }
    fn mixed(&self) -> usize {
        6
    }
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        Some(self.state.borrow().items.clone())
    }
    fn opened(&self) {
        self.scan();
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        let Some(app) = self.app(id) else {
            return Outcome::toast("o app não existe mais");
        };
        match action {
            "alias" => Outcome::query(format!("alias @{id} ")),
            "copy" => match platform::copy(&app.exec.join(" ")) {
                Ok(()) => Outcome::close_with("comando copiado"),
                Err(e) => Outcome::toast(e.to_string()),
            },
            _ => match platform::launch(&app, &self.terminal()) {
                Ok(()) => Outcome::close(),
                Err(e) => Outcome::toast(format!("não abriu: {e}")),
            },
        }
    }
    fn preview(&self, id: &str) -> Preview {
        let Some(app) = self.app(id) else { return Preview::None };
        Preview::Text {
            title: app.name.clone(),
            text: String::new(),
            facts: vec![
                ("comando".into(), app.exec.join(" ")),
                ("id".into(), app.id.clone()),
            ],
        }
    }
}

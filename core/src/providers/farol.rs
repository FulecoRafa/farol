//! farol configuring itself from the search field:
//!   alias zap = whatsapp      alias @apps:x.desktop zap      unalias zap
//!   link gh https://github.com/search?q={query}               unlink gh
//! plus the saved aliases and links as rows (Ctrl+D removes).

use std::cell::{OnceCell, RefCell};
use std::rc::{Rc, Weak};

use crate::db::Db;
use crate::engine::{Engine, Provider, Query};
use crate::item::{Action, Item, Outcome};

pub struct Farol {
    db: Db,
    engine: OnceCell<Weak<Engine>>,
    /// What the row generated last time would do.
    pending: RefCell<Option<Pending>>,
}

#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Alias { name: String, target: String },
    Unalias(String),
    Link { name: String, url: String },
    Unlink(String),
}

impl Farol {
    pub fn new(db: Db) -> Self {
        Self { db, engine: OnceCell::new(), pending: RefCell::new(None) }
    }

    /// The engine, to name alias targets; set once it exists.
    pub fn attach(&self, engine: &Rc<Engine>) {
        let _ = self.engine.set(Rc::downgrade(engine));
    }

    fn engine(&self) -> Option<Rc<Engine>> {
        self.engine.get()?.upgrade()
    }

    /// The row a target stands for: an `@id`, else the best search hit.
    fn resolve(&self, target: &str) -> Option<Item> {
        let engine = self.engine()?;
        if let Some(id) = target.strip_prefix('@') {
            return engine.item(id);
        }
        engine
            .query(target)
            .into_iter()
            .filter(|s| s.provider != "farol" && s.provider != "alias")
            .flat_map(|s| s.items)
            .next()
    }

    fn parse(&self, text: &str) -> Option<(Pending, Item)> {
        let (word, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        let rest = rest.trim();
        let row = |title: String, subtitle: String, icon: &str| Item {
            id: "farol:pending".into(),
            title,
            subtitle,
            icon: icon.into(),
            actions: vec![Action::new("run", "confirmar", "enter")],
            ..Item::default()
        };
        match word {
            "alias" | "apelido" | "pseudonimo" | "pseudônimo" => {
                let (name, target) = if let Some(at) = rest.strip_prefix('@') {
                    // `@id name`: from Ctrl+A on a row.
                    let (id, name) = at.split_once(char::is_whitespace)?;
                    (name.trim().to_string(), format!("@{id}"))
                } else if let Some((n, t)) = rest.split_once('=') {
                    (n.trim().to_string(), t.trim().to_string())
                } else {
                    let (n, t) = rest.split_once(char::is_whitespace)?;
                    (n.trim().to_string(), t.trim().to_string())
                };
                if name.is_empty() || target.len() < 2 || name.contains(char::is_whitespace) {
                    return None;
                }
                let hit = self.resolve(&target)?;
                let stored = hit.id.clone();
                Some((
                    Pending::Alias { name: name.clone(), target: stored },
                    row(format!("“{name}” abre {}", hit.title), format!("criar pseudônimo · {}", hit.id), "ph:at"),
                ))
            }
            "unalias" => {
                let name = rest.to_lowercase();
                self.db.aliases().iter().any(|(a, _)| *a == name).then(|| {
                    (Pending::Unalias(name.clone()), row(format!("apagar o pseudônimo “{name}”"), String::new(), "ph:trash"))
                })
            }
            "link" => {
                let (name, url) = rest.split_once(char::is_whitespace)?;
                let url = url.trim();
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return None;
                }
                Some((
                    Pending::Link { name: name.into(), url: url.into() },
                    row(format!("link “{name}” → {url}"), "criar link rápido · {query} recebe o texto depois do nome".into(), "ph:link"),
                ))
            }
            "unlink" => self.db.quicklinks().iter().any(|(n, _)| n == rest).then(|| {
                (Pending::Unlink(rest.into()), row(format!("apagar o link “{rest}”"), String::new(), "ph:trash"))
            }),
            _ => None,
        }
    }
}

impl Provider for Farol {
    fn id(&self) -> &'static str {
        "farol"
    }
    fn title(&self) -> &'static str {
        "farol"
    }
    fn mixed(&self) -> usize {
        2
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        match self.parse(q.text) {
            Some((p, item)) => {
                *self.pending.borrow_mut() = Some(p);
                vec![item]
            }
            None => Vec::new(),
        }
    }
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        let remove = |what: &str| vec![Action::new("edit", "editar", "enter"), Action::new("remove", what, "ctrl+d")];
        let mut items: Vec<Item> = self
            .db
            .aliases()
            .into_iter()
            .map(|(a, t)| Item {
                id: format!("farol:alias:{a}"),
                title: format!("{a} → {t}"),
                subtitle: "pseudônimo".into(),
                icon: "ph:at".into(),
                keywords: format!("alias pseudônimo {a}"),
                actions: remove("apagar pseudônimo"),
                ..Item::default()
            })
            .collect();
        items.extend(self.db.quicklinks().into_iter().map(|(n, u)| Item {
            id: format!("farol:link:{n}"),
            title: format!("{n} → {u}"),
            subtitle: "link rápido".into(),
            icon: "ph:link".into(),
            keywords: format!("link {n}"),
            actions: remove("apagar link"),
            ..Item::default()
        }));
        Some(Rc::new(items))
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        if let Some(name) = id.strip_prefix("farol:alias:") {
            return if action == "remove" {
                let _ = self.db.remove_alias(name);
                Outcome { query: Some(String::new()), toast: format!("pseudônimo “{name}” apagado"), ..Outcome::default() }
            } else {
                let target = self.db.aliases().into_iter().find(|(a, _)| a == name).map(|(_, t)| t).unwrap_or_default();
                Outcome::query(format!("alias {name} = @{target}"))
            };
        }
        if let Some(name) = id.strip_prefix("farol:link:") {
            return if action == "remove" {
                let _ = self.db.remove_quicklink(name);
                Outcome { query: Some(String::new()), toast: format!("link “{name}” apagado"), ..Outcome::default() }
            } else {
                let url = self.db.quicklinks().into_iter().find(|(n, _)| n == name).map(|(_, u)| u).unwrap_or_default();
                Outcome::query(format!("link {name} {url}"))
            };
        }
        let Some(p) = self.pending.borrow_mut().take() else {
            return Outcome::default();
        };
        let (result, toast) = match p {
            Pending::Alias { name, target } => (self.db.set_alias(&name, &target), format!("“{name}” criado")),
            Pending::Unalias(n) => (self.db.remove_alias(&n).map(|_| ()), format!("“{n}” apagado")),
            Pending::Link { name, url } => (self.db.set_quicklink(&name, &url), format!("link “{name}” criado")),
            Pending::Unlink(n) => (self.db.remove_quicklink(&n).map(|_| ()), format!("link “{n}” apagado")),
        };
        match result {
            Ok(()) => Outcome { query: Some(String::new()), toast, ..Outcome::default() },
            Err(e) => Outcome::toast(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;
    use crate::engine::Query;

    struct Apps(Rc<Vec<Item>>);
    impl Provider for Apps {
        fn id(&self) -> &'static str {
            "apps"
        }
        fn title(&self) -> &'static str {
            "apps"
        }
        fn items(&self) -> Option<Rc<Vec<Item>>> {
            Some(self.0.clone())
        }
        fn activate(&self, _: &str, _: &str) -> Outcome {
            Outcome::close()
        }
    }

    #[test]
    fn alias_from_text_and_from_id() {
        let db = Store::memory();
        let apps = vec![
            Item { id: "apps:whatsapp".into(), title: "WhatsApp".into(), ..Item::default() },
            Item { id: "apps:helium".into(), title: "Helium".into(), ..Item::default() },
        ];
        let farol = Rc::new(Farol::new(db.clone()));
        let engine = Rc::new(Engine::new(db.clone(), vec![Rc::new(Apps(Rc::new(apps))), farol.clone()]));
        farol.attach(&engine);

        let rows = farol.generate(&Query { text: "alias zap = whats", scoped: false });
        assert_eq!(rows[0].title, "“zap” abre WhatsApp");
        farol.activate("farol:pending", "run");
        assert_eq!(db.aliases(), vec![("zap".into(), "apps:whatsapp".into())]);

        engine.query("helium");
        farol.generate(&Query { text: "alias @apps:helium browser", scoped: false });
        farol.activate("farol:pending", "run");
        assert_eq!(engine.query("browser")[0].items[0].id, "apps:helium");

        assert!(farol.generate(&Query { text: "alias x", scoped: false }).is_empty());
        assert!(farol.generate(&Query { text: "unalias nope", scoped: false }).is_empty());
    }
}

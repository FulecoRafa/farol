//! Emoji and symbols (`:`): Unicode names and GitHub shortcodes; Enter copies.

use std::collections::HashMap;
use std::rc::Rc;

use crate::db::Db;
use crate::engine::Provider;
use crate::item::{Action, Item, Outcome};
use crate::platform;

pub struct Emoji {
    db: Db,
    items: Rc<Vec<Item>>,
}

impl Emoji {
    pub fn new(db: Db) -> Self {
        let items = emojis::iter()
            .map(|e| Item {
                id: format!("emoji:{}", e.as_str()),
                title: e.name().to_string(),
                subtitle: e.shortcodes().map(|s| format!(":{s}:")).collect::<Vec<_>>().join(" "),
                glyph: e.as_str().to_string(),
                keywords: e.shortcodes().collect::<Vec<_>>().join(" "),
                actions: vec![Action::new("copy", "copiar", "enter")],
                ..Item::default()
            })
            .collect();
        Self { db, items: Rc::new(items) }
    }
}

impl Provider for Emoji {
    fn id(&self) -> &'static str {
        "emoji"
    }
    fn title(&self) -> &'static str {
        "emoji e símbolos"
    }
    fn prefix(&self) -> Option<&'static str> {
        Some(":")
    }
    fn mixed(&self) -> usize {
        0
    }
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        Some(self.items.clone())
    }
    /// `:` alone: the most used first, then Unicode order.
    fn browse(&self) -> Option<Vec<Item>> {
        let used: HashMap<String, i64> = self
            .db
            .usage()
            .into_iter()
            .filter(|(id, ..)| id.starts_with("emoji:"))
            .map(|(id, count, _)| (id, count))
            .collect();
        let mut top: Vec<&Item> = self.items.iter().filter(|i| used.contains_key(&i.id)).collect();
        top.sort_by_key(|i| std::cmp::Reverse(used[&i.id]));
        let rest = self.items.iter().filter(|i| !used.contains_key(&i.id));
        Some(top.into_iter().chain(rest).take(80).cloned().collect())
    }
    fn activate(&self, id: &str, _action: &str) -> Outcome {
        let glyph = id.strip_prefix("emoji:").unwrap_or_default();
        match platform::copy(glyph) {
            Ok(()) => Outcome::close_with(format!("{glyph} copiado")),
            Err(e) => Outcome::toast(e.to_string()),
        }
    }
}

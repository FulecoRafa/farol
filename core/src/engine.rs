//! The search: providers (plugins) give candidates or generate rows from the
//! text; the engine routes prefixes, ranks with nucleo plus frecency, puts
//! aliases first and runs actions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nucleo_matcher::{Config, Matcher};

use crate::db::{Db, now};
use crate::item::{Action, Item, Outcome, Preview, Section};
use crate::matching;

/// What a provider sees of the query.
pub struct Query<'a> {
    /// The text after the prefix (or all of it), trimmed.
    pub text: &'a str,
    /// The provider was picked by its prefix: show everything it has.
    pub scoped: bool,
}

thread_local! {
    static REFRESH: RefCell<Option<Rc<dyn Fn()>>> = RefCell::new(None);
}

/// Set by the service: how to answer the current text again.
pub fn on_refresh(f: Rc<dyn Fn()>) {
    REFRESH.with(|r| *r.borrow_mut() = Some(f));
}

/// A provider's background work (a file search) finished: answer the
/// current text again, keeping the selection.
pub fn refresh() {
    if let Some(f) = REFRESH.with(|r| r.borrow().clone()) {
        f();
    }
}

/// A plugin. Most only need `items` (a catalog the engine ranks); those that
/// compute rows from the text (calculator, timers) implement `generate`.
pub trait Provider {
    /// Prefix of every item id: `apps` for `apps:helium.desktop`.
    fn id(&self) -> &'static str;
    /// Section title.
    fn title(&self) -> &'static str;
    /// The prefix that scopes the search to this provider (`=`, `:`), if any.
    fn prefix(&self) -> Option<&'static str> {
        None
    }
    /// Rows in the mixed list (no prefix) when the text is not empty;
    /// 0 = only when scoped.
    fn mixed(&self) -> usize {
        5
    }
    /// Count launches for ranking (not for one-off rows like clipboard).
    fn tracks_usage(&self) -> bool {
        true
    }
    /// A catalog to fuzzy-match. Called per query: keep it cached.
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        None
    }
    /// Rows computed from the text, already in order.
    fn generate(&self, _q: &Query) -> Vec<Item> {
        Vec::new()
    }
    /// Rows shown with the field empty, above the recent ones (running
    /// timers).
    fn home(&self) -> Vec<Item> {
        Vec::new()
    }
    /// Shown for an empty scoped search (`:` alone); defaults to the catalog.
    fn browse(&self) -> Option<Vec<Item>> {
        None
    }
    fn activate(&self, id: &str, action: &str) -> Outcome;
    fn preview(&self, _id: &str) -> Preview {
        Preview::None
    }
    /// The launcher opened: refresh what may be stale (cheaply).
    fn opened(&self) {}
}

pub type Providers = Vec<Rc<dyn Provider>>;

pub struct Engine {
    db: Db,
    providers: Providers,
    matcher: RefCell<Matcher>,
    /// Rows of the last answer, by id, for actions and previews.
    last: RefCell<HashMap<String, Item>>,
    /// Per provider: (last, slowest, total) ms and how many answers, for
    /// `farol doctor` and the diagnostics page.
    timings: RefCell<HashMap<&'static str, Timing>>,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct Timing {
    pub last: f64,
    pub max: f64,
    pub total: f64,
    pub count: u64,
}

/// Bonus for launches: count and recency, capped so a perfect match of a
/// rarely used app still beats a weak match of a favorite.
fn frecency(count: i64, last: i64, now: i64) -> i64 {
    let days = ((now - last).max(0) / 86_400_000) as f64;
    let recency = 1.0 / (1.0 + days / 7.0);
    ((count as f64 + 1.0).ln() * 14.0 * (0.5 + recency)).min(70.0) as i64
}

impl Engine {
    pub fn new(db: Db, providers: Providers) -> Self {
        Self {
            db,
            providers,
            matcher: RefCell::new(Matcher::new(Config::DEFAULT)),
            last: RefCell::new(HashMap::new()),
            timings: RefCell::default(),
        }
    }

    fn time(&self, id: &'static str, started: std::time::Instant) {
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut t = self.timings.borrow_mut();
        let e = t.entry(id).or_default();
        e.last = ms;
        e.max = e.max.max(ms);
        e.total += ms;
        e.count += 1;
    }

    /// How long each provider took, in the mixed list (scoped searches go
    /// under their provider too).
    pub fn timings(&self) -> Vec<(&'static str, Timing)> {
        let mut v: Vec<_> = self.timings.borrow().iter().map(|(k, v)| (*k, *v)).collect();
        v.sort_by(|a, b| b.1.max.total_cmp(&a.1.max));
        v
    }

    fn provider(&self, id: &str) -> Option<&Rc<dyn Provider>> {
        let name = id.split(':').next()?;
        self.providers.iter().find(|p| p.id() == name)
    }

    /// The prefixes, from settings (`prefixes`: {"=": "calc"}) over defaults.
    fn prefixes(&self) -> Vec<(String, Rc<dyn Provider>)> {
        let custom: HashMap<String, String> = self.db.get("prefixes", HashMap::new());
        let mut out: Vec<(String, Rc<dyn Provider>)> = Vec::new();
        for p in &self.providers {
            let custom_prefix = custom.iter().find(|(_, v)| *v == p.id()).map(|(k, _)| k.clone());
            if let Some(prefix) = custom_prefix.or_else(|| p.prefix().map(String::from)) {
                out.push((prefix, p.clone()));
            }
        }
        // Longest first, so ">>" wins over ">".
        out.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
        out
    }

    pub fn opened(&self) {
        for p in &self.providers {
            p.opened();
        }
    }

    pub fn query(&self, raw: &str) -> Vec<Section> {
        let text = raw.trim_start();
        let mut sections = Vec::new();
        if let Some((prefix, p)) = self.prefixes().into_iter().find(|(k, _)| text.starts_with(k.as_str())) {
            let rest = text[prefix.len()..].trim();
            let q = Query { text: rest, scoped: true };
            let started = std::time::Instant::now();
            let mut items = p.generate(&q);
            if rest.is_empty() {
                items.extend(p.browse().or_else(|| p.items().map(|i| i.iter().take(60).cloned().collect())).unwrap_or_default());
            } else if let Some(catalog) = p.items() {
                items.extend(self.rank(&catalog, rest, 80));
            }
            self.time(p.id(), started);
            sections.push(Section { provider: p.id().into(), title: p.title().into(), items });
        } else {
            let text = text.trim();
            if text.is_empty() {
                for p in &self.providers {
                    let items = p.home();
                    if !items.is_empty() {
                        sections.push(Section { provider: p.id().into(), title: p.title().into(), items });
                    }
                }
                sections.push(self.recent());
            } else {
                sections.extend(self.alias_hits(text));
                let q = Query { text, scoped: false };
                for p in &self.providers {
                    let started = std::time::Instant::now();
                    let mut items = p.generate(&q);
                    if p.mixed() > 0 {
                        if let Some(catalog) = p.items() {
                            items.extend(self.rank(&catalog, text, p.mixed()));
                        }
                    }
                    self.time(p.id(), started);
                    if !items.is_empty() {
                        sections.push(Section { provider: p.id().into(), title: p.title().into(), items });
                    }
                }
            }
        }
        sections.retain(|s| !s.items.is_empty());
        self.annotate(&mut sections);
        let mut last = self.last.borrow_mut();
        last.clear();
        for s in &sections {
            for i in &s.items {
                last.insert(i.id.clone(), i.clone());
            }
        }
        sections
    }

    /// Rank a catalog against the text (see [`matching`]) plus frecency.
    fn rank(&self, catalog: &[Item], text: &str, limit: usize) -> Vec<Item> {
        let query = matching::Query::new(text);
        let usage: HashMap<String, (i64, i64)> =
            self.db.usage().into_iter().map(|(id, c, l)| (id, (c, l))).collect();
        let now = now();
        let mut matcher = self.matcher.borrow_mut();
        let mut scored: Vec<(i64, &Item)> = catalog
            .iter()
            .filter_map(|item| {
                let s = query.score(&item.title, &item.keywords, &mut matcher)?;
                let bonus = usage.get(&item.id).map(|&(c, l)| frecency(c, l, now)).unwrap_or(0);
                Some((s + bonus + item.score, item))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.len().cmp(&b.1.title.len())));
        scored.into_iter().take(limit).map(|(_, i)| i.clone()).collect()
    }

    /// Nothing typed: the most recently used, then the most used.
    fn recent(&self) -> Section {
        let mut usage = self.db.usage();
        usage.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));
        let mut items = Vec::new();
        for (id, _, _) in usage {
            if items.len() >= 8 {
                break;
            }
            if let Some(item) = self.find(&id) {
                items.push(item);
            }
        }
        Section { provider: "recent".into(), title: "recentes".into(), items }
    }

    /// An item of any catalog by id.
    fn find(&self, id: &str) -> Option<Item> {
        let p = self.provider(id)?;
        p.items()?.iter().find(|i| i.id == id).cloned()
    }

    /// `zap` typed: the alias target first. A target is an item id, or text
    /// searched across every catalog (best hit wins).
    fn alias_hits(&self, text: &str) -> Option<Section> {
        let key = text.to_lowercase();
        let mut items = Vec::new();
        for (alias, target) in self.db.aliases() {
            if alias != key && !(key.len() >= 2 && alias.starts_with(&key)) {
                continue;
            }
            let hit = self.find(&target).or_else(|| {
                self.providers
                    .iter()
                    .filter(|p| p.mixed() > 0)
                    .filter_map(|p| p.items().and_then(|c| self.rank(&c, &target, 1).into_iter().next()))
                    .next()
            });
            if let Some(mut item) = hit {
                item.hint = alias.clone();
                if alias == key {
                    items.insert(0, item);
                } else {
                    items.push(item);
                }
            }
        }
        (!items.is_empty()).then(|| Section { provider: "alias".into(), title: "pseudônimos".into(), items })
    }

    /// Hints: the aliases each row has; and drop rows an alias already showed.
    fn annotate(&self, sections: &mut [Section]) {
        let aliases = self.db.aliases();
        let shown: Vec<String> = sections
            .iter()
            .filter(|s| s.provider == "alias")
            .flat_map(|s| s.items.iter().map(|i| i.id.clone()))
            .collect();
        for s in sections.iter_mut() {
            if s.provider != "alias" {
                s.items.retain(|i| !shown.contains(&i.id));
            }
            for item in &mut s.items {
                // Rows that run something can get an alias and a shortcut.
                let runs = self.provider(&item.id).is_some_and(|p| p.tracks_usage());
                for (id, title, key) in [("alias", "criar pseudônimo", "ctrl+a"), ("hotkey", "criar atalho", "ctrl+s")] {
                    if runs && !item.actions.is_empty() && !item.actions.iter().any(|a| a.id == id) {
                        item.actions.push(Action::new(id, title, key));
                    }
                }
                if item.hint.is_empty() {
                    if let Some((a, _)) = aliases.iter().find(|(_, t)| *t == item.id) {
                        item.hint = a.clone();
                    }
                }
            }
        }
    }

    pub fn activate(&self, id: &str, action: &str) -> Outcome {
        match action {
            "alias" => return Outcome::query(format!("alias @{id} ")),
            "hotkey" => return Outcome::query(format!("atalho @{id} ")),
            _ => {}
        }
        let Some(p) = self.provider(id).cloned() else {
            return Outcome::toast(format!("nada responde por {id}"));
        };
        let out = p.activate(id, action);
        if p.tracks_usage() && (action.is_empty() || action == "open") {
            self.db.used(id);
        }
        out
    }

    pub fn preview(&self, id: &str) -> Preview {
        self.provider(id).map(|p| p.preview(id)).unwrap_or_default()
    }

    /// The row as last answered (for the `@id` forms and actions).
    pub fn item(&self, id: &str) -> Option<Item> {
        self.last.borrow().get(id).cloned().or_else(|| self.find(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    struct Fixed(Rc<Vec<Item>>);
    impl Provider for Fixed {
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

    fn app(id: &str, title: &str) -> Item {
        Item { id: format!("apps:{id}"), title: title.into(), ..Item::default() }
    }

    fn engine() -> (Db, Engine) {
        let db = Store::memory();
        let apps = vec![app("helium", "Helium"), app("whatsapp", "WhatsApp"), app("help", "Help"), app("ghostty", "Ghostty")];
        let e = Engine::new(db.clone(), vec![Rc::new(Fixed(Rc::new(apps)))]);
        (db, e)
    }

    fn ids(s: &[Section]) -> Vec<String> {
        s.iter().flat_map(|s| s.items.iter().map(|i| i.id.clone())).collect()
    }

    #[test]
    fn usage_breaks_ties() {
        let (db, e) = engine();
        assert_eq!(ids(&e.query("hel"))[0], "apps:help");
        db.used("apps:helium");
        assert_eq!(ids(&e.query("hel"))[0], "apps:helium");
    }

    #[test]
    fn aliases_come_first_by_text_or_id() {
        let (db, e) = engine();
        db.set_alias("zap", "whatsapp").unwrap();
        db.set_alias("browser", "apps:helium").unwrap();
        let s = e.query("zap");
        assert_eq!(s[0].provider, "alias");
        assert_eq!(s[0].items[0].id, "apps:whatsapp");
        assert_eq!(s[0].items[0].hint, "zap");
        assert_eq!(ids(&e.query("browser"))[0], "apps:helium");
        // Not repeated below.
        assert_eq!(ids(&e.query("browser")).iter().filter(|i| *i == "apps:helium").count(), 1);
    }

    #[test]
    fn empty_lists_recent() {
        let (db, e) = engine();
        assert!(e.query("").is_empty());
        e.activate("apps:ghostty", "");
        db.used("apps:help");
        db.used("apps:help");
        assert_eq!(ids(&e.query("  ")), vec!["apps:help", "apps:ghostty"].iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }
}

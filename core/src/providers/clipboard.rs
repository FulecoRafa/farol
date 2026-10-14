//! Clipboard history (`>`): what was copied, newest first and pinned on
//! top. Enter copies an entry back, Ctrl+D forgets it, Alt+P pins it.
//!
//! Entries come from the platform's watcher (on Linux, Wayland
//! data-control: by event, never polled) through [`Clipboard::record`],
//! on the service thread. Settings: `clipboard.limit` (entries kept,
//! default 500; pinned ones always stay), `clipboard.primary` (also record
//! the primary selection, read at start).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::db::{Clip, ClipData, Db, now};
use crate::engine::Provider;
use crate::item::{Action, Item, Outcome, Preview};
use crate::platform;

/// Entries kept when `clipboard.limit` is unset.
const LIMIT: usize = 500;
/// Rows for `>` alone; typing searches every entry.
const BROWSE: usize = 100;
/// Characters of text a preview shows.
const PREVIEW: usize = 4000;
const TITLE: usize = 90;

pub struct Clipboard {
    db: Db,
    /// The rows, built on demand; dropped when an entry changes and on
    /// every opening (their times are relative).
    items: RefCell<Option<Rc<Vec<Item>>>>,
}

impl Clipboard {
    pub fn new(db: Db) -> Self {
        Self { db, items: RefCell::new(None) }
    }

    /// Record a selection the watcher read.
    pub fn record(&self, clip: platform::Clip) {
        let hash = fnv(&clip.data);
        let limit = self.db.get("clipboard.limit", LIMIT);
        let result = if clip.mime == "text/plain" {
            let text = String::from_utf8_lossy(&clip.data);
            if text.trim().is_empty() {
                return;
            }
            self.db.clip_record(hash, ClipData::Text(&text), now(), limit)
        } else {
            let (width, height) = imagesize::blob_size(&clip.data)
                .map(|s| (s.width as i64, s.height as i64))
                .unwrap_or((0, 0));
            let image = ClipData::Image { mime: &clip.mime, data: &clip.data, width, height };
            self.db.clip_record(hash, image, now(), limit)
        };
        match result {
            Ok(evicted) => evicted.into_iter().for_each(forget_image),
            Err(e) => eprintln!("farol: clipboard: {e}"),
        }
        self.items.replace(None);
    }

    /// Also record the primary selection (`clipboard.primary`).
    pub fn primary(&self) -> bool {
        self.db.get("clipboard.primary", false)
    }

    fn key(id: &str) -> Option<i64> {
        id.strip_prefix("clipboard:")?.parse().ok()
    }

    fn row(c: &Clip, now: i64, index: usize) -> Item {
        let image = c.mime.starts_with("image/");
        let pin = if c.pinned { "desafixar" } else { "fixar" };
        Item {
            id: format!("clipboard:{}", c.id),
            title: if image { image_title(c) } else { title(&c.text) },
            subtitle: format!("{} · {}", ago(c.at, now), size(c.size)),
            icon: if image { "ph:image" } else { "ph:clipboard-text" }.into(),
            keywords: if image { String::new() } else { collapse(&c.text) },
            actions: vec![
                Action::new("copy", "copiar", "enter"),
                Action::new("delete", "apagar", "ctrl+d"),
                Action::new("pin", pin, "alt+p"),
            ],
            hint: if c.pinned { "fixado".into() } else { String::new() },
            // Newer first among equally good matches.
            score: 10 - index.min(10) as i64,
            ..Item::default()
        }
    }
}

impl Provider for Clipboard {
    fn id(&self) -> &'static str {
        "clipboard"
    }
    fn title(&self) -> &'static str {
        "área de transferência"
    }
    fn prefix(&self) -> Option<&'static str> {
        Some(">")
    }
    fn mixed(&self) -> usize {
        0
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        let mut cache = self.items.borrow_mut();
        let items = cache.get_or_insert_with(|| {
            let now = now();
            let limit = self.db.get("clipboard.limit", LIMIT).max(BROWSE);
            Rc::new(self.db.clips(limit).iter().enumerate().map(|(i, c)| Self::row(c, now, i)).collect())
        });
        Some(items.clone())
    }
    fn browse(&self) -> Option<Vec<Item>> {
        self.items().map(|i| i.iter().take(BROWSE).cloned().collect())
    }
    fn opened(&self) {
        self.items.replace(None);
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        let Some(key) = Self::key(id) else {
            return Outcome::toast("entrada inválida");
        };
        self.items.replace(None);
        match action {
            "delete" => match self.db.clip_delete(key) {
                Some(hash) => {
                    forget_image(hash);
                    Outcome::refresh_with("apagado")
                }
                None => Outcome::refresh_with("já não existe"),
            },
            "pin" => match self.db.clip_pin(key) {
                Some(true) => Outcome::refresh_with("fixado"),
                Some(false) => Outcome::refresh_with("desafixado"),
                None => Outcome::refresh_with("já não existe"),
            },
            _ => {
                let Some((clip, text, data)) = self.db.clip(key) else {
                    return Outcome::refresh_with("já não existe");
                };
                let copied = match data {
                    Some(data) => platform::copy_data(&clip.mime, data),
                    None => platform::copy(&text),
                };
                match copied {
                    Ok(()) => {
                        // farol's own selections are not recorded: move it up here.
                        self.db.clip_touch(key, now());
                        Outcome::close_with("copiado")
                    }
                    Err(e) => Outcome::toast(e.to_string()),
                }
            }
        }
    }
    fn preview(&self, id: &str) -> Preview {
        let Some((clip, text, data)) = Self::key(id).and_then(|k| self.db.clip(k)) else {
            return Preview::None;
        };
        let mut facts = vec![("copiado".to_string(), ago(clip.at, now())), ("tamanho".to_string(), size(clip.size))];
        match data {
            Some(data) => {
                let Some(path) = cached_image(clip.hash, &clip.mime, &data) else {
                    return Preview::None;
                };
                facts.push(("tipo".into(), clip.mime.clone()));
                Preview::Image { title: image_title(&clip), src: format!("file:{}", path.display()), facts }
            }
            None => {
                let lines = text.lines().count();
                if lines > 1 {
                    facts.push(("linhas".into(), lines.to_string()));
                }
                let mut shown: String = text.chars().take(PREVIEW).collect();
                if shown.len() < text.len() {
                    shown.push('…');
                }
                Preview::Text { title: "texto".into(), text: shown, facts }
            }
        }
    }
}

/// FNV-1a: a stable 64-bit hash of the bytes (dedupe key and file name).
fn fnv(data: &[u8]) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h as i64
}

/// Whitespace runs to one space, trimmed.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first line with text, collapsed and cut.
fn title(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = collapse(line);
    if line.chars().count() > TITLE {
        line.chars().take(TITLE - 1).collect::<String>() + "…"
    } else {
        line
    }
}

fn image_title(c: &Clip) -> String {
    if c.width > 0 { format!("imagem {}×{}", c.width, c.height) } else { "imagem".into() }
}

/// "agora", "há 5 min", "ontem", "há 3 dias".
fn ago(at: i64, now: i64) -> String {
    let s = (now - at).max(0) / 1000;
    match s {
        0..60 => "agora".into(),
        60..3600 => format!("há {} min", s / 60),
        3600..86_400 => format!("há {} h", s / 3600),
        86_400..172_800 => "ontem".into(),
        172_800..2_592_000 => format!("há {} dias", s / 86_400),
        2_592_000..31_536_000 => format!("há {} meses", (s / 2_592_000).max(1)),
        _ => format!("há {} anos", s / 31_536_000),
    }
    .replace("há 1 meses", "há 1 mês")
    .replace("há 1 anos", "há 1 ano")
}

/// Bytes in a few characters, Brazilian decimal comma: "1,2 kB".
fn size(bytes: i64) -> String {
    let b = bytes as f64;
    let s = if bytes < 1000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{:.1} kB", b / 1e3)
    } else {
        format!("{:.1} MB", b / 1e6)
    };
    s.replace('.', ",")
}

const IMAGE_TYPES: &[(&str, &str)] =
    &[("image/png", "png"), ("image/jpeg", "jpg"), ("image/gif", "gif"), ("image/webp", "webp"), ("image/bmp", "bmp")];

fn image_dir() -> PathBuf {
    platform::cache_dir().join("farol").join("clip")
}

/// The image as a file the renderer may show (`$XDG_CACHE_HOME/farol/clip`),
/// written once.
fn cached_image(hash: i64, mime: &str, data: &[u8]) -> Option<PathBuf> {
    let ext = IMAGE_TYPES.iter().find(|(m, _)| *m == mime).map(|(_, e)| *e).unwrap_or("img");
    let path = image_dir().join(format!("{:016x}.{ext}", hash as u64));
    if !path.exists() {
        std::fs::create_dir_all(image_dir()).ok()?;
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, data).ok()?;
        std::fs::rename(&tmp, &path).ok()?;
    }
    Some(path)
}

/// An entry is gone: so is its cached image.
fn forget_image(hash: i64) {
    let dir = image_dir();
    for ext in IMAGE_TYPES.iter().map(|(_, e)| *e).chain(["img"]) {
        let _ = std::fs::remove_file(dir.join(format!("{:016x}.{ext}", hash as u64)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    fn text(s: &str) -> platform::Clip {
        platform::Clip { mime: "text/plain".into(), data: s.as_bytes().to_vec() }
    }

    #[test]
    fn rows_newest_first_with_titles() {
        let c = Clipboard::new(Store::memory());
        c.record(text("  first line\n\n  second\tline  "));
        c.record(text("   \n "));
        c.record(text("other"));
        let rows = c.browse().unwrap();
        let titles: Vec<&str> = rows.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["other", "first line"]);
        assert_eq!(rows[1].keywords, "first line second line");
        assert_eq!(rows[0].subtitle, "agora · 5 B");
        // Pinning lifts the older one and refreshes the list.
        let out = c.activate(&rows[1].id, "pin");
        assert!(out.refresh);
        assert_eq!(c.browse().unwrap()[0].hint, "fixado");
        assert!(c.activate(&rows[0].id, "delete").refresh);
        assert_eq!(c.browse().unwrap().len(), 1);
    }

    #[test]
    fn words() {
        assert_eq!(title(&"x".repeat(200)).chars().count(), TITLE);
        assert_eq!(ago(0, 61_000), "há 1 min");
        assert_eq!(ago(0, 40 * 86_400_000), "há 1 mês");
        assert_eq!(size(1234), "1,2 kB");
    }
}

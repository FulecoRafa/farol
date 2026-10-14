//! Files (`/`): a search of the home as you type, the recently changed ones
//! for `/` alone, and a few hits in the mixed list. No index and no watcher:
//! a walk that respects `.gitignore` and skips hidden entries takes a few
//! milliseconds, runs off the service thread and is dropped when a newer
//! text arrives.
//!
//! `/ nix/hosts` matches the path, not just the name. Previews: the head of
//! a text file, an image, a folder's entries.

use std::cell::RefCell;
use std::rc::Rc;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use tessela_service::{spawn, unblock};

use crate::db::Db;
use crate::engine::{self, Provider, Query};
use crate::item::{Action, Item, Outcome, Preview};
use crate::matching::fold;
use crate::platform::{self, home};

/// Rows kept per search; the mixed list shows a few of them.
const LIMIT: usize = 60;
const MIXED: usize = 5;
/// Recently changed: within this, newest first.
const RECENT: Duration = Duration::from_secs(7 * 86_400);

pub struct Files {
    db: Db,
    /// The last search: its key and rows (shared with the search task).
    last: Rc<RefCell<(Option<(String, bool)>, Vec<Item>)>>,
    /// Bumped per search; a walk whose generation is stale stops.
    generation: Arc<AtomicU64>,
    /// Search in place (no executor: the `query` command).
    inline: bool,
}

struct Hit {
    path: PathBuf,
    dir: bool,
    modified: SystemTime,
    score: i64,
}

impl Files {
    pub fn new(db: Db, inline: bool) -> Self {
        Self { db, last: Rc::default(), generation: Arc::default(), inline }
    }

    fn roots(&self) -> Vec<PathBuf> {
        let roots: Vec<String> = self.db.get("files.roots", Vec::new());
        if roots.is_empty() {
            vec![home()]
        } else {
            roots.iter().map(|r| expand(r)).collect()
        }
    }

    fn start(&self, text: String, scoped: bool) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let current = self.generation.clone();
        let roots = self.roots();
        let hidden = self.db.get("files.hidden", false);
        let key = (text.clone(), scoped);
        let (last, latest) = (self.last.clone(), self.generation.clone());
        let task = unblock(move || {
            let stale = || current.load(Ordering::SeqCst) != generation;
            walk(&roots, &text, hidden, &stale)
        });
        spawn(async move {
            let hits = task.await;
            if latest.load(Ordering::SeqCst) != generation {
                return;
            }
            let items = hits.into_iter().map(row).collect();
            *last.borrow_mut() = (Some(key), items);
            engine::refresh();
        })
        .detach();
    }
}

/// `~/x` → the home's `x`.
fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

/// `/home/me/Dev/x` → `~/Dev/x`.
fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

fn walk(roots: &[PathBuf], text: &str, hidden: bool, stale: &dyn Fn() -> bool) -> Vec<Hit> {
    let q = fold(text.trim());
    let by_path = q.contains('/');
    let words: Vec<&str> = q.split_whitespace().collect();
    let now = SystemTime::now();
    let mut hits = Vec::new();
    for root in roots {
        let walker = ignore::WalkBuilder::new(root)
            .hidden(!hidden)
            .git_ignore(true)
            .git_global(false)
            .require_git(false)
            .follow_links(false)
            .max_depth(Some(16))
            .filter_entry(|e| !matches!(e.file_name().to_str(), Some("node_modules" | ".git" | "target")))
            .build();
        for (n, entry) in walker.flatten().enumerate() {
            if n % 256 == 0 && stale() {
                return Vec::new();
            }
            if entry.depth() == 0 {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let age = now.duration_since(modified).unwrap_or_default();
            let score = if words.is_empty() {
                // `/` alone: recent files only.
                if meta.is_dir() || age > RECENT {
                    continue;
                }
                0
            } else {
                let name = fold(&entry.file_name().to_string_lossy());
                let hay = if by_path { fold(&entry.path().strip_prefix(root).unwrap_or(entry.path()).to_string_lossy()) } else { name.clone() };
                if !words.iter().all(|w| hay.contains(w)) {
                    continue;
                }
                let tier = if name.starts_with(&q) || name.starts_with(words[0]) {
                    300
                } else if name.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(words[0])) {
                    200
                } else {
                    100
                };
                let fresh = if age < RECENT { 30 } else { 0 };
                tier + fresh - 6 * entry.depth() as i64
            };
            hits.push(Hit { path: entry.into_path(), dir: meta.is_dir(), modified, score });
        }
    }
    if words.is_empty() {
        hits.sort_by(|a, b| b.modified.cmp(&a.modified));
    } else {
        hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| b.modified.cmp(&a.modified)));
    }
    hits.truncate(LIMIT);
    hits
}

fn kind_icon(path: &Path, dir: bool) -> &'static str {
    if dir {
        return "ph:folder";
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        e if IMAGES.contains(&e) => "ph:image",
        "md" | "txt" | "org" | "rst" => "ph:file-text",
        "pdf" => "ph:file-pdf",
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "rar" => "ph:file-zip",
        "mp3" | "flac" | "ogg" | "wav" | "opus" | "m4a" => "ph:file-audio",
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "ph:file-video",
        "nix" | "rs" | "ts" | "tsx" | "js" | "qml" | "py" | "go" | "sh" | "toml" | "json" | "yaml" | "yml" | "lua" | "c" | "h" => "ph:file-code",
        _ => "ph:file",
    }
}

const IMAGES: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "svg", "bmp"];

/// "agora", "há 5 min", "há 3 h", "ontem", "há 4 dias", "12/03/2025".
fn ago(t: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(t).unwrap_or_default().as_secs();
    match secs {
        0..60 => "agora".into(),
        60..3600 => format!("há {} min", secs / 60),
        3600..86_400 => format!("há {} h", secs / 3600),
        86_400..172_800 => "ontem".into(),
        _ if secs < 30 * 86_400 => format!("há {} dias", secs / 86_400),
        _ => {
            let days = (t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() / 86_400) as i64;
            let (y, m, d) = civil(days);
            format!("{d:02}/{m:02}/{y}")
        }
    }
}

/// Days since the epoch → (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
        _ => format!("{:.1} GB", bytes as f64 / 1_073_741_824.0),
    }
}

fn row(h: Hit) -> Item {
    let name = h.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = h.path.parent().map(tilde).unwrap_or_default();
    let mut actions = vec![
        Action::new("open", "abrir", "enter"),
        Action::new("reveal", if h.dir { "abrir no gerenciador" } else { "mostrar na pasta" }, "ctrl+f"),
        Action::new("terminal", "terminal aqui", "ctrl+t"),
    ];
    if !h.dir {
        actions.push(Action::new("edit", "editar", "ctrl+e"));
    }
    actions.push(Action::new("copy", "copiar caminho", "ctrl+c"));
    actions.push(Action::new("copy-file", "copiar como arquivo", "ctrl+u"));
    Item {
        id: format!("files:{}", h.path.display()),
        title: name,
        subtitle: format!("{} · {parent}", ago(h.modified)),
        icon: kind_icon(&h.path, h.dir).into(),
        actions,
        path: h.path.display().to_string(),
        ..Item::default()
    }
}

/// `{path}` and `{dir}` filled in a command from the settings.
fn fill(argv: &[String], path: &Path) -> Vec<String> {
    let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
    argv.iter()
        .map(|a| a.replace("{path}", &path.to_string_lossy()).replace("{dir}", &dir.to_string_lossy()))
        .collect()
}

impl Provider for Files {
    fn id(&self) -> &'static str {
        "files"
    }
    fn title(&self) -> &'static str {
        "arquivos"
    }
    fn prefix(&self) -> Option<&'static str> {
        Some("/")
    }
    fn mixed(&self) -> usize {
        0
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        if !q.scoped && (q.text.chars().count() < 3 || !self.db.get("files.mixed", true)) {
            return Vec::new();
        }
        let key = (q.text.to_string(), q.scoped);
        {
            let last = self.last.borrow();
            if last.0.as_ref() == Some(&key) {
                return last.1.iter().take(if q.scoped { LIMIT } else { MIXED }).cloned().collect();
            }
        }
        // Not searched yet: start, and show nothing until it answers (a few
        // ms; the answer refreshes the list in place).
        if self.inline {
            let hits = walk(&self.roots(), &key.0, self.db.get("files.hidden", false), &|| false);
            let items: Vec<Item> = hits.into_iter().map(row).collect();
            *self.last.borrow_mut() = (Some(key), items);
            return self.generate(q);
        }
        self.start(key.0, key.1);
        Vec::new()
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        let path = PathBuf::from(id.strip_prefix("files:").unwrap_or_default());
        let text = path.to_string_lossy().into_owned();
        let result = match action {
            "reveal" => platform::reveal(&path),
            "terminal" => {
                let argv: Vec<String> = self.db.get("terminal.here", platform::default_terminal_here());
                platform::spawn(&fill(&argv, &path))
            }
            "edit" => {
                let argv: Vec<String> = self.db.get("editor", platform::default_editor());
                platform::spawn(&fill(&argv, &path))
            }
            "copy" => platform::copy(&text).map(|_| ()),
            "copy-file" => platform::copy_files(&[path.clone()]),
            _ => platform::open(&text),
        };
        match (result, action) {
            (Err(e), _) => Outcome::toast(e.to_string()),
            (Ok(()), "copy") => Outcome::close_with("caminho copiado"),
            (Ok(()), "copy-file") => Outcome::close_with("arquivo copiado"),
            (Ok(()), _) => Outcome::close(),
        }
    }
    fn preview(&self, id: &str) -> Preview {
        let path = PathBuf::from(id.strip_prefix("files:").unwrap_or_default());
        preview(&path)
    }
}

fn preview(path: &Path) -> Preview {
    let Ok(meta) = path.metadata() else { return Preview::None };
    let title = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut facts = vec![("pasta".to_string(), path.parent().map(tilde).unwrap_or_default())];
    if let Ok(m) = meta.modified() {
        facts.push(("alterado".into(), ago(m)));
    }
    if meta.is_dir() {
        let mut names: Vec<(bool, String)> = std::fs::read_dir(path)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .map(|e| (!e.path().is_dir(), e.file_name().to_string_lossy().into_owned()))
                    .collect()
            })
            .unwrap_or_default();
        facts.push(("itens".into(), names.len().to_string()));
        names.sort();
        let text = names
            .iter()
            .take(40)
            .map(|(file, n)| if *file { n.clone() } else { format!("{n}/") })
            .collect::<Vec<_>>()
            .join("\n");
        return Preview::Text { title, text, facts };
    }
    facts.push(("tamanho".into(), size(meta.len())));
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if IMAGES.contains(&ext.as_str()) {
        if let Some(src) = platform::viewable(path) {
            return Preview::Image { title, src: format!("file:{}", src.display()), facts };
        }
    }
    let text = head(path).unwrap_or_default();
    Preview::Text { title, text, facts }
}

/// The first 4 KB of a text file, `None` for binary files.
fn head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = vec![0; 4096];
    let n = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
    buf.truncate(n);
    if buf.contains(&0) {
        return None;
    }
    // A multi-byte character may be cut at the end.
    let text = match std::str::from_utf8(&buf) {
        Ok(t) => t.to_string(),
        Err(e) if e.valid_up_to() + 4 >= n => String::from_utf8_lossy(&buf[..e.valid_up_to()]).into_owned(),
        Err(_) => return None,
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_matches_names_and_paths() {
        let dir = std::env::temp_dir().join(format!("farol-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("hosts/laptop")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::write(dir.join("hosts/laptop/configuration.nix"), "{}").unwrap();
        std::fs::write(dir.join("node_modules/x/configuration.nix"), "{}").unwrap();
        std::fs::write(dir.join("notes.md"), "hi").unwrap();
        let never = || false;
        let names = |hits: Vec<Hit>| hits.iter().map(|h| h.path.strip_prefix(&dir).unwrap().display().to_string()).collect::<Vec<_>>();
        assert_eq!(names(walk(&[dir.clone()], "configuration", false, &never)), vec!["hosts/laptop/configuration.nix"]);
        assert_eq!(names(walk(&[dir.clone()], "laptop/conf", false, &never)), vec!["hosts/laptop/configuration.nix"]);
        // Alone: recent files, no folders.
        let recent = names(walk(&[dir.clone()], "", false, &never));
        assert!(recent.contains(&"notes.md".to_string()) && !recent.contains(&"hosts".to_string()));
        assert!(walk(&[dir.clone()], "notes", false, &|| true).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_723), (2024, 1, 1));
    }
}

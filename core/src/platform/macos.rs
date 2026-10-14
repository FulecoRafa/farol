//! macOS: `~/Library`, `.app` bundles, `open`, `pbcopy`.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use super::{App, home};

pub fn data_dir() -> PathBuf {
    home().join("Library/Application Support")
}

pub fn cache_dir() -> PathBuf {
    home().join("Library/Caches")
}

pub(super) fn detach(cmd: &mut Command) {
    cmd.process_group(0);
}

pub fn open(target: &str) -> std::io::Result<()> {
    super::spawn(&["open", target])
}

pub fn copy(text: &str) -> std::io::Result<()> {
    super::pipe(&["pbcopy"], text.as_bytes())
}

pub(super) fn launch_app(app: &App) -> std::io::Result<()> {
    super::spawn(&app.exec)
}

fn app_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Utilities"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
        home().join("Applications"),
    ]
}

pub fn apps_stamp() -> SystemTime {
    app_dirs()
        .iter()
        .filter_map(|d| d.metadata().and_then(|m| m.modified()).ok())
        .max()
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

pub fn apps() -> Vec<App> {
    let mut out = Vec::new();
    for dir in app_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "app") {
                out.push(bundle(&p));
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn bundle(path: &Path) -> App {
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // A PNG next to the bundle's icon, when the bundle ships one; .icns is
    // not something the renderer decodes.
    let png = path.join("Contents/Resources/AppIcon.png");
    App {
        id: path.to_string_lossy().into_owned(),
        name,
        comment: String::new(),
        keywords: String::new(),
        icon: if png.exists() { format!("file:{}", png.display()) } else { String::new() },
        exec: vec!["open".into(), "-a".into(), path.to_string_lossy().into_owned()],
        terminal: false,
    }
}

/// Selects `path` in a Finder window.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    super::spawn(&[std::ffi::OsStr::new("open"), std::ffi::OsStr::new("-R"), path.as_os_str()])
}

/// Files on the pasteboard as files, through AppleScript's `POSIX file`.
pub fn copy_files(paths: &[PathBuf]) -> std::io::Result<()> {
    let list = paths
        .iter()
        .map(|p| format!("POSIX file \"{}\"", p.display().to_string().replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(", ");
    super::spawn(&["osascript", "-e", &format!("set the clipboard to {{{list}}}")])
}

pub fn default_terminal_here() -> Vec<String> {
    ["open", "-a", "Terminal", "{dir}"].map(String::from).to_vec()
}

pub fn default_editor() -> Vec<String> {
    ["open", "-t", "{path}"].map(String::from).to_vec()
}

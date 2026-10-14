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

/// Only text for now: images need NSPasteboard (objc), not `pbcopy`.
pub fn copy_data(mime: &str, data: Vec<u8>) -> std::io::Result<()> {
    if !mime.starts_with("text/") {
        return Err(std::io::Error::other("só texto no macOS por enquanto"));
    }
    super::pipe(&["pbcopy"], &data)
}

/// No history yet: NSPasteboard has no change notification, only a
/// `changeCount` to poll, and farol does not poll. Left off on purpose.
pub fn watch_clipboard(_primary: bool, _tx: async_channel::Sender<super::Clip>) {}

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
        wm_class: String::new(),
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

/// Open windows: not wired on this OS yet (phase 5 of the plan).
pub fn windows() -> Vec<super::Window> {
    Vec::new()
}

pub fn has_windows() -> bool {
    false
}

pub fn window_op(_window: Option<&super::Window>, _op: super::WindowOp) -> std::io::Result<()> {
    Err(std::io::Error::other("window management is not available on this OS yet"))
}

/// A Notification Center banner, through AppleScript.
pub fn notify(title: &str, body: &str) -> std::io::Result<()> {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\" sound name \"Glass\"", quote(body), quote(title));
    super::spawn(&["osascript", "-e", &script])
}

// Global hotkeys on this OS are phase 5 of the plan (a native registration,
// e.g. the `global-hotkey` crate); until then farol has none here.
pub fn desktop_binds() -> Vec<super::DesktopBind> {
    Vec::new()
}

pub fn has_hotkeys() -> bool {
    false
}

pub fn bind_hotkey(_keys: &str, _command: &str) -> std::io::Result<Option<super::DesktopBind>> {
    Err(std::io::Error::other("atalhos globais ainda não existem neste sistema"))
}

pub fn unbind_hotkey(_keys: &str, _restore: Option<&super::DesktopBind>) -> std::io::Result<()> {
    Ok(())
}

pub fn run_desktop_bind(_b: &super::DesktopBind) -> std::io::Result<()> {
    Ok(())
}

pub fn on_desktop_reload(_tx: async_channel::Sender<()>) {}

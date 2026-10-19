//! macOS: `~/Library`, `.app` bundles, `open`, `pbcopy`.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
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
    convert_icons(out.iter().map(|a| PathBuf::from(&a.id)).collect());
    out
}

fn icons_dir() -> PathBuf {
    cache_dir().join("farol").join("icons")
}

/// Where a bundle's icon goes as a PNG, named after the bundle's path.
fn icon_png(bundle: &Path) -> PathBuf {
    let name: String = bundle
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    icons_dir().join(format!("{}.png", name.trim_start_matches('_')))
}

/// Bundles ship their icon as `.icns`, which the renderer does not decode:
/// `sips` turns each into a PNG in the cache, off the service thread, once
/// per icon (again when the bundle's Info.plist is newer, as after an
/// update). Rows point at the PNGs already, and the renderer shows each as
/// soon as it exists.
fn convert_icons(bundles: Vec<PathBuf>) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    let modified = |p: &Path| p.metadata().and_then(|m| m.modified()).ok();
    let stale: Vec<PathBuf> = bundles
        .into_iter()
        .filter(|b| {
            let png = modified(&icon_png(b));
            png.is_none() || png < modified(&b.join("Contents/Info.plist"))
        })
        .collect();
    if stale.is_empty() || RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::fs::create_dir_all(icons_dir());
    std::thread::spawn(move || {
        for b in stale {
            if let Some(icns) = icns(&b) {
                let _ = Command::new("sips")
                    .args(["-s", "format", "png", "-Z", "128"])
                    .arg(&icns)
                    .arg("--out")
                    .arg(icon_png(&b))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        RUNNING.store(false, Ordering::Release);
    });
}

/// The bundle's `.icns`: `CFBundleIconFile` from its Info.plist (XML or
/// binary, so through `plutil`), with or without the extension.
fn icns(bundle: &Path) -> Option<PathBuf> {
    let out = Command::new("plutil")
        .args(["-extract", "CFBundleIconFile", "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist"))
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let name = String::from_utf8(out.stdout).ok()?;
    let name = name.trim();
    if !out.status.success() || name.is_empty() {
        return None;
    }
    let file = bundle.join("Contents/Resources").join(name);
    let file = if file.extension().is_some_and(|e| e == "icns") { file } else { file.with_extension("icns") };
    file.is_file().then_some(file)
}

fn bundle(path: &Path) -> App {
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    App {
        id: path.to_string_lossy().into_owned(),
        name,
        comment: String::new(),
        keywords: String::new(),
        icon: format!("file:{}", icon_png(path).display()),
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

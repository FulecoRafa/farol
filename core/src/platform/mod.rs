//! What differs per OS: where data lives, how to start things, the
//! clipboard and which apps are installed. Linux is the full one; macOS and
//! Windows get the same functions over their own conventions.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
pub mod hyprland;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "linux")]
mod wayland;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// An installed application.
#[derive(Clone, Debug, Default)]
pub struct App {
    /// Stable: the desktop entry id, the bundle path, the shortcut path.
    pub id: String,
    pub name: String,
    pub comment: String,
    /// Generic name, keywords, categories: matched, not shown.
    pub keywords: String,
    /// An image source for the renderer (`icon:name` or `file:/path`).
    pub icon: String,
    pub exec: Vec<String>,
    /// The window class its windows carry (`StartupWMClass`), when declared.
    pub wm_class: String,
    pub terminal: bool,
}

pub fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Start a program detached from farol: its own session, no stdio, reaped
/// by a thread so it never lingers as a zombie.
pub fn spawn<S: AsRef<OsStr>>(argv: &[S]) -> std::io::Result<()> {
    let Some((program, args)) = argv.split_first() else {
        return Err(std::io::Error::other("empty command"));
    };
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(home());
    detach(&mut cmd);
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Run a shell command line (user-defined actions).
pub fn shell(line: &str) -> std::io::Result<()> {
    if cfg!(windows) {
        spawn(&["cmd", "/C", line])
    } else {
        spawn(&["sh", "-c", line])
    }
}

/// Run and wait, feeding `input`; for small helpers like `wl-copy`.
pub fn pipe<S: AsRef<OsStr>>(argv: &[S], input: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let Some((program, args)) = argv.split_first() else {
        return Ok(());
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input)?;
    }
    // wl-copy forks to serve the selection; the parent exits right away.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// A new clipboard selection, as the watcher read it: `text/plain` (UTF-8)
/// or an image type with its bytes.
pub struct Clip {
    pub mime: String,
    pub data: Vec<u8>,
}

/// Launch an app, in a terminal when it asks for one.
pub fn launch(app: &App, terminal: &[String]) -> std::io::Result<()> {
    if app.terminal && !terminal.is_empty() {
        let mut argv = terminal.to_vec();
        argv.extend(app.exec.iter().cloned());
        spawn(&argv)
    } else {
        launch_app(app)
    }
}

/// `file:///home/me/My%20File.pdf`: the URI of a path, percent-encoded.
pub fn file_uri(path: &std::path::Path) -> String {
    let mut out = String::from("file://");
    let text = path.to_string_lossy();
    let text = if cfg!(windows) { format!("/{}", text.replace('\\', "/")) } else { text.into_owned() };
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A link to `path` under `<cache>/farol/<dir>`, named after the path: the
/// renderer shows `file:` images only from a few places, the user's cache
/// among them. Kept while it points at the same file.
#[cfg(unix)]
pub fn cache_link(path: &std::path::Path, dir: &str) -> Option<PathBuf> {
    let ext = path.extension()?.to_str()?;
    let dir = cache_dir().join("farol").join(dir);
    let name: String = path
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    let name = name.trim_start_matches('_');
    let link = dir.join(if name.ends_with(ext) { name.to_string() } else { format!("{name}.{ext}") });
    if std::fs::read_link(&link).ok().as_deref() != Some(path) {
        std::fs::create_dir_all(&dir).ok()?;
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(path, &link).ok()?;
    }
    Some(link)
}

#[cfg(not(unix))]
pub fn cache_link(path: &std::path::Path, _dir: &str) -> Option<PathBuf> {
    Some(path.to_path_buf())
}

/// An image file the launcher may draw (see [`cache_link`]).
pub fn viewable(path: &std::path::Path) -> Option<PathBuf> {
    cache_link(path, "view")
}

#[cfg(test)]
mod tests {
    #[test]
    fn uris_are_encoded() {
        let p = std::path::Path::new("/home/me/My File#1.pdf");
        assert_eq!(super::file_uri(p), "file:///home/me/My%20File%231.pdf");
    }
}

/// An open window, for the window plugin.
#[derive(Clone, Debug, Default)]
pub struct Window {
    /// What the window manager calls it (Hyprland: the address).
    pub id: String,
    /// Its app: class or app id (`helium`, `com.mitchellh.ghostty`).
    pub app: String,
    pub title: String,
    pub workspace: String,
    pub pid: i64,
}

/// Something done to a window: a listed one, or the focused one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowOp {
    Focus,
    Close,
    /// SIGKILL to its process.
    Kill,
    Float,
    Fullscreen,
    /// On every workspace (floating only).
    Pin,
    Center,
    /// Move to a workspace and follow it.
    MoveTo(u32),
    /// Switch to a workspace.
    GoTo(u32),
}

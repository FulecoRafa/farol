//! What differs per OS: where data lives, how to start things, the
//! clipboard and which apps are installed. Linux is the full one; macOS and
//! Windows get the same functions over their own conventions.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

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

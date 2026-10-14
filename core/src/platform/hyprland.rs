//! Hyprland over its IPC socket (what `hyprctl` does, without the process):
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`, one
//! request per connection.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

fn socket(name: &str) -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(PathBuf::from(runtime).join("hypr").join(sig).join(name))
}

/// Whether farol runs under Hyprland.
pub fn present() -> bool {
    socket(".socket.sock").is_some_and(|p| p.exists())
}

/// One request (`j/clients`, `dispatch workspace 3`), its whole answer.
pub fn request(cmd: &str) -> std::io::Result<String> {
    let path = socket(".socket.sock").ok_or_else(|| std::io::Error::other("not running under Hyprland"))?;
    let mut s = UnixStream::connect(path)?;
    s.set_read_timeout(Some(Duration::from_secs(1)))?;
    s.write_all(cmd.as_bytes())?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

/// `dispatch <args>`; Hyprland answers "ok" or the error.
pub fn dispatch(args: &str) -> std::io::Result<()> {
    let answer = request(&format!("dispatch {args}"))?;
    if answer.trim() == "ok" { Ok(()) } else { Err(std::io::Error::other(answer.trim().to_string())) }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub address: String,
    pub mapped: bool,
    pub hidden: bool,
    pub class: String,
    pub title: String,
    pub pid: i64,
    pub workspace: Workspace,
    pub floating: bool,
    #[serde(rename = "focusHistoryID")]
    pub focus_history_id: i64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Workspace {
    pub name: String,
}

/// Open windows, most recently focused first.
pub fn clients() -> Vec<Client> {
    let Ok(json) = request("j/clients") else { return Vec::new() };
    let mut list: Vec<Client> = serde_json::from_str(&json).unwrap_or_default();
    list.retain(|c| c.mapped && !c.hidden && c.pid > 0);
    list.sort_by_key(|c| c.focus_history_id);
    list
}

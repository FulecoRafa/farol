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

/// `keyword <args>` (a runtime config change: `bind ...`, `unbind ...`).
pub fn keyword(args: &str) -> std::io::Result<()> {
    let answer = request(&format!("keyword {args}"))?;
    if answer.trim() == "ok" { Ok(()) } else { Err(std::io::Error::other(answer.trim().to_string())) }
}

/// A keybinding as Hyprland has it now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bind {
    /// SHIFT 1, CTRL 4, ALT 8, SUPER 64.
    pub mods: u32,
    pub key: String,
    pub submap: String,
    pub description: String,
    pub dispatcher: String,
    pub arg: String,
}

/// Every binding, from `binds` (its text form: the JSON one of some
/// versions is not valid JSON).
pub fn binds() -> Vec<Bind> {
    request("binds").map(|t| parse_binds(&t)).unwrap_or_default()
}

pub(crate) fn parse_binds(text: &str) -> Vec<Bind> {
    let mut out = Vec::new();
    let mut cur: Option<Bind> = None;
    for line in text.lines() {
        if !line.starts_with('\t') {
            out.extend(cur.take());
            if line.starts_with("bind") {
                cur = Some(Bind::default());
            }
            continue;
        }
        let Some(b) = cur.as_mut() else { continue };
        let Some((k, v)) = line.trim_start().split_once(':') else { continue };
        let v = v.strip_prefix(' ').unwrap_or(v).to_string();
        match k {
            "modmask" => b.mods = v.parse().unwrap_or(0),
            "submap" => b.submap = v,
            "key" => b.key = v,
            "description" => b.description = v,
            "dispatcher" => b.dispatcher = v,
            "arg" => b.arg = v,
            _ => {}
        }
    }
    out.extend(cur);
    out
}

const MODS: &[(&str, u32)] = &[("super", 64), ("ctrl", 4), ("alt", 8), ("shift", 1)];

/// `super+alt+space` → (72, "space"); `None` without a key.
pub fn split_keys(keys: &str) -> Option<(u32, String)> {
    let mut mods = 0;
    let mut key = None;
    for part in keys.split('+') {
        match MODS.iter().find(|(n, _)| *n == part) {
            Some((_, bit)) => mods |= bit,
            None => key = Some(part.to_string()),
        }
    }
    Some((mods, key?))
}

/// (72, "space") → `super+alt+space`, the order farol stores.
pub fn join_keys(mods: u32, key: &str) -> String {
    let mut parts: Vec<String> = MODS.iter().filter(|(_, b)| mods & b != 0).map(|(n, _)| n.to_string()).collect();
    parts.push(key.to_lowercase());
    parts.join("+")
}

/// `SUPER ALT,space`: how `bind`/`unbind` name a combination.
pub fn bind_spec(mods: u32, key: &str) -> String {
    let names: Vec<String> = MODS.iter().filter(|(_, b)| mods & b != 0).map(|(n, _)| n.to_uppercase()).collect();
    format!("{},{key}", names.join(" "))
}

/// Calls `f` whenever Hyprland reloads its config (which drops runtime
/// binds). A thread blocked on the event socket: no polling.
pub fn on_reload(tx: async_channel::Sender<()>) {
    let Some(path) = socket(".socket2.sock") else { return };
    std::thread::Builder::new()
        .name("hypr-events".into())
        .spawn(move || {
            use std::io::BufRead;
            let Ok(s) = UnixStream::connect(&path) else { return };
            for line in std::io::BufReader::new(s).lines() {
                let Ok(line) = line else { break };
                if line.starts_with("configreloaded>>") && tx.send_blocking(()).is_err() {
                    break;
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_from_text() {
        let text = "bind\n\tmodmask: 64\n\tsubmap: \n\tkey: Q\n\tkeycode: 0\n\tcatchall: false\n\tdescription: \n\tdispatcher: killactive\n\targ: \n\nbindl\n\tmodmask: 0\n\tsubmap: \n\tkey: XF86AudioMute\n\tdispatcher: exec\n\targ: wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle\n";
        let b = parse_binds(text);
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].mods, b[0].key.as_str(), b[0].dispatcher.as_str()), (64, "Q", "killactive"));
        assert_eq!(b[1].arg, "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle");
    }

    #[test]
    fn keys() {
        assert_eq!(split_keys("super+alt+space"), Some((72, "space".into())));
        assert_eq!(join_keys(72, "SPACE"), "super+alt+space");
        assert_eq!(bind_spec(72, "space"), "SUPER ALT,space");
        assert_eq!(split_keys("super+alt"), None);
    }
}

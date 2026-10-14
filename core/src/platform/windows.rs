//! Windows: `%APPDATA%`, Start menu shortcuts, `start`, `clip`.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use super::{App, home};

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn env_dir(key: &str, fallback: &str) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| home().join(fallback))
}

pub fn data_dir() -> PathBuf {
    env_dir("APPDATA", "AppData/Roaming")
}

pub fn cache_dir() -> PathBuf {
    env_dir("LOCALAPPDATA", "AppData/Local")
}

pub(super) fn detach(cmd: &mut Command) {
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

pub fn open(target: &str) -> std::io::Result<()> {
    super::spawn(&["cmd", "/C", "start", "", target])
}

pub fn copy(text: &str) -> std::io::Result<()> {
    super::pipe(&["clip"], text.as_bytes())
}

pub(super) fn launch_app(app: &App) -> std::io::Result<()> {
    super::spawn(&app.exec)
}

fn app_dirs() -> Vec<PathBuf> {
    let programs = "Microsoft/Windows/Start Menu/Programs";
    let mut dirs = vec![data_dir().join(programs)];
    if let Some(all) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(all).join(programs));
    }
    dirs
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
        collect(&dir, &mut out);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    out
}

fn collect(dir: &Path, out: &mut Vec<App>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if name.to_lowercase().contains("uninstall") {
                continue;
            }
            let target = p.to_string_lossy().into_owned();
            out.push(App {
                id: target.clone(),
                name,
                exec: vec!["cmd".into(), "/C".into(), "start".into(), String::new(), target],
                ..App::default()
            });
        }
    }
}

/// Selects `path` in an Explorer window.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    super::spawn(&["explorer".to_string(), format!("/select,{}", path.display())])
}

/// Files on the clipboard as files (a file drop list), through PowerShell.
pub fn copy_files(paths: &[PathBuf]) -> std::io::Result<()> {
    let list = paths
        .iter()
        .map(|p| format!("'{}'", p.display().to_string().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    super::spawn(&["powershell", "-NoProfile", "-Command", &format!("Set-Clipboard -Path {list}")])
}

pub fn default_terminal_here() -> Vec<String> {
    ["wt", "-d", "{dir}"].map(String::from).to_vec()
}

pub fn default_editor() -> Vec<String> {
    ["notepad", "{path}"].map(String::from).to_vec()
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

//! Linux (freedesktop): XDG dirs, desktop entries, `xdg-open`, `wl-copy`.

use std::collections::HashSet;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use super::{App, home};

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".local/share"))
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".cache"))
}

pub(super) fn detach(cmd: &mut Command) {
    // Its own process group: a Ctrl+C or SIGTERM to farol's group skips it.
    cmd.process_group(0);
}

pub fn open(target: &str) -> std::io::Result<()> {
    super::spawn(&["xdg-open", target])
}

pub fn copy(text: &str) -> std::io::Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        super::pipe(&["wl-copy"], text.as_bytes())
    } else {
        super::pipe(&["xclip", "-selection", "clipboard"], text.as_bytes())
    }
}

pub(super) fn launch_app(app: &App) -> std::io::Result<()> {
    super::spawn(&app.exec)
}

/// Directories with desktop entries, most important first.
pub fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![data_dir()];
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let system = if system.is_empty() { "/usr/local/share:/usr/share".into() } else { system };
    dirs.extend(system.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    let mut seen = HashSet::new();
    dirs.into_iter()
        .map(|d| d.join("applications"))
        .filter(|d| seen.insert(d.clone()) && d.is_dir())
        .collect()
}

/// Newest change among the entry directories: rescan only when it moves.
pub fn apps_stamp() -> SystemTime {
    let mut newest = SystemTime::UNIX_EPOCH;
    for dir in app_dirs() {
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            if let Ok(m) = d.metadata().and_then(|m| m.modified()) {
                newest = newest.max(m);
            }
            if let Ok(rd) = std::fs::read_dir(&d) {
                stack.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            }
        }
    }
    newest
}

pub fn apps() -> Vec<App> {
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .map(|s| s.to_lowercase())
        .collect();
    let locale = locale();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for dir in app_dirs() {
        let mut files = Vec::new();
        collect(&dir, &mut files);
        for path in files {
            let rel = path.strip_prefix(&dir).unwrap_or(&path);
            let id = rel.to_string_lossy().replace('/', "-");
            // The first directory wins, even when its entry hides the app.
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(app) = parse_entry(&id, &text, &locale, &desktops) {
                out.push(app);
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "desktop") {
            out.push(p);
        }
    }
}

/// `pt_BR.UTF-8` → ["pt_BR", "pt"], for `Name[pt_BR]`.
fn locale() -> Vec<String> {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    let base = raw.split(['.', '@']).next().unwrap_or("").to_string();
    let mut out = Vec::new();
    if !base.is_empty() && base != "C" && base != "POSIX" {
        if let Some((lang, _)) = base.split_once('_') {
            out.push(base.clone());
            out.push(lang.to_string());
        } else {
            out.push(base);
        }
    }
    out
}

pub(crate) fn parse_entry(id: &str, text: &str, locale: &[String], desktops: &[String]) -> Option<App> {
    let mut in_main = false;
    // key → (locale rank, value): lower rank = better match.
    let mut fields: std::collections::HashMap<&str, (usize, String)> = Default::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        let (key, rank) = match k.split_once('[') {
            Some((key, loc)) => {
                let loc = loc.trim_end_matches(']');
                match locale.iter().position(|l| l == loc) {
                    Some(r) => (key, r),
                    None => continue,
                }
            }
            None => (k, locale.len()),
        };
        let slot = fields.entry(key).or_insert((usize::MAX, String::new()));
        if rank < slot.0 {
            *slot = (rank, unescape(v));
        }
    }
    let get = |k: &str| fields.get(k).map(|(_, v)| v.as_str()).unwrap_or("");
    let yes = |k: &str| get(k).eq_ignore_ascii_case("true");
    if get("Type") != "Application" || yes("NoDisplay") || yes("Hidden") || get("Exec").is_empty() {
        return None;
    }
    let list = |k: &str| get(k).split(';').filter(|s| !s.is_empty()).map(|s| s.to_lowercase()).collect::<Vec<_>>();
    let only = list("OnlyShowIn");
    if !only.is_empty() && !only.iter().any(|d| desktops.contains(d)) {
        return None;
    }
    if list("NotShowIn").iter().any(|d| desktops.contains(d)) {
        return None;
    }
    let exec = parse_exec(get("Exec"), get("Name"), get("Icon"));
    if exec.is_empty() {
        return None;
    }
    let icon = get("Icon");
    let icon = if icon.is_empty() {
        String::new()
    } else if icon.starts_with('/') {
        linked_icon(Path::new(icon)).map(|p| format!("file:{}", p.display())).unwrap_or_default()
    } else {
        format!("icon:{icon}")
    };
    let keywords = [get("GenericName"), &get("Keywords").replace(';', " "), &get("Categories").replace(';', " "), id.trim_end_matches(".desktop")]
        .join(" ");
    Some(App {
        id: id.to_string(),
        name: get("Name").to_string(),
        comment: if get("Comment").is_empty() { get("GenericName") } else { get("Comment") }.to_string(),
        keywords,
        icon,
        exec,
        terminal: yes("Terminal"),
    })
}

/// An icon given by absolute path (often deep in /nix/store or /opt),
/// linked into `~/.cache/farol/icons`: renderers show `file:` images only
/// from a few places, and the user's cache is one of them.
fn linked_icon(path: &Path) -> Option<PathBuf> {
    let ext = path.extension()?.to_str()?;
    let dir = cache_dir().join("farol").join("icons");
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

/// Value escapes of the spec: `\s \n \t \r \\`.
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// `Exec` into argv: quoted args per the spec, field codes dropped (farol
/// opens apps without files), `%c` the name and `%i` the icon.
pub(crate) fn parse_exec(exec: &str, name: &str, icon: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    let mut it = exec.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' if !quoted => {
                if any || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            _ => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        args.push(cur);
    }
    let mut out = Vec::new();
    for a in args {
        match a.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" | "%k" => {}
            "%c" => out.push(name.to_string()),
            "%i" => {
                if !icon.is_empty() {
                    out.push("--icon".into());
                    out.push(icon.to_string());
                }
            }
            _ => out.push(a.replace("%%", "%")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_quotes_and_codes() {
        assert_eq!(parse_exec("firefox %u", "", ""), vec!["firefox"]);
        assert_eq!(
            parse_exec(r#""/opt/My App/run" --name "a \"b\"" %F"#, "", ""),
            vec!["/opt/My App/run", "--name", r#"a "b""#]
        );
        assert_eq!(parse_exec("x 100%%", "", ""), vec!["x", "100%"]);
    }

    #[test]
    fn entry_locale_and_visibility() {
        let text = "[Desktop Entry]\nType=Application\nName=Files\nName[pt_BR]=Arquivos\nName[pt]=Ficheiros\nExec=nautilus --new-window\nIcon=org.gnome.Nautilus\nKeywords=folder;manager;\n[Desktop Action new]\nName=New\n";
        let loc = vec!["pt_BR".to_string(), "pt".to_string()];
        let app = parse_entry("org.gnome.Nautilus.desktop", text, &loc, &[]).unwrap();
        assert_eq!(app.name, "Arquivos");
        assert_eq!(app.icon, "icon:org.gnome.Nautilus");
        assert!(app.keywords.contains("folder manager"));
        let hidden = text.replace("Type=Application", "Type=Application\nNoDisplay=true");
        assert!(parse_entry("x.desktop", &hidden, &loc, &[]).is_none());
        let gnome_only = text.replace("Type=Application", "Type=Application\nOnlyShowIn=GNOME;");
        assert!(parse_entry("x.desktop", &gnome_only, &loc, &["hyprland".into()]).is_none());
    }
}

//! Starting farol: where the tessela runner and the project are, `farol
//! start` (what login runs), `farol dev` (a second copy with devtools) and
//! starting at login on each OS.
//!
//! The project is `$FAROL_PROJECT_DIR`, else `<prefix>/share/farol` next
//! to an installed binary, else the checkout this binary was built in
//! (`core/target/release/farol` → the folder with farol's tessela.toml).
//! The runner is `$TESSELA`, else a `tessela` beside this binary, else the
//! sibling checkout's build, else `tessela` on the PATH.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("farol"))
}

fn is_farol(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("tessela.toml"))
        .is_ok_and(|t| t.lines().any(|l| l.trim().replace(' ', "") == "name=\"farol\""))
}

pub fn project_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FAROL_PROJECT_DIR") {
        return Some(dir.into());
    }
    let exe = exe();
    let installed = exe.parent()?.parent()?.join("share/farol");
    if installed.join("tessela.toml").exists() {
        return Some(installed);
    }
    exe.ancestors().skip(1).find(|d| is_farol(d)).map(Path::to_path_buf)
}

pub fn tessela() -> PathBuf {
    if let Some(t) = std::env::var_os("TESSELA") {
        return t.into();
    }
    let name = if cfg!(windows) { "tessela.exe" } else { "tessela" };
    let beside = exe().with_file_name(name);
    if beside.exists() {
        return beside;
    }
    if let Some(dev) = project_dir().and_then(|p| p.parent().map(|d| d.join("tessela/target/release").join(name))) {
        if dev.exists() {
            return dev;
        }
    }
    PathBuf::from(name)
}

/// A farol answers on the socket.
fn running() -> bool {
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(crate::client::socket()).is_ok()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// `farol start`: run the launcher (the runner, which starts the core).
/// Already running: nothing to do.
pub fn start() -> ExitCode {
    if running() {
        eprintln!("farol: já está rodando");
        return ExitCode::SUCCESS;
    }
    let Some(dir) = project_dir() else {
        eprintln!("farol: não achei o projeto (defina FAROL_PROJECT_DIR)");
        return ExitCode::FAILURE;
    };
    let mut cmd = Command::new(tessela());
    cmd.arg("run").arg(&dir);
    exec(cmd)
}

/// `farol dev [--nested]`: a development copy beside the installed one,
/// named `farol-dev` (own sockets, own database `farol-dev.db`), with the
/// tessela devtools served and its window opened.
pub fn dev(nested: bool) -> ExitCode {
    let Some(dir) = project_dir() else {
        eprintln!("farol: não achei o projeto (defina FAROL_PROJECT_DIR)");
        return ExitCode::FAILURE;
    };
    let db = crate::platform::data_dir().join("farol").join("farol-dev.db");
    let tessela = tessela();
    let mut run = Command::new(&tessela);
    run.arg("run").arg(&dir).args(["--name", "farol-dev", "--devtools", "--dev"]);
    if nested {
        run.arg("--nested");
    }
    run.env("FAROL_DB", &db).env("FAROL_PROJECT", "farol-dev");
    // Stopping `farol dev` (Ctrl+C, a kill) stops the copy too.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: prctl only, between fork and exec.
        unsafe {
            run.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    eprintln!("farol dev: {} (banco {})", dir.display(), db.display());
    eprintln!("farol dev: abra com FAROL_PROJECT=farol-dev farol toggle");
    let mut child = match run.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("farol: {}: {e}", tessela.display());
            return ExitCode::FAILURE;
        }
    };
    // The devtools window, once the runner serves it (not for --nested,
    // which keeps everything off the screen).
    if nested {
        eprintln!("farol dev: devtools com tessela devtools --project farol-dev ui");
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let devtools = runtime.join("tessela/farol-dev/devtools.sock");
    std::thread::spawn(move || {
        for _ in 0..100 {
            if nested {
                return;
            }
            if devtools.exists() {
                let _ = Command::new(&tessela).args(["devtools", "--project", "farol-dev", "ui"]).spawn();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    match child.wait() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

#[cfg(unix)]
fn exec(mut cmd: Command) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let e = cmd.exec();
    eprintln!("farol: {:?}: {e}", cmd.get_program());
    ExitCode::FAILURE
}

#[cfg(not(unix))]
fn exec(mut cmd: Command) -> ExitCode {
    match cmd.status() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

// ── at login ────────────────────────────────────────────────────────────

/// Whether farol starts at login.
pub fn autostart_enabled() -> bool {
    platform_autostart::enabled()
}

/// Start (or stop starting) at login, with `farol start`.
pub fn set_autostart(on: bool) -> Result<(), String> {
    platform_autostart::set(on, &exe())
}

#[cfg(target_os = "linux")]
mod platform_autostart {
    //! A systemd user unit bound to the graphical session when systemd
    //! runs the session; else an XDG autostart entry.
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn config() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| crate::platform::home().join(".config"))
    }

    fn unit() -> PathBuf {
        config().join("systemd/user/farol.service")
    }

    fn desktop() -> PathBuf {
        config().join("autostart/farol.desktop")
    }

    fn systemctl(args: &[&str]) -> bool {
        Command::new("systemctl").arg("--user").args(args).status().is_ok_and(|s| s.success())
    }

    fn systemd_session() -> bool {
        systemctl(&["--quiet", "is-active", "graphical-session.target"])
    }

    pub fn enabled() -> bool {
        (unit().exists() && systemctl(&["--quiet", "is-enabled", "farol.service"])) || desktop().exists()
    }

    pub fn set(on: bool, exe: &Path) -> Result<(), String> {
        let err = |e: std::io::Error| e.to_string();
        if !on {
            if unit().exists() {
                systemctl(&["disable", "farol.service"]);
                std::fs::remove_file(unit()).map_err(err)?;
                systemctl(&["daemon-reload"]);
            }
            if desktop().exists() {
                std::fs::remove_file(desktop()).map_err(err)?;
            }
            return Ok(());
        }
        if systemd_session() {
            let text = format!(
                "[Unit]\nDescription=farol launcher\nPartOf=graphical-session.target\nAfter=graphical-session.target\n\n\
                 [Service]\nExecStart={} start\nRestart=on-failure\n\n[Install]\nWantedBy=graphical-session.target\n",
                exe.display()
            );
            std::fs::create_dir_all(unit().parent().unwrap_or(Path::new("."))).map_err(err)?;
            std::fs::write(unit(), text).map_err(err)?;
            systemctl(&["daemon-reload"]);
            if !systemctl(&["enable", "farol.service"]) {
                return Err("systemctl --user enable farol.service falhou".into());
            }
        } else {
            let text = format!(
                "[Desktop Entry]\nType=Application\nName=farol\nComment=launcher\nExec=\"{}\" start\nX-GNOME-Autostart-enabled=true\n",
                exe.display()
            );
            std::fs::create_dir_all(desktop().parent().unwrap_or(Path::new("."))).map_err(err)?;
            std::fs::write(desktop(), text).map_err(err)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform_autostart {
    //! A LaunchAgent that runs `farol start` at login.
    use std::path::{Path, PathBuf};

    fn plist() -> PathBuf {
        crate::platform::home().join("Library/LaunchAgents/io.github.fuleco.farol.plist")
    }

    pub fn enabled() -> bool {
        plist().exists()
    }

    pub fn set(on: bool, exe: &Path) -> Result<(), String> {
        if !on {
            return if plist().exists() { std::fs::remove_file(plist()).map_err(|e| e.to_string()) } else { Ok(()) };
        }
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\n<key>Label</key><string>io.github.fuleco.farol</string>\n\
             <key>ProgramArguments</key><array><string>{}</string><string>start</string></array>\n\
             <key>RunAtLoad</key><true/>\n</dict></plist>\n",
            exe.display()
        );
        std::fs::create_dir_all(plist().parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
        std::fs::write(plist(), text).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
mod platform_autostart {
    //! A small script in the user's Startup folder.
    use std::path::{Path, PathBuf};

    fn script() -> PathBuf {
        crate::platform::data_dir().join("Microsoft/Windows/Start Menu/Programs/Startup/farol.cmd")
    }

    pub fn enabled() -> bool {
        script().exists()
    }

    pub fn set(on: bool, exe: &Path) -> Result<(), String> {
        if !on {
            return if script().exists() { std::fs::remove_file(script()).map_err(|e| e.to_string()) } else { Ok(()) };
        }
        std::fs::write(script(), format!("@start \"\" \"{}\" start\r\n", exe.display())).map_err(|e| e.to_string())
    }
}

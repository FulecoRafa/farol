//! farol: the launcher's core, and its command line.
//!
//!   farol start               run farol (what login runs; nothing if running)
//!   farol dev [--nested]      a development copy, `farol-dev`, with devtools
//!   farol doctor              what farol finds here, and how fast it searches
//!   farol autostart on|off    start at login (systemd, LaunchAgent, Startup)
//!   farol toggle [text]       open or close the launcher, optionally with a text
//!   farol show [text] | hide | settings (the settings window)
//!   farol alias NAME TARGET   alias -d NAME   aliases
//!   farol link NAME URL       link -d NAME    links
//!   farol hotkey KEYS [TEXT]  hotkey -d KEYS  hotkeys   (applied when farol starts)
//!   farol run ID [ACTION]     run a row (what a shortcut to a row calls)
//!   farol set KEY JSON        get KEY         unset KEY   options
//!   farol query TEXT          answer a search here (debugging)
//!   farol serve               the service itself (started by `tessela run`)
//!
//! The service is one socket, `farol.sock`, in the project's services dir.

mod client;
mod db;
mod engine;
mod item;
mod launch;
mod matching;
mod platform;
mod providers;
mod service;
mod settings;

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: farol start | dev [--nested] | doctor | autostart on|off\n       \
         farol toggle|show [text] | hide | settings\n       \
         farol alias NAME TARGET | alias -d NAME | aliases\n       \
         farol link NAME URL | link -d NAME | links\n       \
         farol hotkey KEYS [TEXT] | hotkey -d KEYS | hotkeys   farol run ID [ACTION]\n       \
         farol set KEY JSON | get KEY | options   farol query TEXT   farol serve"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        // No arguments: the service, as `tessela run` starts it.
        [] | ["serve"] => return service::serve(),
        ["start"] => return launch::start(),
        ["dev"] => return launch::dev(false),
        ["dev", "--nested"] => return launch::dev(true),
        ["doctor"] => doctor(),
        ["autostart", on @ ("on" | "off")] => launch::set_autostart(*on == "on").map(|_| String::new()),
        ["autostart"] => Ok(if launch::autostart_enabled() { "on" } else { "off" }.into()),
        ["toggle" | "show", rest @ ..] => client::call(words[0], serde_json::json!({ "query": rest.join(" ") })),
        ["hide" | "settings"] => client::call(words[0], serde_json::Value::Null),
        ["run", id, action @ ..] => client::call("run", serde_json::json!({ "id": id, "action": action.first().copied().unwrap_or("") })),
        ["query", rest @ ..] => query(&rest.join(" ")),
        _ => match offline(&words) {
            Some(r) => r,
            None => return usage(),
        },
    };
    match result {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("farol: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Answer a search here, without a running service: rows and timing.
fn query(text: &str) -> Result<String, String> {
    let engine = service::build(true)?.engine;
    engine.opened();
    let t = std::time::Instant::now();
    let sections = engine.query(text);
    let took = t.elapsed();
    let mut out = String::new();
    for s in sections {
        out += &format!("── {}\n", s.title);
        for i in s.items {
            out += &format!("  {}  {}  {}\n", i.title, i.subtitle, i.hint);
        }
    }
    out += &format!("({:.2} ms)", took.as_secs_f64() * 1000.0);
    Ok(out)
}

/// Commands that only touch the database (the service needs not run).
fn offline(words: &[&str]) -> Option<Result<String, String>> {
    let db = match db::Store::open() {
        Ok(db) => db,
        Err(e) => return Some(Err(format!("{}: {e}", db::path().display()))),
    };
    let err = |e: rusqlite::Error| e.to_string();
    let lines = |rows: Vec<(String, String)>, sep: &str| {
        rows.into_iter().map(|(a, b)| format!("{a}{sep}{b}")).collect::<Vec<_>>().join("\n")
    };
    Some(match words {
        ["alias", "-d", name] => db.remove_alias(name).map_err(err).and_then(|found| {
            if found { Ok(String::new()) } else { Err(format!("no alias {name}")) }
        }),
        ["alias", name, target @ ..] if !target.is_empty() => {
            db.set_alias(name, &target.join(" ")).map(|_| String::new()).map_err(err)
        }
        ["aliases"] => Ok(lines(db.aliases(), " = ")),
        ["link", "-d", name] => db.remove_quicklink(name).map(|_| String::new()).map_err(err),
        ["link", name, url] => db.set_quicklink(name, url).map(|_| String::new()).map_err(err),
        ["links"] => Ok(lines(db.quicklinks(), " ")),
        ["hotkey", "-d", keys] => db.remove_hotkey(keys).map(|_| String::new()).map_err(err),
        ["hotkey", keys, query @ ..] => db.set_hotkey(keys, &query.join(" ")).map(|_| String::new()).map_err(err),
        ["hotkeys"] => Ok(lines(db.hotkeys(), " → ")),
        ["set", key, value @ ..] if !value.is_empty() => {
            let text = value.join(" ");
            // Not JSON: take it as a string.
            let v = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
            db.set(key, &v).map(|_| String::new()).map_err(err)
        }
        ["unset", key] => db.unset(key).map(|_| String::new()).map_err(err),
        ["get", key] => Ok(db.setting(key).map(|v| v.to_string()).unwrap_or_default()),
        ["options"] => Ok(db
            .settings()
            .into_iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join("\n")),
        _ => return None,
    })
}

/// `farol doctor`: where things are, what works on this desktop, and how
/// long each provider takes (from the running farol; a cold local one
/// otherwise).
fn doctor() -> Result<String, String> {
    use serde_json::Value;
    let (s, live) = match client::call("status", Value::Null) {
        Ok(text) => (serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?, true),
        Err(_) => {
            let core = service::build(true)?;
            core.engine.opened();
            for text in ["fir", "2+2", ":heart", "/plano", "zap"] {
                core.engine.query(text);
            }
            (settings::status(&db::Store::open().map_err(|e| e.to_string())?, &core.engine), false)
        }
    };
    let yes = |v: &Value| if v.as_bool() == Some(true) { "sim" } else { "não" };
    let str = |v: &Value| v.as_str().unwrap_or("—").to_string();
    let d = &s["database"];
    let mut out = format!(
        "farol {} ({})\n  rodando        {}\n  projeto        {}\n  tessela        {}\n  socket         {}\n  banco          {} ({} KB)\n                 {} pseudônimos · {} atalhos · {} links · {} no clipboard · {} timers\n  atalhos        {}\n  janelas        {}\n  no login       {}\n",
        str(&s["version"]),
        str(&s["os"]),
        if live { format!("sim, {} MB", s["memory_kb"].as_u64().unwrap_or(0) / 1024) } else { "não (medido agora, a frio)".into() },
        str(&s["project"]),
        str(&s["tessela"]),
        str(&s["socket"]),
        str(&d["path"]),
        d["bytes"].as_u64().unwrap_or(0) / 1024,
        d["aliases"], d["hotkeys"], d["links"], d["clipboard"], d["timers"],
        yes(&s["hotkeys"]),
        yes(&s["windows"]),
        yes(&s["autostart"]),
    );
    out += "  busca por provider (ms)   última   máx   média   vezes\n";
    for t in s["timings"].as_array().into_iter().flatten() {
        out += &format!(
            "    {:<22} {:>7.2} {:>6.2} {:>7.2} {:>7}\n",
            str(&t["provider"]),
            t["last"].as_f64().unwrap_or(0.0),
            t["max"].as_f64().unwrap_or(0.0),
            t["avg"].as_f64().unwrap_or(0.0),
            t["count"]
        );
    }
    Ok(out.trim_end().to_string())
}

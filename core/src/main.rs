//! farol: the launcher's core, and its command line.
//!
//!   farol                     serve (started by `tessela run`, see tessela.toml)
//!   farol toggle [text]       open or close the launcher, optionally with a text
//!   farol show [text] | hide
//!   farol alias NAME TARGET   alias -d NAME   aliases
//!   farol link NAME URL       link -d NAME    links
//!   farol set KEY JSON        get KEY         settings
//!   farol query TEXT          answer a search here (debugging)
//!
//! The service is one socket, `farol.sock`, in the project's services dir.

mod client;
mod db;
mod engine;
mod item;
mod matching;
mod platform;
mod providers;
mod service;

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: farol [serve]\n       farol toggle|show [text]   farol hide\n       \
         farol alias NAME TARGET | alias -d NAME | aliases\n       \
         farol link NAME URL | link -d NAME | links\n       farol set KEY JSON | get KEY | settings"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        [] | ["serve"] => return service::serve(),
        ["toggle" | "show", rest @ ..] => client::call(words[0], serde_json::json!({ "query": rest.join(" ") })),
        ["hide"] => client::call("hide", serde_json::Value::Null),
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
    let (_, engine) = service::build()?;
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
        ["set", key, value @ ..] if !value.is_empty() => {
            let text = value.join(" ");
            // Not JSON: take it as a string.
            let v = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
            db.set(key, &v).map(|_| String::new()).map_err(err)
        }
        ["get", key] => Ok(db.setting(key).map(|v| v.to_string()).unwrap_or_default()),
        ["settings"] => Ok(db
            .settings()
            .into_iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join("\n")),
        _ => return None,
    })
}

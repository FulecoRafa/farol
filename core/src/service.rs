//! The `farol` service: what the launcher surface subscribes to and calls.
//!
//! topics   ui       {open, query, nonce}   open/close requests (hotkeys, CLI)
//!          results  {seq, text, sections}  the answer to the last `query`
//!          preview  {id, ...}              the selected row's preview
//! methods  query {text} · activate {id, action} · preview {id}
//!          toggle {query} · show {query} · hide

use std::process::ExitCode;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tessela_service::{Host, Service, Topic};

use crate::db::Store;
use crate::engine::{Engine, Providers};
use crate::item::{Preview, Section};
use crate::providers::{apps::Apps, calc::Calc, emoji::Emoji, farol::Farol, files::Files};

#[derive(Clone, Default, Serialize)]
pub struct Ui {
    pub open: bool,
    /// Text the field starts with when opening.
    pub query: String,
    /// Bumped on every request, so reopening with the same text still lands.
    pub nonce: u64,
}

#[derive(Clone, Default, Serialize)]
struct Results {
    /// Bumped per answer: the launcher resets its selection on a new one.
    seq: u64,
    /// The text answered; the field shows it (the service owns the text).
    text: String,
    sections: Vec<Section>,
}

#[derive(Clone, Default, Serialize)]
struct Shown {
    id: String,
    #[serde(flatten)]
    preview: Preview,
}

#[derive(Deserialize)]
struct Text {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize, Default)]
struct Open {
    #[serde(default)]
    query: String,
}

#[derive(Deserialize)]
struct Activate {
    id: String,
    #[serde(default)]
    action: String,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

fn set_open(ui: &Topic<Ui>, open: bool, query: String) {
    ui.update(|u| {
        u.open = open;
        u.query = query;
        u.nonce += 1;
    });
}

/// The service and its engine. `inline`: no executor around (the `query`
/// command), so background searches run in place.
pub fn build(inline: bool) -> Result<(Service, Rc<Engine>), String> {
    let db = Store::open().map_err(|e| format!("{}: {e}", crate::db::path().display()))?;
    let farol = Rc::new(Farol::new(db.clone()));
    let providers: Providers = vec![
        Rc::new(Calc::new()),
        farol.clone(),
        Rc::new(Apps::new(db.clone())),
        Rc::new(Files::new(db.clone(), inline)),
        Rc::new(Emoji::new(db.clone())),
    ];
    let engine = Rc::new(Engine::new(db, providers));
    farol.attach(&engine);

    let mut svc = Service::new("farol");
    let ui = svc.topic("ui", Ui::default());
    let results = svc.topic("results", Results::default());
    let preview = svc.topic("preview", Shown::default());

    // Answer a text: results, and the first row's preview (the selection
    // starts there).
    let answer = {
        let (e, r, p) = (engine.clone(), results.clone(), preview.clone());
        Rc::new(move |text: String, fresh: bool| {
            let sections = e.query(&text);
            let first = sections.first().and_then(|s| s.items.first()).map(|i| i.id.clone());
            let seq = r.get().seq + u64::from(fresh);
            r.publish(Results { seq, text, sections });
            // A late answer fills the preview only if nothing had one yet.
            if fresh || p.get().id.is_empty() {
                p.publish(match first {
                    Some(id) => Shown { preview: e.preview(&id), id },
                    None => Shown::default(),
                });
            }
        })
    };
    // Late rows (a file search) refresh the same answer: same seq, so the
    // selection stays where the user put it.
    let (a, r) = (answer.clone(), results.clone());
    crate::engine::on_refresh(Rc::new(move || a(r.get().text, false)));

    let a = answer.clone();
    svc.method_sync("query", move |t: Text| {
        a(t.text, true);
        Ok(())
    });

    let (e, u) = (engine.clone(), ui.clone());
    svc.method_sync("activate", move |a: Activate| {
        let out = e.activate(&a.id, &a.action);
        if out.close {
            set_open(&u, false, String::new());
        }
        Ok(out)
    });

    let (e, p) = (engine.clone(), preview.clone());
    svc.method_sync("preview", move |a: Id| {
        p.publish(Shown { preview: e.preview(&a.id), id: a.id });
        Ok(())
    });

    let show = {
        let (e, u, a) = (engine.clone(), ui.clone(), answer.clone());
        Rc::new(move |query: String| {
            e.opened();
            a(query.clone(), true);
            set_open(&u, true, query);
        })
    };

    let (u, s) = (ui.clone(), show.clone());
    svc.method_sync("toggle", move |a: Option<Open>| {
        let a = a.unwrap_or_default();
        let current = u.get();
        // Toggling with another text while open switches to that text.
        let open = !current.open || (!a.query.is_empty() && a.query != current.query);
        if open {
            s(a.query);
        } else {
            set_open(&u, false, String::new());
        }
        Ok(json!({ "open": open }))
    });

    let s = show.clone();
    svc.method_sync("show", move |a: Option<Open>| {
        s(a.unwrap_or_default().query);
        Ok(())
    });

    let u = ui.clone();
    svc.method_sync("hide", move |_: Value| {
        set_open(&u, false, String::new());
        Ok(())
    });

    Ok((svc, engine))
}

pub fn serve() -> ExitCode {
    let (svc, _engine) = match build(false) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("farol: {e}");
            return ExitCode::FAILURE;
        }
    };
    match Host::new().add(svc).run(async {}) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("farol: {e}");
            ExitCode::FAILURE
        }
    }
}

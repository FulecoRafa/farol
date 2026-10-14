//! The `farol` service: what the launcher surface subscribes to and calls.
//!
//! topics   ui       {open, query, nonce}   open/close requests (hotkeys, CLI)
//!          results  {text, sections}       the answer to the last `query`
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
use crate::providers::{apps::Apps, calc::Calc, emoji::Emoji, farol::Farol};

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

pub fn build() -> Result<(Service, Rc<Engine>), String> {
    let db = Store::open().map_err(|e| format!("{}: {e}", crate::db::path().display()))?;
    let farol = Rc::new(Farol::new(db.clone()));
    let providers: Providers = vec![
        Rc::new(Calc::new()),
        farol.clone(),
        Rc::new(Apps::new(db.clone())),
        Rc::new(Emoji::new(db.clone())),
    ];
    let engine = Rc::new(Engine::new(db, providers));
    farol.attach(&engine);

    let mut svc = Service::new("farol");
    let ui = svc.topic("ui", Ui::default());
    let results = svc.topic("results", Results::default());
    let preview = svc.topic("preview", Shown::default());

    let (e, r) = (engine.clone(), results.clone());
    svc.method_sync("query", move |a: Text| {
        let sections = e.query(&a.text);
        r.publish(Results { text: a.text, sections });
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

    let (e, u) = (engine.clone(), ui.clone());
    svc.method_sync("toggle", move |a: Option<Open>| {
        let a = a.unwrap_or_default();
        let current = u.get();
        // Toggling with another text while open switches to that text.
        let open = !current.open || (!a.query.is_empty() && a.query != current.query);
        if open {
            e.opened();
        }
        set_open(&u, open, a.query);
        Ok(json!({ "open": open }))
    });

    let (e, u) = (engine.clone(), ui.clone());
    svc.method_sync("show", move |a: Option<Open>| {
        e.opened();
        set_open(&u, true, a.unwrap_or_default().query);
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
    let (svc, _engine) = match build() {
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

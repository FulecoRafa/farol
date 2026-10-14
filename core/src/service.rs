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
use crate::platform;
use crate::providers::{
    apps::Apps, calc::Calc, clipboard::Clipboard, emoji::Emoji, farol::Farol, files::Files, windows::Windows,
};

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

/// The service, its engine, and what the clipboard watcher feeds.
pub struct Core {
    pub service: Service,
    pub engine: Rc<Engine>,
    pub clipboard: Rc<Clipboard>,
    /// Answer the current text again if the clipboard rows are on screen.
    pub clipboard_changed: Rc<dyn Fn()>,
}

/// `inline`: no executor around (the `query` command), so background
/// searches run in place.
pub fn build(inline: bool) -> Result<Core, String> {
    let db = Store::open().map_err(|e| format!("{}: {e}", crate::db::path().display()))?;
    let farol = Rc::new(Farol::new(db.clone()));
    let clipboard = Rc::new(Clipboard::new(db.clone()));
    let apps = Rc::new(Apps::new(db.clone()));
    let (switcher, closer) = Windows::pair(apps.clone());
    let providers: Providers = vec![
        Rc::new(Calc::new()),
        farol.clone(),
        apps,
        Rc::new(switcher),
        Rc::new(closer),
        Rc::new(Files::new(db.clone(), inline)),
        Rc::new(Emoji::new(db.clone())),
        clipboard.clone(),
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

    let (e, u, r, a) = (engine.clone(), ui.clone(), results.clone(), answer.clone());
    svc.method_sync("activate", move |act: Activate| {
        let out = e.activate(&act.id, &act.action);
        if out.close {
            set_open(&u, false, String::new());
        } else if out.refresh {
            a(r.get().text, false);
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

    let (u, r, a) = (ui.clone(), results.clone(), answer.clone());
    let clipboard_changed: Rc<dyn Fn()> = Rc::new(move || {
        let shown = r.get();
        if u.get().open && shown.sections.iter().any(|s| s.provider == "clipboard") {
            a(shown.text, false);
        }
    });

    Ok(Core { service: svc, engine, clipboard, clipboard_changed })
}

pub fn serve() -> ExitCode {
    let core = match build(false) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("farol: {e}");
            return ExitCode::FAILURE;
        }
    };
    // The clipboard watcher (a thread) hands selections over; they are
    // stored here, on the thread that owns the database.
    let (tx, rx) = async_channel::unbounded();
    platform::watch_clipboard(core.clipboard.primary(), tx);
    let (clipboard, changed) = (core.clipboard.clone(), core.clipboard_changed.clone());
    let record = async move {
        while let Ok(clip) = rx.recv().await {
            clipboard.record(clip);
            changed();
        }
    };
    match Host::new().add(core.service).run(record) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("farol: {e}");
            ExitCode::FAILURE
        }
    }
}

//! The Wayland clipboard, natively: one thread speaking data-control
//! (`ext-data-control-v1`, else wlroots' `zwlr_data_control_manager_v1`).
//!
//! It sleeps in `poll` on the display socket and a wake pipe, so it costs
//! nothing until something is copied. Each new selection is read once (the
//! best text type, else an image) and sent to the service thread, which
//! owns the database. Copying from farol makes this thread the selection's
//! source: it serves the bytes until another client replaces them, so no
//! `wl-copy` process is left behind.
//!
//! Skipped: offers marked by password managers (`x-kde-passwordManagerHint`)
//! and farol's own selections (they carry [`MARKER`]). Without a
//! data-control global (GNOME, X11) it logs once and the thread ends.

use std::collections::HashMap;
use std::fs::File;
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, event_created_child};

use super::Clip;

/// Offered with every selection farol sets, so the watcher knows its own.
const MARKER: &str = "application/x-farol";
/// Password managers mark secrets with it (KeePassXC, KDE).
const SECRET: &str = "x-kde-passwordManagerHint";
/// Text types, best first.
const TEXT: &[&str] = &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "STRING", "TEXT"];
pub const MAX_TEXT: usize = 1 << 20;
pub const MAX_IMAGE: usize = 10 << 20;
/// A source that does not answer in time is skipped.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// A request from another thread: become the selection with these bytes.
struct Set {
    mimes: Vec<String>,
    data: Arc<[u8]>,
}

/// How other threads reach the watcher: a queue, and a pipe to wake it.
struct Handle {
    tx: mpsc::Sender<Set>,
    wake: PipeWriter,
}

static HANDLE: OnceLock<Handle> = OnceLock::new();

/// Become the clipboard selection with `data` under `mimes`. False when no
/// watcher runs (no compositor support): the caller falls back.
pub fn set(mimes: &[&str], data: Vec<u8>) -> bool {
    let Some(h) = HANDLE.get() else {
        return false;
    };
    let set = Set { mimes: mimes.iter().map(|m| m.to_string()).collect(), data: data.into() };
    h.tx.send(set).is_ok() && (&h.wake).write_all(&[1]).is_ok()
}

/// Start the watcher thread. New selections go to `tx`; `primary` also
/// records the primary selection (middle-click paste).
pub fn watch(primary: bool, tx: async_channel::Sender<Clip>) {
    let spawned = std::thread::Builder::new().name("clipboard".into()).spawn(move || {
        if let Err(e) = run(primary, tx) {
            eprintln!("farol: clipboard: {e}");
        }
    });
    if let Err(e) = spawned {
        eprintln!("farol: clipboard: {e}");
    }
}

// ── the two protocols, behind one shape ─────────────────────────────────

mod ext {
    pub use wayland_protocols::ext::data_control::v1::client::{
        ext_data_control_device_v1 as device, ext_data_control_manager_v1 as manager,
        ext_data_control_offer_v1 as offer, ext_data_control_source_v1 as source,
    };
    pub type Manager = manager::ExtDataControlManagerV1;
    pub type Device = device::ExtDataControlDeviceV1;
    pub type Source = source::ExtDataControlSourceV1;
    pub type Offer = offer::ExtDataControlOfferV1;
}

mod wlr {
    pub use wayland_protocols_wlr::data_control::v1::client::{
        zwlr_data_control_device_v1 as device, zwlr_data_control_manager_v1 as manager,
        zwlr_data_control_offer_v1 as offer, zwlr_data_control_source_v1 as source,
    };
    pub type Manager = manager::ZwlrDataControlManagerV1;
    pub type Device = device::ZwlrDataControlDeviceV1;
    pub type Source = source::ZwlrDataControlSourceV1;
    pub type Offer = offer::ZwlrDataControlOfferV1;
}

/// An object of either protocol; both have the same requests.
enum Either<E, W> {
    Ext(E),
    Wlr(W),
}

type Manager = Either<ext::Manager, wlr::Manager>;
type Device = Either<ext::Device, wlr::Device>;
type Source = Either<ext::Source, wlr::Source>;
type Offer = Either<ext::Offer, wlr::Offer>;

macro_rules! each {
    ($e:expr, $x:ident => $body:expr) => {
        match $e {
            Either::Ext($x) => $body,
            Either::Wlr($x) => $body,
        }
    };
}

impl Offer {
    fn id(&self) -> ObjectId {
        each!(self, o => o.id())
    }
    fn receive(&self, mime: &str, fd: std::os::fd::BorrowedFd) {
        each!(self, o => o.receive(mime.into(), fd))
    }
    fn destroy(&self) {
        each!(self, o => o.destroy())
    }
}

impl Source {
    fn id(&self) -> ObjectId {
        each!(self, s => s.id())
    }
    fn destroy(&self) {
        each!(self, s => s.destroy())
    }
}

impl Manager {
    fn source(&self, qh: &QueueHandle<State>, mimes: &[String]) -> Source {
        let s = match self {
            Either::Ext(m) => Either::Ext(m.create_data_source(qh, ())),
            Either::Wlr(m) => Either::Wlr(m.create_data_source(qh, ())),
        };
        for mime in mimes.iter().map(String::as_str).chain([MARKER]) {
            each!(&s, s => s.offer(mime.into()));
        }
        s
    }
    fn device(&self, seat: &WlSeat, qh: &QueueHandle<State>) -> Device {
        match self {
            Either::Ext(m) => Either::Ext(m.get_data_device(seat, qh, ())),
            Either::Wlr(m) => Either::Wlr(m.get_data_device(seat, qh, ())),
        }
    }
}

impl Device {
    fn set_selection(&self, source: &Source) {
        match (self, source) {
            (Either::Ext(d), Either::Ext(s)) => d.set_selection(Some(s)),
            (Either::Wlr(d), Either::Wlr(s)) => d.set_selection(Some(s)),
            _ => {}
        }
    }
}

// ── state ───────────────────────────────────────────────────────────────

struct State {
    primary: bool,
    tx: async_channel::Sender<Clip>,
    /// Types announced by each live offer.
    mimes: HashMap<ObjectId, Vec<String>>,
    /// A new selection to read once the events are dispatched.
    pending: Option<Offer>,
    /// farol's live sources and what they serve.
    sources: HashMap<ObjectId, (Source, Arc<[u8]>, Vec<String>)>,
    /// The seat is gone: stop.
    finished: bool,
}

impl State {
    /// A selection changed (`None`: cleared).
    fn selection(&mut self, offer: Option<Offer>, primary: bool) {
        let Some(offer) = offer else { return };
        if primary && !self.primary {
            self.drop_offer(offer);
            return;
        }
        if let Some(old) = self.pending.replace(offer) {
            self.drop_offer(old);
        }
    }

    fn drop_offer(&mut self, offer: Offer) {
        self.mimes.remove(&offer.id());
        offer.destroy();
    }

    /// Read the pending selection, if worth keeping, and hand it over.
    fn read_pending(&mut self, conn: &Connection) {
        let Some(offer) = self.pending.take() else { return };
        let mimes = self.mimes.get(&offer.id()).cloned().unwrap_or_default();
        if let Some((mime, max)) = pick(&mimes) {
            match receive(conn, &offer, mime, max) {
                Ok(data) if !data.is_empty() => {
                    let mime = if max == MAX_TEXT { "text/plain" } else { mime };
                    let _ = self.tx.try_send(Clip { mime: mime.into(), data });
                }
                Ok(_) => {}
                Err(e) => eprintln!("farol: clipboard: {mime}: {e}"),
            }
        }
        self.drop_offer(offer);
    }

    fn set(&mut self, manager: &Manager, device: &Device, qh: &QueueHandle<State>, set: Set) {
        let source = manager.source(qh, &set.mimes);
        device.set_selection(&source);
        self.sources.insert(source.id(), (source, set.data, set.mimes));
    }

    /// A client pastes from farol's selection: write it on a short thread
    /// (a big image would stall the loop on a slow reader).
    fn send(&self, source: ObjectId, mime: &str, fd: OwnedFd) {
        if let Some((_, data, mimes)) = self.sources.get(&source) {
            if mimes.iter().any(|m| m == mime) {
                let data = data.clone();
                std::thread::spawn(move || {
                    let _ = File::from(fd).write_all(&data);
                });
            }
        }
    }

    fn cancelled(&mut self, source: ObjectId) {
        if let Some((s, ..)) = self.sources.remove(&source) {
            s.destroy();
        }
    }
}

/// The type to read and its size cap: text first, else PNG, else any
/// image. Nothing for secrets, farol's own selections or other types.
fn pick(mimes: &[String]) -> Option<(&str, usize)> {
    if mimes.iter().any(|m| m == SECRET || m == MARKER) {
        return None;
    }
    let has = |t: &str| mimes.iter().find(|m| *m == t).map(String::as_str);
    if let Some(t) = TEXT.iter().find_map(|t| has(t)) {
        return Some((t, MAX_TEXT));
    }
    has("image/png")
        .or_else(|| mimes.iter().find(|m| m.starts_with("image/")).map(String::as_str))
        .map(|t| (t, MAX_IMAGE))
}

/// Ask the source for `mime` and read it whole, within the timeout and
/// the cap (too big: an error, the entry is skipped).
fn receive(conn: &Connection, offer: &Offer, mime: &str, max: usize) -> std::io::Result<Vec<u8>> {
    let (mut r, w) = std::io::pipe()?;
    offer.receive(mime, w.as_fd());
    drop(w);
    conn.flush().map_err(std::io::Error::other)?;
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut out = Vec::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !readable(&r, left)? {
            return Err(std::io::Error::other("timed out"));
        }
        match r.read(&mut buf) {
            Ok(0) => return Ok(out),
            Ok(n) if out.len() + n > max => return Err(std::io::Error::other("too big")),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

fn readable(r: &PipeReader, timeout: Duration) -> std::io::Result<bool> {
    let mut fds = [libc::pollfd { fd: r.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
    Ok(poll(&mut fds, timeout.as_millis() as i32)? > 0)
}

fn poll(fds: &mut [libc::pollfd], timeout: i32) -> std::io::Result<i32> {
    loop {
        // SAFETY: `fds` is a valid, exclusively borrowed array.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
        if n >= 0 {
            return Ok(n);
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

// ── the loop ────────────────────────────────────────────────────────────

fn run(primary: bool, tx: async_channel::Sender<Clip>) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("no wayland display ({e}); history off"))?;
    let (globals, mut queue): (_, EventQueue<State>) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let manager = match globals.bind::<ext::Manager, _, _>(&qh, 1..=1, ()) {
        Ok(m) => Either::Ext(m),
        Err(_) => match globals.bind::<wlr::Manager, _, _>(&qh, 1..=2, ()) {
            Ok(m) => Either::Wlr(m),
            Err(_) => return Err("the compositor has no data-control protocol; history off".into()),
        },
    };
    let seat: WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("no seat: {e}"))?;
    let device = manager.device(&seat, &qh);
    let name = if matches!(manager, Either::Ext(_)) { "ext-data-control-v1" } else { "wlr-data-control" };
    eprintln!("farol: clipboard: watching ({name})");

    let (wake_r, wake_w) = std::io::pipe().map_err(|e| e.to_string())?;
    let (set_tx, set_rx) = mpsc::channel();
    let _ = HANDLE.set(Handle { tx: set_tx, wake: wake_w });

    let mut state = State {
        primary,
        tx,
        mimes: HashMap::new(),
        pending: None,
        sources: HashMap::new(),
        finished: false,
    };
    let mut wake_r = wake_r;
    loop {
        queue.dispatch_pending(&mut state).map_err(|e| e.to_string())?;
        state.read_pending(&conn);
        if state.finished {
            return Err("the seat went away; history off".into());
        }
        conn.flush().map_err(|e| e.to_string())?;
        let Some(guard) = queue.prepare_read() else { continue };
        let mut fds = [
            libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: wake_r.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        poll(&mut fds, -1).map_err(|e| e.to_string())?;
        if fds[0].revents != 0 {
            if let Err(e) = guard.read() {
                if !matches!(&e, wayland_client::backend::WaylandError::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock) {
                    return Err(format!("display: {e}"));
                }
            }
        } else {
            drop(guard);
        }
        if fds[1].revents != 0 {
            let mut buf = [0u8; 64];
            let _ = wake_r.read(&mut buf);
            while let Ok(set) = set_rx.try_recv() {
                state.set(&manager, &device, &qh, set);
            }
        }
    }
}

// ── dispatch ────────────────────────────────────────────────────────────

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlSeat, ()> for State {
    fn event(_: &mut Self, _: &WlSeat, _: <WlSeat as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// The same handlers for both protocols.
macro_rules! dispatch {
    ($p:ident, $tag:ident) => {
        impl Dispatch<$p::Manager, ()> for State {
            fn event(_: &mut Self, _: &$p::Manager, _: $p::manager::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }

        impl Dispatch<$p::Device, ()> for State {
            fn event(state: &mut Self, _: &$p::Device, event: $p::device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
                use $p::device::Event;
                match event {
                    Event::DataOffer { id } => {
                        state.mimes.insert(id.id(), Vec::new());
                    }
                    Event::Selection { id } => state.selection(id.map(Either::$tag), false),
                    Event::PrimarySelection { id } => state.selection(id.map(Either::$tag), true),
                    Event::Finished => state.finished = true,
                    _ => {}
                }
            }

            event_created_child!(State, $p::Device, [
                $p::device::EVT_DATA_OFFER_OPCODE => ($p::Offer, ()),
            ]);
        }

        impl Dispatch<$p::Offer, ()> for State {
            fn event(state: &mut Self, offer: &$p::Offer, event: $p::offer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
                if let $p::offer::Event::Offer { mime_type } = event {
                    state.mimes.entry(offer.id()).or_default().push(mime_type);
                }
            }
        }

        impl Dispatch<$p::Source, ()> for State {
            fn event(state: &mut Self, source: &$p::Source, event: $p::source::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
                match event {
                    $p::source::Event::Send { mime_type, fd } => state.send(source.id(), &mime_type, fd),
                    $p::source::Event::Cancelled => state.cancelled(source.id()),
                    _ => {}
                }
            }
        }
    };
}

dispatch!(ext, Ext);
dispatch!(wlr, Wlr);

#[cfg(test)]
mod tests {
    use super::*;

    fn types(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn picks_text_then_png_and_skips_secrets() {
        assert_eq!(pick(&types(&["text/plain", "UTF8_STRING"])), Some(("UTF8_STRING", MAX_TEXT)));
        assert_eq!(pick(&types(&["text/html", "image/jpeg", "image/png"])), Some(("image/png", MAX_IMAGE)));
        assert_eq!(pick(&types(&["image/webp"])), Some(("image/webp", MAX_IMAGE)));
        assert_eq!(pick(&types(&["text/plain", SECRET])), None);
        assert_eq!(pick(&types(&["text/plain", MARKER])), None);
        assert_eq!(pick(&types(&["text/html"])), None);
    }
}

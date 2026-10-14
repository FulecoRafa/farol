//! Timers and alarms, typed in the field:
//!   timer 10m chá · t 25 min foco · timer 1h30 · timer 90s · pomodoro
//!   alarme 7:30 acordar · alarme 19h · alarme 0715
//! They live in the database, so they survive restarts; one task sleeps
//! until the next one ends and notifies. Running ones show with the field
//! empty, and under `timers`, to pause or cancel.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures_lite::future;
use tessela_service::Timer;

use crate::db::{Db, now};
use crate::engine::{Provider, Query};
use crate::item::{Action, Item, Outcome};
use crate::platform;

#[derive(Clone, Debug, PartialEq)]
struct Running {
    id: i64,
    label: String,
    /// Total length, ms.
    duration: i64,
    /// When it ends (epoch ms); meaningless while paused.
    ends: i64,
    /// What was left when paused, ms.
    left: i64,
    paused: bool,
}

impl Running {
    fn remaining(&self, now: i64) -> i64 {
        if self.paused { self.left } else { (self.ends - now).max(0) }
    }
}

pub struct Timers {
    db: Db,
    pending: RefCell<Option<(String, i64)>>,
    /// Wakes the scheduler when timers change.
    changed: async_channel::Sender<()>,
    wake: async_channel::Receiver<()>,
}

/// `10m`, `25 min`, `1h30`, `1h30m`, `90s`, `2` (minutes) → ms.
fn parse_duration(text: &str) -> Option<(i64, &str)> {
    let text = text.trim_start();
    let mut total = 0f64;
    let mut rest = text;
    let mut any = false;
    loop {
        let digits = rest.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(rest.len());
        if digits == 0 {
            break;
        }
        let value: f64 = rest[..digits].replace(',', ".").parse().ok()?;
        let after = rest[digits..].trim_start();
        let unit_len = after.find(|c: char| !c.is_alphabetic()).unwrap_or(after.len());
        let unit = &after[..unit_len];
        let factor = match unit {
            "h" | "hr" | "hora" | "horas" => 3_600_000.0,
            "m" | "min" | "mins" | "minuto" | "minutos" | "" => 60_000.0,
            "s" | "seg" | "segs" | "segundo" | "segundos" => 1000.0,
            // "5 chá": a bare number is minutes; the word is the label.
            _ if !any => {
                total = value * 60_000.0;
                any = true;
                rest = after;
                break;
            }
            _ => break,
        };
        // "1h30": minutes after hours need no unit.
        let factor = if unit.is_empty() && any { 60_000.0 } else { factor };
        total += value * factor;
        any = true;
        rest = &after[unit_len..];
        if unit.is_empty() {
            break;
        }
    }
    (any && total >= 1000.0 && total <= 7.0 * 86_400_000.0).then(|| (total.round() as i64, rest.trim()))
}

/// `7:30`, `7h`, `7h30`, `19.30`, `0715` → (hour, minute).
fn parse_clock(word: &str) -> Option<(u32, u32)> {
    let w = word.trim().to_lowercase();
    let (h, m) = if let Some((h, m)) = w.split_once([':', 'h', '.']) {
        (h.parse().ok()?, if m.is_empty() { 0 } else { m.parse().ok()? })
    } else if w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()) {
        (w[..2].parse().ok()?, w[2..].parse().ok()?)
    } else {
        (w.parse().ok()?, 0)
    };
    (h < 24 && m < 60).then_some((h, m))
}

/// ms until the next `hh:mm` local time.
fn until_clock(h: u32, m: u32) -> i64 {
    let offset = platform::utc_offset_secs() * 1000;
    let now = now();
    let day = 86_400_000;
    let local = (now + offset).rem_euclid(day);
    let target = i64::from(h * 3_600_000 + m * 60_000);
    let wait = target - local;
    if wait <= 0 { wait + day } else { wait }
}

/// "4:05", "1:02:00".
fn clock(ms: i64) -> String {
    let s = (ms + 999) / 1000;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn span(ms: i64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s} s"),
        60..3600 if s % 60 == 0 => format!("{} min", s / 60),
        60..3600 => format!("{} min {} s", s / 60, s % 60),
        _ if s % 3600 == 0 => format!("{} h", s / 3600),
        _ => format!("{} h {} min", s / 3600, s / 60 % 60),
    }
}

impl Timers {
    pub fn new(db: Db) -> Self {
        let (changed, wake) = async_channel::unbounded();
        Self { db, pending: RefCell::new(None), changed, wake }
    }

    fn running(&self) -> Vec<Running> {
        self.db.rows("SELECT id, label, duration, ends, left, paused FROM timers ORDER BY ends", [], |r| {
            Ok(Running { id: r.get(0)?, label: r.get(1)?, duration: r.get(2)?, ends: r.get(3)?, left: r.get(4)?, paused: r.get(5)? })
        })
    }

    fn add(&self, label: &str, ms: i64) {
        let _ = self.db.conn().execute(
            "INSERT INTO timers (label, duration, ends, left) VALUES (?1, ?2, ?3, ?2)",
            rusqlite::params![label, ms, now() + ms],
        );
        let _ = self.changed.try_send(());
    }

    fn row(&self, t: &Running, now: i64) -> Item {
        let left = t.remaining(now);
        Item {
            id: format!("timers:{}", t.id),
            title: t.label.clone(),
            subtitle: if t.paused { format!("pausado · faltam {}", clock(left)) } else { format!("faltam {} de {}", clock(left), span(t.duration)) },
            icon: if t.paused { "ph:pause-circle".into() } else { "ph:timer".into() },
            keywords: "timer alarme".into(),
            actions: vec![
                Action::new("pause", if t.paused { "continuar" } else { "pausar" }, "enter"),
                Action::new("cancel", "cancelar", "ctrl+d"),
            ],
            ..Item::default()
        }
    }

    /// Sleep until the next timer ends (or the list changes); notify the
    /// ended ones and drop them. Nothing to wait for: sleep on the channel.
    pub async fn run(self: Rc<Self>) {
        loop {
            let now = now();
            let mut next: Option<i64> = None;
            for t in self.running() {
                if t.paused {
                    continue;
                }
                if t.ends <= now {
                    let late = now - t.ends;
                    let body = if late > 60_000 { format!("acabou há {}", span(late)) } else { format!("{} se passaram", span(t.duration)) };
                    let _ = platform::notify(&t.label, &body);
                    let _ = self.db.conn().execute("DELETE FROM timers WHERE id = ?1", [t.id]);
                } else {
                    next = Some(next.map_or(t.ends, |n: i64| n.min(t.ends)));
                }
            }
            let wake = async {
                let _ = self.wake.recv().await;
            };
            match next {
                Some(at) => {
                    let sleep = async {
                        Timer::at(Instant::now() + Duration::from_millis((at - now).max(0) as u64)).await;
                    };
                    future::or(wake, sleep).await;
                }
                None => wake.await,
            }
        }
    }
}

impl Provider for Timers {
    fn id(&self) -> &'static str {
        "timers"
    }
    fn title(&self) -> &'static str {
        "timers"
    }
    fn mixed(&self) -> usize {
        3
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn home(&self) -> Vec<Item> {
        let now = now();
        self.running().iter().map(|t| self.row(t, now)).collect()
    }
    fn items(&self) -> Option<Rc<Vec<Item>>> {
        Some(Rc::new(self.home()))
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        let text = q.text.trim();
        let lower = text.to_lowercase();
        let (word, rest) = lower.split_once(char::is_whitespace).unwrap_or((&lower, ""));
        let rest_original = text.get(word.len()..).unwrap_or("").trim();
        let (label, ms, title) = match word {
            "pomodoro" => ("pomodoro".to_string(), 25 * 60_000, "pomodoro de 25 min".to_string()),
            "timer" | "t" | "temporizador" => {
                let Some((ms, label)) = parse_duration(rest_original) else { return Vec::new() };
                let label = if label.is_empty() { "timer".to_string() } else { label.to_string() };
                let title = format!("timer de {}{}", span(ms), if label == "timer" { String::new() } else { format!(" · {label}") });
                (label, ms, title)
            }
            "alarme" | "alarm" | "despertar" => {
                let (when, label) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let Some((h, m)) = parse_clock(when) else { return Vec::new() };
                let label_original = rest_original.get(when.len()..).unwrap_or("").trim();
                let label = if label.is_empty() { format!("alarme {h:02}:{m:02}") } else { label_original.to_string() };
                (label.clone(), until_clock(h, m), format!("alarme às {h:02}:{m:02}{}", if label.starts_with("alarme ") { String::new() } else { format!(" · {label}") }))
            }
            _ => return Vec::new(),
        };
        *self.pending.borrow_mut() = Some((label, ms));
        vec![Item {
            id: "timers:new".into(),
            title,
            subtitle: format!("avisa em {}", span(ms)),
            icon: "ph:timer".into(),
            actions: vec![Action::new("start", "começar", "enter")],
            ..Item::default()
        }]
    }
    fn activate(&self, id: &str, action: &str) -> Outcome {
        let rest = id.strip_prefix("timers:").unwrap_or_default();
        if rest == "new" {
            let Some((label, ms)) = self.pending.borrow_mut().take() else { return Outcome::default() };
            self.add(&label, ms);
            return Outcome::close_with(format!("{label}: avisa em {}", span(ms)));
        }
        let Ok(id) = rest.parse::<i64>() else { return Outcome::default() };
        let Some(t) = self.running().into_iter().find(|t| t.id == id) else {
            return Outcome::refresh_with("o timer já acabou");
        };
        let now = now();
        let conn = self.db.conn();
        let toast = if action == "cancel" {
            let _ = conn.execute("DELETE FROM timers WHERE id = ?1", [id]);
            format!("{} cancelado", t.label)
        } else if t.paused {
            let _ = conn.execute("UPDATE timers SET paused = 0, ends = ?2 WHERE id = ?1", rusqlite::params![id, now + t.left]);
            format!("{} continua", t.label)
        } else {
            let _ = conn.execute("UPDATE timers SET paused = 1, left = ?2 WHERE id = ?1", rusqlite::params![id, t.remaining(now)]);
            format!("{} pausado", t.label)
        };
        let _ = self.changed.try_send(());
        Outcome::refresh_with(toast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("10m chá"), Some((600_000, "chá")));
        assert_eq!(parse_duration("25 min foco"), Some((1_500_000, "foco")));
        assert_eq!(parse_duration("1h30"), Some((5_400_000, "")));
        assert_eq!(parse_duration("1h30m pão"), Some((5_400_000, "pão")));
        assert_eq!(parse_duration("90s"), Some((90_000, "")));
        assert_eq!(parse_duration("2"), Some((120_000, "")));
        assert_eq!(parse_duration("5 chá"), Some((300_000, "chá")));
        assert_eq!(parse_duration("chá"), None);
        assert_eq!(parse_duration("0s"), None);
    }

    #[test]
    fn clocks() {
        assert_eq!(parse_clock("7:30"), Some((7, 30)));
        assert_eq!(parse_clock("19h"), Some((19, 0)));
        assert_eq!(parse_clock("7h15"), Some((7, 15)));
        assert_eq!(parse_clock("0715"), Some((7, 15)));
        assert_eq!(parse_clock("25:00"), None);
        let wait = until_clock(0, 0);
        assert!(wait > 0 && wait <= 86_400_000);
    }

    #[test]
    fn start_pause_cancel() {
        let t = Timers::new(Store::memory());
        let rows = t.generate(&Query { text: "timer 10m Chá verde", scoped: false });
        assert_eq!(rows[0].title, "timer de 10 min · Chá verde");
        t.activate("timers:new", "start");
        let home = t.home();
        assert_eq!(home.len(), 1);
        assert!(home[0].subtitle.starts_with("faltam 10:00") || home[0].subtitle.starts_with("faltam 9:59"));
        let id = home[0].id.clone();
        t.activate(&id, "pause");
        assert!(t.home()[0].subtitle.starts_with("pausado"));
        t.activate(&id, "pause");
        assert!(t.home()[0].subtitle.starts_with("faltam"));
        t.activate(&id, "cancel");
        assert!(t.home().is_empty());
        assert!(t.generate(&Query { text: "timeline", scoped: false }).is_empty());
    }
}

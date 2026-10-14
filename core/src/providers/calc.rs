//! Calculator: fend (units, bases, dates). `= expr` always; bare text when
//! it reads as math ("2^10", "5 km to mi").

use std::cell::RefCell;
use std::time::{Duration, Instant};

use crate::engine::{Provider, Query};
use crate::item::{Action, Item, Outcome};
use crate::platform;

pub struct Calc {
    last: RefCell<String>,
}

struct Deadline(Instant);

impl fend_core::Interrupt for Deadline {
    fn should_interrupt(&self) -> bool {
        Instant::now() > self.0
    }
}

/// Worth trying without "=": a digit, and an operator or a conversion.
fn looks_like_math(text: &str) -> bool {
    let has_digit = text.chars().any(|c| c.is_ascii_digit());
    let has_op = text.chars().any(|c| "+-*/^%()×÷!".contains(c))
        || [" to ", " in ", " as ", " em ", " para "].iter().any(|w| text.contains(w))
        || text.starts_with("sqrt") || text.starts_with("0x");
    has_digit && has_op
}

impl Calc {
    pub fn new() -> Self {
        Self { last: RefCell::new(String::new()) }
    }

    fn eval(&self, text: &str, preview: bool) -> Result<String, String> {
        let mut ctx = fend_core::Context::new();
        let int = Deadline(Instant::now() + Duration::from_millis(40));
        // pt-BR conversions read like English ones to fend.
        let text = text.replace(" em ", " to ").replace(" para ", " to ");
        if preview {
            let r = fend_core::evaluate_preview_with_interrupt(&text, &ctx, &int);
            let s = r.get_main_result().to_string();
            if s.is_empty() { Err(String::new()) } else { Ok(s) }
        } else {
            fend_core::evaluate_with_interrupt(&text, &mut ctx, &int).map(|r| r.get_main_result().to_string())
        }
    }
}

impl Provider for Calc {
    fn id(&self) -> &'static str {
        "calc"
    }
    fn title(&self) -> &'static str {
        "calculadora"
    }
    fn prefix(&self) -> Option<&'static str> {
        Some("=")
    }
    fn mixed(&self) -> usize {
        0
    }
    fn tracks_usage(&self) -> bool {
        false
    }
    fn generate(&self, q: &Query) -> Vec<Item> {
        if q.text.is_empty() || (!q.scoped && !looks_like_math(q.text)) {
            return Vec::new();
        }
        let (title, subtitle, ok) = match self.eval(q.text, !q.scoped) {
            Ok(v) if !q.scoped && v.trim() == q.text.trim() => return Vec::new(),
            Ok(v) => (v, format!("= {}", q.text), true),
            Err(_) if !q.scoped => return Vec::new(),
            Err(e) => ("…".into(), e, false),
        };
        if ok {
            *self.last.borrow_mut() = title.clone();
        }
        vec![Item {
            id: "calc:result".into(),
            title,
            subtitle,
            icon: "ph:calculator".into(),
            actions: if ok { vec![Action::new("copy", "copiar resultado", "enter")] } else { Vec::new() },
            ..Item::default()
        }]
    }
    fn activate(&self, _id: &str, _action: &str) -> Outcome {
        let value = self.last.borrow().clone();
        match platform::copy(&value) {
            Ok(()) => Outcome::close_with(format!("{value} copiado")),
            Err(e) => Outcome::toast(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, scoped: bool) -> Vec<Item> {
        Calc::new().generate(&Query { text, scoped })
    }

    #[test]
    fn math_with_and_without_prefix() {
        assert_eq!(run("2^10", false)[0].title, "1024");
        assert_eq!(run("2 + 2", true)[0].title, "4");
        assert!(run("helium", false).is_empty());
        assert!(run("42", false).is_empty());
        assert!(run("1 km em m", false)[0].title.starts_with("1000"));
    }

    #[test]
    fn errors_only_when_scoped() {
        assert!(run("2 +", false).is_empty());
        assert_eq!(run("2 +", true)[0].title, "…");
    }
}

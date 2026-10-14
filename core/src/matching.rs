//! How well a row matches what was typed. Tiers first, so the obvious hit
//! always wins over a clever fuzzy one:
//!
//! | tier | when                                    | example (query → title)        |
//! |------|-----------------------------------------|--------------------------------|
//! | 300  | the title starts with the text          | hel → Helium                   |
//! | 220  | a word starts with it                   | code → Visual Studio Code      |
//! | 200  | every typed word starts a title word    | vis code → Visual Studio Code  |
//! | 160  | the letters are word starts (acronym)   | vsc → Visual Studio Code       |
//! | 120  | it appears inside the title             | app → WhatsApp                 |
//! |   0  | a tight fuzzy match (a typo, a skip)    | heluim → Helium                |
//! |  80  | a keyword has it (no fuzzy)             | browser → Helium (WebBrowser)  |
//!
//! Within a tier, nucleo's score orders. Accents and case never matter.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};

pub struct Query {
    folded: String,
    words: Vec<String>,
    pattern: Pattern,
    letters: usize,
}

/// Lowercase without accents: "Configurações" → "configuracoes".
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// Char positions where a word starts: after a non-alphanumeric, or a
/// lower→upper turn ("WhatsApp" → W, A).
fn word_starts(title: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    for (i, c) in title.chars().enumerate() {
        let start = match prev {
            None => c.is_alphanumeric(),
            Some(p) => c.is_alphanumeric() && (!p.is_alphanumeric() || (p.is_lowercase() && c.is_uppercase())),
        };
        if start {
            out.push(i);
        }
        prev = Some(c);
    }
    out
}

fn words(folded: &str) -> impl Iterator<Item = &str> {
    folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty())
}

impl Query {
    pub fn new(text: &str) -> Self {
        let folded = fold(text.trim());
        Self {
            words: words(&folded).map(String::from).collect(),
            letters: folded.chars().filter(|c| !c.is_whitespace()).count(),
            pattern: Pattern::parse(text, CaseMatching::Ignore, Normalization::Smart),
            folded,
        }
    }

    pub fn score(&self, title: &str, keywords: &str, matcher: &mut Matcher) -> Option<i64> {
        if self.folded.is_empty() {
            return Some(0);
        }
        let t = fold(title);
        let title_words: Vec<&str> = words(&t).collect();
        let mut buf = Vec::new();
        let mut indices = Vec::new();
        let fuzzy = self.pattern.indices(Utf32Str::new(title, &mut buf), matcher, &mut indices).map(i64::from);
        let tier = if t.starts_with(&self.folded) {
            Some(300)
        } else if title_words.iter().any(|w| w.starts_with(&self.folded)) {
            Some(220)
        } else if self.words.len() > 1 && self.words.iter().all(|q| title_words.iter().any(|w| w.starts_with(q.as_str()))) {
            Some(200)
        } else if fuzzy.is_some() && self.letters >= 2 && {
            let starts = word_starts(title);
            indices.iter().all(|i| starts.contains(&(*i as usize)))
        } {
            Some(160)
        } else if t.contains(&self.folded) {
            Some(120)
        } else if fuzzy.is_some() && {
            indices.sort_unstable();
            let span = (indices.last().copied().unwrap_or(0) - indices.first().copied().unwrap_or(0) + 1) as usize;
            self.letters >= 3 && span <= self.letters + self.letters / 2
        } {
            Some(0)
        } else {
            None
        };
        if let Some(tier) = tier {
            return Some(tier + fuzzy.unwrap_or(0).min(99));
        }
        let kw = fold(keywords);
        let kw: Vec<&str> = words(&kw).collect();
        self.words
            .iter()
            .all(|q| kw.iter().any(|w| w.starts_with(q.as_str()) || (q.len() >= 4 && w.contains(q.as_str()))))
            .then_some(80)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleo_matcher::Config;

    fn order(q: &str, titles: &[(&str, &str)]) -> Vec<String> {
        let mut m = Matcher::new(Config::DEFAULT);
        let q = Query::new(q);
        let mut s: Vec<(i64, &str)> = titles
            .iter()
            .filter_map(|(t, k)| q.score(t, k, &mut m).map(|s| (s, *t)))
            .collect();
        s.sort_by(|a, b| b.0.cmp(&a.0));
        s.into_iter().map(|(_, t)| t.to_string()).collect()
    }

    const APPS: &[(&str, &str)] = &[
        ("Yazi File Manager", "terminal"),
        ("Firefox", "browser web"),
        ("Visual Studio Code", "editor"),
        ("WhatsApp", "chat"),
        ("Helium", "WebBrowser internet"),
        ("Configurações", "settings"),
    ];

    #[test]
    fn tiers() {
        assert_eq!(order("fir", APPS), vec!["Firefox"]);
        assert_eq!(order("code", APPS)[0], "Visual Studio Code");
        assert_eq!(order("vsc", APPS), vec!["Visual Studio Code"]);
        assert_eq!(order("vis code", APPS), vec!["Visual Studio Code"]);
        assert_eq!(order("app", APPS), vec!["WhatsApp"]);
        assert_eq!(order("configura", APPS), vec!["Configurações"]);
        assert_eq!(order("heluim", APPS), Vec::<String>::new());
        assert_eq!(order("helim", APPS), vec!["Helium"]);
        // Keywords only when nothing in a title matched as well.
        assert_eq!(order("browser", APPS), vec!["Firefox", "Helium"]);
        assert_eq!(order("fm", APPS), vec!["Yazi File Manager"]);
    }
}

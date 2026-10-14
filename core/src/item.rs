//! What providers hand the launcher: results, their actions and what
//! happens after one runs. Also the wire format for external plugins.

use serde::{Deserialize, Serialize};

/// One result row.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Item {
    /// `<provider>:<key>`, stable across queries (usage and aliases use it).
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    /// An image source the renderer resolves (`icon:firefox`, `file:/x.png`)
    /// or a phosphor glyph name (`ph:calculator`). Empty: the group's icon.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// A character drawn instead of an icon (emoji).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub glyph: String,
    /// Words that match but are not shown (keywords, generic names).
    #[serde(default, skip_serializing)]
    pub keywords: String,
    /// What Enter does first; the rest in order of their keys.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
    /// A file this row stands for: draggable out of the launcher, previewable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// Right-hand hint (an alias, a shortcut, "fixado").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hint: String,
    /// Ranking inside the group; providers that sort themselves leave it 0.
    #[serde(default, skip)]
    pub score: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    pub title: String,
    /// `enter`, `ctrl+c`, ... shown and matched by the launcher.
    pub key: String,
}

impl Action {
    pub fn new(id: &str, title: &str, key: &str) -> Self {
        Self { id: id.into(), title: title.into(), key: key.into() }
    }
}

/// A titled run of results from one provider.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub provider: String,
    pub title: String,
    pub items: Vec<Item>,
}

/// What the launcher does after an action ran.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Outcome {
    /// Hide the launcher.
    #[serde(default)]
    pub close: bool,
    /// Replace the search text (and stay open).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Shown under the field for a moment ("copiado", an error).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub toast: String,
    /// Answer the current text again (the rows changed: deleted, pinned).
    /// Handled by the service, never sent.
    #[serde(default, skip)]
    pub refresh: bool,
}

impl Outcome {
    pub fn close() -> Self {
        Self { close: true, ..Self::default() }
    }
    pub fn query(text: impl Into<String>) -> Self {
        Self { query: Some(text.into()), ..Self::default() }
    }
    pub fn toast(text: impl Into<String>) -> Self {
        Self { toast: text.into(), ..Self::default() }
    }
    pub fn close_with(text: impl Into<String>) -> Self {
        Self { close: true, toast: text.into(), ..Self::default() }
    }
    pub fn refresh_with(text: impl Into<String>) -> Self {
        Self { refresh: true, toast: text.into(), ..Self::default() }
    }
}

/// A preview for the selected row.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Preview {
    #[default]
    None,
    Text {
        title: String,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        facts: Vec<(String, String)>,
    },
    Image {
        title: String,
        src: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        facts: Vec<(String, String)>,
    },
}

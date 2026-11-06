//! [`Target`]: how actions and assertions find elements.

use std::fmt;

use serde_json::{Value, json};

/// What an action or assertion acts on: a CSS selector, a test id, or the
/// text a user sees, with optional filters.
///
/// | Target | Finds |
/// |---|---|
/// | `"#deal-title"`, `".card"`, `"[name=email]"`, `"css=main h1"` | A CSS selector: anything starting with `#`, `.` or `[`, or prefixed with `css=` |
/// | `"@deal-card"` | The element with `data-testid="deal-card"` |
/// | `"Email"` | For fields: the input whose label, `aria-label`, `aria-labelledby`, placeholder or `name` is `Email`. For buttons and links: the one whose text or accessible name is `Email`. Otherwise any element whose text is `Email`. |
///
/// Text matches exactly after trimming and collapsing whitespace, case
/// included. Only visible elements match; when several do, actions use the
/// first, in document order.
///
/// ```
/// use navree_browser::Target;
///
/// let renewal = Target::from("@deal-card").containing("Lakhotia renewal");
/// assert_eq!(renewal.to_string(), r#""@deal-card" containing "Lakhotia renewal""#);
/// let second = Target::from(".card").nth(1);
/// assert_eq!(second.to_string(), r#"".card" nth 1"#);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    raw: String,
    kind: Kind,
    containing: Option<String>,
    nth: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Css(String),
    TestId(String),
    Text(String),
}

impl Target {
    /// Parses a target string (see the table above).
    pub fn new(target: impl Into<String>) -> Target {
        let raw = target.into();
        let kind = if let Some(css) = raw.strip_prefix("css=") {
            Kind::Css(css.trim().to_owned())
        } else if let Some(id) = raw.strip_prefix('@') {
            Kind::TestId(id.to_owned())
        } else if raw.starts_with(['#', '.', '[']) {
            Kind::Css(raw.clone())
        } else {
            Kind::Text(raw.split_whitespace().collect::<Vec<_>>().join(" "))
        };
        Target {
            raw,
            kind,
            containing: None,
            nth: None,
        }
    }

    /// Keeps only the matches whose visible text contains `text`.
    pub fn containing(mut self, text: impl Into<String>) -> Target {
        self.containing = Some(text.into());
        self
    }

    /// Picks the match at position `index` (from 0, in document order, after
    /// the other filters).
    pub fn nth(mut self, index: usize) -> Target {
        self.nth = Some(index);
        self
    }

    /// The target string as written.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Whether [`nth`](Self::nth) picked one match.
    pub(crate) fn has_nth(&self) -> bool {
        self.nth.is_some()
    }

    /// The JSON the page script resolves.
    pub(crate) fn to_json(&self) -> Value {
        let (kind, value) = match &self.kind {
            Kind::Css(css) => ("css", css),
            Kind::TestId(id) => ("testid", id),
            Kind::Text(text) => ("text", text),
        };
        json!({
            "kind": kind,
            "value": value,
            "containing": self.containing,
            "nth": self.nth,
        })
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.raw)?;
        if let Some(text) = &self.containing {
            write!(f, " containing {text:?}")?;
        }
        if let Some(index) = self.nth {
            write!(f, " nth {index}")?;
        }
        Ok(())
    }
}

impl From<&str> for Target {
    fn from(target: &str) -> Target {
        Target::new(target)
    }
}

impl From<String> for Target {
    fn from(target: String) -> Target {
        Target::new(target)
    }
}

impl From<&String> for Target {
    fn from(target: &String) -> Target {
        Target::new(target.as_str())
    }
}

impl From<&Target> for Target {
    fn from(target: &Target) -> Target {
        target.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(target: &str) -> (String, String) {
        let json = Target::from(target).to_json();
        (
            json["kind"].as_str().unwrap().to_owned(),
            json["value"].as_str().unwrap().to_owned(),
        )
    }

    #[test]
    fn css_selectors_start_with_hash_dot_or_bracket_or_css_prefix() {
        assert_eq!(kind("#deal-title"), ("css".into(), "#deal-title".into()));
        assert_eq!(kind(".board .card"), ("css".into(), ".board .card".into()));
        assert_eq!(kind("[name=email]"), ("css".into(), "[name=email]".into()));
        assert_eq!(kind("css=main h1"), ("css".into(), "main h1".into()));
    }

    #[test]
    fn at_names_a_test_id() {
        assert_eq!(kind("@deal-card"), ("testid".into(), "deal-card".into()));
    }

    #[test]
    fn anything_else_is_text_with_whitespace_collapsed() {
        assert_eq!(kind("Email"), ("text".into(), "Email".into()));
        assert_eq!(kind("  Sign \n in "), ("text".into(), "Sign in".into()));
        assert_eq!(kind("main h1"), ("text".into(), "main h1".into()));
    }

    #[test]
    fn filters_are_carried_to_the_page_and_shown() {
        let target = Target::from("@deal-card").containing("Lakhotia").nth(2);
        let json = target.to_json();
        assert_eq!(json["containing"], "Lakhotia");
        assert_eq!(json["nth"], 2);
        assert_eq!(
            target.to_string(),
            r#""@deal-card" containing "Lakhotia" nth 2"#
        );
        assert_eq!(target.as_str(), "@deal-card");
        let plain = Target::from("Save").to_json();
        assert_eq!(plain["containing"], Value::Null);
        assert_eq!(plain["nth"], Value::Null);
    }

    #[test]
    fn strings_and_targets_convert() {
        let owned = String::from("Save");
        assert_eq!(Target::from(&owned), Target::from("Save"));
        assert_eq!(Target::from(owned.clone()), Target::new(owned));
        let target = Target::from("@x").nth(0);
        assert_eq!(Target::from(&target), target);
    }
}

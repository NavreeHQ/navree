//! [`Envelope`]: what the server answers a native client with, and [`Screen`]:
//! a whole page as parts.

use serde::{Deserialize, Serialize};

use crate::node::{Node, is_false};
use crate::op::Op;

/// A way back: the screen before this one in the app's own order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Back {
    /// The screen's name.
    pub title: String,
    /// Its address.
    pub href: String,
}

/// A search a screen or a table offers. The client sends the typed words as
/// `name`, with `hidden` beside them, as a query to `action`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Search {
    /// The name the words are sent under.
    pub name: String,
    /// What was searched for to get this screen.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// What the box shows while empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Where the query goes. The screen's own address when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
    /// What else the query carries: the sort and the filters in force.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<crate::action::Pair>,
}

/// A whole page, as parts.
///
/// The screen's actions carry roles, never positions: each system's design
/// layer decides where the primary action, creation, the secondary ones and the
/// destructive ones go on each form factor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screen {
    /// The screen's name.
    pub title: String,
    /// A line under the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// The id of the screen's content, when the page gave one: what an op that
    /// changes all of it names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The screen before this one in the app's own order: the last step of the
    /// page's breadcrumb.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<Back>,
    /// The whole trail to this screen, the app's first screen first, when it is
    /// longer than `back` alone. A wide screen may draw it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trail: Vec<Back>,
    /// The address of the shell's destination this screen belongs to, so the
    /// client marks it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nav: Option<String>,
    /// What a reader may do with the screen as a whole: buttons and menus.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Node>,
    /// The screen's search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<Search>,
    /// Whether pulling down fetches the screen again.
    #[serde(default, skip_serializing_if = "is_false")]
    pub refresh: bool,
    /// The screen's content.
    pub body: Node,
}

/// "Show this in the web view": the page holds something with no native
/// meaning yet. The client loads `url` in its web view with
/// [`EMBED_HEADER`](crate::EMBED_HEADER), inside its own chrome.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Web {
    /// The page's address.
    pub url: String,
    /// The page's name, for the client's own bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What could not be drawn natively, each as `kind:name` (`slot:calendar`,
    /// `tag:canvas`, `vocab:chart`): for the coverage report and a developer's
    /// overlay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<String>,
}

/// "This app is too old to serve safely."
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upgrade {
    /// The oldest version of the app the server still serves.
    pub min_app: String,
    /// Where the newer app is.
    pub url: String,
}

/// What the server answers a native client with, as
/// [`CONTENT_TYPE`](crate::CONTENT_TYPE). It holds a screen, ops for the screen
/// in front, both (a screen and a toast left for it), or a word to show the page
/// in the web view.
///
/// ```
/// use navree_native::{Envelope, Node, Screen};
///
/// let envelope = Envelope::screen(
///     "3f9a1c07b2de",
///     Screen::new("Orders", Node::text("No orders yet")),
/// );
/// assert_eq!(
///     serde_json::to_string(&envelope).unwrap(),
///     r#"{"navree":1,"build":"3f9a1c07b2de","vocab":0,"screen":{"title":"Orders","body":{"t":"text","value":"No orders yet"}}}"#
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// The protocol's major version.
    pub navree: u32,
    /// The server's build. A client that sees one it has not seen drops the
    /// screens it kept.
    pub build: String,
    /// The vocabulary the answer is written in: the client's own, or an older one.
    pub vocab: u32,
    /// A whole page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<Screen>,
    /// Changes to the screen in front, applied in order; after `screen`, when
    /// both are here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ops: Vec<Op>,
    /// The page-level fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<Web>,
    /// The app must be updated before it is served.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<Upgrade>,
}

impl Envelope {
    fn empty(build: impl Into<String>) -> Self {
        Self {
            navree: crate::MAJOR,
            build: build.into(),
            vocab: crate::VOCAB,
            screen: None,
            ops: Vec::new(),
            web: None,
            upgrade: None,
        }
    }

    /// An answer that is a whole page.
    pub fn screen(build: impl Into<String>, screen: Screen) -> Self {
        Self {
            screen: Some(screen),
            ..Self::empty(build)
        }
    }

    /// An answer that changes the screen in front.
    pub fn ops(build: impl Into<String>, ops: Vec<Op>) -> Self {
        Self {
            ops,
            ..Self::empty(build)
        }
    }

    /// An answer that sends the client to its web view.
    pub fn web(build: impl Into<String>, web: Web) -> Self {
        Self {
            web: Some(web),
            ..Self::empty(build)
        }
    }

    /// An answer that asks for a newer app.
    pub fn upgrade(build: impl Into<String>, upgrade: Upgrade) -> Self {
        Self {
            upgrade: Some(upgrade),
            ..Self::empty(build)
        }
    }

    /// The same answer, written in vocabulary `vocab`.
    #[must_use]
    pub fn in_vocab(mut self, vocab: u32) -> Self {
        self.vocab = vocab;
        self
    }
}

impl Screen {
    /// A screen named `title` that holds `body`.
    pub fn new(title: impl Into<String>, body: Node) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            id: None,
            back: None,
            trail: Vec::new(),
            nav: None,
            actions: Vec::new(),
            search: None,
            refresh: false,
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_holds_only_what_it_answers_with() {
        let web = Envelope::web(
            "b1",
            Web {
                url: "/reports/42".to_owned(),
                title: Some("Report".to_owned()),
                why: vec!["slot:chart".to_owned(), "tag:canvas".to_owned()],
            },
        );
        let json = serde_json::to_string(&web).unwrap();
        assert_eq!(
            json,
            r#"{"navree":1,"build":"b1","vocab":0,"web":{"url":"/reports/42","title":"Report","why":["slot:chart","tag:canvas"]}}"#
        );
        assert_eq!(serde_json::from_str::<Envelope>(&json).unwrap(), web);

        let upgrade = Envelope::upgrade(
            "b1",
            Upgrade {
                min_app: "1.2.0".to_owned(),
                url: "https://example.com/app".to_owned(),
            },
        );
        let json = serde_json::to_string(&upgrade).unwrap();
        assert_eq!(
            json,
            r#"{"navree":1,"build":"b1","vocab":0,"upgrade":{"min_app":"1.2.0","url":"https://example.com/app"}}"#
        );

        let ops = Envelope::ops("b1", vec![Op::Reload]);
        assert_eq!(
            serde_json::to_string(&ops).unwrap(),
            r#"{"navree":1,"build":"b1","vocab":0,"ops":[{"op":"reload"}]}"#
        );
    }
}

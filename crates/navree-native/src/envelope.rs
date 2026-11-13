//! [`Envelope`]: what the server answers a native client with, and [`Screen`]:
//! a whole page as parts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::charts::DayPager;
use crate::node::{Icon, IconShape, Node, TableRow, is_false};
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
    /// The control that steps the screen from day to day, when the screen is
    /// one day of many. It belongs to the screen and not to its content: a
    /// client puts it where its system puts what a whole screen turns on, in
    /// the bar at the title's place on a narrow window. The title still names
    /// the screen wherever a name is needed: the way back, the app switcher,
    /// assistive technology.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pager: Option<DayPager>,
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
    /// The toolkit's build, as the web knows it: a stamp of its own assets. A
    /// native client keeps screens by `app` and `shell` instead.
    pub build: String,
    /// The vocabulary the answer is written in: the client's own, or an older one.
    pub vocab: u32,
    /// The app's stamp: it changes when the server's app does. A client that
    /// sees one it has not seen drops the screens it kept.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app: String,
    /// The shell's stamp: the [`stamp`](crate::Shell::stamp) of the shell
    /// document in force. A client that sees one it has not seen fetches the
    /// shell again.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shell: String,
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
    /// Icon shapes by name. An icon of this envelope that has a name and no
    /// shapes of its own is drawn with the shapes held here under its name;
    /// a client puts them back with [`inline_icons`](Self::inline_icons) as it
    /// reads the envelope.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub icons: BTreeMap<String, IconShape>,
}

impl Envelope {
    fn empty(build: impl Into<String>) -> Self {
        Self {
            navree: crate::MAJOR,
            build: build.into(),
            vocab: crate::VOCAB,
            app: String::new(),
            shell: String::new(),
            screen: None,
            ops: Vec::new(),
            web: None,
            upgrade: None,
            icons: BTreeMap::new(),
        }
    }

    /// The same answer, stamped with the app's stamp and the shell's.
    #[must_use]
    pub fn stamped(mut self, app: impl Into<String>, shell: impl Into<String>) -> Self {
        self.app = app.into();
        self.shell = shell.into();
        self
    }

    /// Calls `each` with every icon the envelope holds: on the screen, among
    /// its actions and in the nodes of its ops.
    fn each_icon(&mut self, mut each: impl FnMut(&mut Icon)) {
        let mut stack: Vec<&mut Node> = Vec::new();
        if let Some(screen) = &mut self.screen {
            stack.push(&mut screen.body);
            stack.extend(&mut screen.actions);
            // The day's scores a pager carries are parts too.
            let scores = screen.pager.as_mut().and_then(|pager| pager.scores.as_deref_mut());
            stack.extend(scores);
        }
        for op in &mut self.ops {
            match op {
                Op::Morph { node, .. } | Op::Replace { node, .. } => stack.push(node),
                Op::Append { nodes, .. } | Op::Prepend { nodes, .. } | Op::Merge { nodes, .. } => {
                    stack.extend(nodes);
                }
                Op::Rows { rows, .. } => stack.extend(rows.iter_mut().flat_map(TableRow::nodes_mut)),
                Op::Row { row, .. } => stack.extend(row.nodes_mut()),
                _ => {}
            }
        }
        while let Some(node) = stack.pop() {
            for icon in node.icons_mut() {
                each(icon);
            }
            stack.extend(node.children_mut());
        }
    }

    /// Moves the shapes of every named icon into the envelope's icon table,
    /// once per name, leaving each icon its name. An icon drawn differently
    /// from the one already held under its name, or with no name, keeps its
    /// shapes. Icon shapes are a sixth of a typical envelope; most recur.
    pub fn share_icons(&mut self) {
        let mut table = std::mem::take(&mut self.icons);
        self.each_icon(|icon| {
            let (Some(name), Some(shape)) = (&icon.name, icon.shape()) else {
                return;
            };
            match table.get(name) {
                None => {
                    table.insert(name.clone(), shape);
                    icon.clear_shape();
                }
                Some(held) if *held == shape => icon.clear_shape(),
                Some(_) => {}
            }
        });
        self.icons = table;
    }

    /// Gives every named icon with no shapes of its own the shapes the icon
    /// table holds under its name, and empties the table: what a client does
    /// as it reads an envelope, so the screens it keeps stand on their own.
    pub fn inline_icons(&mut self) {
        let table = std::mem::take(&mut self.icons);
        if table.is_empty() {
            return;
        }
        self.each_icon(|icon| {
            if icon.shape().is_none()
                && let Some(shape) = icon.name.as_ref().and_then(|name| table.get(name))
            {
                icon.set_shape(shape.clone());
            }
        });
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
            pager: None,
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Button, Stack};

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

    #[test]
    fn an_envelope_carries_the_stamps_of_the_app_and_the_shell() {
        let envelope = Envelope::ops("b1", vec![Op::Reload]).stamped("app-7f3a", "shell-91c2");
        let json = serde_json::to_string(&envelope).unwrap();
        assert_eq!(
            json,
            r#"{"navree":1,"build":"b1","vocab":0,"app":"app-7f3a","shell":"shell-91c2","ops":[{"op":"reload"}]}"#
        );
        assert_eq!(serde_json::from_str::<Envelope>(&json).unwrap(), envelope);
    }

    fn icon(name: Option<&str>, path: &str) -> Icon {
        Icon {
            name: name.map(str::to_owned),
            paths: vec![path.to_owned()],
            ..Icon::default()
        }
    }

    fn button(icon: Icon) -> Node {
        Node::Button(Button {
            label: "Go".to_owned(),
            icon: Some(icon),
            ..Button::default()
        })
    }

    #[test]
    fn an_icon_shape_is_sent_once_per_envelope_and_put_back_where_it_was() {
        let plus = icon(Some("plus"), "M5 12h14M12 5v14");
        let body = Node::Stack(Stack {
            children: vec![
                Node::Icon(plus.clone()),
                button(plus.clone()),
                // The same name drawn differently stays where it is.
                button(icon(Some("plus"), "M4 12h16")),
                // So does an icon with no name to look it up by.
                Node::Icon(icon(None, "M3 3h18v18H3z")),
            ],
            ..Stack::default()
        });
        let mut envelope = Envelope::screen("b1", Screen::new("Orders", body));
        envelope.ops = vec![Op::Append {
            target: "feed".to_owned(),
            nodes: vec![button(plus.clone())],
            more: None,
        }];
        let before = envelope.clone();
        envelope.share_icons();
        assert_eq!(
            envelope.icons,
            BTreeMap::from([(
                "plus".to_owned(),
                IconShape {
                    paths: vec!["M5 12h14M12 5v14".to_owned()],
                    ..IconShape::default()
                }
            )])
        );
        let json = serde_json::to_string(&envelope).unwrap();
        assert_eq!(json.matches("M5 12h14M12 5v14").count(), 1, "{json}");
        assert_eq!(json.matches("M4 12h16").count(), 1, "{json}");
        assert_eq!(json.matches("M3 3h18v18H3z").count(), 1, "{json}");
        envelope.inline_icons();
        assert_eq!(envelope, before);
    }
}

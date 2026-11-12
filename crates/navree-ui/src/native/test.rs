//! [`TestNative`]: drive a page as a native client in a plain test.

use navree_native::node as n;
use navree_native::{Action, Envelope, FormFactor, Platform, Screen};

use super::lower::N;
use super::page::{self, Page};
use super::scope::{self, Asked};
use crate::node::Node;
use crate::view::View;

/// A page under test, as a native client gets it.
///
/// ```
/// use navree_native::Action;
/// use navree_ui::native::TestNative;
/// use navree_ui::{Button, View};
///
/// let page = TestNative::of(&View::new("x").body(Button::primary("Edit").href("/edit")));
/// assert!(page.is_native());
/// assert!(page.sees("Edit"));
/// assert_eq!(page.tap("Edit"), Some(Action::navigate("/edit")));
/// ```
#[derive(Clone, Debug)]
pub struct TestNative {
    client: navree_native::Client,
    page: Page,
}

/// The client a test draws for unless it says another: a phone on Android.
fn phone() -> navree_native::Client {
    navree_native::Client::new(Platform::Android, FormFactor::Phone)
}

impl TestNative {
    /// The page `view`, as a phone on Android gets it.
    ///
    /// The page was built before this is called, so a component that builds
    /// itself another way for a native client (a table's rows) did not know
    /// one was asking. Build the page inside [`TestNative::build`] for those.
    pub fn of(view: &View) -> Self {
        let mut view = view.clone();
        Self {
            client: phone(),
            page: page::of_view(&mut view, false),
        }
    }

    /// A part of a page.
    pub fn of_node(node: impl Into<Node>) -> Self {
        Self {
            client: phone(),
            page: page::read("", &node.into(), false, None),
        }
    }

    /// The page `build` makes while `client` is the one asking, as a handler's
    /// page is made during that client's request.
    ///
    /// ```
    /// use navree_native::{Client, FormFactor, Platform};
    /// use navree_ui::native::{self, TestNative};
    /// use navree_ui::{Text, View};
    ///
    /// let watch = Client::new(Platform::Watchos, FormFactor::Watch);
    /// let page = TestNative::build(watch, || {
    ///     View::new("Today").body((
    ///         Text::new("3 orders to pack"),
    ///         native::web_only(Text::new("Drag a card to move it")),
    ///     ))
    /// });
    /// assert!(page.sees("3 orders to pack"));
    /// assert!(!page.sees("Drag a card"));
    /// ```
    pub fn build(client: navree_native::Client, build: impl FnOnce() -> View) -> Self {
        let asked = Asked::Native(std::sync::Arc::new(scope::Request::new(client.clone(), "")));
        let mut view = scope::sync(asked, build);
        Self {
            page: page::of_view(&mut view, false),
            client,
        }
    }

    /// Whether every part of the page was read: the client gets a screen.
    pub fn is_native(&self) -> bool {
        self.page.is_native()
    }

    /// What could not be read, each thing once: why the client would show the
    /// page in its web view.
    pub fn why(&self) -> Vec<String> {
        self.page.why()
    }

    /// The page as it was read.
    pub fn page(&self) -> &Page {
        &self.page
    }

    /// The page's screen.
    pub fn screen(&self) -> &Screen {
        &self.page.screen
    }

    /// The envelope the client gets for the address `url`.
    pub fn envelope_at(&self, url: &str) -> Envelope {
        let request = scope::Request::new(self.client.clone(), url);
        // A screen too deep for a client is not copied: it goes to the web view.
        let page = if super::swap::too_deep(&self.page.screen) {
            Page {
                screen: Screen::new(
                    self.page.screen.title.clone(),
                    N::Stack(n::Stack::default()),
                ),
                ops: self.page.ops.clone(),
                misses: vec![super::Miss {
                    what: "depth".to_owned(),
                    why: "deeper than a client reads".to_owned(),
                }],
                dropped: Vec::new(),
            }
        } else {
            self.page.clone()
        };
        super::swap::finished(super::swap::envelope(page, &request).0, &request, "")
    }

    /// The envelope's JSON, as it goes on the wire.
    pub fn json(&self) -> String {
        serde_json::to_string(&self.envelope_at("")).unwrap_or_default()
    }

    /// Every node of the screen, its actions first, in the order a reader
    /// meets them.
    fn nodes(&self) -> Vec<&N> {
        let mut out = Vec::new();
        let mut stack: Vec<&N> = self
            .page
            .screen
            .actions
            .iter()
            .chain(std::iter::once(&self.page.screen.body))
            .rev()
            .collect();
        while let Some(node) = stack.pop() {
            out.push(node);
            stack.extend(node.children().into_iter().rev());
        }
        out
    }

    /// The node with this id.
    pub fn find(&self, id: &str) -> Option<&N> {
        self.nodes().into_iter().find(|node| node.id() == Some(id))
    }

    /// Whether a reader can read `text` on the screen: in its title, or in the
    /// words of any part.
    pub fn sees(&self, text: &str) -> bool {
        if self.page.screen.title.contains(text) {
            return true;
        }
        self.nodes().into_iter().any(|node| {
            let has = |words: &Option<String>| words.as_deref().is_some_and(|w| w.contains(text));
            match node {
                N::Text(node) => node.value.contains(text),
                N::Rich(node) => node.runs.iter().any(|run| run.text.contains(text)),
                N::Badge(node) => node.text.contains(text),
                N::Button(node) => node.label.contains(text),
                N::Link(node) => node.label.contains(text),
                N::Section(node) => has(&node.title) || has(&node.subtitle),
                N::Row(node) => node.title.contains(text) || has(&node.subtitle),
                N::Banner(node) => has(&node.title) || has(&node.text),
                N::Empty(node) => node.title.contains(text) || has(&node.text),
                N::Dialog(node) => has(&node.title) || has(&node.text),
                N::Sheet(node) => has(&node.title) || has(&node.subtitle),
                N::Field(node) => has(&node.label) || has(&node.help) || has(&node.error),
                N::Facts(node) => node.items.iter().any(|fact| {
                    fact.label.contains(text) || fact.details.iter().any(|d| d.contains(text))
                }),
                N::Table(node) => {
                    has(&node.caption) || node.columns.iter().any(|col| col.label.contains(text))
                }
                N::Disclosure(node) => node.label.contains(text),
                N::Code(node) => node.text.contains(text),
                N::Menu(node) => node.label.contains(text),
                _ => false,
            }
        })
    }

    /// What a tap on the button or link named `label` does. `None` when the
    /// screen has none, or it does nothing.
    pub fn tap(&self, label: &str) -> Option<Action> {
        self.nodes().into_iter().find_map(|node| match node {
            N::Button(n::Button {
                label: own, action, ..
            }) if own == label => action.clone(),
            N::Link(link) if link.label == label => Some(link.action.clone()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Button, Dialog, Text, el};

    #[test]
    fn a_page_is_driven_as_a_client_would() {
        let view = View::new("Order CS-20481").body((
            el("div").id("order-page").children((
                Text::new("Paid"),
                Button::outline("Refund").opens_dialog("refund"),
            )),
            Dialog::new("refund", "Refund CS-20481").body(Text::new("How much?")),
        ));
        let page = TestNative::of(&view);
        assert!(page.is_native(), "{:?}", page.why());
        assert!(page.sees("Order CS-20481"));
        assert!(page.sees("Paid"));
        assert!(page.sees("How much?"));
        assert!(!page.sees("Shipped"));
        assert!(matches!(page.find("refund"), Some(N::Dialog(_))));
        assert!(matches!(page.find("order-page"), Some(N::Stack(_))));
        assert_eq!(
            page.tap("Refund"),
            Some(Action::command(navree_native::Command::Open {
                id: "refund".into()
            }))
        );
        assert_eq!(page.tap("Cancel"), None);
        let envelope = page.envelope_at("/orders/1");
        assert!(envelope.screen.is_some());
        assert!(envelope.web.is_none());
        assert_eq!(envelope.build, crate::assets::BUILD);
    }

    #[test]
    fn a_page_that_can_not_be_read_answers_the_web_view_with_why() {
        let view = View::new("Board").body((Text::new("Cards"), el("canvas")));
        let page = TestNative::of(&view);
        assert!(!page.is_native());
        assert_eq!(page.why(), ["tag:canvas"]);
        let envelope = page.envelope_at("/board?lane=2");
        assert!(envelope.screen.is_none());
        let web = envelope.web.unwrap();
        assert_eq!(web.url, "/board?lane=2");
        assert_eq!(web.title.as_deref(), Some("Board"));
        assert_eq!(web.why, ["tag:canvas"]);
    }

    #[test]
    fn a_page_deeper_than_a_client_reads_is_answered_with_the_web_view() {
        let deep = |levels: usize| {
            let mut node: Node = Text::new("the bottom").into();
            for level in 0..levels {
                node = el("section")
                    .children((Text::new(format!("level {level}")), node))
                    .into();
            }
            View::new("Deep").body(node)
        };
        // Read whole: the server never runs out of stack reading it.
        let page = TestNative::of(&deep(500));
        assert!(page.is_native());
        // But no client reads a tree that deep, so it goes to the web view.
        let envelope = page.envelope_at("/deep");
        assert_eq!(envelope.web.unwrap().why, ["depth"]);
        assert_eq!(envelope.vocab, navree_native::VOCAB);
        let shallow = TestNative::of(&deep(20)).envelope_at("/deep");
        assert!(shallow.screen.is_some());
    }

    #[test]
    fn a_part_can_be_kept_for_one_side() {
        let build = || {
            View::new("x").body((
                crate::native::only(Text::new("Scan a code")),
                crate::native::web_only(Text::new("Type the code")),
            ))
        };
        let native = TestNative::build(phone(), build);
        assert!(native.sees("Scan a code"));
        assert!(!native.sees("Type the code"));
        // Built for a browser, the same page keeps the other part.
        let html = build().to_html();
        assert!(html.contains("Type the code"));
        assert!(!html.contains("Scan a code"));
    }
}

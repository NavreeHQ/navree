//! Native clients: the same pages, answered as a tree of parts a system's own
//! toolkit draws.
//!
//! A page is written once, as today. A browser gets it as HTML, byte for byte
//! what it got before. A native client sends `X-Navree-Native: 1` with its
//! descriptor in `X-Navree-Client`, and the same handler's answer leaves as
//! the envelope of [`navree_native`]: the page as a [`Screen`], a reply's ops
//! with nodes where the web has markup, or the word to show the page in the
//! client's web view when a part of it has no native meaning yet.
//!
//! - [`lower`] reads a finished tree into native parts and lists what it could
//!   not read ([`Lowered`], [`Miss`]). It never guesses.
//! - [`Client`] is the extractor for the handler whose screen really differs
//!   from one system to another, and [`only`] and [`web_only`] keep a part for
//!   one side.
//! - [`TestNative`] drives a page as a native client in a plain test, and
//!   [`Coverage`] lists which pages are native, which fall back and why.
//! - The app's shell is answered at [`SHELL_PATH`](navree_native::SHELL_PATH).
//!
//! ```
//! use navree_ui::native::TestNative;
//! use navree_ui::{Button, Text, View};
//!
//! let view = View::new("Orders").body((
//!     Text::new("No orders yet"),
//!     Button::primary("New order").href("/orders/new"),
//! ));
//! let page = TestNative::of(&view);
//! assert!(page.is_native());
//! assert!(page.sees("No orders yet"));
//! assert_eq!(
//!     page.json(),
//!     format!(
//!         concat!(
//!             r#"{{"navree":1,"build":"{}","vocab":0,"app":"{}","#,
//!             r#""screen":{{"title":"Orders","body":{{"t":"stack","children":["#,
//!             r#"{{"t":"text","value":"No orders yet"}},"#,
//!             r#"{{"t":"button","label":"New order","role":"primary","prominence":"high","#,
//!             r#""action":{{"t":"navigate","href":"/orders/new"}}}}]}}}}}}"#
//!         ),
//!         navree_ui::assets::BUILD,
//!         navree_ui::native::Stamp::of(None).as_str()
//!     )
//! );
//! ```

mod coverage;
mod icon;
mod lower;
pub(crate) mod page;
pub(crate) mod routes;
pub(crate) mod scope;
mod slots;
pub(crate) mod survey;
pub(crate) mod swap;
mod table;
mod test;

pub use crate::asked::{WhoAsked, who_asked};
pub use coverage::{Coverage, Grade, graded};
pub use lower::{Lowered, Miss, lower, lower_keeping_markup};
pub use navree_native as protocol;
pub use page::Page;
pub use routes::{LOWERING, Stamp};
pub(crate) use routes::{Routes, app_stamp};
pub(crate) use slots::chart::written as chart_numbers;

/// Keeps the typed values of the chart `id`: its labels and its series, written only
/// while a native client asks.
pub(crate) fn record_chart_numbers<'a>(
    id: &str,
    labels: impl Iterator<Item = &'a str>,
    series: impl Iterator<Item = (&'a str, &'a [Option<f64>])>,
    extras: Option<serde_json::Value>,
) {
    if let Some(values) = chart_numbers(labels, series, extras) {
        record_chart(id, values);
    }
}

/// Keeps the tree of the table rows written as `html` for the native client whose
/// request is current; a browser's request keeps nothing.
pub(crate) fn keep_rows(html: &str, rows: Node) {
    if let Some(request) = scope::native() {
        request.keep_rows(html, rows);
    }
}

/// Keeps the typed values of the chart `id` for the native client whose request
/// is current; a browser's request keeps nothing.
pub(crate) fn record_chart(id: &str, values: String) {
    if let Some(request) = scope::native() {
        request.record_chart(id, values);
    }
}
pub use scope::Client;
pub use swap::{Answered, How, NodeOp};
pub use test::TestNative;

use crate::node::Node;

/// `node` for a native client, and nothing for a browser: a part that only
/// makes sense where the device can do it.
///
/// It asks who the current request is from, so it is called while the page
/// is built, in the handler.
///
/// ```
/// use navree_ui::{Button, Node, native};
///
/// // Outside a native client's request, it is nothing.
/// assert!(matches!(native::only(Button::new("Scan a code")), Node::Empty));
/// ```
pub fn only(node: impl Into<Node>) -> Node {
    if Client::current().is_native() {
        node.into()
    } else {
        Node::Empty
    }
}

/// `node` for a browser, and nothing for a native client: a part with no
/// native meaning that the page can do without, so the rest of it stays
/// native.
///
/// ```
/// use navree_ui::{Node, Text, native};
///
/// assert!(matches!(native::web_only(Text::new("Drag to reorder")), Node::Element(_)));
/// ```
pub fn web_only(node: impl Into<Node>) -> Node {
    if Client::current().is_native() {
        Node::Empty
    } else {
        node.into()
    }
}

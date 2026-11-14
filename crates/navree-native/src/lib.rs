//! # navree-native
//!
//! The protocol between a Navree app and a native client: what the client
//! says about itself, and what the server answers with in place of HTML.
//!
//! A page is written once, as typed components. A browser gets it as HTML. A
//! native client asks for the same address and gets it as a tree of parts from
//! a closed vocabulary (a list, a field, a button with a role), which it draws
//! with its system's own toolkit and places as that system's guidelines say.
//! The tree is data: nothing in it is code, and every list in it is closed.
//!
//! - [`ClientDescriptor`]: what a client sends in [`CLIENT_HEADER`] on every
//!   request, beside [`NATIVE_HEADER`]; [`WebViewToken`]: how a page in its
//!   web view is known.
//! - [`Envelope`]: the answer, as [`CONTENT_TYPE`]. It holds a [`Screen`], ops
//!   for the screen in front, or a word to show the page in the web view, with
//!   the stamps of the app and of the shell.
//! - [`Node`]: the vocabulary. [`Action`] and [`Command`]: what a control does.
//!   [`Op`]: what the server changes.
//! - [`charts`]: the parts a chart is made of (a colour by name, a hint) and
//!   the kinds a health or fitness screen draws, as typed data.
//! - [`compose`]: the parts a screen with a design of its own is laid out
//!   with: cards, parts over each other, shapes, a run that scrolls across,
//!   and what changes with the room a screen has.
//! - [`ui`]: Navree Native UI, the way a screen is written in Rust for native
//!   clients: builders on these same parts.
//! - [`device`]: the device features a tap may start ([`DeviceCall`]), the few
//!   a reply may start ([`ReplyCall`]), and the sealed one-time [`Ticket`] a
//!   result is posted back with.
//! - [`Shell`]: the app's chrome, fetched from [`SHELL_PATH`].
//! - [`vocab`]: which vocabulary each kind arrived in, so an old app is
//!   answered in what it can draw.
//! - [`schema`]: the JSON Schema every client's models are generated from.
//!
//! The crate has no dependency on the toolkit that builds pages. The example
//! envelopes in `corpus/` are written from these types, and every client decodes
//! all of them in its own tests; `corpus/from-the-future.json`, written by hand,
//! holds what no build knows, and every client reads it without failing.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod action;
pub mod charts;
pub mod client;
pub mod compose;
pub mod device;
pub mod envelope;
pub mod input;
mod lenient;
pub mod node;
pub mod op;
pub mod schema;
pub mod shell;
pub mod symbols;
pub mod ui;
pub mod vocab;

pub use action::{Action, Command, Pair, Present, State};
pub use client::{
    Client, ClientDescriptor, ClientError, Contrast, DescriptorError, FormFactor, Platform, Scheme,
    WebViewToken,
};
pub use device::{DeviceCall, ReplyCall, Ticket};
pub use envelope::{Back, Envelope, Screen, Search, Upgrade, Web};
pub use input::Input;
pub use node::{Node, Prominence, Role, Standard, Tone};
pub use op::Op;
pub use shell::Shell;
pub use vocab::Kind;

/// The protocol's major version: the value of [`NATIVE_HEADER`] and of an
/// envelope's `navree`. It changes only when the envelope or the behaviour of
/// what is in it breaks; each side refuses a major it does not know.
pub const MAJOR: u32 = 1;

/// The newest vocabulary: the number a current client sends as `vocab`. It
/// stays 0 while the vocabulary may still change, and counts up from the
/// first release that freezes it.
pub const VOCAB: u32 = 0;

/// The deepest a node may sit inside other nodes in an answer: a client
/// refuses a deeper tree, with an error and never by running out of stack, so
/// a server answers a deeper page with the web view. See [`Node::depth`].
pub const MAX_DEPTH: usize = 64;

/// The deepest objects and arrays may nest in an answer's JSON: what the
/// readers of every client take. A server answers a deeper page with the web
/// view. See [`nesting`].
pub const MAX_NESTING: usize = 127;

/// How deep the objects and arrays of `json` nest, read without recursion.
///
/// ```
/// assert_eq!(navree_native::nesting(r#"{"t":"stack","children":[{"t":"text","value":"[{"}]}"#), 3);
/// assert_eq!(navree_native::nesting("7"), 0);
/// ```
pub fn nesting(json: &str) -> usize {
    let (mut depth, mut deepest) = (0_usize, 0_usize);
    let (mut in_text, mut escaped) = (false, false);
    for byte in json.bytes() {
        if in_text {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_text = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_text = true,
            b'{' | b'[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// The request header that says "answer me as a native client", with the
/// protocol's major version as its value.
pub const NATIVE_HEADER: &str = "x-navree-native";

/// The request header that carries the [`Client`] descriptor.
pub const CLIENT_HEADER: &str = "x-navree-client";

/// The request header a client's web view sends with the value `1`: the page
/// comes as HTML without the app's chrome, which the client draws itself. A
/// web view sends it with its first request only, so a page also knows it is
/// in one by the [`WebViewToken`] in its user agent.
pub const EMBED_HEADER: &str = "x-navree-embed";

/// The response header, with the value `1`, on a page's answer whose page may
/// be kept on the device between launches: its `Cache-Control` does not say
/// `no-store`. An answer without it is not kept. Ops, refusals, redirects and
/// the shell never carry it.
pub const KEEP_HEADER: &str = "x-navree-keep";

/// The product a native client's web view adds to its user agent:
/// `NavreeNative/1 (platform=ios; lib=0.3.0)`. See [`WebViewToken`].
pub const USER_AGENT_PRODUCT: &str = "NavreeNative";

/// The media type of an [`Envelope`] and of a [`Shell`].
pub const CONTENT_TYPE: &str = "application/vnd.navree.native+json";

/// Where a client fetches the app's [`Shell`].
pub const SHELL_PATH: &str = "/_navree/native/shell";

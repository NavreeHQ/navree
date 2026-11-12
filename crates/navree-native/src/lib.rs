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
//! - [`Client`]: the descriptor a client sends in [`CLIENT_HEADER`] on every
//!   request, beside [`NATIVE_HEADER`].
//! - [`Envelope`]: the answer, as [`CONTENT_TYPE`]. It holds a [`Screen`], ops
//!   for the screen in front, or a word to show the page in the web view.
//! - [`Node`]: the vocabulary. [`Action`] and [`Command`]: what a control does.
//!   [`Op`]: what the server changes.
//! - [`Shell`]: the app's chrome, fetched from [`SHELL_PATH`].
//! - [`vocab`]: which vocabulary each kind arrived in, so an old app is
//!   answered in what it can draw.
//!
//! The crate has no dependency on the toolkit that builds pages. The example
//! envelopes in `corpus/` are written from these types, and every client decodes
//! all of them in its own tests.
#![forbid(unsafe_code)]

pub mod action;
pub mod client;
pub mod envelope;
pub mod input;
pub mod node;
pub mod op;
pub mod shell;
pub mod vocab;

pub use action::{Action, Command, Pair, Present, State};
pub use client::{Client, ClientError, Contrast, FormFactor, Platform, Scheme};
pub use envelope::{Back, Envelope, Screen, Search, Upgrade, Web};
pub use input::Input;
pub use node::{Node, Prominence, Role, Tone};
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

/// The request header that says "answer me as a native client", with the
/// protocol's major version as its value.
pub const NATIVE_HEADER: &str = "x-navree-native";

/// The request header that carries the [`Client`] descriptor.
pub const CLIENT_HEADER: &str = "x-navree-client";

/// The request header a client's web view sends with the value `1`: the page
/// comes as HTML without the app's chrome, which the client draws itself.
pub const EMBED_HEADER: &str = "x-navree-embed";

/// The media type of an [`Envelope`] and of a [`Shell`].
pub const CONTENT_TYPE: &str = "application/vnd.navree.native+json";

/// Where a client fetches the app's [`Shell`].
pub const SHELL_PATH: &str = "/_navree/native/shell";

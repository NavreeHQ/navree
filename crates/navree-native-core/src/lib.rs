//! # navree-native-core
//!
//! The runtime core of Navree Native: everything a native client does that is
//! not drawing, placement or talking to the system. It is written once, here,
//! and every client runs it: SwiftUI on Apple's systems, Jetpack Compose on
//! Android, and whatever comes after.
//!
//! The core is a pure function of state and event. A client tells it what
//! happened with [`Core::send`] and gets back an [`Update`]: the parts of the
//! [`ViewState`] that changed, and the [`Effect`]s to carry out. The core never
//! does I/O, reads no clock and rolls no dice: it asks for a request, a timer
//! or a stream with an effect and learns the outcome from a later [`Event`].
//! So every rule in it can be tested with plain values, and four clients
//! cannot drift apart.
//!
//! ```
//! use navree_native::{Client, FormFactor, Platform};
//! use navree_native_core::{Config, Core, Effect, Event};
//!
//! let client = Client::new(Platform::Ios, FormFactor::Phone);
//! let mut core = Core::new(Config::new("https://shop.example", client));
//! let update = core.send(Event::Start);
//! // The first thing the core asks for is the app's shell.
//! let Effect::Fetch(request) = &update.effects[0] else { panic!() };
//! assert_eq!(request.url, "https://shop.example/_navree/native/shell");
//! assert!(core.view().destinations.is_empty());
//! ```
//!
//! - [`Config`]: what an app's build tells the core once.
//! - [`Event`]: what a client tells the core. [`Effect`]: what the core asks of it.
//! - [`ViewState`]: what a client draws from, read whole or in parts with [`Query`].
//! - `ffi`: the C interface, and the glue for Kotlin behind the `jni` feature.
//!   It is the only file with `unsafe` in it.
//!
//! The conversations with a server (pages, forms, ops, regions, lists, the
//! table's query, redirects, the web fallback, failures) are written down as
//! transcripts in `transcripts/`, which the tests replay.
#![deny(unsafe_code)]

use std::collections::BTreeSet;

mod cache;
mod commands;
mod config;
mod decode;
mod effect;
mod event;
#[allow(unsafe_code)]
pub mod ffi;
mod forms;
mod navigation;
mod lists;
mod ops;
mod push;
mod origin;
mod reduce;
mod regions;
mod sign_in;
pub mod snapshot;
pub mod schema;
mod state;
mod tree;

pub use config::{CacheConfig, Config, DeepLinks, SignInConfig};
pub use effect::{Body, Change, Effect, Part, Request, Update, Verb};
pub use snapshot::{SNAPSHOT_VERSION, Snapshot};
pub use event::{Event, FieldValue, FileRef, Response, StreamState, TransportError};
pub use state::{
    App, Content, Destination, Failure, FieldView, Focus, FormView, ListView, Message, Modal, Notice, Query, Question, RegionView, ScreenId, ScreenView, Stage, ViewState, Width, Window,
};

/// The runtime core. One per running app.
///
/// It is not shared between threads while it is in use: every call takes the
/// core for itself. A client calls it from one thread at a time, in the order
/// things happened.
#[derive(Debug)]
pub struct Core {
    pub(crate) config: Config,
    pub(crate) view: ViewState,
    pub(crate) inner: state::Inner,
    pub(crate) changed: BTreeSet<Change>,
    pub(crate) effects: Vec<Effect>,
}

impl Core {
    /// A core for an app built with `config`. Nothing happens until
    /// [`Event::Start`] is sent. A configuration the core can't use (an origin
    /// that is not `https`) leaves it in [`Stage::Failed`] with
    /// [`Failure::Config`], and it does nothing.
    pub fn new(config: Config) -> Self {
        let mut view = ViewState::default();
        let origin = origin::Origin::parse(&config.origin, &config.also, config.loopback_http);
        if origin.is_none() {
            view.stage = Stage::Failed;
            view.failure = Some(Failure::Config);
        }
        let inner = state::Inner {
            usable: origin.is_some(),
            origin: origin.unwrap_or_default(),
            ..state::Inner::default()
        };
        Self {
            config,
            view,
            inner,
            changed: BTreeSet::new(),
            effects: Vec::new(),
        }
    }

    /// What a client draws from.
    pub fn view(&self) -> &ViewState {
        &self.view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use navree_native::{Client, FormFactor, Platform};

    #[test]
    fn a_new_core_waits_to_start() {
        let client = Client::new(Platform::Android, FormFactor::Phone);
        let core = Core::new(Config::new("https://shop.example", client.clone()));
        assert_eq!(core.view().stage, Stage::Starting);
        assert!(core.view().screens.is_empty());

        let mut core = Core::new(Config::new("http://shop.example", client));
        assert_eq!(core.view().stage, Stage::Failed);
        assert_eq!(core.view().failure, Some(Failure::Config));
        assert_eq!(core.send(Event::Start), Update::default());
    }
}

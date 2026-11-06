//! An assistant on Navree UI pages (feature `ui`).
//!
//! [`Assistant`] runs an agent's conversations for server-rendered pages:
//!
//! - **Streaming into the page.** A message starts the agent's run in a task of its
//!   own. What it says is written into the conversation's items every
//!   [`flush_every`](AssistantConfig::flush_every) (120 ms by default), each write at the
//!   conversation's next revision, taken where it commits, followed by a nudge through
//!   the live hub. The page's [`Conversation`](navree_ui::genui::Conversation) is a live
//!   list: it asks for what changed since its revision and merges the answer in place,
//!   so focus and what the reader types stay as they are, in every tab. Stop reaches the
//!   run in whichever process writes it.
//! - **Components the model chooses.** With a [`Catalog`](navree_ui::genui::Catalog)
//!   the model is offered one `show_` tool per component the reader may see. Its
//!   arguments pass the catalogue's one check, or the model is told why and may try
//!   again; after two refusals it is told to answer in words. A component is stored as
//!   its name and props, and drawn when the page is, under the reader's own access at
//!   that moment, so the model never sees what it shows.
//! - **Buttons that act safely.** A component's buttons are signed for the reader, the
//!   conversation, the component's item, the action and its input, with a nonce used
//!   once in the action's own transaction. A post is checked again from scratch: the
//!   signature, the reader, the item, that the component declares the action, the
//!   reader's access to it now, and its input. A destructive action asks first in a
//!   dialog the page draws.
//! - **Approvals in the page's own place.** A tool marked `needs_approval` stops the
//!   run; its card is drawn in a live region outside the conversation, where nothing a
//!   model chose can be drawn, and Approve or Reject streams the rest of the run.
//! - **What the reader is looking at.** A page signs a reference to its record for the
//!   reader ([`Assistant::context_ref`]); the composer sends it, and the server resolves
//!   it under the reader's access ([`PageContext`]) before the model hears of it, as data.
//! - **Working state.** A conversation may hold a typed state ([`AssistantConfig::state`])
//!   that the model changes through a checked `update_state` tool and the page shows and
//!   edits ([`Assistant::state`], [`Assistant::set_state`]).
//!
//! The routes, under the assistant's path (`/assistant` here), each for the signed-in
//! reader's own conversations only:
//!
//! | Route | What it does |
//! |---|---|
//! | `POST /assistant` | Starts a conversation with `message`, then goes to its page |
//! | `POST /assistant/{id}/messages` | Sends `message` (and the page's signed `context`) |
//! | `GET /assistant/{id}/items` | The conversation's items, by cursor or since a revision |
//! | `GET /assistant/{id}/approval` | The approval's place, for its live region |
//! | `POST /assistant/{id}/stop` | Stops the answer being written |
//! | `POST /assistant/{id}/approve`, `/reject` | Answers the call that waits (`call_id`, `reason`) |
//! | `POST /assistant/act?…` | A component's button, as the server signed it |
//!
//! The conversation's page itself, `GET /assistant/{id}`, is the app's: it draws
//! [`Assistant::page`]'s parts in its own shell. Apps need `AuthModule`, `AccessModule`,
//! `DatabaseModule`, `RealtimeModule` (the live hub), `SessionModule` and `UiModule`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use navree_db::Db;
use navree_http::{AppError, ModuleBuilderHttpExt, UrlSigner};
use navree_kernel::{BoxFuture, Factory, ModuleBuilder, ProviderKey, Resolver};
use navree_realtime::Live;
use navree_ui::genui::{Catalog, ChatLabels, FrameLabels, Props, SurfaceCx};
use uuid::Uuid;

use crate::agent::{Agent, AgentRun};
use crate::memory::Conversations;
use crate::Ai;

mod host;
mod routes;
mod signing;
pub(crate) mod store;
mod tools;

pub use host::{Acted, Assistant, AssistantPage, PageOptions};
pub use navree_ui::genui;
pub use routes::AssistantController;

/// The live topic of a conversation's page: heard by its owner only.
pub fn topic(conversation: Uuid) -> String {
    format!("assistant.{conversation}")
}

/// A component's code reads the reader's access through the run it is drawn for.
impl SurfaceCx for AgentRun {
    fn can(&self, action: &str, subject: &str) -> bool {
        self.ability().can(action, subject, None)
    }
}

/// What the assistant is told about the record a page is about.
pub trait PageContext: Send + Sync + 'static {
    /// A short description of record `key`, loaded under `run`'s access (a record the
    /// reader can't see is a 404 or a 403, and the message is refused). It reaches the
    /// model as data, never as instructions.
    fn describe<'a>(
        &'a self,
        key: &'a str,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<String, AppError>>;
}

/// How an [`Assistant`] is set up: where its routes are, its catalogue, its words, the
/// pages' contexts it reads and its working state.
#[derive(Clone)]
pub struct AssistantConfig {
    pub(crate) path: String,
    pub(crate) catalog: Option<Arc<Catalog<AgentRun>>>,
    pub(crate) labels: ChatLabels,
    pub(crate) frame_labels: FrameLabels,
    pub(crate) tool_labels: BTreeMap<String, String>,
    pub(crate) flush: Duration,
    pub(crate) contexts: BTreeMap<String, Arc<dyn PageContext>>,
    pub(crate) state: Option<tools::StateSpec>,
}

impl AssistantConfig {
    /// An assistant whose routes are under `path`: `/assistant`.
    ///
    /// # Panics
    ///
    /// When `path` is not `/` and plain segments (letters, digits, `-`, `_`), with no
    /// `/` at its end: a mistake in the app's code, found at boot.
    pub fn at(path: &str) -> Self {
        let plain = path.len() > 1
            && path.starts_with('/')
            && !path.ends_with('/')
            && path[1..].split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            });
        assert!(
            plain,
            "an assistant's path is `/` and plain segments, with no `/` at its end: `{path}`"
        );
        Self {
            path: path.to_owned(),
            catalog: None,
            labels: ChatLabels::default(),
            frame_labels: FrameLabels::default(),
            tool_labels: BTreeMap::new(),
            flush: Duration::from_millis(120),
            contexts: BTreeMap::new(),
            state: None,
        }
    }

    /// The components the model may choose, and their actions.
    pub fn catalog(mut self, catalog: Catalog<AgentRun>) -> Self {
        self.catalog = Some(Arc::new(catalog));
        self
    }

    /// Replaces the conversation's words.
    pub fn labels(mut self, labels: ChatLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Replaces the words around a component.
    pub fn frame_labels(mut self, labels: FrameLabels) -> Self {
        self.frame_labels = labels;
        self
    }

    /// What tool `tool` is called on the page, in its card and its approval:
    /// `Refund an order`. Without one, its name in words (`refund_order` is `Refund
    /// order`).
    pub fn tool_label(mut self, tool: &str, label: impl Into<String>) -> Self {
        self.tool_labels.insert(tool.to_owned(), label.into());
        self
    }

    /// How often the answer's text is written into the page while it streams (120 ms
    /// by default, from 50 ms to 1 s).
    pub fn flush_every(mut self, every: Duration) -> Self {
        self.flush = every.clamp(Duration::from_millis(50), Duration::from_secs(1));
        self
    }

    /// Reads pages about records of `kind` (`order`) with `resolver`.
    pub fn context(mut self, kind: &str, resolver: impl PageContext) -> Self {
        self.contexts.insert(kind.to_owned(), Arc::new(resolver));
        self
    }

    /// Gives each conversation a working state of type `T`, which the model changes
    /// through a checked `update_state` tool and the page shows and edits.
    pub fn state<T: Props>(mut self) -> Self {
        self.state = Some(tools::StateSpec::of::<T>());
        self
    }

    /// The words for tool `tool`.
    pub(crate) fn tool_label(&self, tool: &str) -> String {
        if let Some(label) = self.tool_labels.get(tool) {
            return label.clone();
        }
        let words = tool.replace('_', " ");
        let mut chars = words.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => String::new(),
        }
    }
}

/// Assistant registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderAssistantExt {
    /// Serves agent `A`'s conversations on pages, under `config`'s path: provides
    /// `Assistant<A>` and mounts its routes. Register the agent too, with
    /// `m.agent::<A>()`.
    fn assistant<A: Agent>(&mut self, config: AssistantConfig) -> &mut Self;
}

impl ModuleBuilderAssistantExt for ModuleBuilder {
    fn assistant<A: Agent>(&mut self, config: AssistantConfig) -> &mut Self {
        self.factory(
            Factory::<Assistant<A>>::new(move |r: Resolver| {
                let config = config.clone();
                async move {
                    let ai = r.get::<Ai>().await?;
                    let db = r.get::<Db>().await?;
                    let signer = r.get::<UrlSigner>().await?;
                    let live = r.app().try_get::<Live>().await;
                    if live.is_none() {
                        tracing::warn!(
                            target: "navree::ai",
                            "an assistant without RealtimeModule: its pages change only when drawn again"
                        );
                    }
                    let core = host::Core::new(
                        (*ai).clone(),
                        store::Store::new(db.clone()),
                        Conversations::new(Some(db)),
                        live,
                        signer,
                        config,
                    );
                    Ok(Arc::new(Assistant::<A>::new(Arc::new(core))))
                }
            })
            .deps(vec![
                ProviderKey::of::<Ai>(),
                ProviderKey::of::<Db>(),
                ProviderKey::of::<UrlSigner>(),
            ]),
        )
        .exports::<Assistant<A>>()
        .controller::<AssistantController<A>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_named_in_words() {
        let config = AssistantConfig::at("/assistant").tool_label("refund_order", "Refund an order");
        assert_eq!(config.tool_label("refund_order"), "Refund an order");
        assert_eq!(config.tool_label("list_payments"), "List payments");
    }

    #[test]
    #[should_panic(expected = "an assistant's path")]
    fn a_path_with_a_query_fails_at_boot() {
        let _ = AssistantConfig::at("/assistant?x=1");
    }
}

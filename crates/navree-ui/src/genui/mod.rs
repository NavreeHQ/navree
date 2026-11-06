//! Generative UI: what an assistant may put on a page.
//!
//! The rule everything here serves: **a model never writes markup and never names an
//! action.** It chooses a component from a closed [`Catalog`] the app wrote in Rust, and
//! the values of that component's props from a closed set of types. The server checks
//! every value, loads what the component shows under the reader's own access when the
//! page is drawn, and draws it with the same code as any other part of the page.
//!
//! - [`props`]: the values a model may choose ([`Prop`]): plain text with a most length
//!   ([`UserText`]), whole numbers in a range ([`Int`]), closed choices
//!   ([`choices!`](crate::choices)), record keys ([`Key`]), days ([`Day`]), lists with a
//!   most length ([`List`]), the app's own pages ([`AppLink`]), and structs of them
//!   ([`props!`](crate::props)). A free string, raw HTML, a colour or a list with no
//!   bound does not compile as a prop.
//! - [`Catalog`]: the components a model may choose ([`Surface`]), the tools it is
//!   offered for them, what the system prompt says of them, and the one check their
//!   arguments pass; the actions their buttons may take ([`Action`]).
//! - The parts a conversation is drawn with ([`chat`]): the [`Conversation`] that
//!   holds it, each [`ChatMessage`], the [`ToolCallCard`] of a tool the assistant used,
//!   [`Reasoning`], [`Sources`] and [`Citation`]s, [`Suggestions`], the [`PromptInput`],
//!   and the [`ApprovalCard`] only the page draws. With no script each is a plain link,
//!   form or list; the `genui` runtime module adds Enter to send and keeps a reader who
//!   is at the end of a conversation there while an answer grows.

pub mod catalog;
pub mod chat;
pub mod props;

pub use chat::{
    ApprovalCard, ApprovalState, ChatLabels, ChatMessage, ChatRole, ChatState, Citation,
    Conversation, MAX_MESSAGE, PromptInput, Reasoning, Sources, Suggestions, ToolCallCard,
    ToolState, approval_notice, chat_notice, writing_key,
};
pub use catalog::{
    Action, ActionButton, ActionDone, ActionRefusal, Catalog, CatalogTool, Checked, Confirm,
    Limits, Refusal, RunAction, Surface, SurfaceCx, SurfaceView, TOOL_PREFIX,
};
#[doc(hidden)]
pub use props::__private;
pub use props::{
    AppLink, Checker, Day, Int, Key, LinkPattern, List, MAX_KEY, MAX_LIST, MAX_PROBLEMS, MAX_TEXT,
    Problem, Prop, Props, UserText,
};

/// What a component's code returns from [`Surface::render`] and an action's from
/// [`RunAction::run`].
pub use navree_http::AppError;
/// The future a component draws in.
pub use navree_kernel::BoxFuture;

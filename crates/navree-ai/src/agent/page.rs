//! [`PageTools`]: tools a page adds to a streamed run and answers itself, such as the
//! components a model may choose from a page's catalogue and the conversation's working
//! state. Their calls never reach the agent: the page checks each one and says what the
//! model reads back and what the stream carries.

use navree_access::PrincipalAccess;
use serde_json::Value;

use crate::stream::StreamEvent;

/// A tool a page offers a run.
#[derive(Clone, Debug, PartialEq)]
pub struct PageTool {
    /// Its name, unique among the run's tools.
    pub name: String,
    /// What the model chooses it by.
    pub description: String,
    /// The JSON Schema of its arguments.
    pub parameters: Value,
}

/// What a page answers to a call of one of its tools.
#[derive(Clone, Debug, PartialEq)]
pub enum PageAnswer {
    /// The call passed: the model reads `result`, and the stream carries `event`.
    Accepted {
        /// What the model reads back.
        result: Value,
        /// What the stream carries, for the page and for API clients.
        event: StreamEvent,
    },
    /// The call was refused: the model reads `message` to repair it.
    Refused {
        /// Why, in words a model can repair its call from.
        message: String,
    },
}

/// Tools a page adds to a run (see the module's docs). Only a streamed run offers them:
/// what they make arrives as stream events, which a run that answers once has no place
/// for.
pub trait PageTools: Send + Sync + 'static {
    /// The tools offered to a user with `access`.
    fn tools(&self, access: &PrincipalAccess) -> Vec<PageTool>;

    /// What the run's system prompt says about them.
    fn prompt(&self) -> String;

    /// Answers a call of `tool` with `arguments`.
    fn answer(&self, tool: &str, arguments: &Value) -> PageAnswer;

    /// How many components one answer may show (6 by default); past that a call is
    /// refused and the model told to answer in words.
    fn most_shown(&self) -> usize {
        6
    }

    /// How many refused calls of one tool an answer may make (2 by default); past that
    /// the model is told to stop calling it and answer in words.
    fn most_tries(&self) -> usize {
        2
    }
}

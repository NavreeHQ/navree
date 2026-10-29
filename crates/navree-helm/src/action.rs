//! [`Action`]: work on the records ticked on the list, or on one record.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use navree_access::{ActionMarker, UPDATE};
use navree_http::AppError;
use navree_kernel::BoxFuture;
use navree_orm::{Model, Record};
use serde::{Deserialize, Serialize};

use crate::field::Field;
use crate::{ActionContext, slug};

/// The work of an action.
pub(crate) type Handler<M> = Arc<
    dyn Fn(
            ActionContext,
            Vec<Record<M>>,
        ) -> BoxFuture<'static, Result<ActionResult, AppError>>
        + Send
        + Sync,
>;

/// Where an action's button appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    Everywhere,
    Index,
    Detail,
}

/// An action: a button on the list (for the ticked rows) and on a record's
/// page. It names an ability and runs only on the records the user has
/// that ability on.
///
/// ```ignore
/// Action::new("mark-won", "Mark as won")
///     .ability(Move)
///     .confirm("Mark the selected deals as won?")
///     .handle(|ctx, deals| async move {
///         for mut deal in deals {
///             deal.update(ctx.uow(), |d| d.stage = Stage::Won).await?;
///         }
///         Ok(ActionResult::message("Deals marked as won."))
///     })
/// ```
pub struct Action<M: Model> {
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) ability: &'static str,
    pub(crate) confirm: Option<String>,
    pub(crate) destructive: bool,
    pub(crate) fields: Vec<Field>,
    pub(crate) placement: Placement,
    pub(crate) queued: bool,
    pub(crate) handler: Option<Handler<M>>,
}

impl<M: Model> Action<M> {
    /// An action named `name` (in URLs and the audit log; kebab-case is
    /// best) with the button label `label`.
    pub fn new(name: &str, label: &str) -> Self {
        Self {
            name: slug(name),
            label: label.to_owned(),
            ability: UPDATE,
            confirm: None,
            destructive: false,
            fields: Vec::new(),
            placement: Placement::Everywhere,
            queued: false,
            handler: None,
        }
    }

    /// The action marker each selected record is checked against
    /// (`Update` by default).
    pub fn ability<A: ActionMarker>(mut self, _marker: A) -> Self {
        self.ability = A::NAME;
        self
    }

    /// Asks `text` before running.
    pub fn confirm(mut self, text: &str) -> Self {
        self.confirm = Some(text.to_owned());
        self
    }

    /// Always asks before running, with a red button.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// A small form shown before the action runs; the handler reads the
    /// values with [`ActionContext::input`].
    pub fn fields(mut self, fields: Vec<Field>) -> Self {
        self.fields = fields;
        self
    }

    /// Only on a record's page.
    pub fn only_on_detail(mut self) -> Self {
        self.placement = Placement::Detail;
        self
    }

    /// Only on the list.
    pub fn only_on_index(mut self) -> Self {
        self.placement = Placement::Index;
        self
    }

    /// Runs as a `helm.action` job on the queue (a `dyn Dispatcher`, such as
    /// QueueModule) and answers at once. Without a dispatcher it runs in
    /// the request.
    pub fn queued(mut self) -> Self {
        self.queued = true;
        self
    }

    /// The work: `f(ctx, records)` gets an [`ActionContext`] and the
    /// selected records the user may act on (navree-orm records, read in
    /// the unit of work and locked for update where the database can).
    pub fn handle<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(ActionContext, Vec<Record<M>>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ActionResult, AppError>> + Send + 'static,
    {
        self.handler = Some(Arc::new(move |ctx, records| Box::pin(f(ctx, records))));
        self
    }

    /// The action's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the button shows on the list.
    pub(crate) fn on_index(&self) -> bool {
        self.placement != Placement::Detail
    }

    /// Whether the button shows on a record's page.
    pub(crate) fn on_detail(&self) -> bool {
        self.placement != Placement::Index
    }

    /// The question asked before running, if any.
    pub(crate) fn question(&self) -> Option<String> {
        match (&self.confirm, self.destructive) {
            (Some(text), _) => Some(text.clone()),
            (None, true) => Some(format!("{}?", self.label)),
            (None, false) => None,
        }
    }
}

impl<M: Model> Clone for Action<M> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            label: self.label.clone(),
            ability: self.ability,
            confirm: self.confirm.clone(),
            destructive: self.destructive,
            fields: self.fields.clone(),
            placement: self.placement,
            queued: self.queued,
            handler: self.handler.clone(),
        }
    }
}

impl<M: Model> fmt::Debug for Action<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Action")
            .field("name", &self.name)
            .field("ability", &self.ability)
            .field("queued", &self.queued)
            .finish_non_exhaustive()
    }
}

/// What an action answers: a message to show, a page to go to, or a file to
/// download (for example a temporary URL of an export).
///
/// Over the API it is `{"type": "message", "message": ".."}`,
/// `{"type": "redirect", "url": ".."}` or `{"type": "download", "url": ".."}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionResult {
    /// Shows a message.
    Message {
        /// The text.
        message: String,
    },
    /// Goes to a URL.
    Redirect {
        /// Where.
        url: String,
    },
    /// Starts a download.
    Download {
        /// The file's URL.
        url: String,
    },
}

impl ActionResult {
    /// Shows `text`.
    pub fn message(text: impl Into<String>) -> Self {
        ActionResult::Message {
            message: text.into(),
        }
    }

    /// Goes to `url`.
    pub fn redirect(url: impl Into<String>) -> Self {
        ActionResult::Redirect { url: url.into() }
    }

    /// Downloads `url`.
    pub fn download(url: impl Into<String>) -> Self {
        ActionResult::Download { url: url.into() }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn results_serialize_with_their_type() {
        assert_eq!(
            serde_json::to_value(ActionResult::message("Done.")).unwrap(),
            json!({ "type": "message", "message": "Done." })
        );
        assert_eq!(
            serde_json::to_value(ActionResult::download("/files/x.csv")).unwrap(),
            json!({ "type": "download", "url": "/files/x.csv" })
        );
    }
}

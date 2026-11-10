//! The forms of the dashboard: pause and continue, retry and forget. Each
//! posts, does its work through [`Fleet`] and comes back to the page it was
//! sent from with a message. The same routes answer the runtime with the
//! message as a toast and the page redrawn, and a browser without script with
//! a redirect.

use navree_http::AppError;
use navree_http::response::{IntoResponse, Response};
use navree_ui::pages::ReturnTo;
use navree_ui::{LocalHref, Reply, ToastTone};
use serde::Deserialize;

use super::format::number;
use crate::Fleet;
use crate::dashboard::api_error;

/// What the forms post.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Params {
    /// The supervisor to pause or continue; empty for all.
    pub(crate) supervisor: Option<String>,
    /// For "retry all": only the failed jobs this text matches.
    pub(crate) search: Option<String>,
    /// The page to come back to.
    pub(crate) next: Option<String>,
}

impl Params {
    fn supervisor(&self) -> Option<&str> {
        self.supervisor.as_deref().filter(|name| !name.is_empty())
    }

    fn search(&self) -> Option<&str> {
        self.search.as_deref().filter(|text| !text.is_empty())
    }
}

/// Where the form sends the user back to: its `next` when that is a page of
/// this site, else `fallback`.
fn back(form: &Params, fallback: &str) -> LocalHref {
    ReturnTo::resolve(form.next.as_deref(), fallback)
}

/// Pauses or continues one supervisor, or all of them.
pub(crate) async fn supervisors(
    fleet: &Fleet,
    base: &str,
    form: &Params,
    pause: bool,
) -> Result<Response, AppError> {
    let name = form.supervisor();
    let result = if pause {
        fleet.pause(name).await
    } else {
        fleet.resume(name).await
    };
    result.map_err(api_error)?;
    let message = match (name, pause) {
        (Some(name), true) => format!("{name} is paused."),
        (Some(name), false) => format!("{name} continues."),
        (None, true) => "Every supervisor is paused.".to_owned(),
        (None, false) => "Every supervisor continues.".to_owned(),
    };
    Ok(Reply::redirect(back(form, base))
        .toast(ToastTone::Success, message)
        .into_response())
}

/// Puts one failed job back on its queue.
pub(crate) async fn retry(
    fleet: &Fleet,
    base: &str,
    id: &str,
    form: &Params,
) -> Result<Response, AppError> {
    fleet.retry(id).await.map_err(api_error)?;
    Ok(Reply::redirect(back(form, &format!("{base}/failed")))
        .toast(ToastTone::Success, "The job is back on its queue.")
        .into_response())
}

/// Forgets one failed job.
pub(crate) async fn forget(
    fleet: &Fleet,
    base: &str,
    id: &str,
    form: &Params,
) -> Result<Response, AppError> {
    fleet.forget(id).await.map_err(api_error)?;
    Ok(Reply::redirect(back(form, &format!("{base}/failed")))
        .toast(ToastTone::Success, "The job is forgotten.")
        .into_response())
}

/// Puts every failed job, or those a search matches, back on their queues.
pub(crate) async fn retry_all(
    fleet: &Fleet,
    base: &str,
    form: &Params,
) -> Result<Response, AppError> {
    let retried = fleet.retry_all(form.search()).await.map_err(api_error)?;
    let message = match retried {
        0 => "No failed job matched.".to_owned(),
        1 => "The failed job is back on its queue.".to_owned(),
        n => format!("{} failed jobs are back on their queues.", number(n)),
    };
    Ok(Reply::redirect(back(form, &format!("{base}/failed")))
        .toast(ToastTone::Success, message)
        .into_response())
}

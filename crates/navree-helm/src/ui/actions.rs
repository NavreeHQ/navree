//! Actions on records: an action's form or question before it runs, its run on
//! the rows ticked, and deleting a record after a question.

use http::{HeaderMap, StatusCode};
use navree_http::extract::Request;
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Path};
use navree_ui::described::DescribedForm;
use navree_ui::{Button, Card, Stack, Text, TextVariant, ToastTone};

use super::forms::{self, FormSpec};
use super::frame::Frame;
use super::{form_input, page_error, redirect_to, redirect_with, return_url, submission};
use crate::ActionResult;
use crate::api::resource;
use crate::field::Field;
use crate::handle::{ActionInfo, ResourceInfo};
use crate::panel::Panel;
use crate::present::encode;

/// `POST {path}/resources/{resource}/{id}/delete`: deletes a record, after a page
/// that asks first when the form did not say it was asked already.
pub(crate) async fn destroy(
    panel: Panel,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        let info = resource.info();
        if info.read_only {
            return Err(AppError::forbidden());
        }
        let submission = submission(cx, info, request, false).await?;
        let base = cx.core.options.base();
        let list_url = format!("{base}/resources/{}", info.slug);
        if submission.meta.get("confirmed").map(String::as_str) != Some("1") {
            let record = resource
                .find(cx, &id)
                .await?
                .ok_or_else(AppError::not_found)?;
            let item_url = format!("{list_url}/{}", encode(&record.id));
            let frame = Frame::new(&panel, &headers, item_url.clone());
            let label = format!("Delete {}", info.singular.to_lowercase());
            let mut form = DescribedForm::new("helm-delete")
                .view()
                .post(format!("{item_url}/delete"))
                .hidden("_confirmed", "1")
                .submit(label.clone())
                .cancel("Cancel", item_url.clone());
            if let Some(token) = frame.csrf() {
                form = form.csrf(token.to_owned());
            }
            let page = frame
                .page(label)
                .await
                .crumb(cx.core.options.title.clone(), base.to_owned())
                .crumb(info.label, list_url.clone())
                .crumb(record.title.clone(), item_url)
                .body(Card::new().body((
                    Text::new(format!("Delete {}?", record.title)).variant(TextVariant::Lead),
                    form,
                )));
            return Ok(page.into_response());
        }
        resource.delete(cx, &id).await?;
        Ok(redirect_with(
            &list_url,
            &format!("{} deleted.", info.singular),
            ToastTone::Success,
        ))
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// `POST {path}/resources/{resource}/actions`: runs the action named by the
/// pressed button on the ticked rows; shows its form or its question first when
/// it has one.
pub(crate) async fn run_action(
    panel: Panel,
    Path(slug): Path<String>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let core = panel.cx.core.clone();
    action_run(panel, slug, headers, request)
        .await
        .unwrap_or_else(|error| page_error(&core, error))
}

/// What the action page shows.
struct ActionPage<'a> {
    info: &'a ResourceInfo,
    action: &'a ActionInfo,
    submission: &'a super::Submission,
    back: &'a str,
}

/// The action's form (its fields and its question) for the ticked rows.
async fn action_form(
    frame: &Frame<'_>,
    page: &ActionPage<'_>,
    status: StatusCode,
    error: Option<&AppError>,
) -> Result<Response, AppError> {
    let ActionPage {
        info,
        action,
        submission,
        back,
    } = page;
    let mut errors = error
        .and_then(|e| e.errors.as_deref().cloned())
        .unwrap_or_default();
    if let Some(error) = error.filter(|e| e.errors.is_none()) {
        errors.insert(
            "_form".to_owned(),
            vec![error.detail.clone().unwrap_or_else(|| error.title.clone())],
        );
    }
    let submitted = error.is_some().then_some(&submission.values);
    let fields: Vec<&Field> = action.fields.iter().collect();
    let (view, _) = forms::view(
        frame.cx,
        FormSpec {
            id: "helm-action",
            fields: &fields,
            record: None,
            shown: None,
            submitted,
            errors: &errors,
            locked: &[],
        },
    )
    .await?;
    let base = frame.base();
    let list_url = format!("{base}/resources/{}", info.slug);
    let mut view = view
        .post(format!("{list_url}/actions"))
        .hidden("_return", back.to_owned())
        .hidden("_confirmed", "1")
        .hidden("_step", "run")
        .hidden("action", action.name.clone());
    for id in &submission.ids {
        view = view.hidden("ids", id.clone());
    }
    view = view
        .submit(action.label.clone())
        .cancel("Cancel", back.to_owned());
    if let Some(token) = frame.csrf() {
        view = view.csrf(token.to_owned());
    }
    let count = submission.ids.len();
    let lead = match &action.question {
        Some(question) => question.clone(),
        None => format!(
            "Run “{}” on {count} {}.",
            action.label,
            if count == 1 { "record" } else { "records" }
        ),
    };
    let page = frame
        .page(action.label.clone())
        .await
        .crumb(frame.core().options.title.clone(), base.to_owned())
        .crumb(info.label, list_url)
        .status(status)
        .body(Card::new().body((Text::new(lead).variant(TextVariant::Lead), view)));
    Ok(page.into_response())
}

async fn action_run(
    mut panel: Panel,
    slug: String,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let resource = resource(&panel.cx, &slug)?.clone();
    let info = resource.info();
    let submission = submission(&panel.cx, info, request, false).await?;
    panel.cx.tz_offset = submission.tz_offset();
    let base = panel.cx.core.options.base().to_owned();
    let list_url = format!("{base}/resources/{}", info.slug);
    let back = return_url(&submission, &base, &list_url);
    let frame = Frame::new(&panel, &headers, back.clone());
    let name = submission.values.get("action").cloned().unwrap_or_default();
    if name.is_empty() {
        return Ok(redirect_with(
            &back,
            "Choose an action first.",
            ToastTone::Warning,
        ));
    }
    let Some(action) = info.actions.iter().find(|a| a.name == name) else {
        return Err(AppError::not_found().with_detail(format!("No action `{name}`.")));
    };
    if submission.ids.is_empty() {
        return Ok(redirect_with(
            &back,
            "Select at least one row first.",
            ToastTone::Warning,
        ));
    }
    let page = ActionPage {
        info,
        action,
        submission: &submission,
        back: &back,
    };
    let step = submission.meta.get("step").map(String::as_str);
    let confirmed = submission.meta.get("confirmed").map(String::as_str) == Some("1");
    if (!action.fields.is_empty() && step != Some("run"))
        || (action.question.is_some() && !confirmed)
    {
        return action_form(&frame, &page, StatusCode::OK, None).await;
    }
    let fields: Vec<&Field> = action.fields.iter().collect();
    let input = form_input(&fields, &submission, None, frame.cx.zone);
    let outcome = match resource
        .run_action(frame.cx, &name, submission.ids.clone(), input, false)
        .await
    {
        Ok(outcome) => outcome,
        Err(error) if error.status == StatusCode::BAD_REQUEST && !action.fields.is_empty() => {
            return action_form(&frame, &page, StatusCode::BAD_REQUEST, Some(&error)).await;
        }
        Err(error) if error.status == StatusCode::FORBIDDEN => {
            return Ok(redirect_with(
                &back,
                error
                    .detail
                    .as_deref()
                    .unwrap_or("You may not run this action."),
                ToastTone::Danger,
            ));
        }
        Err(error) => return Err(error),
    };
    let skipped = if outcome.skipped > 0 {
        format!(
            " {} skipped: you may not run it on {}.",
            outcome.skipped,
            if outcome.skipped == 1 {
                "that record"
            } else {
                "those records"
            }
        )
    } else {
        String::new()
    };
    match outcome.result {
        ActionResult::Message { message } => Ok(redirect_with(
            &back,
            &format!("{message}{skipped}"),
            if outcome.skipped > 0 {
                ToastTone::Warning
            } else {
                ToastTone::Success
            },
        )),
        ActionResult::Redirect { url } | ActionResult::Download { url }
            if url.starts_with('/') && !url.starts_with("//") =>
        {
            Ok(redirect_to(&url))
        }
        ActionResult::Redirect { url } | ActionResult::Download { url } => {
            let page = frame
                .page(action.label.clone())
                .await
                .crumb(frame.core().options.title.clone(), base.clone())
                .crumb(info.label, list_url)
                .body(
                    Card::new().body(
                        Stack::gap(4).children((
                            Text::new(format!("{} is done.{skipped}", action.label))
                                .variant(TextVariant::Lead),
                            Button::primary("Open")
                                .href(url)
                                .attr("rel", "noopener noreferrer"),
                            Button::ghost("Back").href(back.clone()),
                        )),
                    ),
                );
            Ok(page.into_response())
        }
    }
}

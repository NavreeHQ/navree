//! The panel's pages, drawn with Navree UI in a shell of their own: the menu
//! from the registry, the dashboard and its metric cards, the lists, a record's
//! page, its forms and the actions. The same pages work with JavaScript off;
//! the toolkit's runtime only makes them quicker.

use std::collections::{BTreeMap, HashMap};
use std::error::Error as StdError;

use http::StatusCode;
use http::header::{CONTENT_TYPE, SET_COOKIE};
use navree_access::PinnedWorkspace;
use navree_crypto::Encrypter;
use navree_http::AppError;
use navree_http::body::to_bytes;
use navree_http::extract::{FromRequest, Multipart, Request};
use navree_http::response::{IntoResponse, Response};
use navree_storage::{Storage, UploadRules, store_upload};
use navree_ui::{ErrorPage, Href, Reply, ToastTone};
use serde_json::{Map, Value};

use crate::core::Core;
use crate::cx::Cx;
use crate::field::{Field, FieldKind};
use crate::handle::ResourceInfo;
use crate::money;
use crate::panel::PanelOutside;
use crate::session::{build_cookie, clear_cookie, request_host};
use crate::workspace::{self, WORKSPACE_COOKIE, WORKSPACE_COOKIE_SECS};

mod actions;
mod cells;
mod forms;
mod frame;
mod list;
mod metrics;
mod record;
/// The Settings screens (feature `settings`).
#[cfg(feature = "settings")]
pub(crate) mod settings;

pub(crate) use actions::{destroy, run_action};
pub(crate) use frame::{counted_in, menu_fragment};
pub(crate) use list::{choices, dashboard, list, screen_query};
pub(crate) use record::{create, custom_page, edit_form, new_form, show, update};

// ---------------------------------------------------------------------------
// Errors and redirects
// ---------------------------------------------------------------------------

/// The most of an error's own message an error page shows.
const MESSAGE_CHARS: usize = 300;

/// A standalone error page, drawn by the toolkit like every other error page
/// of the app: its words for the status, `message` (what the error says, when
/// it says something the person can act on) under them, and a way back to the
/// panel.
pub(crate) fn status_page(core: &Core, status: StatusCode, message: Option<String>) -> Response {
    let mut page = ErrorPage::new(status).home(Href::from(core.options.base()));
    if let Some(message) = message {
        let message = message.trim();
        if !message.is_empty() {
            page = page.detail(message.chars().take(MESSAGE_CHARS).collect::<String>());
        }
    }
    page.into_response()
}

/// An error from the panel's own code as a page: database errors mapped
/// like the API maps them, server errors logged.
pub(crate) fn page_error(core: &Core, error: AppError) -> Response {
    let error = map_db(error);
    if error.status.is_server_error() {
        tracing::error!(
            target: "navree::helm",
            status = error.status.as_u16(),
            title = %error.title,
            detail = error.detail.as_deref().unwrap_or(""),
            source = error.source.as_ref().map(|s| s.to_string()).unwrap_or_default(),
            "a panel page failed"
        );
        return status_page(core, error.status, None);
    }
    let message = match &error.errors {
        Some(errors) if !errors.is_empty() => Some(
            errors
                .values()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => error.detail.clone(),
    };
    let mut page = status_page(core, error.status, message);
    for (name, value) in error.headers.iter() {
        page.headers_mut().insert(name.clone(), value.clone());
    }
    page
}

/// A 500 caused by a database error becomes the status the API gives it
/// (404 for a missing record, 409 for a conflict).
fn map_db(error: AppError) -> AppError {
    if error.status != StatusCode::INTERNAL_SERVER_ERROR {
        return error;
    }
    let mut next: Option<&(dyn StdError + 'static)> = error.source.as_deref().map(|e| e as _);
    while let Some(cause) = next {
        if let Some(db) = cause.downcast_ref::<navree_db::DbError>()
            && let Some(mapped) = navree_db::map_db_error(db)
        {
            return mapped;
        }
        next = cause.source();
    }
    error
}

/// Goes to `location` on this site, with a toast saying `text`: the runtime shows
/// it at once, and with JavaScript off the next page does.
fn redirect_with(location: &str, text: &str, tone: ToastTone) -> Response {
    Reply::redirect(location).toast(tone, text).into_response()
}

/// Goes to `location` on this site, saying nothing.
fn redirect_to(location: &str) -> Response {
    Reply::redirect(location).into_response()
}

// ---------------------------------------------------------------------------
// The workspace a browser works in
// ---------------------------------------------------------------------------

/// `POST {path}/workspace`: works in the workspace named by the form's
/// `workspace` field, if the user may, for as long as the sign-in it was chosen
/// in (a cookie sealed to that sign-in), or in none when it is empty.
pub(crate) async fn choose_workspace(
    PanelOutside(panel): PanelOutside,
    request: Request,
) -> Response {
    let core = panel.cx.core.clone();
    let base = core.options.base().to_owned();
    let host = request_host(&request);
    let body = to_bytes(request.into_body(), 16 * 1024)
        .await
        .unwrap_or_default();
    let chosen = form_urlencoded::parse(&body)
        .find(|(name, _)| name == "workspace")
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default();
    // The workspace of a request that is inside one is not the panel's to
    // change.
    if panel.cx.uow.workspace().is_some() {
        return redirect_with(
            &base,
            "This page is inside a workspace already.",
            ToastTone::Warning,
        );
    }
    if chosen.is_empty() {
        let mut response = redirect_to(&base);
        if let Some(header) = clear_cookie(WORKSPACE_COOKIE, &base, &host) {
            response.headers_mut().append(SET_COOKIE, header);
        }
        return response;
    }
    let Ok(workspace) = uuid::Uuid::parse_str(&chosen) else {
        return redirect_with(&base, "That is not a workspace id.", ToastTone::Danger);
    };
    if panel.cx.pin.is_some_and(|pin| pin != workspace) {
        let error = PinnedWorkspace::mismatch();
        let message = error.detail.as_deref().unwrap_or(&error.title);
        return redirect_with(&base, message, ToastTone::Danger);
    }
    let admitted = workspace::admission(
        &panel.cx.app,
        &core,
        panel.cx.user_id,
        &panel.cx.access,
        workspace,
        false,
    )
    .await;
    let refusal = match admitted {
        Ok(Ok(_)) => None,
        Ok(Err(refused)) => Some(refused.error()),
        Err(error) => Some(error),
    };
    if let Some(error) = refusal {
        let message = error.detail.as_deref().unwrap_or(&error.title).to_owned();
        return redirect_with(&base, &message, ToastTone::Danger);
    }
    let value = match panel.sign_in_token() {
        Some(sign_in) => match panel.cx.app.try_get::<Encrypter>().await {
            Some(encrypter) => workspace::sealed(&encrypter, workspace, sign_in),
            None => None,
        },
        None => Some(workspace.to_string()),
    };
    let Some(value) = value else {
        return redirect_with(&base, "The workspace could not be kept.", ToastTone::Danger);
    };
    let mut response = redirect_to(&base);
    if let Some(header) = build_cookie(WORKSPACE_COOKIE, value, &base, WORKSPACE_COOKIE_SECS, &host)
    {
        response.headers_mut().append(SET_COOKIE, header);
    }
    response
}

// ---------------------------------------------------------------------------
// What a form posted
// ---------------------------------------------------------------------------

/// What a form submitted: the values (last per name, `_`-prefixed names
/// dropped), every value of `ids`, and the internal fields.
#[derive(Debug, Default)]
struct Submission {
    values: HashMap<String, String>,
    ids: Vec<String>,
    meta: HashMap<String, String>,
    /// Files stored from a multipart form: field → path.
    files: HashMap<String, String>,
    /// Uploads refused (wrong type, too large, empty): field → messages.
    upload_errors: BTreeMap<String, Vec<String>>,
}

impl Submission {
    fn push(&mut self, name: String, value: String) {
        if name == "ids" || name == "ids[]" {
            self.ids.push(value);
        } else if let Some(meta) = name.strip_prefix('_') {
            self.meta.insert(meta.to_owned(), value);
        } else {
            self.values.insert(name, value);
        }
    }

    fn tz_offset(&self) -> Option<i32> {
        self.meta
            .get("tz_offset")
            .and_then(|v| v.trim().parse().ok())
    }
}

/// Reads a urlencoded or multipart form. With `uploads` (the create and
/// edit forms, after their ability checks), files sent for the resource's
/// file fields are stored on their disks; any other file is ignored.
async fn submission(
    cx: &Cx,
    info: &ResourceInfo,
    request: Request,
    uploads: bool,
) -> Result<Submission, AppError> {
    let multipart = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("multipart/form-data"));
    let mut submission = Submission::default();
    if !multipart {
        let bytes = to_bytes(request.into_body(), 16 * 1024 * 1024)
            .await
            .map_err(|_| AppError::bad_request("The form could not be read."))?;
        for (name, value) in form_urlencoded::parse(&bytes).into_owned() {
            submission.push(name, value);
        }
        return Ok(submission);
    }
    let mut form = Multipart::from_request(request)
        .await
        .map_err(|_| AppError::bad_request("The form could not be read."))?;
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("The form could not be read."))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        let file_field = info
            .field(&name)
            .filter(|f| uploads && matches!(f.kind, FieldKind::File { .. }));
        if file_field.is_none() && field.file_name().is_some() {
            continue;
        }
        match file_field {
            Some(spec) => {
                if field.file_name().is_none_or(str::is_empty) {
                    continue;
                }
                let FieldKind::File { disk } = &spec.kind else {
                    continue;
                };
                let storage =
                    cx.app.get::<Storage>().await.map_err(|_| {
                        AppError::internal("file fields need StorageModule in the app")
                    })?;
                let disk = storage.disk(disk).map_err(AppError::internal)?;
                let rules = UploadRules::new(spec.max_size, spec.accept.clone());
                match store_upload(field, &disk, &info.slug, rules).await {
                    Ok(stored) => {
                        submission.files.insert(name, stored.path);
                    }
                    // A refused upload is shown under its field, with the
                    // rest of the form kept.
                    Err(error)
                        if matches!(
                            error.status,
                            StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
                        ) =>
                    {
                        let message = error
                            .errors
                            .as_deref()
                            .and_then(|errors| errors.get(&name))
                            .and_then(|messages| messages.first().cloned())
                            .or_else(|| error.detail.clone())
                            .unwrap_or_else(|| error.title.clone());
                        // The storage names the field by its form name.
                        let message = match message.strip_prefix(name.as_str()) {
                            Some(rest) => format!("{}{rest}", spec.label_text()),
                            None => message,
                        };
                        submission
                            .upload_errors
                            .entry(name)
                            .or_default()
                            .push(message);
                    }
                    Err(error) => return Err(error),
                }
            }
            None => {
                let value = field
                    .text()
                    .await
                    .map_err(|_| AppError::bad_request("The form could not be read."))?;
                submission.push(name, value);
            }
        }
    }
    Ok(submission)
}

/// Deletes what a form that failed stored on its disks: no record points to it,
/// and the form shown again asks for the file again.
async fn discard_uploads(cx: &Cx, info: &ResourceInfo, submission: &Submission) {
    if submission.files.is_empty() {
        return;
    }
    let Some(storage) = cx.app.try_get::<Storage>().await else {
        return;
    };
    for (name, path) in &submission.files {
        let Some(FieldKind::File { disk }) = info.field(name).map(|f| &f.kind) else {
            continue;
        };
        let deleted = match storage.disk(disk) {
            Ok(disk) => disk
                .delete(path)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = deleted {
            tracing::warn!(target: "navree::helm", %error, field = %name, "could not delete an upload of a form that failed");
        }
    }
}

/// The JSON input of a form submission for `fields` (money converted to
/// minor units, uploads to their paths). A switch that is off posts nothing, so in
/// a post of one of the panel's forms (it says which in `_form`) a boolean field
/// left out is `false`.
fn form_input(
    fields: &[&Field],
    submission: &Submission,
    record: Option<&Value>,
    zone: navree_time::Zone,
) -> Map<String, Value> {
    let mut input = Map::new();
    for field in fields {
        if let Some(path) = submission.files.get(&field.name) {
            input.insert(field.name.clone(), Value::String(path.clone()));
            continue;
        }
        if matches!(field.kind, FieldKind::Boolean)
            && submission.meta.contains_key("form")
            && !submission.values.contains_key(&field.name)
        {
            input.insert(field.name.clone(), Value::String("false".to_owned()));
            continue;
        }
        let Some(text) = submission.values.get(&field.name) else {
            continue;
        };
        if matches!(field.kind, FieldKind::File { .. }) {
            continue;
        }
        let value = match &field.kind {
            FieldKind::Money { currency_column } => {
                let currency = submission
                    .values
                    .get(currency_column)
                    .cloned()
                    .or_else(|| {
                        record
                            .and_then(|r| r.get(currency_column))
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "USD".to_owned());
                match money::parse_major(text, &currency) {
                    Some(cents) => Value::from(cents),
                    None => Value::String(text.clone()),
                }
            }
            // Typed on the reader's clock, which the form showed it on, unless the browser
            // said its own offset.
            FieldKind::DateTime if submission.tz_offset().is_none() => {
                Value::String(wall_to_utc(text, zone).unwrap_or_else(|| text.clone()))
            }
            _ => Value::String(text.clone()),
        };
        input.insert(field.name.clone(), value);
    }
    input
}

/// A date and time typed with no zone (`2026-10-26T17:30`), read on `zone`'s clock, as the
/// moment in UTC (`2026-10-26T12:00:00Z`); `None` for anything else, which is read as it is.
fn wall_to_utc(text: &str, zone: navree_time::Zone) -> Option<String> {
    let text = text.trim();
    let wall = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"]
        .iter()
        .find_map(|format| chrono::NaiveDateTime::parse_from_str(text, format).ok())?;
    Some(
        zone.to_utc(wall)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

/// Where to go back to after an action: `_return`, else the table's own page
/// (`_path` and its state `_q`), when it is a page of the panel.
fn return_url(submission: &Submission, base: &str, fallback: &str) -> String {
    let table = submission.meta.get("path").map(|path| {
        match submission.meta.get("q").filter(|q| !q.is_empty()) {
            Some(q) => format!("{path}?{q}"),
            None => path.clone(),
        }
    });
    match submission.meta.get("return").cloned().or(table) {
        Some(url)
            if (url.starts_with(&format!("{base}/")) || url == base)
                && !url.contains("//")
                && !url.contains('\\') =>
        {
            url
        }
        _ => fallback.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submissions_split_values_ids_and_internal_fields() {
        let mut s = Submission::default();
        for (k, v) in [
            ("title", "A"),
            ("ids", "1"),
            ("ids", "2"),
            ("_csrf", "x"),
            ("_tz_offset", "-120"),
            ("title", "B"),
        ] {
            s.push(k.to_owned(), v.to_owned());
        }
        assert_eq!(s.values["title"], "B");
        assert_eq!(s.ids, ["1", "2"]);
        assert_eq!(s.tz_offset(), Some(-120));
    }

    #[test]
    fn a_date_time_typed_into_a_form_is_read_on_the_readers_clock() {
        let kolkata = navree_time::Zone::parse("Asia/Kolkata").unwrap();
        let at = Field::datetime("closed_at");
        let mut s = Submission::default();
        s.push("closed_at".into(), "2026-10-26T17:30".into());
        let input = form_input(&[&at], &s, None, kolkata);
        assert_eq!(input["closed_at"], "2026-10-26T12:00:00Z");
        // In an app without a zone, the clock is UTC's.
        let input = form_input(&[&at], &s, None, navree_time::Zone::UTC);
        assert_eq!(input["closed_at"], "2026-10-26T17:30:00Z");
        // A moment that names its zone, or a browser that says its offset, is read as it says.
        s.push("closed_at".into(), "2026-10-26T17:30:00+05:30".into());
        assert_eq!(
            form_input(&[&at], &s, None, kolkata)["closed_at"],
            "2026-10-26T17:30:00+05:30"
        );
        s.push("closed_at".into(), "2026-10-26T17:30".into());
        s.push("_tz_offset".into(), "-120".into());
        assert_eq!(
            form_input(&[&at], &s, None, kolkata)["closed_at"],
            "2026-10-26T17:30"
        );
    }

    #[test]
    fn an_action_goes_back_to_the_page_it_came_from() {
        let mut s = Submission::default();
        assert_eq!(
            return_url(&s, "/admin", "/admin/resources/deals"),
            "/admin/resources/deals"
        );
        s.push("_path".into(), "/admin/resources/deals".into());
        s.push("_q".into(), "page=2&sort=-title".into());
        assert_eq!(
            return_url(&s, "/admin", "/x"),
            "/admin/resources/deals?page=2&sort=-title"
        );
        s.push("_return".into(), "/admin/resources/deals/7".into());
        assert_eq!(return_url(&s, "/admin", "/x"), "/admin/resources/deals/7");
        s.push("_return".into(), "//evil.example/admin".into());
        assert_eq!(return_url(&s, "/admin", "/x"), "/x");
    }

    #[test]
    fn a_switch_left_off_is_false() {
        let fields = [Field::boolean("priority"), Field::text("title")];
        let fields: Vec<&Field> = fields.iter().collect();
        let mut s = Submission::default();
        s.push("title".into(), "Gulati renewal".into());
        // A post that is not one of the panel's forms changes only what it names.
        assert!(!form_input(&fields, &s, None, navree_time::Zone::UTC).contains_key("priority"));
        s.push("_form".into(), "helm-edit".into());
        let input = form_input(&fields, &s, None, navree_time::Zone::UTC);
        assert_eq!(input["priority"], Value::String("false".into()));
        assert_eq!(input["title"], Value::String("Gulati renewal".into()));
        s.push("priority".into(), "true".into());
        assert_eq!(
            form_input(&fields, &s, None, navree_time::Zone::UTC)["priority"],
            Value::String("true".into())
        );
    }
}

//! A list as a CSV file: `GET {path}/resources/{resource}/export.csv`, with the
//! list's search, filters and sort in its query.
//!
//! The first chunk of rows is read in the request, so a refusal or a bad filter
//! answers as a page; the rest are read as the response is sent, a chunk at a
//! time, each in a unit of work of its own that reads the user's access again and
//! checks again that they may work in the workspace: a role, a permission or a
//! membership taken away ends the file. So does `helm.export_max_secs`, however
//! slowly the client reads. No more than a chunk is ever held, and no connection
//! is held while the client reads.
//!
//! The audit log gets a `helm.export` entry when the file starts, and a
//! `helm.export.finished` entry with the rows sent and how it ended (`finished`,
//! `stopped`, `time_limit`, or `abandoned` by a client that went).

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::stream;
use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use http::{HeaderMap, Method, StatusCode};
use navree_access::AbilityFactory;
use navree_contracts::{AuditEntry, AuditKind, Principal};
use navree_db::Db;
use navree_http::body::{Body, Bytes};
use navree_http::response::Response;
use navree_http::{AppError, Path, RawQuery};
use navree_kernel::{AppContext, BoxError};
use serde_json::json;
use uuid::Uuid;

use crate::api::resource;
use crate::core::Core;
use crate::cx::{Client, Cx, record_outside};
use crate::handle::{ErasedResource, ResourceInfo};
use crate::panel::Panel;
use crate::present::{Display, Item, Screen, present, screen_fields};
use crate::query::ListQuery;
use crate::ui::page_error;
use crate::workspace::{self, NamedBy, Refused};
use crate::{HELM_SUBJECT, VIEW};

/// How many rows one chunk reads.
const CHUNK: u64 = 500;

/// What a file starts with, for a spreadsheet to read it as UTF-8 and not as the
/// system's code page.
const BYTE_ORDER_MARK: char = '\u{feff}';

// ---------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------

/// `text` as a field of a CSV line (RFC 4180): quoted when it has a comma, a
/// quote or a line break. A text a spreadsheet would run as a formula (it
/// starts with `=`, `+`, `-`, `@`, a tab or a return, and is not a number, with or
/// without a currency symbol and thousands separators) gets a quote in front, so it
/// opens as the text it is.
pub(crate) fn field(text: &str) -> String {
    let guarded = match text.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') if !is_number(text) => format!("'{text}"),
        _ => text.to_owned(),
    };
    if guarded.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// Whether `text` is a number as a person or the panel writes one (`-5`, `+3.25`,
/// `-$1,234.60`, `-1,234`), which a spreadsheet reads as one and not as a formula.
fn is_number(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let digits = digits
        .strip_prefix(['$', '\u{20ac}', '\u{a3}', '\u{a5}', '\u{20b9}'])
        .unwrap_or(digits);
    let mut parts = digits.splitn(2, '.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    whole.starts_with(|c: char| c.is_ascii_digit())
        && whole.bytes().all(|b| b.is_ascii_digit() || b == b',')
        && fraction.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
}

/// One CSV line of `cells`, ended by a CRLF.
pub(crate) fn line<I, S>(cells: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = cells
        .into_iter()
        .map(|cell| field(cell.as_ref()))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str("\r\n");
    out
}

/// The header line: the label of every field of a record's page, less any with
/// [`hide_on_export`](crate::Field::hide_on_export).
pub(crate) fn header(info: &ResourceInfo) -> String {
    line(
        screen_fields(info, Screen::Export)
            .into_iter()
            .map(|f| f.label_text()),
    )
}

/// The line of `item`: what the user may read of it, as text. Dates are
/// ISO 8601 and long text is whole, so the file can be read back; a field
/// the user's rules hide is empty.
pub(crate) fn row(item: &Item) -> String {
    line(item.cells.iter().map(|cell| {
        match &cell.display {
            Display::Hidden | Display::Empty => String::new(),
            Display::Time {
                iso,
                date_only: false,
                ..
            } => chrono::DateTime::parse_from_rfc3339(iso)
                .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                .unwrap_or_else(|_| iso.clone()),
            Display::Time { iso, .. } => iso.clone(),
            display => match (cell.kind, item.values.get(&cell.name)) {
                ("textarea" | "markdown", Some(serde_json::Value::String(text))) => text.clone(),
                _ => display.text(),
            },
        }
    }))
}

/// `deals-2026-11-02.csv`.
pub(crate) fn filename(info: &ResourceInfo, day: chrono::NaiveDate) -> String {
    format!("{}-{}.csv", info.slug, day.format("%Y-%m-%d"))
}

// ---------------------------------------------------------------------------
// The response
// ---------------------------------------------------------------------------

/// What the rest of an export needs, to read a chunk as the user in the
/// workspace the request worked in.
struct Plan {
    app: AppContext,
    core: Arc<Core>,
    slug: String,
    query: ListQuery,
    user_id: Uuid,
    principal: Principal,
    workspace: Option<Uuid>,
    named_by: Option<NamedBy>,
    pin: Option<Uuid>,
    request_id: Option<String>,
}

impl Plan {
    /// The text of the chunk after `cursor`, its rows and where the next one
    /// starts, read in a unit of work of its own with the user's access as it is
    /// now.
    async fn chunk(&self, cursor: Option<String>) -> Result<Chunk, AppError> {
        let handle = self
            .core
            .registry
            .by_slug(&self.slug)
            .ok_or_else(AppError::not_found)?;
        // Their role may have gone since the last chunk, and with it the panel.
        let factory = self
            .app
            .get::<AbilityFactory>()
            .await
            .map_err(AppError::internal)?;
        let access = factory.access_for(Some(&self.principal)).await?;
        if !access.ability.can(VIEW, HELM_SUBJECT, None) {
            return Err(AppError::forbidden());
        }
        let db = self.app.get::<Db>().await.map_err(AppError::internal)?;
        let uow = db.uow(Some(self.user_id), self.request_id.clone()).await;
        if let Some(workspace) = self.workspace {
            // The user may have left the workspace since the last chunk.
            workspace::admission(
                &self.app,
                &self.core,
                self.user_id,
                &access,
                workspace,
                false,
            )
            .await?
            .map_err(Refused::error)?;
            workspace::enter_in(&self.app, &uow, Some(workspace)).await?;
        }
        let cx = Cx {
            app: self.app.clone(),
            uow: uow.clone(),
            access,
            user_id: self.user_id,
            core: self.core.clone(),
            client: Client::default(),
            tz_offset: None,
            zone: navree_time::Zone::UTC,
            named_by: self.named_by,
            pin: self.pin,
            writes: None,
            refused_write: None,
        };
        let read = uow.scope(read_chunk(&cx, handle.as_ref(), &self.query, cursor));
        match read.await {
            Ok(chunk) => {
                uow.commit().await.map_err(AppError::internal)?;
                Ok(chunk)
            }
            Err(error) => {
                let _ = uow.rollback().await;
                Err(error)
            }
        }
    }
}

/// One chunk of a file: its text, its rows and where the next starts.
struct Chunk {
    text: String,
    rows: u64,
    next: Option<String>,
}

/// The chunk after `cursor`.
async fn read_chunk(
    cx: &Cx,
    resource: &dyn ErasedResource,
    query: &ListQuery,
    cursor: Option<String>,
) -> Result<Chunk, AppError> {
    let chunk = resource.export(cx, query, cursor, CHUNK).await?;
    let items = present(cx, resource, &chunk.records, Screen::Export).await?;
    Ok(Chunk {
        text: items.iter().map(row).collect(),
        rows: items.len() as u64,
        next: chunk.next,
    })
}

/// How a file ended, for the audit log; written when the body is dropped, so a
/// client that goes away is counted too.
struct Closing {
    app: AppContext,
    /// The `helm.export.finished` entry, without what only the end knows.
    entry: Option<AuditEntry>,
    metadata: serde_json::Map<String, serde_json::Value>,
    rows: u64,
    outcome: &'static str,
}

impl Closing {
    /// Says how the file ended.
    fn ended(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }
}

impl Drop for Closing {
    fn drop(&mut self) {
        let (Some(entry), Ok(runtime)) = (self.entry.take(), tokio::runtime::Handle::try_current())
        else {
            return;
        };
        let mut metadata = std::mem::take(&mut self.metadata);
        metadata.insert("rows".to_owned(), self.rows.into());
        metadata.insert("outcome".to_owned(), self.outcome.into());
        let entry = entry.metadata(serde_json::Value::Object(metadata));
        let app = self.app.clone();
        runtime.spawn(async move { record_outside(&app, entry).await });
    }
}

/// Where a response body has got to.
struct Sending {
    plan: Plan,
    /// Text read and not yet sent, with its rows.
    pending: Option<(String, u64)>,
    /// Where the next chunk starts; `None` when there is none.
    cursor: Option<String>,
    failed: bool,
    /// When the file must have ended.
    deadline: Instant,
    closing: Closing,
}

/// Whether the request comes from another site of the domain, whose links and
/// scripts carry the panel's cookie (`SameSite` goes by the domain) and so could
/// start an export or read the menu's counts: the browser says where a request
/// comes from, and a client that isn't a browser says nothing.
pub(crate) fn refuse_other_sites(headers: &HeaderMap) -> Result<(), AppError> {
    let site = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok());
    if matches!(site, Some("same-site" | "cross-site")) {
        return Err(AppError::forbidden()
            .with_code("helm_other_site")
            .with_detail("This can only be asked for from the panel."));
    }
    Ok(())
}

/// What a file re-enters a workspace on: an error if the app entered the one the
/// request works in, and the panel couldn't enter it for the rest of the file.
async fn may_be_reentered(cx: &Cx) -> Result<(), AppError> {
    if let (None, Some(entered)) = (cx.named_by, cx.uow.workspace()) {
        workspace::admission(&cx.app, &cx.core, cx.user_id, &cx.access, entered, false)
            .await?
            .map_err(Refused::error)?;
    }
    Ok(())
}

/// Whether the list offers its file: what [`send`] would answer before it reads a
/// row.
pub(crate) async fn offered(cx: &Cx, info: &ResourceInfo) -> bool {
    info.export && may_be_reentered(cx).await.is_ok()
}

/// `GET {path}/resources/{resource}/export.csv`.
pub(crate) async fn export(
    panel: Panel,
    Path(slug): Path<String>,
    RawQuery(query): RawQuery,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let core = panel.cx.core.clone();
    send(panel, slug, query.unwrap_or_default(), method, &headers)
        .await
        .unwrap_or_else(|error| page_error(&core, error))
}

async fn send(
    panel: Panel,
    slug: String,
    query: String,
    method: Method,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    // A HEAD reads nothing and tells nothing: it isn't an export for the audit log.
    if method != Method::GET {
        return Err(AppError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Method Not Allowed",
        ));
    }
    refuse_other_sites(headers)?;
    let cx = &panel.cx;
    let resource = resource(cx, &slug)?.clone();
    let info = resource.info();
    let list = crate::ui::screen_query(info, &query)?;
    // Asked now, as for a queued action: past the first chunk the file enters the
    // workspace itself, which it can't ask the app that entered it about.
    may_be_reentered(cx).await?;
    let first = read_chunk(cx, resource.as_ref(), &list, None).await?;
    let workspace = cx.uow.workspace();
    let metadata = json!({ "resource": info.slug, "query": query, "workspace": workspace });
    let mut entry = AuditEntry::new(AuditKind::Custom, "helm.export").metadata(metadata.clone());
    entry.subject_type = Some(info.subject.to_owned());
    cx.audit_outside(entry).await;
    let mut finished =
        cx.entry_of_client(AuditEntry::new(AuditKind::Custom, "helm.export.finished"));
    finished.subject_type = Some(info.subject.to_owned());
    let day = crate::save::now(&cx.app).await.date_naive();
    let sending = Sending {
        plan: Plan {
            app: cx.app.clone(),
            core: cx.core.clone(),
            slug: info.slug.clone(),
            query: list,
            user_id: cx.user_id,
            principal: panel.principal.clone(),
            workspace,
            named_by: cx.named_by,
            pin: cx.pin,
            request_id: cx.client.request_id.clone(),
        },
        pending: Some((
            format!("{BYTE_ORDER_MARK}{}{}", header(info), first.text),
            first.rows,
        )),
        cursor: first.next,
        failed: false,
        deadline: Instant::now() + Duration::from_secs(cx.core.options.export_max_secs),
        closing: Closing {
            app: cx.app.clone(),
            entry: Some(finished),
            metadata: metadata.as_object().cloned().unwrap_or_default(),
            rows: 0,
            outcome: "abandoned",
        },
    };
    let body = stream::unfold(sending, |mut state| async move {
        if state.failed {
            return None;
        }
        if let Some((text, rows)) = state.pending.take() {
            state.closing.rows += rows;
            return Some((Ok(Bytes::from(text)), state));
        }
        let Some(cursor) = state.cursor.take() else {
            state.closing.ended("finished");
            return None;
        };
        if Instant::now() >= state.deadline {
            tracing::warn!(target: "navree::helm", "an export reached its time limit");
            state.failed = true;
            state.closing.ended("time_limit");
            let stopped: BoxError = "the export stopped: it reached its time limit".into();
            return Some((Err(stopped), state));
        }
        match state.plan.chunk(Some(cursor)).await {
            Ok(chunk) => {
                state.cursor = chunk.next;
                state.closing.rows += chunk.rows;
                Some((Ok(Bytes::from(chunk.text)), state))
            }
            Err(error) => {
                tracing::warn!(target: "navree::helm", status = error.status.as_u16(), "an export stopped");
                state.failed = true;
                state.closing.ended("stopped");
                let stopped: BoxError = format!("the export stopped: {}", error.title).into();
                Some((Err(stopped), state))
            }
        }
    });
    let mut response = Response::new(Body::from_stream(body));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        http::HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    if let Ok(value) =
        http::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename(info, day)))
    {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    headers.insert(
        http::header::X_CONTENT_TYPE_OPTIONS,
        http::HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_quoted_when_they_need_to_be() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("two\nlines"), "\"two\nlines\"");
        assert_eq!(field(""), "");
        assert_eq!(line(["a", "b,c", ""]), "a,\"b,c\",\r\n");
    }

    #[test]
    fn a_spreadsheet_does_not_run_a_field_as_a_formula() {
        for formula in [
            "=1+1",
            "+cmd|' /C calc'!A0",
            "-2+3",
            "@SUM(A1)",
            "\tx",
            "=HYPERLINK(\"http://x\")",
        ] {
            let written = field(formula);
            assert!(
                written.starts_with('\'') || written.starts_with("\"'"),
                "{formula} -> {written}"
            );
        }
        // Numbers stay numbers.
        for number in ["-5", "+3.25", "12", "0.5", "-0", "-$5.00", "+\u{20ac}3.25"] {
            assert_eq!(field(number), number);
        }
        // Thousands separators too, in the quotes a comma needs.
        assert_eq!(field("-1,234"), "\"-1,234\"");
        assert_eq!(field("-$1,234.60"), "\"-$1,234.60\"");
        // Not what merely starts like one.
        for formula in ["-$A$1", "-1+1", "-,1", "-$", "+1 555 0100", "-CHF12"] {
            let written = field(formula);
            assert!(
                written.starts_with('\'') || written.starts_with("\"'"),
                "{formula} -> {written}"
            );
        }
        assert!(!is_number("-"));
        assert!(!is_number("1."));
        assert!(!is_number("-1e5"));
    }
}

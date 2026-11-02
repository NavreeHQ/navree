//! A list as a CSV file: `GET {path}/resources/{resource}/export.csv`, with the
//! list's search, filters and sort in its query.
//!
//! The first chunk of rows is read in the request, so a refusal or a bad filter
//! answers as a page; the rest are read as the response is sent, a chunk at a
//! time, each in a unit of work of its own that checks again that the user may
//! work in the workspace. No more than a chunk is ever held, and no connection
//! is held while the client reads.

use std::sync::Arc;

use futures::stream;
use http::StatusCode;
use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use navree_access::PrincipalAccess;
use navree_contracts::{AuditEntry, AuditKind};
use navree_db::Db;
use navree_http::body::{Body, Bytes};
use navree_http::response::Response;
use navree_http::{AppError, Path, RawQuery};
use navree_kernel::{AppContext, BoxError};
use serde_json::json;
use uuid::Uuid;

use crate::api::resource;
use crate::core::Core;
use crate::cx::{Client, Cx};
use crate::handle::{ErasedResource, ResourceInfo};
use crate::panel::Panel;
use crate::present::{Display, Item, Screen, present, screen_fields};
use crate::query::ListQuery;
use crate::ui::page_error;
use crate::workspace::{self, NamedBy};

/// How many rows one chunk reads.
const CHUNK: u64 = 500;

// ---------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------

/// `text` as a field of a CSV line (RFC 4180): quoted when it has a comma, a
/// quote or a line break. A text a spreadsheet would run as a formula (it
/// starts with `=`, `+`, `-`, `@`, a tab or a return, and is not a number)
/// gets a quote in front, so it opens as the text it is.
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

/// Whether `text` is a plain number (`-5`, `+3.25`), which a spreadsheet reads
/// as one and not as a formula.
fn is_number(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let mut parts = digits.splitn(2, '.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
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

/// The header line: the label of every field of a record's page.
pub(crate) fn header(info: &ResourceInfo) -> String {
    line(
        screen_fields(info, Screen::Detail)
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
    access: Arc<PrincipalAccess>,
    workspace: Option<Uuid>,
    named_by: Option<NamedBy>,
    pin: Option<Uuid>,
    request_id: Option<String>,
}

impl Plan {
    /// The text of the chunk after `cursor` and where the next one starts,
    /// read in a unit of work of its own.
    async fn chunk(&self, cursor: Option<String>) -> Result<(String, Option<String>), AppError> {
        let handle = self
            .core
            .registry
            .by_slug(&self.slug)
            .ok_or_else(AppError::not_found)?;
        let db = self.app.get::<Db>().await.map_err(AppError::internal)?;
        let uow = db.uow(Some(self.user_id), self.request_id.clone()).await;
        if let Some(workspace) = self.workspace {
            // The user may have left the workspace since the last chunk.
            workspace::admission(
                &self.app,
                &self.core,
                self.user_id,
                &self.access,
                workspace,
                false,
            )
            .await?
            .map_err(workspace::Refused::error)?;
            workspace::enter_in(&self.app, &uow, Some(workspace)).await?;
        }
        let cx = Cx {
            app: self.app.clone(),
            uow: uow.clone(),
            access: self.access.clone(),
            user_id: self.user_id,
            core: self.core.clone(),
            client: Client::default(),
            tz_offset: None,
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

/// The text of the chunk after `cursor`, and the cursor after it.
async fn read_chunk(
    cx: &Cx,
    resource: &dyn ErasedResource,
    query: &ListQuery,
    cursor: Option<String>,
) -> Result<(String, Option<String>), AppError> {
    let chunk = resource.export(cx, query, cursor, CHUNK).await?;
    let items = present(cx, resource, &chunk.records, Screen::Detail).await?;
    Ok((items.iter().map(row).collect(), chunk.next))
}

/// Where a response body has got to.
struct Sending {
    plan: Plan,
    /// Text read and not yet sent.
    pending: Option<String>,
    /// Where the next chunk starts; `None` when there is none.
    cursor: Option<String>,
    failed: bool,
}

/// `GET {path}/resources/{resource}/export.csv`.
pub(crate) async fn export(
    panel: Panel,
    Path(slug): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let core = panel.cx.core.clone();
    send(panel, slug, query.unwrap_or_default())
        .await
        .unwrap_or_else(|error| page_error(&core, error))
}

async fn send(panel: Panel, slug: String, query: String) -> Result<Response, AppError> {
    let cx = &panel.cx;
    let resource = resource(cx, &slug)?.clone();
    let info = resource.info();
    let list = ListQuery::parse(&query)?;
    let (first, cursor) = read_chunk(cx, resource.as_ref(), &list, None).await?;
    let mut entry = AuditEntry::new(AuditKind::Custom, "helm.export")
        .metadata(json!({ "resource": info.slug, "query": query }));
    entry.subject_type = Some(info.subject.to_owned());
    cx.audit_outside(entry).await;
    let day = crate::save::now(&cx.app).await.date_naive();
    let sending = Sending {
        plan: Plan {
            app: cx.app.clone(),
            core: cx.core.clone(),
            slug: info.slug.clone(),
            query: list,
            user_id: cx.user_id,
            access: cx.access.clone(),
            workspace: cx.uow.workspace(),
            named_by: cx.named_by,
            pin: cx.pin,
            request_id: cx.client.request_id.clone(),
        },
        pending: Some(format!("{}{first}", header(info))),
        cursor,
        failed: false,
    };
    let body = stream::unfold(sending, |mut state| async move {
        if state.failed {
            return None;
        }
        if let Some(text) = state.pending.take() {
            return Some((Ok(Bytes::from(text)), state));
        }
        let cursor = state.cursor.take()?;
        match state.plan.chunk(Some(cursor)).await {
            Ok((text, next)) => {
                state.cursor = next;
                Some((Ok(Bytes::from(text)), state))
            }
            Err(error) => {
                tracing::warn!(target: "navree::helm", status = error.status.as_u16(), "an export stopped");
                state.failed = true;
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
        for number in ["-5", "+3.25", "12", "0.5", "-0"] {
            assert_eq!(field(number), number);
        }
        assert!(!is_number("-"));
        assert!(!is_number("1."));
        assert!(!is_number("-1e5"));
    }
}

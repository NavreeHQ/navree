---
name: navree-errors-and-responses
description: "Returns errors and responses the Navree way: `AppError` constructors, the `{statusCode, message, error, errors}` JSON shape, `?` and 500s, `map_error` for your own error types, database error mapping, status codes, headers, downloads, streaming and API resources with `__type`. Use when a handler must return a specific status or message, an error type needs an HTTP status, a response needs headers or a file, or an error response doesn't have the expected shape."
---

# Errors and responses

Every error a Navree app sends has one JSON body, whatever produced it (a
handler, validation, the rate limiter, a panic):

```json
{ "statusCode": 404, "message": "Deal not found", "error": "Not Found" }
```

`message` is the detail (a list of every message for validation errors),
`error` is the reason phrase, and validation errors add
`"errors": { "field": ["..."] }`. The request id is in the `X-Request-Id`
header, never in the body. Internal details go to the log, never to the
client. Navree never answers 422 or 419: input errors are 400, CSRF 403.

## Return an error from a handler

Handlers return `Result<T>` (the prelude's `navree::Result<T, AppError>`):

```rust
use navree::prelude::*;
use serde_json::{Value, json};
use uuid::Uuid;

#[get("/{id}")]
async fn show(&self, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    if id.is_nil() {
        return Err(AppError::not_found().with_detail("Note not found"));
    }
    Ok(Json(json!({ "id": id })))
}
```

| Constructor | Status | `message` |
|---|---|---|
| `AppError::bad_request("The export is empty.")` | 400 | the text |
| `AppError::unauthenticated()` | 401 | `Unauthorized` |
| `AppError::forbidden()` | 403 | `Forbidden` |
| `AppError::not_found()` | 404 | `Not Found` |
| `AppError::conflict("Already sent.")` | 409 | the text |
| `AppError::validation(map)` | 400 | every field message, plus `errors` |
| `AppError::too_many_requests(duration)` | 429 | `Too Many Requests`, `Retry-After` |
| `AppError::service_unavailable("..")` | 503 | the text |
| `AppError::internal(error)` | 500 | `Internal server error` (the error is logged) |
| `AppError::from_status(StatusCode::PAYMENT_REQUIRED)` | any | the reason phrase |
| `AppError::new(status, "code", "Title")` | any | the title |

Builders: `.with_detail("..")` sets the message, `.with_header(name, value)`
adds a header, `.with_source(error)` records the cause (logged for 5xx,
never sent). `StatusCode` is `navree::axum::http::StatusCode`.

## `?` and 500s

Any `std::error::Error + Send + Sync` converts into `AppError` with `?` and
becomes a logged 500 with `"message": "Internal server error"`. That
blanket conversion means `impl From<MyError> for AppError` does **not
compile**; map your error types instead.

## Map your own error type

```rust
use std::fmt;

use navree::axum::http::StatusCode;
use navree::prelude::*;

#[derive(Debug)]
pub enum BillingError {
    CardDeclined { reason: String },
    InvoicePaid,
    Gateway(std::io::Error),
}

impl fmt::Display for BillingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BillingError::CardDeclined { reason } => write!(f, "the card was declined: {reason}"),
            BillingError::InvoicePaid => f.write_str("the invoice is already paid"),
            BillingError::Gateway(error) => write!(f, "the payment gateway failed: {error}"),
        }
    }
}

impl std::error::Error for BillingError {}

fn billing_error(error: &BillingError) -> Option<AppError> {
    match error {
        BillingError::CardDeclined { reason } => Some(
            AppError::from_status(StatusCode::PAYMENT_REQUIRED)
                .with_detail(format!("The card was declined: {reason}.")),
        ),
        BillingError::InvoicePaid => Some(AppError::conflict("The invoice is already paid.")),
        BillingError::Gateway(_) => None, // stays a 500
    }
}

#[module(controllers = [InvoicesController], register = Self::wire)]
pub struct BillingModule;

impl BillingModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.map_error::<BillingError>(billing_error);
    }
}
```

Now `self.invoices.charge(id).await?` in a handler answers 402 or 409.
Mappers are app-wide and only see 500s that carry a source; an `AppError`
you build with a status is sent as it is. For a one-off, convert at the
call site: `.map_err(|e| AppError::new(StatusCode::BAD_GATEWAY, "rates_unavailable", "Bad Gateway").with_source(e))?`.

## Errors Navree maps for you

| Source | Response |
|---|---|
| `DbErr::RecordNotFound`, `RecordNotUpdated` (e.g. `Repo::find_or_fail`) | 404 `Not Found` |
| unique constraint violation | 409 `A record with the same unique value already exists.` |
| foreign key violation | 409 `The change conflicts with a related record.` |
| any other `DbErr` | 500 |
| missing route / wrong method | 404 / 405 with `Allow` |
| body over `http.body_limit_bytes` (2 MiB) | 413 |
| wrong `Content-Type` for `Json` | 415 |
| per-IP limit or `#[throttle]` exceeded | 429 with `Retry-After` |
| request over `http.timeout_secs` (30) or `#[timeout]` | 503 |
| panic in a handler | 500 (logged with the panic text) |
| `StorageError::NotFound` / `InvalidPath` | 404 / 400 |

A response status of 400 or above rolls the request's unit of work back,
so a failed request leaves no partial writes.

## Responses

| Return | Response |
|---|---|
| `Json(value)` | 200 `application/json` |
| `Created(location, value)` | 201 with `Location` |
| `NoContent` | 204 |
| `(StatusCode::ACCEPTED, Json(value))` | any status with a body |
| `Page<T>` / `CursorPage<T>` | `{data, meta}` / `{data, next_cursor}` |
| `String`, `&'static str` | 200 `text/plain` |
| `navree::axum::response::Response` | anything, built with `.into_response()` |

```rust
use navree::axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use navree::axum::response::{IntoResponse, Response};

#[get("/export")]
async fn export(&self) -> Response {
    (
        [
            (CONTENT_TYPE, "text/csv; charset=utf-8"),
            (CONTENT_DISPOSITION, "attachment; filename=\"notes.csv\""),
        ],
        "id,body\n",
    )
        .into_response()
}
```

Return `Response` (not `impl IntoResponse`) from controller methods. Stream
a large body with `navree::axum::body::Body::from_stream(stream)` (a storage
disk's `get_stream` gives one). Navree adds `X-Request-Id`, the security
headers and CORS headers to every response; a header you set wins.

## API resources

Don't return database models when the API shape should stay stable: give
each entity a `*Resource` struct with `From<Model>`, a `"__type"` field
holding the subject name (front ends use it for CASL checks) and, for the
OpenAPI document, `ToSchema`:

```rust
#[derive(Clone, Debug, Serialize, ToSchema)]
#[schema(as = Note)]
pub struct NoteResource {
    #[serde(rename = "__type")]
    pub resource_type: &'static str, // always "Note"
    pub id: Uuid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}
```

`page.map(NoteResource::from)` converts a `Page` without touching `meta`.
`navree::api::{resource_json, mask_fields}` add `__type` and drop unreadable
fields for resources built at run time.

## Test

```rust
response.assert_error(404, "Note not found");    // status, shape, message, X-Request-Id
response.assert_error(409, None);                 // any message
response.assert_validation_error("email");        // 400 with errors.email
```

## Common mistakes

- `impl From<MyError> for AppError` (conflicts with the blanket impl): use `map_error`.
- Returning a JSON body with an error status built by hand: it's sent as
  is, not in the standard shape. Return `AppError`.
- Putting the request id or internal details in `message`.
- Catching a `DbErr` unique violation to return 400: let it become the 409,
  or check first with an `AsyncRule::unique` validation rule.

## Docs

- https://navree.com/docs/basics/errors/
- https://navree.com/docs/basics/responses/

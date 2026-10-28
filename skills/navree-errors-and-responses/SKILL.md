---
name: navree-errors-and-responses
description: "Returns errors and responses the Navree way: `AppError` constructors, the `{statusCode, message, error, code, errors}` JSON shape, machine-readable error codes (`with_code`, `navree::http::codes`, `http.errors.code`) that front ends translate, `?` and 500s, `map_error` for your own error types, database error mapping, status codes, headers, downloads, streaming and API resources with `__type`. Use when a handler must return a specific status, message or code, an error type needs an HTTP status, a front end must show a refusal in the user's language, a response needs headers or a file, or an error response doesn't have the expected shape."
---

# Errors and responses

Every error a Navree app sends has one JSON body, whatever produced it (a
handler, validation, the rate limiter, a panic):

```json
{ "statusCode": 404, "message": "Deal not found", "error": "Not Found", "code": "not_found" }
```

`message` is the detail (a list of every message for validation errors),
`error` is the reason phrase, `code` is the stable machine-readable code a
front end maps to a translation key, and validation errors add
`"errors": { "field": ["..."] }` (with `"code": "validation_failed"`). The
request id is in the `X-Request-Id` header, never in the body. Internal
details go to the log, never to the client. Navree never answers 422 or
419: input errors are 400, CSRF 403.

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

| Constructor | Status | `message` | `code` |
|---|---|---|---|
| `AppError::bad_request("The export is empty.")` | 400 | the text | `bad_request` |
| `AppError::validation(map)` | 400 | every field message, plus `errors` | `validation_failed` |
| `AppError::invalid_filter("..")` | 400 | the text | `invalid_filter` |
| `AppError::unauthenticated()` | 401 | `Unauthorized` | `unauthenticated` |
| `AppError::two_factor_required()` | 401 | `Two-factor authentication required` | `two_factor_required` |
| `AppError::forbidden()` | 403 | `Forbidden` | `forbidden` |
| `AppError::demo_read_only()` | 403 | `Demo workspaces are read-only.` | `demo_read_only` |
| `AppError::not_found()` | 404 | `Not Found` | `not_found` |
| `AppError::conflict("Already sent.")` | 409 | the text | `conflict` |
| `AppError::limit_reached("Your plan includes 5 seats.")` | 409 | the text | `limit_reached` |
| `AppError::too_many_requests(duration)` | 429 | `Too Many Requests`, `Retry-After` | `too_many_requests` |
| `AppError::service_unavailable("..")` | 503 | the text | `service_unavailable` |
| `AppError::internal(error)` | 500 | `Internal server error` (the error is logged) | `internal_error` |
| `AppError::from_status(StatusCode::PAYMENT_REQUIRED)` | any | the reason phrase | `payment_required` |
| `AppError::new(status, "code", "Title")` | any | the title | `code` |

Builders: `.with_detail("..")` sets the message, `.with_code("..")` sets
the code, `.with_header(name, value)` adds a header, `.with_source(error)`
records the cause (logged for 5xx, never sent). `StatusCode` is
`navree::axum::http::StatusCode`.

## Error codes

Give every refusal a user can see its own code, naming what happened (not
the status), in lower snake case:

```rust
return Err(AppError::conflict("The campaign is already running.")
    .with_code("campaign_already_running"));
```

- The front end maps `code` to a translation key and branches on it; never
  make client code compare English messages.
- Framework codes are constants in `navree::http::codes` (`codes::ALL`
  lists them; `codes::is_valid(code)` checks a code's form). They never
  change once released.
- Modules send their own: auth `invalid_credentials`, `token_expired`,
  `invalid_two_factor_code`, `too_many_login_attempts`; workspace
  `not_a_member`, `workspace_not_found`, `invitation_expired`; access
  `role_not_found`, `forbidden_fields`; social `email_taken`,
  `registration_closed`; billing `subscription_required`,
  `already_subscribed`; webhooks `invalid_webhook_signature`. The errors
  page lists them all.
- `http.errors.code = false` (`APP_HTTP__ERRORS__CODE=false`) leaves the
  member out of every body; `AppError::code` keeps its value.

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
                .with_code("card_declined")
                .with_detail(format!("The card was declined: {reason}.")),
        ),
        BillingError::InvoicePaid => {
            Some(AppError::conflict("The invoice is already paid.").with_code("invoice_paid"))
        }
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
| `DbErr::RecordNotFound`, `RecordNotUpdated` (e.g. `Repo::find_or_fail`) | 404 `Not Found`, `not_found` |
| unique constraint violation | 409 `A record with the same unique value already exists.`, `conflict` |
| foreign key violation | 409 `The change conflicts with a related record.`, `conflict` |
| any other `DbErr` | 500, `internal_error` |
| missing route / wrong method | 404 `not_found` / 405 `method_not_allowed` with `Allow` |
| body over `http.body_limit_bytes` (2 MiB) | 413 `payload_too_large` |
| wrong `Content-Type` for `Json` | 415 `unsupported_media_type` |
| per-IP limit or `#[throttle]` exceeded | 429 `too_many_requests` with `Retry-After` |
| request over `http.timeout_secs` (30) or `#[timeout]` | 503 `request_timeout` |
| maintenance mode | 503 `maintenance` |
| panic in a handler | 500 `internal_error` (logged with the panic text) |
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
headers to every response, and CORS headers when the request's `Origin` is in
`http.cors.allowed_origins` (other origins get none); a header you set wins.

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
response.assert_error_code(409, "campaign_already_running"); // and the code
response.assert_validation_error("email");        // 400 with errors.email
```

## Common mistakes

- `impl From<MyError> for AppError` (conflicts with the blanket impl): use `map_error`.
- A refusal users see without its own code (two different 409s both
  `conflict`): the front end can't tell them apart. Add `.with_code(..)`.
- Front-end code comparing `message` strings: compare `code`.
- Returning a JSON body with an error status built by hand: it's sent as
  is, not in the standard shape. Return `AppError`.
- Putting the request id or internal details in `message`.
- Catching a `DbErr` unique violation to return 400: let it become the 409,
  or check first with an `AsyncRule::unique` validation rule.

## Docs

- https://navree.com/docs/basics/errors/
- https://navree.com/docs/basics/responses/

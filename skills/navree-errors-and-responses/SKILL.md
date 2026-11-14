---
name: navree-errors-and-responses
description: "Returns errors and responses the Navree way: `AppError` constructors, the `{statusCode, message, error, code, errors}` JSON shape, machine-readable error codes (`with_code`, `navree::http::codes`, `http.errors.code`) that front ends translate, `?` and 500s, `map_error` for your own error types, database error mapping, status codes, headers, downloads, streaming and API resources with `__type`, and errors on page routes: `#[page]` and `#[json_only]` (`RouteDef::page()`, `RouteDef::json_only()`), the `ErrorPages` hook of navree-http and the HTML error pages `UiModule` registers. Use when a handler must return a specific status, message or code, an error type needs an HTTP status, a front end must show a refusal in the user's language, a response needs headers or a file, an error response doesn't have the expected shape, or a page route answers JSON (or an API route answers HTML) when it should not."
---

# Errors and responses

Every error an API route of a Navree app sends has one JSON body, whatever
produced it (a handler, validation, the rate limiter, a panic). A request for
a page in an app with Navree UI gets a page instead (see "Errors on a page
route" below):

```json
{ "statusCode": 404, "message": "Deal not found", "error": "Not Found", "code": "not_found" }
```

`message` is the detail (a list of every message for validation errors),
`error` is the reason phrase, `code` is the stable machine-readable code a
front end maps to a translation key, and validation errors add
`"errors": { "field": ["..."] }` (with `"code": "validation_failed"`). The
request id is in the `X-Request-Id` header, never in the body. Internal
details go to the log, never to the client. The router, the extractors and
the validated extractors never answer 422 or 419: input errors are 400 and
CSRF failures are 403. Navree UI forms answer a rejected form with 422, and
`AppError::page_expired()` is a 419 you can return yourself.

## Return an error from a handler

Handlers return `Result<T>` (the prelude's `navree::Result<T, AppError>`):

```rust,ignore
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
`http::StatusCode`.

## Error codes

Give every refusal a user can see its own code, naming what happened (not
the status), in lower snake case:

```rust,ignore
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

```rust,ignore
use std::fmt;

use http::StatusCode;
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
| `OrmError::ModelNotFound` (e.g. `Post::find_or_fail`), `DbError` of kind `NotFound` | 404 `Not Found`, `not_found` |
| unique constraint violation | 409 `A record with the same unique value already exists.`, `conflict` |
| foreign key violation | 409 `The change conflicts with a related record.`, `conflict` |
| any other `DbError` or `OrmError` | 500, `internal_error` |
| missing route / wrong method | 404 `not_found` / 405 `method_not_allowed` with `Allow` |
| body over `http.body_limit_bytes` (2 MiB) | 413 `payload_too_large` |
| body stalled or too slow (`http.server.body_read_timeout_ms`, `body_min_bytes_per_sec`) | 408 `request_timeout` |
| wrong `Content-Type` for `Json` | 415 `unsupported_media_type` |
| per-IP limit or `#[throttle]` exceeded | 429 `too_many_requests` with `Retry-After` |
| request over `http.timeout_secs` (30) or `#[timeout]` | 503 `request_timeout` |
| maintenance mode | 503 `maintenance` |
| panic in a handler | 500 `internal_error` (logged with the panic text) |
| `StorageError::NotFound` / `InvalidPath` | 404 / 400 |

A response status of 400 or above rolls the request's unit of work back,
so a failed request leaves no partial writes.

## Errors on a page route

An app that names `UiModule` (Navree UI) answers a request for a **page** with
a page: a 401, 403, 404, 405, 409, 410, 413, 419, 422, 429, a 5xx, a refused
CSRF token or a panic. A signed-in user gets it inside the shell, with the
next step and a reference (the request id) to quote. An API route keeps the
JSON body above. `UiModule` does this by contributing an `ErrorPages` with
`m.error_pages(..)`; you add nothing. The mark on the route decides, and the
`Accept` header decides for a route with no mark:

| The request | The answer |
|---|---|
| A route marked `json_only()` (`#[json_only]`) | the JSON body, for every client |
| A client that asks for JSON alone | the JSON body |
| A route under `http.api_prefix` (`/api`) with no `page()` mark | the JSON body, even for a browser |
| A route marked `page()` (`#[page]`) | a page, unless the client asks for JSON alone |
| Any other route, asked by a browser (`text/html`) | a page |
| Any other route, asked with `*/*` or no `Accept` header | the JSON body |

A route's mark covers every error it is the route of, including one raised
before the route runs: a refused CSRF token, a rate limit, a method the route
does not take, a panic. Put the mark on a route, or on the whole `impl` of a
controller:

```rust
use navree::ui::{AppPage, AppShell, Text};
use uuid::Uuid;

#[injectable]
pub struct InvoicePages;

#[controller]
impl InvoicePages {
    /// A page: a missing invoice is a 404 page in the shell.
    #[get("!/invoices/{id}")]
    #[authenticated]
    #[page]
    async fn show(&self, shell: AppShell, Path(id): Path<Uuid>) -> Result<AppPage> {
        if id.is_nil() {
            return Err(AppError::not_found().with_detail("Invoice not found"));
        }
        Ok(shell.page("Invoice").body(Text::new("Invoice")))
    }

    /// A download beside it: its errors stay JSON for everyone.
    #[get("!/invoices/{id}/file")]
    #[authenticated]
    #[json_only]
    async fn file(&self, Path(id): Path<Uuid>) -> Result<String> {
        Ok(format!("invoice {id}"))
    }
}
```

For a route written by hand, the same marks are `RouteDef::on(..).page()` and
`.json_only()`.

- **Details.** A 5xx shows fixed words: its source, message, SQL, paths and
  panic text never reach the page. A 4xx or a 503 shows the error's own
  `detail`, as the JSON `message` does. The development block ("Development
  only") is on in a build with debug assertions and off in a release build,
  unless `[ui] debug` or `PagesOptions::debug(..)` says otherwise; in
  `production` and `staging` (the app's `env`) it is never shown. An app with no
  configuration to read its `env` from shows it only when the code calls
  `PagesOptions::debug(true)`.
- **Sign-in.** A 401 offers a button to the sign-in page with the page the user
  was on as `next`: set `[ui] sign_in = "/sign-in"` (and `sign_in_redirect =
  true` to send a guest straight there). Read `next` with `navree::ui::ReturnTo`,
  which accepts only a path on your site.
- **Another form of error.** The trait is `ErrorPages` in navree-http. Its
  `render` gets an `ErrorContext` (the method, the URI, the headers a page
  may depend on, the request id, the route's `RouteMeta`, the signed-in user
  and the API prefix) and the `AppError`, and answers `Some(response)` or
  `None` to keep the JSON body. It must not panic. A `json_only` route is never
  asked, and the app uses the first `ErrorPages` contributed. With `UiModule`
  in the app you do not write one.

```rust
use navree::http::response::Response;
use navree::http::{AppError, ErrorContext, ErrorPages, IntoResponse, ModuleBuilderHttpExt, Preference};
use navree::kernel::BoxFuture;

/// A one-line page for a browser's 404. Every other error keeps the JSON body.
pub struct PlainNotFound;

impl ErrorPages for PlainNotFound {
    fn render<'a>(
        &'a self,
        request: &'a ErrorContext,
        error: &'a AppError,
    ) -> BoxFuture<'a, Option<Response>> {
        Box::pin(async move {
            if error.status != http::StatusCode::NOT_FOUND || request.accepts() != Preference::Html {
                return None;
            }
            Some(
                (
                    error.status,
                    [("content-type", "text/html; charset=utf-8")],
                    String::from("<h1>Not found</h1>"),
                )
                    .into_response(),
            )
        })
    }
}

#[module(register = Self::wire)]
pub struct NotFoundPagesModule;

impl NotFoundPagesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.error_pages(PlainNotFound);
    }
}
```

The words of the pages, the sign-in address and the states inside a page
(`Forbidden`, `NotFound`, `SignInRequired`, `EmptyState`) are in `navree-ui`.

## Responses

| Return | Response |
|---|---|
| `Json(value)` | 200 `application/json` |
| `Created(location, value)` | 201 with `Location` |
| `NoContent` | 204 |
| `(StatusCode::ACCEPTED, Json(value))` | any status with a body |
| `Page<T>` / `CursorPage<T>` | `{data, meta}` / `{data, next_cursor}` |
| `String`, `&'static str` | 200 `text/plain` |
| `navree::http::response::Response` | anything, built with `.into_response()` |

```rust,ignore
use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use navree::http::response::{IntoResponse, Response};

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
a large body with `navree::http::body::Body::from_stream(stream)` (a storage
disk's `get_stream` gives one). Navree adds `X-Request-Id`, the security
headers to every response, and CORS headers when the request's `Origin` is in
`http.cors.allowed_origins` (other origins get none); a header you set wins.

## API resources

Don't return database models when the API shape should stay stable: give
each model a `*Resource` struct with `From<&Model>`, a `"__type"` field
holding the subject name (front ends use it for CASL checks) and, for the
OpenAPI document, `ToSchema`:

```rust,ignore
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

```rust,ignore
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
- A page controller whose file download or data route is not marked
  `#[json_only]`: a failed download answers an HTML page.
- An API route marked `#[page]`, or a page route (`!/..`) with no mark that
  scripts call without asking for JSON: they get a page. Mark the route.
- `UiOnDemand` standing in for `UiModule`: it keeps the JSON errors.
- Catching a unique violation (`error.is_unique_violation()`) to return 400: let it become the 409,
  or check first with an `AsyncRule::unique` validation rule.

## Docs

- https://navree.com/docs/basics/errors/
- https://navree.com/docs/basics/responses/
- https://navree.com/docs/ui/errors-and-states/

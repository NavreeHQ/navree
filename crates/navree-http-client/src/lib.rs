//! # navree-http-client
//!
//! `HttpClientModule`: outgoing HTTP with retries, timeouts, an SSRF policy
//! and test fakes, on reqwest and rustls/ring.
//!
//! - [`HttpClientModule`] is global and exports one shared [`HttpClient`],
//!   configured with [`HttpClientModule::for_root`] or read from section
//!   `http_client` with [`HttpClientModule::from_config`].
//! - [`HttpClient::get`] (and `post`, `put`, `patch`, `delete`, `request`)
//!   start a [`RequestBuilder`]; [`RequestBuilder::send`] returns a fully
//!   read [`Response`], and [`RequestBuilder::send_streaming`] a
//!   [`StreamingResponse`] whose body is read as it arrives (server-sent
//!   events), sent once (never retried) under the same SSRF policy.
//! - **Retries**: idempotent requests (`GET`, `HEAD`, `OPTIONS`, `TRACE`,
//!   `PUT`, `DELETE`) are retried on connection errors, 5xx and 429, with
//!   jittered exponential backoff. `POST` and `PATCH` never are.
//!   After the last attempt the last response is returned as it is; call
//!   [`Response::error_for_status`] to turn 4xx/5xx into an error.
//! - **Timeouts**: each attempt has its own timeout, from connecting to the
//!   last body byte; a timed-out attempt is not retried.
//! - **SSRF**: by default requests to loopback, private, link-local, CGNAT,
//!   unique-local and other internal ranges, and to cloud metadata endpoints,
//!   are refused with [`HttpClientError::Blocked`]: literal IP URLs before
//!   sending, host names after DNS resolution, and every redirect hop. See
//!   [`SsrfPolicy`] for the allow list.
//! - **Fakes**: [`HttpClient::fake`] returns a client that never touches the
//!   network and a [`FakeServer`] to stub responses and assert on the
//!   [`RecordedRequest`]s.
//! - **Logs** (`debug`): method, URL without query string or credentials,
//!   status, attempts and duration. Header values and bodies are never
//!   logged.
//!
//! ```
//! use http::Method;
//! use navree_http_client::{FakeResponse, HttpClient};
//! use serde_json::json;
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), navree_http_client::HttpClientError> {
//! let (client, server) = HttpClient::fake();
//! server.on(
//!     Method::GET,
//!     "https://api.example.com/users/*",
//!     FakeResponse::new(200).json(&json!({ "id": 7, "name": "Ada" })),
//! );
//!
//! let user: serde_json::Value = client
//!     .get("https://api.example.com/users/7")
//!     .bearer("token")
//!     .send()
//!     .await?
//!     .error_for_status()?
//!     .json()?;
//!
//! assert_eq!(user["name"], "Ada");
//! server.assert_sent(|r| r.header("authorization") == Some("Bearer token"));
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod backoff;
mod client;
mod error;
mod fake;
mod module;
mod options;
mod request;
mod response;
mod ssrf;
mod streaming;

pub use client::HttpClient;
pub use error::HttpClientError;
pub use fake::{FakeResponse, FakeServer, RecordedRequest};
pub use module::HttpClientModule;
pub use options::{HttpClientOptions, SsrfPolicy};
pub use request::RequestBuilder;
pub use response::Response;
pub use streaming::{BodyStream, StreamingResponse};

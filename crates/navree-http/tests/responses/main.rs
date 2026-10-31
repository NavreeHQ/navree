//! Responses, bodies and the app stack, compared with the previous framework
//! on the same inputs: what a response is made of, how a body streams and
//! fails, how layers wrap an app, and what a client reads off the wire.
//!
//! Accepted differences, each tested on its own:
//!
//! 1. A body's error type is `BoxError`, and `Event::json_data` fails with
//!    `serde_json::Error`: same messages, no wrapper
//!    (`bodies::errors_read_the_same`).
//! 2. A failing `IntoResponseParts` tuple renders the same 500; only its
//!    error's type is an enum (`shapes::bad_headers_are_a_500`).
//! 3. `content-length` and `connection` trade places on a response to a
//!    request without a path, `OPTIONS *` or `CONNECT host:port`
//!    (`wire::a_request_without_a_path_differs_only_in_the_order_of_two_fields`).

mod bodies;
mod shapes;
mod sse;
mod stack;
mod support;
mod wire;

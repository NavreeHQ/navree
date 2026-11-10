//! `navree_http::CorsLayer`: origin handling and preflights, and the signed
//! URL check (`UrlSigner`), which parses a query string from the client.
//!
//! Invariants for CORS: a response carries `Access-Control-Allow-Origin`
//! only when the request had exactly one `Origin` header equal, byte for
//! byte, to a configured origin, and then it echoes it; a preflight never
//! reaches the inner service; every response varies with `Origin`.
#![no_main]

use std::convert::Infallible;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use arbitrary::Arbitrary;
use http::{HeaderValue, Method, Request, Response, StatusCode, Uri};
use libfuzzer_sys::fuzz_target;
use navree_http::{CorsLayer, CorsOptions, UrlSigner};
use tower::{Layer, Service, service_fn};

#[derive(Debug, Arbitrary)]
struct Input {
    configured: Vec<Vec<u8>>,
    credentials: bool,
    origins: Vec<Vec<u8>>,
    options: bool,
    request_method: Option<Vec<u8>>,
    uri: Vec<u8>,
    query: Vec<u8>,
}

fn run<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("a CORS response is ready at once"),
    }
}

fn cors(input: &Input) {
    let configured: Vec<HeaderValue> = input
        .configured
        .iter()
        .filter_map(|bytes| HeaderValue::from_bytes(bytes).ok())
        .collect();
    let layer = CorsLayer::new(configured.clone()).allow_credentials(input.credentials);
    let mut service = layer.layer(service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(()))
    }));
    let mut builder = Request::builder().method(if input.options {
        Method::OPTIONS
    } else {
        Method::GET
    });
    let mut sent = Vec::new();
    for bytes in &input.origins {
        if let Ok(value) = HeaderValue::from_bytes(bytes) {
            builder = builder.header("origin", value.clone());
            sent.push(value);
        }
    }
    if let Some(value) = input
        .request_method
        .as_deref()
        .and_then(|bytes| HeaderValue::from_bytes(bytes).ok())
    {
        builder = builder.header("access-control-request-method", value);
    }
    let request = builder.body(()).expect("a request");
    let response = run(service.call(request)).expect("an answer");
    let allowed = match sent.as_slice() {
        [origin] if configured.contains(origin) => Some(origin),
        _ => None,
    };
    assert_eq!(
        response.headers().get("access-control-allow-origin"),
        allowed,
        "origin handling"
    );
    if allowed.is_none() {
        assert!(
            response
                .headers()
                .get("access-control-allow-credentials")
                .is_none()
        );
    }
    assert!(
        response
            .headers()
            .get_all("vary")
            .iter()
            .any(|value| value.to_str().is_ok_and(|v| v.contains("Origin"))),
        "the response varies with Origin"
    );
    let _ = StatusCode::OK;

    // The same lists as `http.cors` options.
    let options = CorsOptions {
        allowed_origins: input
            .configured
            .iter()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .collect(),
        allow_credentials: input.credentials,
        max_age_secs: u64::MAX,
    };
    let _ = CorsLayer::from_options(&options);
}

fn signed_url(input: &Input) {
    let signer = UrlSigner::new(b"fuzzing-signing-key-0123456789abc");
    let now = chrono::DateTime::from_timestamp(1_792_929_600, 0).expect("a time");
    let later = now + chrono::TimeDelta::hours(1);
    // A URL we signed verifies, and no longer once it has expired. `sign`
    // takes a path and query as they go on the wire: no fragment.
    if let Ok(path) =
        format!("/files/{}", String::from_utf8_lossy(&input.uri)).parse::<http::uri::PathAndQuery>()
    {
        let signed = signer.sign(path.as_str(), later);
        if let Ok(uri) = signed.parse::<Uri>() {
            assert!(signer.verify(&uri, now), "a signed URL verifies: {signed}");
            assert!(!signer.verify(&uri, later + chrono::TimeDelta::seconds(1)));
        }
    }
    // Whatever arrives on the wire.
    let wire = format!("/files?{}", String::from_utf8_lossy(&input.query));
    if let Ok(uri) = wire.parse::<Uri>() {
        let _ = signer.verify(&uri, now);
    }
    if let Ok(uri) = Uri::try_from(input.query.as_slice()) {
        let _ = signer.verify(&uri, now);
    }
}

fuzz_target!(|input: Input| {
    cors(&input);
    signed_url(&input);
});

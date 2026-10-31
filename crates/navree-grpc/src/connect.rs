//! The Connect protocol (<https://connectrpc.com/docs/protocol>): unary
//! calls as plain HTTP POSTs carrying a protobuf or JSON message, and
//! streaming calls as enveloped bodies, translated to gRPC and answered by
//! the same services. JSON follows protobuf's JSON mapping, transcoded with
//! the descriptor sets registered with `m.grpc_descriptors(..)`.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use bytes::Bytes;
use http::header::{CONTENT_ENCODING, CONTENT_TYPE, HeaderName, HeaderValue};
use http::{HeaderMap, Method, Request, Response, StatusCode, Version};
use http_body::Frame;
use http_body_util::{BodyExt, Limited};
use navree_http::body::Body;
use prost::Message as _;
use prost_reflect::{
    DescriptorPool, DeserializeOptions, DynamicMessage, MessageDescriptor, MethodDescriptor,
    SerializeOptions,
};
use tonic::{Code, Status};
use tower::ServiceExt;

use crate::edge::GrpcCore;
use crate::frame::{Frames, HEADER, frame};

/// The message codec of a Connect call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Codec {
    /// `application/proto`, `application/connect+proto`.
    Proto,
    /// `application/json`, `application/connect+json`.
    Json,
}

impl Codec {
    fn unary_content_type(self) -> &'static str {
        match self {
            Codec::Proto => "application/proto",
            Codec::Json => "application/json",
        }
    }

    fn stream_content_type(self) -> &'static str {
        match self {
            Codec::Proto => "application/connect+proto",
            Codec::Json => "application/connect+json",
        }
    }
}

/// What Connect calls need besides the gRPC core: the message types for
/// JSON and the size limit.
pub(crate) struct Connect {
    pub(crate) pool: DescriptorPool,
    pub(crate) max_message_bytes: usize,
}

impl Connect {
    /// The descriptor of the method at `path` (`/pkg.Service/Method`).
    fn method(&self, path: &str) -> Option<MethodDescriptor> {
        let (service, method) = path.trim_start_matches('/').split_once('/')?;
        self.pool
            .get_service_by_name(service)?
            .methods()
            .find(|m| m.name() == method)
    }

    /// The method's descriptor, required for JSON.
    fn json_method(&self, path: &str) -> Result<MethodDescriptor, Status> {
        self.method(path).ok_or_else(|| {
            Status::unimplemented(format!(
                "JSON for {path} needs the service's descriptors: register them with m.grpc_descriptors(..)"
            ))
        })
    }
}

/// The headers that belong to the protocol, not to the call's metadata.
fn is_protocol_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    matches!(
        name,
        "content-type"
            | "content-length"
            | "content-encoding"
            | "accept-encoding"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "connection"
            | "host"
    ) || name.starts_with("connect-")
        || name.starts_with("grpc-")
}

/// The gRPC request for a Connect request's parts and body.
fn grpc_request(
    parts: http::request::Parts,
    body: tonic::body::Body,
) -> Result<Request<tonic::body::Body>, Status> {
    let timeout = match parts.headers.get("connect-timeout-ms") {
        Some(value) => Some(
            value
                .to_str()
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(Duration::from_millis)
                .ok_or_else(|| Status::invalid_argument("connect-timeout-ms is not a number"))?,
        ),
        None => None,
    };
    let mut request = Request::new(body);
    *request.method_mut() = Method::POST;
    *request.uri_mut() = parts.uri;
    *request.version_mut() = Version::HTTP_2;
    let headers = request.headers_mut();
    for (name, value) in &parts.headers {
        if !is_protocol_header(name) {
            headers.append(name.clone(), value.clone());
        }
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
    headers.insert("te", HeaderValue::from_static("trailers"));
    if let Some(timeout) = timeout
        && let Ok(value) =
            HeaderValue::from_str(&format!("{}m", timeout.as_millis().min(99_999_999)))
    {
        headers.insert("grpc-timeout", value);
    }
    *request.extensions_mut() = parts.extensions;
    Ok(request)
}

/// Whether the request is compressed with anything but `identity`.
fn compressed(headers: &HeaderMap, name: &str) -> bool {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| !v.trim().eq_ignore_ascii_case("identity"))
}

/// A Connect unary call.
pub(crate) async fn unary(
    connect: Arc<Connect>,
    core: GrpcCore,
    request: Request<Body>,
    codec: Codec,
) -> Response<Body> {
    match try_unary(&connect, core, request, codec).await {
        Ok(response) => response,
        Err(status) => unary_error(&status, &HeaderMap::new(), &HeaderMap::new()),
    }
}

async fn try_unary(
    connect: &Connect,
    core: GrpcCore,
    request: Request<Body>,
    codec: Codec,
) -> Result<Response<Body>, Status> {
    let (parts, body) = request.into_parts();
    if parts.method != Method::POST {
        return Err(Status::unimplemented(
            "only POST is supported for Connect unary calls",
        ));
    }
    if compressed(&parts.headers, CONTENT_ENCODING.as_str()) {
        return Err(Status::unimplemented(
            "compressed Connect requests are not supported",
        ));
    }
    let path = parts.uri.path().to_owned();
    let method = match codec {
        Codec::Json => Some(connect.json_method(&path)?),
        Codec::Proto => None,
    };
    let bytes = Limited::new(body, connect.max_message_bytes)
        .collect()
        .await
        .map_err(|_| {
            Status::resource_exhausted(format!(
                "the message is larger than {} bytes",
                connect.max_message_bytes
            ))
        })?
        .to_bytes();
    let message = match &method {
        Some(method) => json_to_proto(&method.input(), &bytes)?,
        None => bytes,
    };
    let grpc = grpc_request(
        parts,
        tonic::body::Body::new(http_body_util::Full::new(frame(0, &message))),
    )?;
    let response = core
        .oneshot(grpc)
        .await
        .unwrap_or_else(|never| match never {});
    let (head, body) = response.into_parts();
    let collected = body
        .collect()
        .await
        .map_err(|error| Status::internal(format!("reading the response failed: {error}")))?;
    let trailers = collected.trailers().cloned().unwrap_or_default();
    let status = Status::from_header_map(&trailers)
        .or_else(|| Status::from_header_map(&head.headers))
        .unwrap_or_else(|| Status::internal("the service sent no status"));
    if status.code() != Code::Ok {
        return Ok(unary_error(&status, &head.headers, &trailers));
    }
    let mut frames = Frames::default();
    frames.push(collected.to_bytes());
    let Some((_, payload)) = frames.next_frame() else {
        return Ok(unary_error(
            &Status::internal("the service sent no message"),
            &head.headers,
            &trailers,
        ));
    };
    let body = match &method {
        Some(method) => proto_to_json(&method.output(), &payload)?,
        None => payload,
    };
    let mut response = Response::new(Body::from(body));
    copy_metadata(&head.headers, &trailers, response.headers_mut());
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(codec.unary_content_type()),
    );
    Ok(response)
}

/// Response metadata: the initial metadata as headers, trailers as
/// `trailer-<name>` headers.
fn copy_metadata(headers: &HeaderMap, trailers: &HeaderMap, out: &mut HeaderMap) {
    for (name, value) in headers {
        if !is_protocol_header(name) {
            out.append(name.clone(), value.clone());
        }
    }
    for (name, value) in trailers {
        if is_protocol_header(name) {
            continue;
        }
        if let Ok(prefixed) = HeaderName::from_bytes(format!("trailer-{name}").as_bytes()) {
            out.append(prefixed, value.clone());
        }
    }
}

/// A unary error response: the HTTP status of the code and a JSON body.
fn unary_error(status: &Status, headers: &HeaderMap, trailers: &HeaderMap) -> Response<Body> {
    let body = serde_json::to_vec(&error_json(status)).unwrap_or_default();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = http_status(status.code());
    copy_metadata(headers, trailers, response.headers_mut());
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// The HTTP status of a Connect error with `code`.
pub(crate) fn http_status(code: Code) -> StatusCode {
    let status = match code {
        Code::Ok => 200,
        Code::Cancelled => 499,
        Code::InvalidArgument | Code::FailedPrecondition | Code::OutOfRange => 400,
        Code::DeadlineExceeded => 504,
        Code::NotFound => 404,
        Code::AlreadyExists | Code::Aborted => 409,
        Code::PermissionDenied => 403,
        Code::ResourceExhausted => 429,
        Code::Unimplemented => 501,
        Code::Unavailable => 503,
        Code::Unauthenticated => 401,
        Code::Unknown | Code::Internal | Code::DataLoss => 500,
    };
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Connect's name of a code (`not_found`).
pub(crate) fn code_name(code: Code) -> &'static str {
    match code {
        Code::Ok => "ok",
        Code::Cancelled => "canceled",
        Code::Unknown => "unknown",
        Code::InvalidArgument => "invalid_argument",
        Code::DeadlineExceeded => "deadline_exceeded",
        Code::NotFound => "not_found",
        Code::AlreadyExists => "already_exists",
        Code::PermissionDenied => "permission_denied",
        Code::ResourceExhausted => "resource_exhausted",
        Code::FailedPrecondition => "failed_precondition",
        Code::Aborted => "aborted",
        Code::OutOfRange => "out_of_range",
        Code::Unimplemented => "unimplemented",
        Code::Internal => "internal",
        Code::Unavailable => "unavailable",
        Code::DataLoss => "data_loss",
        Code::Unauthenticated => "unauthenticated",
    }
}

/// `{"code", "message", "details"}`: details are the `google.protobuf.Any`
/// entries of the status, base64-encoded.
fn error_json(status: &Status) -> serde_json::Value {
    let mut error = serde_json::json!({ "code": code_name(status.code()) });
    if !status.message().is_empty() {
        error["message"] = status.message().into();
    }
    if !status.details().is_empty()
        && let Ok(rpc) = tonic_types::pb::Status::decode(status.details())
        && !rpc.details.is_empty()
    {
        error["details"] = rpc
            .details
            .iter()
            .map(|any| {
                let kind = any.type_url.rsplit('/').next().unwrap_or(&any.type_url);
                serde_json::json!({ "type": kind, "value": STANDARD_NO_PAD.encode(&any.value) })
            })
            .collect();
    }
    error
}

fn json_to_proto(input: &MessageDescriptor, json: &[u8]) -> Result<Bytes, Status> {
    let mut deserializer = serde_json::Deserializer::from_slice(json);
    let options = DeserializeOptions::new().deny_unknown_fields(false);
    let message =
        DynamicMessage::deserialize_with_options(input.clone(), &mut deserializer, &options)
            .and_then(|message| deserializer.end().map(|()| message))
            .map_err(|error| {
                Status::invalid_argument(format!(
                    "the JSON is not a valid {}: {error}",
                    input.full_name()
                ))
            })?;
    Ok(Bytes::from(message.encode_to_vec()))
}

fn proto_to_json(output: &MessageDescriptor, proto: &[u8]) -> Result<Bytes, Status> {
    let message = DynamicMessage::decode(output.clone(), proto).map_err(|error| {
        Status::internal(format!(
            "the response is not a valid {}: {error}",
            output.full_name()
        ))
    })?;
    let mut serializer = serde_json::Serializer::new(Vec::new());
    message
        .serialize_with_options(&mut serializer, &SerializeOptions::new())
        .map_err(|error| {
            Status::internal(format!("encoding the response as JSON failed: {error}"))
        })?;
    Ok(Bytes::from(serializer.into_inner()))
}

/// A Connect streaming call (client, server or bidirectional streaming).
pub(crate) async fn streaming(
    connect: Arc<Connect>,
    core: GrpcCore,
    request: Request<Body>,
    codec: Codec,
) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_owned();
    let failed = |status: Status| stream_response(codec, HeaderMap::new(), end_only(&status));
    if parts.method != Method::POST {
        return failed(Status::unimplemented(
            "Connect streaming calls are POST requests",
        ));
    }
    if compressed(&parts.headers, "connect-content-encoding") {
        return failed(Status::unimplemented(
            "compressed Connect messages are not supported",
        ));
    }
    let method = match codec {
        Codec::Json => match connect.json_method(&path) {
            Ok(method) => Some(method),
            Err(status) => return failed(status),
        },
        Codec::Proto => None,
    };
    let body = match &method {
        Some(method) => tonic::body::Body::new(JsonRequest {
            inner: body,
            frames: Frames::default(),
            input: method.input(),
            queue: VecDeque::new(),
            done: false,
            limit: connect.max_message_bytes,
        }),
        None => tonic::body::Body::new(body),
    };
    let grpc = match grpc_request(parts, body) {
        Ok(grpc) => grpc,
        Err(status) => return failed(status),
    };
    let response = core
        .oneshot(grpc)
        .await
        .unwrap_or_else(|never| match never {});
    let (head, body) = response.into_parts();
    let head_status = Status::from_header_map(&head.headers);
    let mut metadata = HeaderMap::new();
    copy_metadata(&head.headers, &HeaderMap::new(), &mut metadata);
    let body = ConnectStream {
        inner: body,
        frames: Frames::default(),
        output: method.map(|m| m.output()),
        head_status,
        queue: VecDeque::new(),
        finished: false,
    };
    stream_response(codec, metadata, Body::new(body))
}

fn stream_response(codec: Codec, metadata: HeaderMap, body: Body) -> Response<Body> {
    let mut response = Response::new(body);
    *response.headers_mut() = metadata;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(codec.stream_content_type()),
    );
    response
}

/// A body holding only an end-of-stream message with `status`.
fn end_only(status: &Status) -> Body {
    Body::from(end_message(Some(status), &HeaderMap::new()))
}

/// The end-of-stream envelope (flag `0x02`): the error, if any, and the
/// trailers as metadata.
fn end_message(error: Option<&Status>, trailers: &HeaderMap) -> Bytes {
    let mut end = serde_json::Map::new();
    if let Some(status) = error.filter(|s| s.code() != Code::Ok) {
        end.insert("error".to_owned(), error_json(status));
    }
    let mut metadata = serde_json::Map::new();
    for (name, value) in trailers {
        if is_protocol_header(name) {
            continue;
        }
        if let Ok(value) = value.to_str()
            && let Some(values) = metadata
                .entry(name.as_str())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                .as_array_mut()
        {
            values.push(value.into());
        }
    }
    if !metadata.is_empty() {
        end.insert("metadata".to_owned(), serde_json::Value::Object(metadata));
    }
    let json = serde_json::to_vec(&serde_json::Value::Object(end)).unwrap_or_default();
    frame(0x02, &json)
}

pin_project_lite::pin_project! {
    /// A JSON request stream as gRPC frames.
    struct JsonRequest {
        #[pin]
        inner: Body,
        frames: Frames,
        input: MessageDescriptor,
        queue: VecDeque<Bytes>,
        done: bool,
        limit: usize,
    }
}

impl http_body::Body for JsonRequest {
    type Data = Bytes;
    type Error = Status;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Status>>> {
        let mut this = self.project();
        loop {
            if let Some(chunk) = this.queue.pop_front() {
                return Poll::Ready(Some(Ok(Frame::data(chunk))));
            }
            if *this.done {
                return Poll::Ready(None);
            }
            match ready!(this.inner.as_mut().poll_frame(cx)) {
                Some(Ok(received)) => {
                    let Ok(data) = received.into_data() else {
                        continue;
                    };
                    this.frames.push(data);
                    if this.frames.pending() > *this.limit + HEADER {
                        *this.done = true;
                        return Poll::Ready(Some(Err(Status::resource_exhausted(format!(
                            "a message is larger than {} bytes",
                            this.limit
                        )))));
                    }
                    while let Some((flag, payload)) = this.frames.next_frame() {
                        if flag & 0x01 != 0 {
                            *this.done = true;
                            return Poll::Ready(Some(Err(Status::unimplemented(
                                "compressed Connect messages are not supported",
                            ))));
                        }
                        match json_to_proto(this.input, &payload) {
                            Ok(proto) => this.queue.push_back(frame(0, &proto)),
                            Err(status) => {
                                *this.done = true;
                                return Poll::Ready(Some(Err(status)));
                            }
                        }
                    }
                }
                Some(Err(error)) => {
                    *this.done = true;
                    return Poll::Ready(Some(Err(Status::cancelled(format!(
                        "reading the request failed: {error}"
                    )))));
                }
                None => *this.done = true,
            }
        }
    }
}

pin_project_lite::pin_project! {
    /// A gRPC response stream as Connect envelopes, ending with the
    /// end-of-stream message.
    struct ConnectStream {
        #[pin]
        inner: tonic::body::Body,
        frames: Frames,
        output: Option<MessageDescriptor>,
        head_status: Option<Status>,
        queue: VecDeque<Bytes>,
        finished: bool,
    }
}

impl http_body::Body for ConnectStream {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let mut this = self.project();
        loop {
            if let Some(chunk) = this.queue.pop_front() {
                return Poll::Ready(Some(Ok(Frame::data(chunk))));
            }
            if *this.finished {
                return Poll::Ready(None);
            }
            match ready!(this.inner.as_mut().poll_frame(cx)) {
                Some(Ok(received)) => match received.into_data() {
                    Ok(data) => {
                        this.frames.push(data);
                        while let Some((_, payload)) = this.frames.next_frame() {
                            let message = match this.output {
                                Some(output) => proto_to_json(output, &payload),
                                None => Ok(payload),
                            };
                            match message {
                                Ok(message) => this.queue.push_back(frame(0, &message)),
                                Err(status) => {
                                    this.queue
                                        .push_back(end_message(Some(&status), &HeaderMap::new()));
                                    *this.finished = true;
                                    break;
                                }
                            }
                        }
                    }
                    Err(received) => {
                        if let Ok(trailers) = received.into_trailers() {
                            let status = Status::from_header_map(&trailers);
                            this.queue
                                .push_back(end_message(status.as_ref(), &trailers));
                            *this.finished = true;
                        }
                    }
                },
                Some(Err(error)) => {
                    let status = Status::internal(format!("the response failed: {error}"));
                    this.queue
                        .push_back(end_message(Some(&status), &HeaderMap::new()));
                    *this.finished = true;
                }
                None => {
                    this.queue
                        .push_back(end_message(this.head_status.as_ref(), &HeaderMap::new()));
                    *this.finished = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_have_connect_names_and_http_statuses() {
        assert_eq!(code_name(Code::NotFound), "not_found");
        assert_eq!(code_name(Code::Cancelled), "canceled");
        assert_eq!(http_status(Code::NotFound), StatusCode::NOT_FOUND);
        assert_eq!(http_status(Code::Unauthenticated), StatusCode::UNAUTHORIZED);
        assert_eq!(
            http_status(Code::FailedPrecondition),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(http_status(Code::Cancelled).as_u16(), 499);
    }

    #[test]
    fn end_messages_carry_the_error_and_the_trailers() {
        let mut trailers = HeaderMap::new();
        trailers.insert("grpc-status", HeaderValue::from_static("5"));
        trailers.insert("x-trace", HeaderValue::from_static("abc"));
        let end = end_message(Some(&Status::not_found("no deal")), &trailers);
        assert_eq!(end[0], 0x02);
        let json: serde_json::Value = serde_json::from_slice(&end[HEADER..]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "error": { "code": "not_found", "message": "no deal" },
                "metadata": { "x-trace": ["abc"] },
            })
        );
        let ok = end_message(Some(&Status::ok("")), &HeaderMap::new());
        assert_eq!(&ok[HEADER..], b"{}");
    }
}

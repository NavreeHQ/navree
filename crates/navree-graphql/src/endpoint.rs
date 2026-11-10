//! The HTTP endpoint: `POST <path>` executes operations, `GET <path>`
//! serves GraphiQL (or answers 405), `GET <path>/assets/{file}` serves the
//! files GraphiQL loads, `GET <path>/ws` serves subscriptions.

use std::sync::Arc;

use async_graphql::{BatchRequest, BatchResponse, Response};
use http::header::{ACCEPT, ALLOW, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST};
use http::request::Parts;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree_contracts::AuthState;
use navree_http::body::{Body, Bytes};
use navree_http::response::{IntoResponse, Response as HttpResponse};
use navree_http::ws::WebSocketUpgrade;
use navree_http::{AppError, Controller, Path, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Resolver};

use crate::error::{is_request_error, request_error};
use crate::graphiql;
use crate::operation::Origin;
use crate::schema::GraphqlSchema;
use crate::ws;

/// The routes of `GraphqlModule`.
pub(crate) struct GraphqlController {
    schema: Arc<GraphqlSchema>,
}

impl Injectable for GraphqlController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                schema: r.get::<GraphqlSchema>().await?,
            })
        })
    }
}

impl Controller for GraphqlController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let options = self.schema.options();
        let path = format!("!{}", options.path);
        let post = self.schema.clone();
        let get = self.schema.clone();
        let mut routes = vec![
            RouteDef::on(
                Method::POST,
                path.clone(),
                move |parts: Parts, body: Bytes| {
                    let schema = post.clone();
                    async move { execute(&schema, parts, body).await }
                },
            )
            .name("graphql")
            .public(),
            RouteDef::on(Method::GET, path, move |parts: Parts| {
                let schema = get.clone();
                async move { graphiql_page(&schema, &parts) }
            })
            .name("graphql.graphiql")
            .public(),
        ];
        if options.graphiql {
            routes.push(
                RouteDef::on(
                    Method::GET,
                    format!("!{}/{{file}}", graphiql::assets_path(&options.path)),
                    |Path(file): Path<String>| async move { graphiql_asset(&file) },
                )
                .name("graphql.graphiql.assets")
                .public(),
            );
        }
        if options.subscriptions {
            let socket = self.schema.clone();
            routes.push(
                RouteDef::on(
                    Method::GET,
                    format!("!{}", options.ws_path()),
                    move |parts: Parts, upgrade: WebSocketUpgrade| {
                        let schema = socket.clone();
                        async move { ws::upgrade(&schema, parts, upgrade) }
                    },
                )
                .name("graphql.ws")
                .public(),
            );
        }
        routes
    }
}

/// The origin of an operation sent with the request `parts`.
pub(crate) fn origin(parts: &Parts) -> Origin {
    Origin {
        extensions: parts.extensions.clone(),
        headers: parts.headers.clone(),
        uri: parts.uri.clone(),
        principal: parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.clone()),
    }
}

/// `POST <path>`: one operation, or a batch when batching is on.
async fn execute(schema: &GraphqlSchema, parts: Parts, body: Bytes) -> HttpResponse {
    if !is_json(&parts.headers) {
        return AppError::from_status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .with_detail("Expected a GraphQL request with `Content-Type: application/json`")
            .into_response();
    }
    let batch = match serde_json::from_slice::<BatchRequest>(&body) {
        Ok(batch) => batch,
        Err(error) => {
            let response = Response::from_errors(vec![request_error(format!(
                "Invalid GraphQL request: {error}"
            ))]);
            return graphql_response(StatusCode::BAD_REQUEST, &response);
        }
    };
    let origin = origin(&parts);
    match batch {
        BatchRequest::Single(request) => {
            let response = schema.execute_from(request, origin).await;
            let status = if is_request_error(&response) {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::OK
            };
            graphql_response(status, &response)
        }
        BatchRequest::Batch(requests) => {
            if !schema.options().batching {
                let response = Response::from_errors(vec![request_error(
                    "Batched requests are disabled: send one operation per request \
                     (or turn on graphql.batching)",
                )]);
                return graphql_response(StatusCode::BAD_REQUEST, &response);
            }
            let mut responses = Vec::with_capacity(requests.len());
            for request in requests {
                responses.push(schema.execute_from(request, origin.clone()).await);
            }
            let body = serde_json::to_vec(&BatchResponse::Batch(responses)).unwrap_or_default();
            json(StatusCode::OK, body, HeaderMap::new())
        }
    }
}

/// A GraphQL response as JSON; a request error has no `data` member.
fn graphql_response(status: StatusCode, response: &Response) -> HttpResponse {
    let mut value = serde_json::to_value(response).unwrap_or_default();
    if status == StatusCode::BAD_REQUEST
        && let Some(object) = value.as_object_mut()
    {
        object.remove("data");
    }
    let body = serde_json::to_vec(&value).unwrap_or_default();
    json(status, body, response.http_headers.clone())
}

fn json(status: StatusCode, body: Vec<u8>, headers: HeaderMap) -> HttpResponse {
    let mut response = HttpResponse::new(Body::from(body));
    *response.status_mut() = status;
    let target = response.headers_mut();
    for (name, value) in &headers {
        target.append(name, value.clone());
    }
    target.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// Whether the request body is JSON (`application/json` or a `+json` type):
/// a cross-site form cannot send one without a CORS preflight.
fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|essence| essence.trim().to_ascii_lowercase())
        .is_some_and(|essence| essence == "application/json" || essence.ends_with("+json"))
}

/// `GET <path>`: GraphiQL for browsers when it is on; 405 otherwise.
fn graphiql_page(schema: &GraphqlSchema, parts: &Parts) -> HttpResponse {
    let options = schema.options();
    if options.graphiql && accepts_html(&parts.headers) {
        let subscriptions = options.subscriptions.then(|| options.ws_path());
        let html = graphiql::page(&options.path, subscriptions.as_deref());
        return (
            [
                (
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (
                    CONTENT_SECURITY_POLICY,
                    HeaderValue::from_str(&graphiql::csp(request_host(parts)))
                        .unwrap_or_else(|_| HeaderValue::from_static("default-src 'none'")),
                ),
            ],
            html,
        )
            .into_response();
    }
    AppError::from_status(StatusCode::METHOD_NOT_ALLOWED)
        .with_detail("Send GraphQL operations as POST requests with a JSON body")
        .with_header(ALLOW, HeaderValue::from_static("POST"))
        .into_response()
}

/// `GET <path>/assets/{file}`: a vendored GraphiQL file, cached for a year
/// (its name carries the version).
fn graphiql_asset(file: &str) -> HttpResponse {
    let Some(asset) = graphiql::asset(file) else {
        return AppError::not_found().into_response();
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(asset.content_type)),
            (
                CACHE_CONTROL,
                HeaderValue::from_static(graphiql::ASSET_CACHE),
            ),
        ],
        Bytes::from_static(asset.bytes),
    )
        .into_response()
}

/// The host the request was sent to: its `Host` header, else (HTTP/2) the
/// authority of its URI.
fn request_host(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| parts.uri.authority().map(|authority| authority.as_str()))
}

fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get_all(ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| value.contains("text/html"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_bodies_are_recognized() {
        let mut headers = HeaderMap::new();
        assert!(!is_json(&headers));
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        assert!(is_json(&headers));
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/graphql-response+json"),
        );
        assert!(is_json(&headers));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert!(!is_json(&headers));
    }
}

//! The REST client the server engines (Meilisearch, Typesense) and the
//! Qdrant vector store share: navree-http-client with the engine's key,
//! a per-request timeout, failover between nodes, and engine errors.

use std::time::Duration;

use http::Method;
use navree_http_client::{HttpClient, HttpClientError, HttpClientOptions, Response, SsrfPolicy};
use navree_kernel::BoxError;
use serde_json::Value;

use crate::SearchError;

/// How requests authenticate.
#[derive(Clone)]
pub(crate) enum Auth {
    /// `Authorization: Bearer <key>` when there is a key.
    Bearer(Option<String>),
    /// A custom header.
    Header(&'static str, Option<String>),
}

/// A request body.
pub(crate) enum Body {
    Json(Value),
    /// Raw text with its content type (Typesense's JSONL imports).
    Text(String, &'static str),
}

/// A JSON REST API on one or more nodes.
#[derive(Clone)]
pub(crate) struct Api {
    engine: &'static str,
    client: HttpClient,
    nodes: Vec<String>,
    auth: Auth,
}

impl Api {
    /// A client for `nodes` (tried in order). Search servers usually run on
    /// internal addresses, so the client allows private destinations.
    pub(crate) fn new(
        engine: &'static str,
        nodes: Vec<String>,
        auth: Auth,
        timeout: Duration,
    ) -> Result<Api, BoxError> {
        let client = HttpClient::new(HttpClientOptions {
            timeout_secs: timeout.as_secs().max(1),
            retries: 1,
            ssrf: SsrfPolicy {
                block_private: false,
                allow_hosts: Vec::new(),
            },
            ..HttpClientOptions::default()
        })?;
        Ok(Self::with_client(engine, nodes, auth, client))
    }

    /// The same, with a given client (tests use a fake).
    pub(crate) fn with_client(
        engine: &'static str,
        nodes: Vec<String>,
        auth: Auth,
        client: HttpClient,
    ) -> Api {
        Api {
            engine,
            client,
            nodes: nodes
                .into_iter()
                .map(|n| n.trim_end_matches('/').to_owned())
                .collect(),
            auth,
        }
    }

    /// Sends a request to the first node that answers. A connection error,
    /// a timeout or a 5xx moves on to the next node; when every node fails
    /// the error is [`SearchError::Unavailable`]. A status in `allowed` is
    /// returned as it is; any other 4xx is [`SearchError::Engine`].
    pub(crate) async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&Body>,
        allowed: &[u16],
    ) -> Result<Response, SearchError> {
        let mut last = String::from("no node configured");
        for node in &self.nodes {
            let mut request = self.client.request(method.clone(), format!("{node}{path}"));
            request = match &self.auth {
                Auth::Bearer(Some(key)) => request.bearer(key),
                Auth::Header(name, Some(key)) => request.header(*name, key.as_str()),
                _ => request,
            };
            request = match body {
                Some(Body::Json(json)) => request.json(json),
                Some(Body::Text(text, content_type)) => request
                    .header("content-type", *content_type)
                    .body(text.clone()),
                None => request,
            };
            match request.send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if response.is_success() || allowed.contains(&status) {
                        return Ok(response);
                    }
                    if status >= 500 {
                        last = format!("{} answered {status}: {}", node, message_of(&response));
                        continue;
                    }
                    return Err(SearchError::Engine {
                        engine: self.engine,
                        message: format!("{status}: {}", message_of(&response)),
                    });
                }
                Err(
                    error @ (HttpClientError::Connect { .. }
                    | HttpClientError::Timeout { .. }
                    | HttpClientError::Request { .. }),
                ) => {
                    last = error.to_string();
                }
                Err(error) => {
                    return Err(SearchError::Engine {
                        engine: self.engine,
                        message: error.to_string(),
                    });
                }
            }
        }
        Err(SearchError::Unavailable {
            engine: self.engine,
            message: last,
        })
    }

    /// [`send`](Api::send) with a JSON answer.
    pub(crate) async fn json(
        &self,
        method: Method,
        path: &str,
        body: Option<&Body>,
    ) -> Result<Value, SearchError> {
        let response = self.send(method, path, body, &[]).await?;
        self.decode(&response)
    }

    /// The JSON body of `response`.
    pub(crate) fn decode(&self, response: &Response) -> Result<Value, SearchError> {
        if response.bytes().is_empty() {
            return Ok(Value::Null);
        }
        response.json().map_err(|e| SearchError::Engine {
            engine: self.engine,
            message: format!("unexpected answer: {e}"),
        })
    }
}

/// The error message an engine put in its answer (`message`), or the body.
fn message_of(response: &Response) -> String {
    let text = response.text();
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or(text)
}

/// Percent-encodes a path segment or query value.
pub(crate) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_keeps_unreserved_characters() {
        assert_eq!(encode("crm_contacts-1.x~"), "crm_contacts-1.x~");
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
        assert_eq!(encode("\u{E000}"), "%EE%80%80");
    }
}

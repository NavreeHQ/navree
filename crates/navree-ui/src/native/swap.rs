//! Where a tree becomes bytes: the answers to a native client.
//!
//! A browser's request passes through here untouched. For a native client's
//! request the reply layer makes the client current, and then:
//!
//! - a [`View`] answers the native envelope with the page as a screen, or the
//!   word to show the page in the web view when a part of it could not be read;
//! - [`Op::morph`](crate::Op::morph) and the like carry the node instead of
//!   markup, and a reply's ops go out as native ops; a reply whose ops can't
//!   all be said natively answers its fallback, the redirect a browser
//!   without scripts would get, which the client follows;
//! - HTML that reached the layer without passing through a `View` is never
//!   sent as it is: the client is told to show the address in its web view.
//!
//! For the web view inside a native client (`X-Navree-Embed: 1`), a page is
//! written as HTML without the app's chrome, which the client draws itself.

use std::sync::Arc;

use http::header::{
    CACHE_CONTROL, CONTENT_ENCODING, CONTENT_SECURITY_POLICY, CONTENT_TYPE, SET_COOKIE, VARY,
};
use http::{HeaderValue, StatusCode};
use navree_http::extract::Request;
use navree_http::middleware::Next;
use navree_http::response::{IntoResponse, Response};
use navree_native::{CONTENT_TYPE as NATIVE_TYPE, EMBED_HEADER, Envelope, NATIVE_HEADER, Web};
use serde::Serialize;

use super::lower::{Cx, Miss, N};
use super::page::{self, Page};
use super::scope::{self, Asked, Reading};
use crate::node::Node;
use crate::op::{Op, PendingOps, UI_HEADER};
use crate::view::View;
use crate::{assets, compress};

/// How a node op changes the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum How {
    /// In place of the target, keeping what the reader did.
    Morph,
    /// In place of the target, as new.
    Replace,
    /// At the end of the target.
    Append,
    /// At the start of the target.
    Prepend,
}

/// An op whose payload is native parts: what [`Op::morph`], [`Op::replace`],
/// [`Op::append`] and [`Op::prepend`] build when a native client asked, in
/// place of the markup a browser gets.
///
/// [`Op::morph`]: crate::Op::morph
/// [`Op::replace`]: crate::Op::replace
/// [`Op::append`]: crate::Op::append
/// [`Op::prepend`]: crate::Op::prepend
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NodeOp {
    /// How the screen changes.
    pub how: How,
    /// The id of the node that changes.
    pub target: String,
    /// The parts: one for a morph or a replace, any number to add.
    pub nodes: Vec<N>,
    /// What could not be read. A reply with a miss in any op answers its fallback.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub misses: Vec<Miss>,
}

/// The node op for `node`, when a native client asked: what the ops'
/// constructors call before they render.
#[inline]
pub(crate) fn node_op(how: How, target: &str, node: &Node) -> Option<Op> {
    let request = scope::native()?;
    let mut cx = Cx::new(false);
    let mut nodes = cx.sequence(std::slice::from_ref(node));
    if matches!(how, How::Morph | How::Replace) && nodes.len() != 1 {
        nodes = vec![super::lower::one(nodes)];
    }
    let mut misses = std::mem::take(&mut cx.misses);
    for node in &nodes {
        if let Some(kind) = navree_native::vocab::newer_than(node, request.client.vocab) {
            misses.push(Miss {
                what: format!("vocab:{}", kind.as_str()),
                why: "newer than the client draws".to_owned(),
            });
        }
    }
    Some(Op::Native(NodeOp {
        how,
        target: target.to_owned(),
        nodes,
        misses,
    }))
}

/// A reply's ops as native ops. `None` when one of them can't be said
/// natively: markup built outside a native request, or a node with a miss.
pub(crate) fn native_ops(ops: &[Op]) -> Option<Vec<navree_native::Op>> {
    use navree_native::Op as O;
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        out.push(match op {
            Op::Native(op) => {
                if !op.misses.is_empty() {
                    return None;
                }
                let target = op.target.clone();
                let mut nodes = op.nodes.clone();
                match op.how {
                    How::Morph => O::Morph {
                        target,
                        node: nodes.pop()?,
                    },
                    How::Replace => O::Replace {
                        target,
                        node: nodes.pop()?,
                    },
                    How::Append => O::Append {
                        target,
                        nodes,
                        more: None,
                    },
                    How::Prepend => O::Prepend { target, nodes },
                }
            }
            Op::Morph { .. } | Op::Replace { .. } | Op::Append { .. } | Op::Prepend { .. } => {
                return None;
            }
            Op::Remove { target } => O::Remove {
                target: target.clone(),
            },
            Op::Errors { form, fields } => O::Errors {
                form: form.clone(),
                fields: fields.clone(),
            },
            // A toast is the same data on both wires, less how long it stays.
            Op::Toast(toast) => O::Toast(
                serde_json::to_value(toast)
                    .ok()
                    .and_then(|value| serde_json::from_value(value).ok())?,
            ),
            Op::Open { target } => O::Open {
                target: target.clone(),
            },
            Op::Close { target } => O::Close {
                target: target.clone(),
            },
            Op::Focus { target } => O::Focus {
                target: target.clone(),
            },
            Op::Url { href, mode } => O::Url {
                href: href.clone(),
                mode: match mode {
                    crate::op::UrlMode::Push => navree_native::op::UrlMode::Push,
                    crate::op::UrlMode::Replace => navree_native::op::UrlMode::Replace,
                },
            },
            Op::Navigate { href } => O::Navigate { href: href.clone() },
            Op::NavigateExternal { href } => O::NavigateExternal { href: href.clone() },
            Op::Reload => O::Reload,
            Op::Invalidate { target } => O::Invalidate {
                target: target.clone(),
            },
        });
    }
    Some(out)
}

/// What a native answer was, kept in the response's extensions: for a test,
/// the coverage report and a developer's overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answered {
    /// Whether the page went out as a screen. `false` for the web view.
    pub native: bool,
    /// What could not be read, when it did not.
    pub why: Vec<String>,
    /// Decoration that was left out of the screen.
    pub dropped: Vec<String>,
}

/// An envelope as a response.
fn respond(status: StatusCode, cache: &'static str, envelope: &Envelope) -> Response {
    let body = serde_json::to_string(envelope)
        .unwrap_or_else(|_| format!("{{\"navree\":{}}}", navree_native::MAJOR));
    (
        status,
        [
            (CONTENT_TYPE, HeaderValue::from_static(NATIVE_TYPE)),
            (CACHE_CONTROL, HeaderValue::from_static(cache)),
            (VARY, HeaderValue::from_static(NATIVE_HEADER)),
        ],
        body,
    )
        .into_response()
}

/// The envelope a page answers a native client with: its screen, or the word
/// to show it in the web view, with why.
pub(crate) fn envelope(page: Page, request: &scope::Request) -> (Envelope, Answered) {
    let mut why = page.why();
    if why.is_empty() {
        let newer = std::iter::once(&page.screen.body)
            .chain(&page.screen.actions)
            .find_map(|node| navree_native::vocab::newer_than(node, request.client.vocab));
        if let Some(kind) = newer {
            why.push(format!("vocab:{}", kind.as_str()));
        }
    }
    let vocab = request.client.vocab.min(navree_native::VOCAB);
    if why.is_empty() {
        let mut envelope = Envelope::screen(assets::BUILD, page.screen).in_vocab(vocab);
        envelope.ops = page.ops;
        (
            envelope,
            Answered {
                native: true,
                why: Vec::new(),
                dropped: page.dropped,
            },
        )
    } else {
        let mut envelope = Envelope::web(
            assets::BUILD,
            Web {
                url: request.url.clone(),
                title: Some(page.screen.title),
                why: why.clone(),
            },
        )
        .in_vocab(vocab);
        // A toast left for the page is shown whichever way the page is.
        envelope.ops = page.ops;
        (
            envelope,
            Answered {
                native: false,
                why,
                dropped: Vec::new(),
            },
        )
    }
}

/// A page's answer when the request did not come from a browser. `Err` hands
/// the page back for the HTML path, untouched: the request came from a
/// browser, or from outside any request.
#[inline]
pub(crate) fn view(view: View) -> Result<Response, View> {
    match scope::asked() {
        None => Err(view),
        Some(Asked::Native(request)) => Ok(native(view, &request)),
        Some(Asked::Embed) => Ok(embed(view)),
    }
}

fn native(mut view: View, request: &Arc<scope::Request>) -> Response {
    let parts = view.native_parts();
    let page = page::read(parts.title, parts.body, false);
    let (envelope, answered) = envelope(page, request);
    let mut response = respond(parts.status, parts.cache, &envelope);
    for cookie in parts.cookies.iter() {
        response.headers_mut().append(SET_COOKIE, cookie.clone());
    }
    response.extensions_mut().insert(answered);
    response
}

/// The page as HTML without the app's chrome: what stands in `<main>`, with
/// the page's own toasts. Its links are not boosted, so each is a navigation
/// the client can take back.
fn embed(mut view: View) -> Response {
    {
        let parts = view.native_parts();
        unshell(parts.body);
    }
    let html = view.to_html().replacen("<body data-ui-boost>", "<body>", 1);
    let parts = view.native_parts();
    let mut response = (
        parts.status,
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (
                CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(crate::view::PAGE_CSP),
            ),
            (CACHE_CONTROL, HeaderValue::from_static(parts.cache)),
            (VARY, HeaderValue::from_static(EMBED_HEADER)),
        ],
        html,
    )
        .into_response();
    for cookie in parts.cookies.iter() {
        response.headers_mut().append(SET_COOKIE, cookie.clone());
    }
    response
}

/// Takes the app's shell off a page: the content of `<main>`, less its
/// heading (the client's own bar says it), takes the shell's place.
fn unshell(body: &mut Node) {
    let nodes = match body {
        Node::Fragment(nodes) => nodes,
        _ => return,
    };
    for node in nodes.iter_mut() {
        let Node::Element(wrapper) = node else {
            continue;
        };
        if super::lower::slot(wrapper).as_deref() != Some("sidebar-wrapper") {
            continue;
        }
        let Some(mut main) = take_main(&mut wrapper.children) else {
            return;
        };
        for child in &mut main.children {
            let Node::Element(header) = child else {
                continue;
            };
            if super::lower::slot(header).as_deref() == Some("page-header") {
                header.children.retain(|part| {
                    !matches!(part, Node::Element(title)
                        if super::lower::slot(title).as_deref() == Some("page-title"))
                });
            }
        }
        *node = Node::Element(main);
        return;
    }
}

/// Moves the `<main>` of a page out of the shell it stands in.
fn take_main(nodes: &mut [Node]) -> Option<crate::node::Element> {
    for node in nodes.iter_mut() {
        match node {
            Node::Element(element) => {
                if super::lower::slot(element).as_deref() == Some("page") {
                    return std::mem::take(node).into_element();
                }
                if let Some(found) = take_main(&mut element.children) {
                    return Some(found);
                }
            }
            Node::Fragment(nodes) => {
                if let Some(found) = take_main(nodes) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// The layer around a request that did not come from a browser. `Err` hands
/// the request back for the web path, untouched, at the cost of two header
/// lookups.
pub(crate) async fn layer(request: Request, next: Next) -> Result<Response, (Request, Next)> {
    if !request.headers().contains_key(NATIVE_HEADER)
        && !request.headers().contains_key(EMBED_HEADER)
    {
        return Err((request, next));
    }
    let url = request.uri().path_and_query().map_or_else(
        || request.uri().path().to_owned(),
        |url| url.as_str().to_owned(),
    );
    Ok(match scope::read(request.headers(), &url) {
        Reading::Web => return Err((request, next)),
        Reading::UnknownMajor(major) => refuse(
            StatusCode::NOT_ACCEPTABLE,
            &format!(
                "this server speaks version {} of the native protocol, not `{major}`",
                navree_native::MAJOR
            ),
        ),
        Reading::BadClient(error) => refuse(StatusCode::BAD_REQUEST, &error.to_string()),
        Reading::Asked(Asked::Embed) => embedded(request, next).await,
        Reading::Asked(Asked::Native(native)) => answer(native, request, next).await,
    })
}

/// Why a native request was not served, as plain text.
fn refuse(status: StatusCode, why: &str) -> Response {
    (
        status,
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        why.to_owned(),
    )
        .into_response()
}

/// A request from the web view inside a native client: the web path, with
/// pages drawn without the app's chrome.
async fn embedded(request: Request, next: Next) -> Response {
    let from_runtime = request.headers().contains_key(UI_HEADER);
    let accept = compress::accept_of(request.headers());
    let method = request.method().clone();
    let mut response = scope::scope(Asked::Embed, next.run(request)).await;
    if let Some(PendingOps(envelope, status)) = response.extensions_mut().remove::<PendingOps>() {
        if from_runtime {
            response = crate::module::to_ops(response, envelope, status);
        }
        response
            .headers_mut()
            .append(VARY, HeaderValue::from_static(UI_HEADER));
    }
    compress::encode(&method, &accept, response).await
}

/// A native client's request: the handler runs with the client current, and
/// whatever it answers leaves as an envelope or as something that is not a page.
async fn answer(native: Arc<scope::Request>, request: Request, next: Next) -> Response {
    let accept = compress::accept_of(request.headers());
    let mut response = scope::scope(Asked::Native(native.clone()), next.run(request)).await;
    if let Some(PendingOps(envelope, status)) = response.extensions_mut().remove::<PendingOps>() {
        // Ops that can all be said natively take the fallback's place. When
        // they can't, the fallback stands: a redirect the client follows to a
        // screen, or a page that was answered as a screen already.
        if let Some(ops) = native_ops(&envelope.ops) {
            let (parts, _) = crate::module::to_ops(response, envelope, status).into_parts();
            let mut native = respond(
                parts.status,
                "no-store",
                &Envelope::ops(assets::BUILD, ops)
                    .in_vocab(native.client.vocab.min(navree_native::VOCAB)),
            );
            for (name, value) in &parts.headers {
                if !matches!(
                    *name,
                    CONTENT_TYPE | CACHE_CONTROL | http::header::CONTENT_LENGTH | CONTENT_ENCODING
                ) {
                    native.headers_mut().append(name, value.clone());
                }
            }
            *native.extensions_mut() = parts.extensions;
            response = native;
        }
    }
    let html = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    if html {
        // Markup some other way than through a `View`: a fragment a handler
        // rendered itself, an error page from outside the toolkit.
        let (parts, _) = response.into_parts();
        let (envelope, answered) = envelope(
            Page {
                screen: navree_native::Screen::new("", N::Stack(Default::default())),
                ops: Vec::new(),
                misses: vec![Miss {
                    what: "html".to_owned(),
                    why: "markup that was not built as a page".to_owned(),
                }],
                dropped: Vec::new(),
            },
            &native,
        );
        let mut wrapped = respond(parts.status, "no-store", &envelope);
        for value in parts.headers.get_all(SET_COOKIE) {
            wrapped.headers_mut().append(SET_COOKIE, value.clone());
        }
        *wrapped.extensions_mut() = parts.extensions;
        wrapped.extensions_mut().insert(answered);
        response = wrapped;
    }
    squeeze(&accept, response).await
}

/// Sends an envelope in brotli to a client that takes it, when it is big
/// enough to be worth it.
async fn squeeze(accept: &str, response: Response) -> Response {
    let ours = response
        .headers()
        .get(CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes() == NATIVE_TYPE.as_bytes());
    if !ours || response.headers().contains_key(CONTENT_ENCODING) {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = navree_http::body::to_bytes(body, usize::MAX).await else {
        return Response::from_parts(parts, navree_http::body::Body::empty());
    };
    if !compress::worth_brotli(bytes.len(), accept) {
        return Response::from_parts(parts, navree_http::body::Body::from(bytes));
    }
    parts.headers.remove(http::header::CONTENT_LENGTH);
    parts
        .headers
        .insert(CONTENT_ENCODING, HeaderValue::from_static("br"));
    parts
        .headers
        .append(VARY, HeaderValue::from_static("accept-encoding"));
    Response::from_parts(
        parts,
        navree_http::body::Body::from(bytes::Bytes::from(compress::brotli(&bytes))),
    )
}

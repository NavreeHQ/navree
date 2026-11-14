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
use navree_native::{CONTENT_TYPE as NATIVE_TYPE, Envelope, NATIVE_HEADER, Web};
use serde::Serialize;

use super::lower::{Cx, Miss, N};
use super::page::{self, Page};
use super::scope::{self, Asked, Reading};
use crate::asked::{WhoAsked, in_web_view};
use crate::node::Node;
use crate::op::{OPS_CONTENT_TYPE, Op, PendingOps, UI_HEADER, is_prefetch};
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

/// Whether a post to `target` stays on this site: a path, a query or a
/// fragment, never another host or a scheme. Only such a post is given the
/// session's token.
pub(crate) fn on_this_site(target: &str) -> bool {
    crate::html::LocalHref::parse(target).is_ok()
}

/// What a native envelope varies on: who asked, and what the client said of
/// itself (its system, form and vocabulary).
const ENVELOPE_VARY: &str = "x-navree-native, x-navree-client";

/// Adds each header of `vary` (a page's own, such as `Cookie`) to the `Vary`
/// of `response`.
pub(crate) fn vary_also(response: &mut Response, vary: &[&'static str]) {
    for header in vary {
        if let Ok(value) = HeaderValue::from_str(header) {
            response.headers_mut().append(VARY, value);
        }
    }
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
        if node.depth() > navree_native::MAX_DEPTH {
            misses.push(Miss {
                what: "depth".to_owned(),
                why: "deeper than a client reads".to_owned(),
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
                    How::Prepend => O::Prepend {
                        target,
                        nodes,
                        before: None,
                    },
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
            Op::Toast(toast) => {
                let mut said: navree_native::op::Toast = serde_json::to_value(toast)
                    .ok()
                    .and_then(|value| serde_json::from_value(value).ok())?;
                // A client posts a toast's action itself: with the page's token, when it was
                // given and the post stays on this site.
                if let (Some(action), Some(token)) = (
                    said.action.as_mut(),
                    toast.offered().and_then(crate::ToastAction::token),
                ) && action.post
                    && on_this_site(&action.href)
                {
                    action.fields.push(navree_native::action::Pair {
                        name: crate::form::CSRF_FIELD.to_owned(),
                        value: token.to_owned(),
                    });
                }
                O::Toast(said)
            }
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
            Op::Navigate { href } => O::Navigate {
                href: href.clone(),
                present: navree_native::Present::Auto,
            },
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
#[non_exhaustive]
pub struct Answered {
    /// Whether the page went out as native parts. `false` for the web view.
    pub native: bool,
    /// How much the answer belongs to the client's system.
    pub grade: super::Grade,
    /// What could not be read, when it did not.
    pub why: Vec<String>,
    /// What keeps a native screen from being idiom, in words.
    pub missing: Vec<String>,
    /// Decoration that was left out of the screen.
    pub dropped: Vec<String>,
}

impl Answered {
    /// A screen, graded.
    pub fn screen(screen: &navree_native::Screen, dropped: Vec<String>) -> Self {
        let (grade, missing) = super::graded(screen);
        Self {
            native: true,
            grade,
            why: Vec::new(),
            missing,
            dropped,
        }
    }

    /// Native parts that are not a screen: rows, options.
    pub fn parts(dropped: Vec<String>) -> Self {
        Self {
            native: true,
            grade: super::Grade::Idiom,
            why: Vec::new(),
            missing: Vec::new(),
            dropped,
        }
    }

    /// The web view, or nothing native, and why.
    pub fn web(why: Vec<String>) -> Self {
        Self {
            native: false,
            grade: super::Grade::Web,
            why,
            missing: Vec::new(),
            dropped: Vec::new(),
        }
    }

    /// What an envelope a client was sent says of itself: for a test that
    /// reads the answers of a running app.
    pub fn of(envelope: &Envelope) -> Self {
        match (&envelope.screen, &envelope.web) {
            (_, Some(web)) => Self::web(web.why.clone()),
            (Some(screen), None) => Self::screen(screen, Vec::new()),
            (None, None) => Self::parts(Vec::new()),
        }
    }
}

/// `envelope` as the client `request` gets it: in the client's vocabulary,
/// stamped with the app's stamp and `shell`'s, its icon shapes said once each.
pub(crate) fn finished(envelope: Envelope, request: &scope::Request, shell: &str) -> Envelope {
    let mut envelope = envelope
        .in_vocab(vocab_of(request))
        .stamped(request.stamp.as_str(), shell);
    envelope.share_icons();
    envelope
}

/// An envelope as the client `request` gets it, as a response: in the
/// client's vocabulary, stamped with the app's stamp and `shell`'s, its icon
/// shapes said once each. JSON nested deeper than a client reads is refused
/// as the web view.
pub(crate) fn respond(
    status: StatusCode,
    cache: &'static str,
    mut envelope: Envelope,
    request: &scope::Request,
    shell: &str,
) -> Response {
    envelope = finished(envelope, request, shell);
    // Measured before it is written: serialising goes down a tree on the stack, so a tree
    // deeper than a client reads is never handed to it, whichever path made it.
    let too_deep = deepest(&envelope) > navree_native::MAX_DEPTH;
    let mut body = if too_deep {
        String::new()
    } else {
        serde_json::to_string(&envelope)
            .unwrap_or_else(|_| format!("{{\"navree\":{}}}", navree_native::MAJOR))
    };
    if too_deep || navree_native::nesting(&body) > navree_native::MAX_NESTING {
        let web = Envelope::web(
            assets::BUILD,
            Web {
                url: request.url.clone(),
                title: envelope.screen.map(|screen| screen.title),
                why: vec!["depth".to_owned()],
            },
        )
        .in_vocab(envelope.vocab)
        .stamped(envelope.app, envelope.shell);
        body = serde_json::to_string(&web).unwrap_or_default();
    }
    (
        status,
        [
            (CONTENT_TYPE, HeaderValue::from_static(NATIVE_TYPE)),
            (CACHE_CONTROL, HeaderValue::from_static(cache)),
            (VARY, HeaderValue::from_static(ENVELOPE_VARY)),
        ],
        body,
    )
        .into_response()
}

/// How deep the nodes of `envelope` nest, its screen's and its ops': measured with
/// a list of its own, so it costs no stack however deep they are.
pub(crate) fn deepest(envelope: &Envelope) -> usize {
    use navree_native::Op as O;
    let mut deepest = 0;
    if let Some(screen) = &envelope.screen {
        for node in std::iter::once(&screen.body).chain(&screen.actions) {
            deepest = deepest.max(node.depth());
        }
    }
    for op in &envelope.ops {
        let nodes: Vec<&N> = match op {
            O::Morph { node, .. } | O::Replace { node, .. } => vec![node],
            O::Append { nodes, .. } | O::Prepend { nodes, .. } | O::Merge { nodes, .. } => {
                nodes.iter().collect()
            }
            O::Rows { rows, .. } => rows.iter().flat_map(|row| row.nodes()).collect(),
            O::Row { row, .. } => row.nodes().collect(),
            _ => Vec::new(),
        };
        for node in nodes {
            deepest = deepest.max(node.depth());
        }
    }
    deepest
}

/// The envelope a page answers a native client with: its screen in the
/// client's vocabulary, or the word to show it in the web view, with why: a
/// part that could not be read, a kind the client lacks with no older form,
/// or a tree deeper than a client reads.
pub(crate) fn envelope(page: Page, request: &scope::Request) -> (Envelope, Answered) {
    let mut why = page.why();
    let mut screen = page.screen;
    if why.is_empty() {
        let to = request.client.vocab;
        let body = std::mem::replace(&mut screen.body, N::Stack(Default::default()));
        let parts = std::iter::once(body).chain(std::mem::take(&mut screen.actions));
        let mut read = Vec::new();
        for part in parts {
            match navree_native::vocab::downgrade(part, to) {
                navree_native::vocab::Downgraded::Node(node) => read.push(node),
                navree_native::vocab::Downgraded::Web { why: more } => why.extend(more),
            }
        }
        let mut read = read.into_iter();
        if let Some(body) = read.next() {
            screen.body = body;
        }
        screen.actions = read.collect();
    }
    if why.is_empty() && too_deep(&screen) {
        why.push("depth".to_owned());
    }
    if why.is_empty() {
        let answered = Answered::screen(&screen, page.dropped);
        let mut envelope = Envelope::screen(assets::BUILD, screen);
        envelope.ops = page.ops;
        (envelope, answered)
    } else {
        let mut envelope = Envelope::web(
            assets::BUILD,
            Web {
                url: request.url.clone(),
                title: Some(screen.title),
                why: why.clone(),
            },
        );
        // A toast left for the page is shown whichever way the page is.
        envelope.ops = page.ops;
        (envelope, Answered::web(why))
    }
}

/// Whether a client can't read `screen`: a part sits deeper in it than
/// [`MAX_DEPTH`](navree_native::MAX_DEPTH). Measured without recursion.
pub(crate) fn too_deep(screen: &navree_native::Screen) -> bool {
    std::iter::once(&screen.body)
        .chain(&screen.actions)
        .any(|node| node.depth() > navree_native::MAX_DEPTH)
}

/// What a form that was refused answers a native client: the messages at its
/// fields, as an op. `None` when no native client asked.
pub(crate) fn errors(
    form: &str,
    fields: &std::collections::BTreeMap<String, Vec<String>>,
) -> Option<Response> {
    let request = scope::native()?;
    let envelope = Envelope::ops(
        assets::BUILD,
        vec![navree_native::Op::Errors {
            form: form.to_owned(),
            fields: fields.clone(),
        }],
    )
    .in_vocab(vocab_of(&request));
    Some(respond(
        StatusCode::UNPROCESSABLE_ENTITY,
        "no-store",
        envelope,
        &request,
        "",
    ))
}

/// The native client that sent a request with these headers, for an answer
/// made outside the reply layer: an error page. `None` for anyone else, and
/// for a descriptor that can't be read.
pub(crate) async fn client_of(
    headers: &http::HeaderMap,
    uri: &http::Uri,
    ctx: &navree_kernel::AppContext,
) -> Option<Arc<scope::Request>> {
    let url = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), |url| url.as_str().to_owned());
    match scope::read(headers, &url) {
        Reading::Asked(Asked::Native(request)) => Some(stamped(request, Some(ctx)).await),
        _ => None,
    }
}

/// `ops` as the envelope a native client gets, with `status`. Ops that can't
/// be said natively leave the client nothing to change: the status says what
/// happened.
pub(crate) fn ops_for(request: &scope::Request, status: StatusCode, ops: &[Op]) -> Response {
    let envelope = match native_ops(ops) {
        Some(ops) => Envelope::ops(assets::BUILD, ops),
        None => Envelope::ops(assets::BUILD, Vec::new()),
    }
    .in_vocab(vocab_of(request));
    respond(status, "no-store", envelope, request, "")
}

/// `ops` for the native client the current request comes from, as an
/// envelope with `status`. `None` when no native client asked, or one of the
/// ops can't be said natively.
pub(crate) fn ops(status: StatusCode, ops: &[Op]) -> Option<Response> {
    let request = scope::native()?;
    let native = native_ops(ops)?;
    Some(respond(
        status,
        "no-store",
        Envelope::ops(assets::BUILD, native),
        &request,
        "",
    ))
}

/// A response with what the answer was kept beside it, for the coverage
/// report.
fn kept(mut response: Response, answered: Answered) -> Response {
    response.extensions_mut().insert(answered);
    response
}

/// The new rows of a live feed for the native client the current request comes from:
/// the events to merge into the feed's list at its top, with the cursor of the newest
/// as the revision the list is at now (the one it asked from, when nothing is new).
/// `None` when no native client asked.
pub(crate) fn feed_rows(rows: &Node, cursor: Option<&str>) -> Option<Response> {
    let request = scope::native()?;
    let vocab = vocab_of(&request);
    let mut cx = Cx::new(false);
    let read = super::slots::feed::rows(&mut cx, std::slice::from_ref(rows));
    let nodes = match read {
        Ok(nodes) if cx.misses.is_empty() => nodes,
        other => {
            let why = match other {
                Err(fail) => vec![fail.what.unwrap_or_else(|| "slot:activity-item".to_owned())],
                Ok(_) => {
                    let mut why: Vec<String> = Vec::new();
                    for miss in &cx.misses {
                        if !why.contains(&miss.what) {
                            why.push(miss.what.clone());
                        }
                    }
                    why
                }
            };
            let envelope = Envelope::web(
                assets::BUILD,
                Web {
                    url: request.url.clone(),
                    title: None,
                    why: why.clone(),
                },
            )
            .in_vocab(vocab);
            return Some(kept(
                respond(StatusCode::OK, "no-store", envelope, &request, ""),
                Answered::web(why),
            ));
        }
    };
    let query = request.url.split_once('?').map_or("", |(_, query)| query);
    let asked = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("since="))
        .and_then(unpercent)
        .unwrap_or_default();
    let ops = vec![navree_native::Op::Merge {
        target: request.region.clone().unwrap_or_default(),
        nodes,
        gone: Vec::new(),
        since: cursor.map_or(asked, str::to_owned),
        at: navree_native::op::Edge::Start,
        more: false,
    }];
    let envelope = Envelope::ops(assets::BUILD, ops).in_vocab(vocab);
    Some(kept(
        respond(StatusCode::OK, "no-store", envelope, &request, ""),
        Answered::parts(std::mem::take(&mut cx.dropped)),
    ))
}

/// `text` as a query value: the bytes that are not plain stay as `%` and two digits.
pub(crate) fn percent(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[usize::from(byte >> 4)]));
                out.push(char::from(b"0123456789ABCDEF"[usize::from(byte & 15)]));
            }
        }
    }
    out
}

/// The address of the next rows of a lazy list, from the address that asked:
/// its own query less the list's words, then the direction and the cursor.
fn next_rows(url: &str, dir: &str, cursor: &str) -> String {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let mut out = String::from(path);
    let mut first = true;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let name = pair.split_once('=').map_or(pair, |(name, _)| name);
        if matches!(name, "dir" | "cursor" | "since" | "from" | "to") {
            continue;
        }
        out.push(if first { '?' } else { '&' });
        out.push_str(pair);
        first = false;
    }
    out.push(if first { '?' } else { '&' });
    out.push_str("dir=");
    out.push_str(dir);
    out.push_str("&cursor=");
    out.push_str(&percent(cursor));
    out
}

/// A lazy list's page of rows for the native client the current request
/// comes from: the rows as an `append` (or a `prepend`, for the rows before)
/// to the list the client named, with the address of the rows after them (or
/// before them). A live ask, for what changed since a revision, is a `merge`:
/// the rows that are new or changed, the keys that left and the revision the
/// list is at now. `None` when no native client asked.
pub(crate) fn lazy_page(
    items: &[Node],
    more: bool,
    since: Option<&str>,
    gone: &[String],
    joins_start: bool,
) -> Option<Response> {
    let request = scope::native()?;
    let target = request.region.clone().unwrap_or_default();
    let vocab = vocab_of(&request);
    let query = request.url.split_once('?').map_or("", |(_, query)| query);
    let before = matches!(
        crate::components::LazyQuery::parse(query).ask(),
        crate::components::LazyAsk::Before(_)
    );
    let mut cx = Cx::new(false);
    let mut nodes = Vec::with_capacity(items.len());
    let mut first = None;
    let mut last = None;
    for item in items {
        let Node::Element(item) = item else { continue };
        let key = super::lower::attr(item, "data-key").map(std::borrow::Cow::into_owned);
        last = super::lower::attr(item, "data-cursor").map(std::borrow::Cow::into_owned);
        if first.is_none() {
            first.clone_from(&last);
        }
        let read = cx.of(item);
        nodes.push(super::lower::with_id(super::lower::one(read), key));
    }
    let misses = std::mem::take(&mut cx.misses);
    let mut why: Vec<String> = Vec::new();
    for miss in misses {
        if !why.contains(&miss.what) {
            why.push(miss.what);
        }
    }
    if !why.is_empty() {
        // Rows a client can't draw are not sent: the list is the web view's,
        // and the client is told why.
        let envelope = Envelope::web(
            assets::BUILD,
            Web {
                url: request.url.clone(),
                title: None,
                why: why.clone(),
            },
        )
        .in_vocab(vocab);
        return Some(kept(
            respond(StatusCode::OK, "no-store", envelope, &request, ""),
            Answered::web(why),
        ));
    }
    let ops = if let Some(since) = since {
        // The answer says where a row that is new joins.
        let at = if joins_start {
            navree_native::op::Edge::Start
        } else {
            navree_native::op::Edge::End
        };
        vec![navree_native::Op::Merge {
            target,
            nodes,
            gone: gone.to_vec(),
            since: since.to_owned(),
            at,
            more,
        }]
    } else if before {
        let rows_before = more
            .then(|| first.map(|cursor| next_rows(&request.url, "start", &cursor)))
            .flatten();
        vec![navree_native::Op::Prepend {
            target,
            nodes,
            before: rows_before,
        }]
    } else {
        let more = more
            .then(|| last.map(|cursor| next_rows(&request.url, "end", &cursor)))
            .flatten();
        vec![navree_native::Op::Append {
            target,
            nodes,
            more,
        }]
    };
    let envelope = Envelope::ops(assets::BUILD, ops).in_vocab(vocab);
    Some(kept(
        respond(StatusCode::OK, "no-store", envelope, &request, ""),
        Answered::parts(std::mem::take(&mut cx.dropped)),
    ))
}

/// A searched list's options for the native client the current request comes
/// from: a screen whose body is a choice of them, and a way to the next ones.
/// `None` when no native client asked.
pub(crate) fn choices<'a>(
    choices: impl Iterator<Item = &'a crate::select::Choice>,
    more: Option<&str>,
) -> Option<Response> {
    use navree_native::input as i;

    let request = scope::native()?;
    let options = choices
        .map(|choice| i::ChoiceOption {
            value: choice.value.clone(),
            label: choice.label.clone(),
            detail: choice.description.clone(),
            disabled: choice.disabled,
        })
        .collect();
    // The next options are found where the options say, as a search's are.
    let body = N::Field(navree_native::node::Field {
        input: i::Input::Choice(i::ChoiceInput {
            kind: i::ChoiceKind::List,
            options,
            more: more.map(str::to_owned),
            ..i::ChoiceInput::default()
        }),
        ..navree_native::node::Field::default()
    });
    let envelope = Envelope::screen(assets::BUILD, navree_native::Screen::new("", body))
        .in_vocab(vocab_of(&request));
    Some(kept(
        respond(StatusCode::OK, "no-store", envelope, &request, ""),
        Answered::parts(Vec::new()),
    ))
}

/// The vocabulary an answer is written in: the client's, no newer than this server's.
#[allow(
    clippy::unnecessary_min_or_max,
    reason = "while the vocabulary is 0 every client's is at least it; the bound is for the day it is not"
)]
fn vocab_of(request: &scope::Request) -> u32 {
    request.client.vocab.min(navree_native::VOCAB)
}

/// A page as the native client `request` gets it, outside the reply layer.
pub(crate) fn view_for(view: View, request: &Arc<scope::Request>) -> Response {
    native(view, request)
}

/// A page's answer when the request did not come from a browser. `Err` hands
/// the page back for the HTML path, untouched: the request came from a
/// browser, or from outside any request.
#[inline]
#[allow(
    clippy::result_large_err,
    reason = "the page is handed back whole, untouched, for the HTML path"
)]
pub(crate) fn view(view: View) -> Result<Response, View> {
    match scope::asked() {
        None => Err(view),
        Some(Asked::Native(request)) => Ok(native(view, &request)),
        Some(Asked::Embed) => Ok(embed(view)),
    }
}

fn native(mut view: View, request: &Arc<scope::Request>) -> Response {
    let page = page::of_view(&mut view, false);
    // The stamp of the shell the page stands in, so the client knows its own is current.
    let shell = view
        .note()
        .map(|note| note.shell_stamp(request.sign_in.as_deref(), &request.shell_parts))
        .unwrap_or_default();
    let parts = view.native_parts();
    let rows = rows_after(&page, request);
    // Only a whole page may be kept on the device, and only when its author
    // let a private cache keep it: never rows, a region or a failed page.
    let keep = rows.is_none();
    let (envelope, answered) = match rows {
        Some(rows) => rows,
        None => envelope(page, request),
    };
    let mut response = respond(parts.status, parts.cache, envelope, request, &shell);
    if keep {
        say_keep(&mut response, parts.status, parts.cache, request);
    }
    for cookie in parts.cookies.iter() {
        response.headers_mut().append(SET_COOKIE, cookie.clone());
    }
    vary_also(&mut response, parts.vary);
    response.extensions_mut().insert(answered);
    response
}

/// Says on a native answer that its page may be kept on the device between
/// launches ([`navree_native::KEEP_HEADER`]), when it may: a whole page, not a
/// region, answered with success, whose `cache` allows it. One rule for a page
/// read from a [`View`] and a screen written for native clients.
pub(crate) fn say_keep(
    response: &mut Response,
    status: StatusCode,
    cache: &str,
    request: &scope::Request,
) {
    if request.region.is_none() && status.is_success() && may_keep(cache) {
        response
            .headers_mut()
            .insert(navree_native::KEEP_HEADER, HeaderValue::from_static("1"));
    }
}

/// Whether a page whose `Cache-Control` is `cache` may be kept on a native
/// client's device between launches: the rule a browser keeps. `no-store`
/// anywhere in it means no; anything else (`private`, `no-cache`,
/// `max-age=…`, `public`) means yes; a value that can't be read means no.
fn may_keep(cache: &str) -> bool {
    let value = cache.trim();
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        && !value.to_ascii_lowercase().contains("no-store")
}

/// A query value as the text it stands for. `None` when the bytes are not text.
fn unpercent(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' => {
                let pair = value.get(at + 1..at + 3)?;
                out.push(u8::from_str_radix(pair, 16).ok()?);
                at += 3;
            }
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The rows after, or before, the ones a client has, when that is what it asked: the
/// page is the table's page it was sent the address of (the address says which table
/// and which end), and the answer is its rows as an op that adds them, with the address
/// of the rows beyond them: after the last for rows that join the end, before the first
/// (or none) for rows that join the start. `None` for any other request, and when the
/// page did not read whole or holds no such table: the client is then given the page.
fn rows_after(page: &Page, request: &scope::Request) -> Option<(Envelope, Answered)> {
    use navree_native::Op as O;
    let query = request.url.split_once('?')?.1;
    let (start, region) = query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        match name {
            super::table::ROWS_WORD => Some((false, unpercent(value))),
            super::table::ROWS_BEFORE_WORD => Some((true, unpercent(value))),
            _ => None,
        }
    })?;
    let region = region?;
    if !page.misses.is_empty() {
        return None;
    }
    let target = super::table::table_id(&region);
    let N::Table(table) = page.screen.body.find(&target)? else {
        return None;
    };
    let to = request.client.vocab;
    let readable = table.rows.iter().all(|row| {
        row.nodes().all(|node| {
            navree_native::vocab::newer_than(node, to).is_none()
                && node.depth() <= navree_native::MAX_DEPTH - 4
        })
    });
    if !readable {
        return None;
    }
    let mut ops = vec![if start {
        // Rows that join the start move the cursor before the table back with them; an
        // empty address says there is none before. The cursor after stays as it was.
        O::Rows {
            target,
            rows: table.rows.clone(),
            at: navree_native::op::Edge::Start,
            more: None,
            prev: Some(table.prev.clone().unwrap_or_default()),
        }
    } else {
        O::Rows {
            target,
            rows: table.rows.clone(),
            at: navree_native::op::Edge::End,
            more: table.more.clone(),
            prev: None,
        }
    }];
    ops.extend(page.ops.iter().cloned());
    Some((
        Envelope::ops(assets::BUILD, ops),
        Answered::parts(page.dropped.clone()),
    ))
}

/// What a web view's page varies on: who asked, by the native header, the
/// first load's header and the user agent's last word.
const WEB_VIEW_VARY: &str = "x-navree-native, x-navree-embed, user-agent";

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
            // Not the browser's page, and not the native client's envelope.
            (VARY, HeaderValue::from_static(WEB_VIEW_VARY)),
        ],
        html,
    )
        .into_response();
    for cookie in parts.cookies.iter() {
        response.headers_mut().append(SET_COOKIE, cookie.clone());
    }
    vary_also(&mut response, parts.vary);
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

/// The layer around a request that did not come from a browser, given who
/// asked. `Err` hands the request back for the web path, untouched: a
/// browser's at no cost, the runtime's at the cost of reading whether it runs
/// in a native client's web view.
#[allow(
    clippy::result_large_err,
    reason = "the request is handed back whole, untouched, for the web path"
)]
pub(crate) async fn layer(
    asked: WhoAsked,
    request: Request,
    next: Next,
) -> Result<Response, (Request, Next)> {
    match asked {
        WhoAsked::Native | WhoAsked::WebView => {}
        WhoAsked::Runtime if in_web_view(request.headers()) => {}
        _ => return Err((request, next)),
    }
    let url = request.uri().path_and_query().map_or_else(
        || request.uri().path().to_owned(),
        |url| url.as_str().to_owned(),
    );
    Ok(match scope::read(request.headers(), &url) {
        Reading::Web => return Err((request, next)),
        Reading::Refused(error) => refuse(
            StatusCode::from_u16(error.status()).unwrap_or(StatusCode::BAD_REQUEST),
            &error.to_string(),
        ),
        Reading::Asked(Asked::Embed) => embedded(request, next).await,
        Reading::Asked(Asked::Native(native)) => {
            let ctx = request
                .extensions()
                .get::<navree_kernel::AppContext>()
                .cloned();
            // Only the answer to a read may be kept on the device: never what
            // a write answered (a page meant to be seen once).
            let read = matches!(*request.method(), http::Method::GET | http::Method::HEAD);
            let mut response = answer(stamped(native, ctx.as_ref()).await, request, next).await;
            if !read {
                response.headers_mut().remove(navree_native::KEEP_HEADER);
            }
            response
        }
    })
}

/// Says that a browser's page, fragment or ops for a URL are not what a
/// native client gets for it, so a cache keeps the two apart. One header
/// lookup, and a header added to such an answer.
#[inline]
pub(crate) fn vary_on_native(response: &mut Response) {
    let differs = response.headers().get(CONTENT_TYPE).is_some_and(|value| {
        let value = value.as_bytes();
        value.starts_with(b"text/html") || value.starts_with(OPS_CONTENT_TYPE.as_bytes())
    });
    if !differs {
        return;
    }
    response
        .headers_mut()
        .append(VARY, HeaderValue::from_static(NATIVE_HEADER));
    // A page a shared cache may keep is not what a native client's web view
    // gets for the address either (it is drawn without the app's chrome),
    // and the web view is known by its user agent.
    if shared_cache_may_keep(response) {
        response
            .headers_mut()
            .append(VARY, HeaderValue::from_static("x-navree-embed, user-agent"));
    }
}

/// Whether a shared cache may keep `response`: its `Cache-Control` says
/// neither `no-store` nor `private` (or there is none, and a cache may
/// decide for itself).
fn shared_cache_may_keep(response: &Response) -> bool {
    let Some(value) = response.headers().get(CACHE_CONTROL) else {
        return true;
    };
    let value = value.to_str().unwrap_or("no-store").to_ascii_lowercase();
    !value
        .split(',')
        .map(str::trim)
        .any(|directive| directive == "no-store" || directive.starts_with("private"))
}

/// `native`, with the stamp of the app `ctx` answers for.
async fn stamped(
    native: Arc<scope::Request>,
    ctx: Option<&navree_kernel::AppContext>,
) -> Arc<scope::Request> {
    let Some(ctx) = ctx else {
        return native;
    };
    let Some(stamp) = ctx.try_get::<super::Stamp>().await else {
        return native;
    };
    let sign_in = ctx
        .try_get::<crate::pages::PagesOptions>()
        .await
        .map(|options| options.sign_in.clone());
    let parts = shell_parts(ctx, Some(&native.client)).await;
    match Arc::try_unwrap(native) {
        Ok(mut request) => {
            request.stamp = (*stamp).clone();
            request.sign_in = sign_in;
            request.shell_parts = parts;
            Arc::new(request)
        }
        Err(native) => native,
    }
}

/// What the app's modules say of the shell document for `client`.
pub(crate) async fn shell_parts(
    ctx: &navree_kernel::AppContext,
    client: Option<&navree_native::Client>,
) -> crate::NativeShellParts {
    use navree_native::Platform as P;
    let who = crate::NativeClient::new(match client.map(|client| client.platform) {
        Some(P::Ios | P::Ipados | P::Macos | P::Watchos | P::Tvos | P::Visionos) => {
            crate::NativeFamily::Apple
        }
        Some(P::Android | P::Wearos) => crate::NativeFamily::Android,
        _ => crate::NativeFamily::Other,
    });
    let mut parts = crate::NativeShellParts::default();
    for part in ctx
        .get_all::<dyn crate::NativeShellPart>()
        .await
        .unwrap_or_default()
    {
        part.contribute(&mut parts, &who);
    }
    parts
}

/// The layer in the context slot, innermost, that makes current again who
/// asked, as the reply layer read it: the unit of work may finish a request
/// on a task of its own, where the reply layer's scope is not. A browser's
/// request costs one look in its extensions.
///
/// Here, inside authentication, it also names on every native answer the
/// person it was made for ([`navree_native::PERSON_HEADER`], as the shell's
/// answer names them), so a client sees at once when the session changed
/// under it and keeps a page only for that person.
pub(crate) async fn context_layer(request: Request, next: Next) -> Response {
    match request.extensions().get::<Asked>().cloned() {
        Some(asked) => {
            let native = matches!(asked, Asked::Native(_));
            let (parts, body) = request.into_parts();
            let user = native
                .then(|| navree_http::CurrentPrincipal::from_parts(&parts).ok())
                .flatten()
                .map(|principal| principal.0.user_id);
            let request = Request::from_parts(parts, body);
            let mut response = scope::scope(asked, next.run(request)).await;
            if native {
                made_for(&mut response, user);
            }
            response
        }
        None => next.run(request).await,
    }
}

/// Names on a native answer the person it was made for, as the shell's
/// answer names them. Nobody signed in names nobody: a name a handler or an
/// upstream put on the answer is taken off.
fn made_for(response: &mut Response, user: Option<uuid::Uuid>) {
    match super::routes::person(user) {
        Some(value) => {
            response
                .headers_mut()
                .insert(navree_native::PERSON_HEADER, value);
        }
        None => {
            response.headers_mut().remove(navree_native::PERSON_HEADER);
        }
    }
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
async fn embedded(mut request: Request, next: Next) -> Response {
    // Made current again where the handler runs: see `context_layer`.
    request.extensions_mut().insert(Asked::Embed);
    let from_runtime = request.headers().contains_key(UI_HEADER);
    let accept = compress::accept_of(&request);
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
async fn answer(native: Arc<scope::Request>, mut request: Request, next: Next) -> Response {
    // Made current again where the handler runs: see `context_layer`.
    request
        .extensions_mut()
        .insert(Asked::Native(native.clone()));
    let accept = compress::accept_of(&request);
    // Recorded as the runtime's requests are: each is an operation of the app.
    let operation = (!is_prefetch(request.headers()))
        .then(|| crate::module::operation_kind(request.method(), request.headers()));
    let started = std::time::Instant::now();
    let mut response = scope::scope(Asked::Native(native.clone()), next.run(request)).await;
    let pending = response
        .extensions()
        .get::<PendingOps>()
        .map(|pending| pending.0.ops.len());
    if let Some(PendingOps(envelope, status)) = response.extensions_mut().remove::<PendingOps>() {
        // Ops that can all be said natively take the fallback's place. When
        // they can't, or there are none (a handler that keeps its ops for the
        // browser's runtime), the fallback stands: a redirect the client
        // follows to a screen, or a page that was answered as a screen already.
        let signed_out = response
            .extensions()
            .get::<crate::op::SignedOut>()
            .is_some();
        let said = native_ops(&envelope.ops).map(|mut ops| {
            // The person signed out: before the rest, which still applies.
            if signed_out {
                ops.insert(0, navree_native::Op::SignedOut);
            }
            ops
        });
        if let Some(ops) = said.filter(|ops| !ops.is_empty()) {
            let (parts, _) = crate::module::to_ops(response, envelope, status).into_parts();
            let mut native = respond(
                parts.status,
                "no-store",
                Envelope::ops(assets::BUILD, ops),
                &native,
                "",
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
        let mut wrapped = respond(parts.status, "no-store", envelope, &native, "");
        for value in parts.headers.get_all(SET_COOKIE) {
            wrapped.headers_mut().append(SET_COOKIE, value.clone());
        }
        *wrapped.extensions_mut() = parts.extensions;
        wrapped.extensions_mut().insert(answered);
        response = wrapped;
    }
    if let Some(kind) = operation {
        let ops = pending.map_or(0, |ops| u32::try_from(ops).unwrap_or(u32::MAX));
        crate::module::record_operation(kind, &response, ops, started);
    }
    squeeze(&accept, response).await
}

/// Sends an envelope in brotli to a client that takes it, when it is big
/// enough to be worth it and may be compressed at all (its route didn't opt
/// out, and it doesn't say `no-transform`).
async fn squeeze(accept: &str, response: Response) -> Response {
    let ours = response
        .headers()
        .get(CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes() == NATIVE_TYPE.as_bytes());
    if !ours
        || response.headers().contains_key(CONTENT_ENCODING)
        || !navree_http::may_compress(&response)
    {
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

#[cfg(test)]
mod tests {
    use std::hint::black_box;
    use std::time::Instant;

    use http::{HeaderMap, HeaderValue};
    use navree_http::response::IntoResponse;

    use super::*;
    use crate::{Button, Card, Stack, Text};

    #[tokio::test]
    async fn an_answer_deeper_than_a_client_reads_is_the_web_views_before_it_is_written() {
        use navree_native::node as n;
        let chain = |levels: usize| {
            let mut node = N::text("the bottom");
            for _ in 0..levels {
                node = N::Stack(n::Stack {
                    children: vec![node],
                    ..n::Stack::default()
                });
            }
            node
        };
        let request = scope::Request::new(
            navree_native::Client::new(
                navree_native::Platform::Android,
                navree_native::FormFactor::Phone,
            ),
            "/deep",
        );
        let said = |envelope: Envelope| {
            let response = respond(StatusCode::OK, "no-store", envelope, &request, "");
            async move {
                let bytes = navree_http::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("a body");
                String::from_utf8(bytes.to_vec()).expect("text")
            }
        };
        // As deep as a client reads (the JSON nests two levels a part and two more around
        // the screen): the screen.
        let fine = Envelope::screen(
            assets::BUILD,
            navree_native::Screen::new("Deep", chain(navree_native::MAX_DEPTH / 2 + 20)),
        );
        assert!(deepest(&fine) <= navree_native::MAX_DEPTH);
        assert!(said(fine).await.contains("the bottom"));
        // One level more, and a tree a thousand deep: the word to show the page, with why.
        for levels in [navree_native::MAX_DEPTH, 1000] {
            let deep = Envelope::screen(
                assets::BUILD,
                navree_native::Screen::new("Deep", chain(levels)),
            );
            assert!(deepest(&deep) > navree_native::MAX_DEPTH);
            let body = said(deep).await;
            assert!(
                body.contains(r#""web""#) && body.contains("depth"),
                "{body}"
            );
            assert!(!body.contains("the bottom"), "{body}");
        }
        // The same for what an op carries.
        let op = Envelope::ops(
            assets::BUILD,
            vec![navree_native::Op::Append {
                target: "list".to_owned(),
                nodes: vec![chain(200)],
                more: None,
            }],
        );
        assert!(deepest(&op) > navree_native::MAX_DEPTH);
        assert!(said(op).await.contains("depth"));
    }

    #[test]
    fn every_native_answer_names_the_person_it_was_made_for() {
        let id = uuid::Uuid::parse_str("0193a4b2-7c1d-7e3f-9a2b-5c6d7e8f9a0b").unwrap();
        let answer = |keep: bool| {
            let mut response = (StatusCode::OK, "{}").into_response();
            if keep {
                response
                    .headers_mut()
                    .insert(navree_native::KEEP_HEADER, HeaderValue::from_static("1"));
            }
            response
        };
        let person = |response: &Response| {
            response
                .headers()
                .get(navree_native::PERSON_HEADER)
                .map(|value| value.to_str().unwrap_or("").to_owned())
        };
        let mut kept = answer(true);
        made_for(&mut kept, Some(id));
        assert_eq!(
            person(&kept).as_deref(),
            Some("0193a4b2-7c1d-7e3f-9a2b-5c6d7e8f9a0b")
        );
        // Nobody signed in: no name, also when a handler put one on.
        let mut kept = answer(true);
        made_for(&mut kept, None);
        assert_eq!(person(&kept), None);
        let mut passed_on = answer(false);
        passed_on.headers_mut().insert(
            navree_native::PERSON_HEADER,
            HeaderValue::from_static("0193a4b2-0000-7000-8000-00000000hs02"),
        );
        made_for(&mut passed_on, None);
        assert_eq!(person(&passed_on), None);
        // An answer that may not be kept names them too.
        let mut not_kept = answer(false);
        made_for(&mut not_kept, Some(id));
        assert_eq!(
            person(&not_kept).as_deref(),
            Some("0193a4b2-7c1d-7e3f-9a2b-5c6d7e8f9a0b")
        );
    }

    #[test]
    fn a_page_may_be_kept_on_the_device_unless_its_cache_says_no_store() {
        for kept in [
            "private, no-cache",
            "private",
            "no-cache",
            "max-age=60",
            "public, max-age=300",
            "Private, No-Cache",
        ] {
            assert!(may_keep(kept), "{kept:?}");
        }
        for not_kept in [
            "no-store",
            "private, no-store",
            "No-Store",
            "no-cache,no-store,must-revalidate",
            "",
            "  ",
            "private\u{7}",
            "privé",
        ] {
            assert!(!may_keep(not_kept), "{not_kept:?}");
        }
    }

    #[test]
    fn a_native_answer_says_its_page_may_be_kept_only_when_the_page_allows_it() {
        let client = navree_native::Client::new(
            navree_native::Platform::Android,
            navree_native::FormFactor::Phone,
        );
        let answer = |view: View, region: Option<&str>| {
            let mut request = scope::Request::new(client.clone(), "/catalogue");
            request.region = region.map(str::to_owned);
            let request = Arc::new(request);
            let response = scope::sync(Asked::Native(request.clone()), || view_for(view, &request));
            response
                .headers()
                .get(navree_native::KEEP_HEADER)
                .map(|value| value.to_str().unwrap_or("").to_owned())
        };
        let page = |cache: Option<&'static str>| {
            let view = View::new("Catalogue").body(Text::new("Kesar, 250 g"));
            match cache {
                Some(cache) => view.cache(cache),
                None => view,
            }
        };
        assert_eq!(
            answer(page(Some("private, no-cache")), None).as_deref(),
            Some("1")
        );
        // A page says `no-store` unless its author says otherwise.
        assert_eq!(answer(page(None), None), None);
        assert_eq!(answer(page(Some("private, no-store")), None), None);
        // A region of a page is never kept by itself.
        assert_eq!(
            answer(page(Some("private, no-cache")), Some("figures")),
            None
        );
        // Nor a page that failed.
        assert_eq!(
            answer(
                page(Some("private, no-cache")).status(StatusCode::NOT_FOUND),
                None
            ),
            None
        );
    }

    #[test]
    fn a_toasts_post_carries_the_token_it_was_given() {
        use crate::{Toast, ToastAction, ToastTone};
        let said = |action: ToastAction| {
            let ops = native_ops(&[Op::Toast(
                Toast::new(ToastTone::Success, "Archived").action(action),
            )])
            .expect("native ops");
            let [navree_native::Op::Toast(toast)] = ops.as_slice() else {
                panic!("not a toast: {ops:?}");
            };
            toast.action.clone().expect("an action")
        };
        let post = said(ToastAction::post("Undo", "/undo").csrf("tok"));
        assert!(post.post);
        assert_eq!(
            post.fields,
            [navree_native::action::Pair {
                name: crate::form::CSRF_FIELD.to_owned(),
                value: "tok".to_owned()
            }]
        );
        // A link posts nothing, and a post given no token sends none.
        assert!(
            said(ToastAction::link("Open", "/x").csrf("tok"))
                .fields
                .is_empty()
        );
        assert!(said(ToastAction::post("Undo", "/undo")).fields.is_empty());
    }

    /// What the web path pays when the feature is on, measured: the reply
    /// layer's reading of who asked (which it did before for the runtime's
    /// header alone), the header that says a browser's answer varies on it, and
    /// the one read of the current client that a page and a node op each make.
    /// Run it with
    /// `cargo test -p navree-ui --features native-preview --release --lib native::swap::tests::what_a_browser -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not a check"]
    fn what_a_browser_pays_for_native_clients() {
        const TIMES: u32 = 2_000_000;
        let mut page = HeaderMap::new();
        for (name, value) in [
            ("host", "example.in"),
            (
                "user-agent",
                "Mozilla/5.0 (Linux; Android 16) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36",
            ),
            ("accept", "text/html,application/xhtml+xml"),
            ("accept-language", "en-IN,en;q=0.9,pa;q=0.8"),
            ("accept-encoding", "gzip, deflate, br, zstd"),
            ("cookie", "navree_session=abc; navree_ui_theme=ocean"),
            ("sec-fetch-site", "same-origin"),
            ("sec-fetch-mode", "navigate"),
        ] {
            page.insert(name, HeaderValue::from_static(value));
        }
        let mut fetch = page.clone();
        fetch.insert("x-navree-ui", HeaderValue::from_static("1"));
        let mut native = 0_u32;
        let started = Instant::now();
        for _ in 0..TIMES {
            native += u32::from(crate::asked::who_asked(black_box(&page)) != WhoAsked::Browser);
        }
        let a_page = started.elapsed() / TIMES;
        let started = Instant::now();
        for _ in 0..TIMES {
            let headers = black_box(&fetch);
            let asked = crate::asked::who_asked(headers);
            native += u32::from(asked == WhoAsked::Runtime && in_web_view(headers));
        }
        let a_fetch = started.elapsed() / TIMES;
        let started = Instant::now();
        for _ in 0..TIMES {
            native += u32::from(black_box(scope::asked()).is_some());
        }
        let ambient = started.elapsed() / TIMES;
        const RESPONSES: u32 = 200_000;
        let mut responses: Vec<Response> = (0..RESPONSES)
            .map(|_| {
                (
                    [(
                        CONTENT_TYPE,
                        HeaderValue::from_static("text/html; charset=utf-8"),
                    )],
                    "",
                )
                    .into_response()
            })
            .collect();
        let started = Instant::now();
        for response in &mut responses {
            vary_on_native(black_box(response));
        }
        let vary = started.elapsed() / RESPONSES;
        assert_eq!(native, 0);
        let lookups = a_page;
        // Beside one small page drawn for a browser.
        let page = || {
            View::new("Order").body(Stack::gap(4).children((
                Card::titled("Items").body(Text::new("Pashmina shawl, Patiala, small")),
                Button::primary("Refund").href("/orders/1/refund"),
            )))
        };
        const PAGES: u32 = 20_000;
        let started = Instant::now();
        for _ in 0..PAGES {
            black_box(page().into_response());
        }
        let drawn = started.elapsed() / PAGES;
        println!("who asked, for a page: {lookups:?}");
        println!("who asked, for the runtime's fetch: {a_fetch:?}");
        println!("the header that says the answer varies: {vary:?}");
        println!("one read of the current client: {ambient:?}");
        println!("a small page built and drawn: {drawn:?}");
    }
}

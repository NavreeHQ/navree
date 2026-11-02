//! The wire protocol: a closed list of [`Op`]s the server sends and the
//! runtime applies, with rendered HTML as payload, and [`Reply`], what an
//! action handler answers.
//!
//! With JavaScript off, an action answers a redirect or a full page. With the
//! runtime, the same route gets `X-Navree-UI: 1` and answers an [`Envelope`]:
//!
//! ```json
//! {"build":"3f9a1c07b2de","ops":[
//!   {"op":"morph","target":"deals","html":"<table>…"},
//!   {"op":"toast","tone":"success","text":"Deal created"}
//! ]}
//! ```
//!
//! A `build` the page doesn't know makes the runtime reload, so a deploy can
//! never run old JavaScript against new ops.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use http::{HeaderMap, HeaderValue, StatusCode, header};
use navree_http::response::{IntoResponse, Response};
use serde::Serialize;

use crate::assets;
use crate::html::{Href, LocalHref};
use crate::node::Node;
use crate::render::render;
use crate::view::View;

/// The request header the runtime sends on every action and fragment fetch.
pub const UI_HEADER: &str = "x-navree-ui";

/// The request header the runtime adds when it fetches a link ahead of a click.
pub const PREFETCH_HEADER: &str = "x-navree-prefetch";

/// Whether this request is a fetch ahead of a click: the user hasn't asked for
/// the page yet, and may never. The runtime says so in [`PREFETCH_HEADER`], and a
/// browser following Speculation Rules in `Sec-Purpose`.
pub fn is_prefetch(headers: &HeaderMap) -> bool {
    headers.contains_key(PREFETCH_HEADER)
        || headers.get("sec-purpose").is_some_and(|purpose| {
            purpose
                .as_bytes()
                .windows(8)
                .any(|part| part == b"prefetch")
        })
}

/// The media type of an [`Envelope`].
pub const OPS_CONTENT_TYPE: &str = "application/vnd.navree.ops+json";

/// A toast's meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToastTone {
    /// Plain information.
    #[default]
    Info,
    /// It worked.
    Success,
    /// Something needs attention.
    Warning,
    /// It failed.
    Danger,
}

/// How a URL change enters history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UrlMode {
    /// A new history entry.
    #[default]
    Push,
    /// Replaces the current entry.
    Replace,
}

/// One change to the page. The runtime handles exactly this list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
#[non_exhaustive]
pub enum Op {
    /// Morphs the element `target` into `html`, keeping focus, caret,
    /// selection, composition and typed values.
    Morph {
        /// The element's id.
        target: String,
        /// One element, rendered.
        html: String,
    },
    /// Replaces the element `target` outright.
    Replace {
        /// The element's id.
        target: String,
        /// The new markup.
        html: String,
    },
    /// Adds `html` at the end of `target`.
    Append {
        /// The element's id.
        target: String,
        /// The new markup.
        html: String,
    },
    /// Adds `html` at the start of `target`.
    Prepend {
        /// The element's id.
        target: String,
        /// The new markup.
        html: String,
    },
    /// Removes the element `target`.
    Remove {
        /// The element's id.
        target: String,
    },
    /// Shows field errors in a form, or clears them when `fields` is empty.
    Errors {
        /// The form's id.
        form: String,
        /// Messages by field name.
        fields: BTreeMap<String, Vec<String>>,
    },
    /// Shows a toast, and announces it to assistive technology.
    Toast {
        /// Its meaning.
        tone: ToastTone,
        /// Its text.
        text: String,
    },
    /// Opens the dialog, popover or details `target`.
    Open {
        /// The element's id.
        target: String,
    },
    /// Closes the dialog, popover or details `target`.
    Close {
        /// The element's id.
        target: String,
    },
    /// Moves focus to `target`.
    Focus {
        /// The element's id.
        target: String,
    },
    /// Changes the address bar without loading anything.
    Url {
        /// The new URL.
        href: String,
        /// How it enters history.
        mode: UrlMode,
    },
    /// Goes to `href`, on this site, the way a boosted link does. The runtime
    /// refuses another origin here.
    Navigate {
        /// The URL.
        href: String,
    },
    /// Leaves for `href` on another site: a real navigation, with no request of the
    /// runtime's. Only [`Op::navigate_external`] and [`Reply::redirect_external`]
    /// write this.
    #[serde(rename = "navigate_external")]
    NavigateExternal {
        /// The URL.
        href: String,
    },
    /// Reloads the page.
    Reload,
    /// Refetches the region `target`.
    Invalidate {
        /// The region's name.
        target: String,
    },
}

impl Op {
    /// Morphs element `target` into `node`.
    pub fn morph(target: impl Into<String>, node: &Node) -> Self {
        Self::Morph {
            target: target.into(),
            html: render(node),
        }
    }

    /// Replaces element `target` with `node`.
    pub fn replace(target: impl Into<String>, node: &Node) -> Self {
        Self::Replace {
            target: target.into(),
            html: render(node),
        }
    }

    /// Adds `node` at the end of `target`.
    pub fn append(target: impl Into<String>, node: &Node) -> Self {
        Self::Append {
            target: target.into(),
            html: render(node),
        }
    }

    /// Adds `node` at the start of `target`.
    pub fn prepend(target: impl Into<String>, node: &Node) -> Self {
        Self::Prepend {
            target: target.into(),
            html: render(node),
        }
    }

    /// A toast.
    pub fn toast(tone: ToastTone, text: impl Into<String>) -> Self {
        Self::Toast {
            tone,
            text: text.into(),
        }
    }

    /// Goes to `href`, on this site: a URL that would leave it goes nowhere.
    pub fn navigate(href: impl Into<LocalHref>) -> Self {
        Self::Navigate {
            href: href.into().as_str().to_owned(),
        }
    }

    /// Leaves for `href`, which may be on another site. Never give this a URL from
    /// a request unchecked: that is an open redirect.
    pub fn navigate_external(href: impl Into<Href>) -> Self {
        Self::NavigateExternal {
            href: href.into().as_str().to_owned(),
        }
    }

    /// Changes the address bar, on this site.
    pub fn url(href: impl Into<LocalHref>, mode: UrlMode) -> Self {
        Self::Url {
            href: href.into().as_str().to_owned(),
            mode,
        }
    }
}

/// What the runtime receives: the ops, the build they belong to and the
/// module files their markup needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Envelope {
    /// The build, compared with the page's.
    pub build: &'static str,
    /// Module scripts the markup needs that the page may not have loaded.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<&'static str>,
    /// The ops, applied in order.
    pub ops: Vec<Op>,
}

impl Envelope {
    /// The ops of this build.
    pub fn new(ops: Vec<Op>) -> Self {
        Self {
            build: assets::BUILD,
            modules: Vec::new(),
            ops,
        }
    }
}

impl IntoResponse for Envelope {
    fn into_response(self) -> Response {
        let body = serde_json::to_string(&self).unwrap_or_else(|_| String::from("{\"ops\":[]}"));
        (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(OPS_CONTENT_TYPE),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            body,
        )
            .into_response()
    }
}

/// What the response is when the request didn't come from the runtime.
#[derive(Clone, Debug)]
enum Fallback {
    /// `303 See Other`.
    Redirect(Href),
    /// A full page.
    View(Box<View>),
    /// `204 No Content`.
    Empty,
}

/// What an action handler answers: ops for the runtime, and what a browser
/// with JavaScript off gets instead.
///
/// [`UiModule`](crate::UiModule) picks between them from the request's
/// `X-Navree-UI` header. Without the module the fallback is sent; the runtime
/// follows a redirect like a boosted link, so actions still work, and only
/// the extra ops are lost.
///
/// ```
/// use navree_ui::{Op, Reply, ToastTone};
///
/// let reply = Reply::redirect("/deals/1").toast(ToastTone::Success, "Deal created");
/// # let _ = reply;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Reply {
    ops: Vec<Op>,
    status: StatusCode,
    fallback: Fallback,
    flash: Option<(ToastTone, String)>,
    uses: crate::node::Uses,
}

impl Reply {
    /// Goes to `href`, on this site. A URL that would leave it (`//host`, `/\\host`,
    /// `https://host`) goes nowhere, so a `next` from a request can't send a user
    /// away. To leave on purpose, use [`redirect_external`](Self::redirect_external).
    pub fn redirect(href: impl Into<LocalHref>) -> Self {
        let href = href.into().into_href();
        Self {
            ops: vec![Op::Navigate {
                href: href.as_str().to_owned(),
            }],
            status: StatusCode::OK,
            fallback: Fallback::Redirect(href),
            flash: None,
            uses: crate::node::Uses::NONE,
        }
    }

    /// Applies ops; with JavaScript off, goes to `href`.
    pub fn ops_or_redirect(href: impl Into<Href>) -> Self {
        Self {
            ops: Vec::new(),
            status: StatusCode::OK,
            fallback: Fallback::Redirect(href.into()),
            flash: None,
            uses: crate::node::Uses::NONE,
        }
    }

    /// Goes to `href`, which may be on another site: a sign-in provider, a payment
    /// page. Never give this a URL from a request unchecked: that is an open redirect.
    pub fn redirect_external(href: impl Into<Href>) -> Self {
        let href = href.into();
        Self {
            ops: vec![Op::NavigateExternal {
                href: href.as_str().to_owned(),
            }],
            status: StatusCode::OK,
            fallback: Fallback::Redirect(href),
            flash: None,
            uses: crate::node::Uses::NONE,
        }
    }

    /// Applies ops; with JavaScript off, shows `view`.
    pub fn view(view: View) -> Self {
        Self {
            ops: Vec::new(),
            status: StatusCode::OK,
            fallback: Fallback::View(Box::new(view)),
            flash: None,
            uses: crate::node::Uses::NONE,
        }
    }

    /// Applies ops; with JavaScript off, answers 204.
    pub fn ops(ops: Vec<Op>) -> Self {
        Self {
            ops,
            status: StatusCode::OK,
            fallback: Fallback::Empty,
            flash: None,
            uses: crate::node::Uses::NONE,
        }
    }

    /// Adds an op.
    pub fn op(mut self, op: Op) -> Self {
        self.ops.push(op);
        self
    }

    /// Morphs element `target` into `node`.
    pub fn morph(mut self, target: impl Into<String>, node: impl Into<Node>) -> Self {
        let node = node.into();
        self.uses = self.uses.with(crate::render::measure(&node).uses);
        self.op(Op::morph(target, &node))
    }

    /// Adds a toast. With JavaScript off, the next page shows it.
    pub fn toast(mut self, tone: ToastTone, text: impl Into<String>) -> Self {
        let text = text.into();
        self.flash = Some((tone, text.clone()));
        // A toast shows before a navigation that follows it.
        let at = self
            .ops
            .iter()
            .position(|op| matches!(op, Op::Navigate { .. } | Op::NavigateExternal { .. }))
            .unwrap_or(self.ops.len());
        self.ops.insert(at, Op::Toast { tone, text });
        self
    }

    /// Sets the status of the ops response.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Answers with the fallback, with the ops kept for [`crate::UiModule`]'s layer.
    fn fallback_response(self) -> (Response, Envelope, StatusCode) {
        let mut envelope = Envelope::new(self.ops);
        envelope.modules = assets::scripts(self.uses).collect();
        let mut response = match self.fallback {
            Fallback::Redirect(href) => {
                let mut response = StatusCode::SEE_OTHER.into_response();
                if let Ok(location) = HeaderValue::from_str(&location_of(href.as_str())) {
                    response.headers_mut().insert(header::LOCATION, location);
                }
                response
            }
            Fallback::View(view) => view.into_response(),
            Fallback::Empty => StatusCode::NO_CONTENT.into_response(),
        };
        if let Some((tone, text)) = self.flash
            && let Ok(cookie) = crate::shell::flash_cookie_toned(tone, &text)
        {
            response.headers_mut().append(header::SET_COOKIE, cookie);
        }
        (response, envelope, self.status)
    }
}

/// `url` as a header can carry it: letters outside ASCII, percent-encoded.
fn location_of(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for character in url.chars() {
        if character.is_ascii() {
            out.push(character);
        } else {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).bytes() {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// The ops of a [`Reply`], waiting in the response's extensions for the
/// module's layer.
#[derive(Clone, Debug)]
pub(crate) struct PendingOps(pub(crate) Envelope, pub(crate) StatusCode);

impl IntoResponse for Reply {
    fn into_response(self) -> Response {
        let (mut response, envelope, status) = self.fallback_response();
        response
            .extensions_mut()
            .insert(PendingOps(envelope, status));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::el;

    #[test]
    fn the_wire_format_is_stable() {
        let ops = vec![
            Op::morph("t", &el("div").child("<x>").into()),
            Op::Errors {
                form: "f".into(),
                fields: BTreeMap::from([("a".to_owned(), vec!["bad".to_owned()])]),
            },
            Op::toast(ToastTone::Success, "ok"),
            Op::url("/a", UrlMode::Replace),
            Op::navigate("/b"),
            Op::Remove { target: "r".into() },
            Op::Reload,
            Op::Invalidate {
                target: "kpis".into(),
            },
        ];
        assert_eq!(
            serde_json::to_string(&ops).unwrap(),
            concat!(
                r#"[{"op":"morph","target":"t","html":"<div>&lt;x&gt;</div>"},"#,
                r#"{"op":"errors","form":"f","fields":{"a":["bad"]}},"#,
                r#"{"op":"toast","tone":"success","text":"ok"},"#,
                r#"{"op":"url","href":"/a","mode":"replace"},"#,
                r#"{"op":"navigate","href":"/b"},"#,
                r#"{"op":"remove","target":"r"},"#,
                r#"{"op":"reload"},"#,
                r#"{"op":"invalidate","target":"kpis"}]"#,
            )
        );
    }

    #[test]
    fn urls_in_ops_are_checked() {
        assert_eq!(
            Op::navigate("javascript:x"),
            Op::Navigate { href: "#".into() }
        );
    }

    #[test]
    fn leaving_the_site_is_said_by_name() {
        assert_eq!(
            serde_json::to_string(&Op::navigate_external("https://a.b/c")).unwrap(),
            r#"{"op":"navigate_external","href":"https://a.b/c"}"#
        );
        assert_eq!(
            Op::navigate_external("javascript:x"),
            Op::NavigateExternal { href: "#".into() }
        );
        let reply = Reply::redirect_external("https://a.b/c").toast(ToastTone::Info, "hi");
        assert!(matches!(reply.ops[0], Op::Toast { .. }));
        let response = reply.into_response();
        assert_eq!(response.headers()[header::LOCATION], "https://a.b/c");
    }

    #[test]
    fn a_toast_comes_before_the_navigation() {
        let reply = Reply::redirect("/x").toast(ToastTone::Info, "hi");
        assert!(matches!(reply.ops[0], Op::Toast { .. }));
        assert!(matches!(reply.ops[1], Op::Navigate { .. }));
    }
}

//! Toast: a short message that goes by itself. A [`Toast`] is data: the server sends
//! one in an op, the page draws one from a command or a failed request, and the
//! shell draws the one a redirect left, so the same message looks the same however
//! it arrives. A [`Toaster`] is the place they show: a labelled region that is always
//! in the page, so a screen reader hears what is added to it.
//!
//! How long a toast lives is a CSS animation, and pausing it under the pointer or
//! focus is `animation-play-state`, so there is no timer to lose and no state that
//! can stick. With no script it fades out on its own.

use std::borrow::Cow;

use serde::Serialize;

use crate::form::csrf_field;
use crate::html::Href;
use crate::node::{Element, Node, el, into_node};
use crate::op::ToastTone;

/// How long a toast stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ToastDuration {
    /// 3 seconds.
    Brief,
    /// 5 seconds.
    Normal,
    /// 8 seconds.
    Long,
    /// Until the reader closes it. Use it for what must not be missed, and give it
    /// a way to close.
    Sticky,
}

impl ToastDuration {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Normal => "normal",
            Self::Long => "long",
            Self::Sticky => "sticky",
        }
    }
}

/// Where the toasts stand on the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToastPosition {
    /// The bottom, at the end of the line: the default.
    #[default]
    BottomEnd,
    /// The bottom, centred.
    BottomCenter,
    /// The bottom, at the start of the line.
    BottomStart,
    /// The top, at the end of the line.
    TopEnd,
    /// The top, centred.
    TopCenter,
    /// The top, at the start of the line.
    TopStart,
}

impl ToastPosition {
    const fn as_str(self) -> &'static str {
        match self {
            Self::BottomEnd => "bottom-end",
            Self::BottomCenter => "bottom-center",
            Self::BottomStart => "bottom-start",
            Self::TopEnd => "top-end",
            Self::TopCenter => "top-center",
            Self::TopStart => "top-start",
        }
    }
}

/// The one thing a toast can offer to do: go somewhere, or post.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToastAction {
    label: String,
    href: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    post: bool,
}

impl ToastAction {
    /// A link.
    pub fn link(label: impl Into<String>, href: impl Into<Href>) -> Self {
        Self {
            label: label.into(),
            href: href.into().as_str().to_owned(),
            post: false,
        }
    }

    /// A button that posts to `href`, such as "Undo". The post answers ops, so it can
    /// replace the toast or raise another.
    pub fn post(label: impl Into<String>, href: impl Into<Href>) -> Self {
        Self {
            label: label.into(),
            href: href.into().as_str().to_owned(),
            post: true,
        }
    }
}

/// A message that goes by itself.
///
/// ```
/// use navree_ui::{Toast, ToastAction, ToastTone, Toaster, render};
///
/// let toast = Toast::new(ToastTone::Success, "Deal archived")
///     .title("Archived")
///     .action(ToastAction::post("Undo", "/deals/1/restore"));
/// let html = render(&Toaster::new().toast(toast).into());
/// assert!(html.contains(r#"role="status""#));
/// assert!(html.contains("Undo"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[must_use]
pub struct Toast {
    tone: ToastTone,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<ToastDuration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<ToastAction>,
    #[serde(skip_serializing_if = "is_true")]
    closable: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(value: &bool) -> bool {
    *value
}

impl Toast {
    /// A toast of `tone` saying `text`.
    pub fn new(tone: ToastTone, text: impl Into<String>) -> Self {
        Self {
            tone,
            text: text.into(),
            title: None,
            id: None,
            duration: None,
            action: None,
            closable: true,
        }
    }

    /// A line above the text, in bold.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Names the toast. One with the same name replaces it, and its time starts again:
    /// "Saving…" becomes "Saved".
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets how long it stays. Without this, a warning, an error or a toast with an
    /// action stays 8 seconds and any other 5.
    pub fn duration(mut self, duration: ToastDuration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Offers one thing to do.
    pub fn action(mut self, action: ToastAction) -> Self {
        self.action = Some(action);
        self
    }

    /// Takes the close button away. Don't, for a toast that stays.
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// The tone.
    pub fn tone(&self) -> ToastTone {
        self.tone
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How long it stays, with the default for its tone and action filled in.
    fn lasts(&self) -> ToastDuration {
        self.duration.unwrap_or(
            if self.action.is_some() || matches!(self.tone, ToastTone::Warning | ToastTone::Danger)
            {
                ToastDuration::Long
            } else {
                ToastDuration::Normal
            },
        )
    }

    fn to_element(&self, csrf: Option<&str>, close_label: &str) -> Element {
        let danger = self.tone == ToastTone::Danger;
        let action = self.action.as_ref().map(|action| {
            if action.post {
                el("form")
                    .attr("method", "post")
                    .attr("action", Href::from(action.href.as_str()))
                    .flag("data-ui-form", true)
                    .children((
                        csrf.map(|token| csrf_field(token.to_owned())),
                        el("button")
                            .slot("toast-action")
                            .attr("type", "submit")
                            .child(action.label.clone()),
                    ))
            } else {
                el("a")
                    .slot("toast-action")
                    .href(Href::from(action.href.as_str()))
                    .child(action.label.clone())
            }
        });
        el("div")
            .slot("toast")
            .attr("data-tone", self.tone.as_str())
            .attr("data-duration", self.lasts().as_str())
            .attr("role", if danger { "alert" } else { "status" })
            .attr_opt("data-toast-id", self.id.clone())
            .children((
                el("div").slot("toast-body").children((
                    self.title
                        .clone()
                        .map(|title| el("p").slot("toast-title").child(title)),
                    el("p").slot("toast-text").child(self.text.clone()),
                )),
                action,
                self.closable.then(|| {
                    el("button")
                        .slot("toast-close")
                        .attr("type", "button")
                        .attr("aria-label", close_label.to_owned())
                }),
            ))
    }
}

/// The place toasts show. It is a labelled region in every page that has the
/// runtime, so what is added to it is announced; the shell puts one in each page, and
/// a toast the last request left is already in it.
///
/// ```
/// use navree_ui::{ToastPosition, Toaster, render};
///
/// let html = render(&Toaster::new().position(ToastPosition::TopEnd).into());
/// assert!(html.contains(r#"aria-label="Notifications""#));
/// assert!(html.contains(r#"data-position="top-end""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Toaster {
    position: ToastPosition,
    toasts: Vec<Toast>,
    label: Cow<'static, str>,
    close_label: Cow<'static, str>,
    csrf: Option<String>,
}

impl Toaster {
    /// An empty toaster at the bottom end of the screen.
    pub fn new() -> Self {
        Self {
            position: ToastPosition::BottomEnd,
            toasts: Vec::new(),
            label: Cow::Borrowed("Notifications"),
            close_label: Cow::Borrowed("Dismiss"),
            csrf: None,
        }
    }

    /// Sets where the toasts stand.
    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    /// Adds a toast that is showing from the first paint.
    pub fn toast(mut self, toast: Toast) -> Self {
        self.toasts.push(toast);
        self
    }

    /// Sets the region's name and the close button's, for another language.
    pub fn labels(
        mut self,
        region: impl Into<Cow<'static, str>>,
        dismiss: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.label = region.into();
        self.close_label = dismiss.into();
        self
    }

    /// Puts the session's CSRF token in the form of a post action.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    fn into_node(self) -> Node {
        let toasts: Vec<Element> = self
            .toasts
            .iter()
            .map(|toast| toast.to_element(self.csrf.as_deref(), &self.close_label))
            .collect();
        el("section")
            .slot("toaster")
            .attr("aria-label", self.label)
            .attr("data-position", self.position.as_str())
            .attr("data-close", self.close_label)
            .flag("data-ui-toaster", true)
            .uses(crate::node::Uses::CORE)
            .child(toasts)
            .into()
    }
}

impl Default for Toaster {
    fn default() -> Self {
        Self::new()
    }
}

into_node!(Toaster);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn html(toaster: Toaster) -> String {
        render(&toaster.into())
    }

    #[test]
    fn the_region_is_labelled_and_always_there() {
        let out = html(Toaster::new());
        assert!(
            out.contains(r#"<section data-slot="toaster" aria-label="Notifications""#),
            "{out}"
        );
        assert!(out.contains(r#"data-position="bottom-end""#), "{out}");
    }

    #[test]
    fn a_toast_is_a_status_and_an_error_is_an_alert() {
        let out = html(
            Toaster::new()
                .toast(Toast::new(ToastTone::Success, "Saved"))
                .toast(Toast::new(ToastTone::Danger, "Failed")),
        );
        assert!(
            out.contains(r#"data-tone="success" data-duration="normal" role="status""#),
            "{out}"
        );
        assert!(
            out.contains(r#"data-tone="danger" data-duration="long" role="alert""#),
            "{out}"
        );
    }

    #[test]
    fn how_long_a_toast_stays_follows_its_tone_and_action() {
        let long = |toast: Toast| toast.lasts();
        assert_eq!(
            long(Toast::new(ToastTone::Info, "x")),
            ToastDuration::Normal
        );
        assert_eq!(
            long(Toast::new(ToastTone::Success, "x")),
            ToastDuration::Normal
        );
        assert_eq!(
            long(Toast::new(ToastTone::Warning, "x")),
            ToastDuration::Long
        );
        assert_eq!(
            long(Toast::new(ToastTone::Danger, "x")),
            ToastDuration::Long
        );
        assert_eq!(
            long(Toast::new(ToastTone::Info, "x").action(ToastAction::link("Open", "/x"))),
            ToastDuration::Long
        );
        assert_eq!(
            long(Toast::new(ToastTone::Danger, "x").duration(ToastDuration::Sticky)),
            ToastDuration::Sticky
        );
    }

    #[test]
    fn title_text_close_and_id() {
        let out = html(
            Toaster::new().toast(
                Toast::new(ToastTone::Info, "Body")
                    .title("Head")
                    .id("job-1"),
            ),
        );
        assert!(
            out.contains(
                r#"<p data-slot="toast-title">Head</p><p data-slot="toast-text">Body</p>"#
            ),
            "{out}"
        );
        assert!(out.contains(r#"data-toast-id="job-1""#), "{out}");
        assert!(
            out.contains(r#"<button data-slot="toast-close" type="button" aria-label="Dismiss">"#),
            "{out}"
        );
        let bare = html(Toaster::new().toast(Toast::new(ToastTone::Info, "x").closable(false)));
        assert!(!bare.contains("toast-close"), "{bare}");
        assert!(!bare.contains("toast-title"), "{bare}");
    }

    #[test]
    fn an_action_is_a_link_or_a_post() {
        let link =
            html(Toaster::new().toast(
                Toast::new(ToastTone::Info, "x").action(ToastAction::link("View", "/deals/1")),
            ));
        assert!(
            link.contains(r#"<a data-slot="toast-action" href="/deals/1">View</a>"#),
            "{link}"
        );
        let post =
            html(Toaster::new().csrf("tok").toast(
                Toast::new(ToastTone::Success, "x").action(ToastAction::post("Undo", "/undo")),
            ));
        assert!(
            post.contains(r#"<form method="post" action="/undo" data-ui-form>"#),
            "{post}"
        );
        assert!(post.contains(r#"name="_csrf" value="tok""#), "{post}");
        assert!(post.contains(r#"type="submit">Undo</button>"#), "{post}");
    }

    #[test]
    fn text_is_text_and_a_bad_link_goes_nowhere() {
        let out = html(
            Toaster::new().labels("<n>", "<d>").toast(
                Toast::new(ToastTone::Info, "<img src=x onerror=alert(1)>")
                    .title("<b>")
                    .id("\"><x>")
                    .action(ToastAction::link("<a>", "javascript:alert(1)")),
            ),
        );
        for bad in ["<img", "<b>", "<x>", "<a>", "<n>", "<d>", "javascript"] {
            assert!(!out.contains(bad), "{bad}: {out}");
        }
    }

    #[test]
    fn the_wire_form_leaves_out_what_was_not_set() {
        assert_eq!(
            serde_json::to_string(&Toast::new(ToastTone::Success, "ok")).unwrap(),
            r#"{"tone":"success","text":"ok"}"#
        );
        assert_eq!(
            serde_json::to_string(
                &Toast::new(ToastTone::Danger, "no")
                    .title("T")
                    .id("i")
                    .duration(ToastDuration::Sticky)
                    .closable(false)
                    .action(ToastAction::post("Undo", "/u"))
            )
            .unwrap(),
            concat!(
                r#"{"tone":"danger","text":"no","title":"T","id":"i","duration":"sticky","#,
                r#""action":{"label":"Undo","href":"/u","post":true},"closable":false}"#
            )
        );
    }

    #[test]
    fn the_toaster_asks_for_the_core_runtime_and_names_its_close_word() {
        let node: Node = Toaster::new().labels("Avisos", "Cerrar").into();
        assert!(
            crate::render::measure(&node)
                .uses
                .has(crate::node::Uses::CORE)
        );
        let out = render(&node);
        assert!(
            out.contains(r#"aria-label="Avisos""#) && out.contains(r#"data-close="Cerrar""#),
            "{out}"
        );
    }

    #[test]
    fn positions_are_closed_choices() {
        for (position, name) in [
            (ToastPosition::BottomEnd, "bottom-end"),
            (ToastPosition::BottomCenter, "bottom-center"),
            (ToastPosition::BottomStart, "bottom-start"),
            (ToastPosition::TopEnd, "top-end"),
            (ToastPosition::TopCenter, "top-center"),
            (ToastPosition::TopStart, "top-start"),
        ] {
            assert!(
                html(Toaster::new().position(position))
                    .contains(&format!(r#"data-position="{name}""#))
            );
        }
    }
}

//! The cards other modules add to the dashboard: each drawn only for a user who passes its
//! check, its data computed only for them, and a card that fails left out.

use navree_ui::{Card, Node, SafeHtml, el};
use serde_json::{Map, Value, json};

use super::record::user_json;
use crate::cx::Cx;

/// The cards the user of `cx` may see, in order: a card whose check they fail is left out
/// before its data function runs, and a card whose data function or template fails is
/// logged and left out, so the dashboard is drawn without it.
pub(super) async fn cards(cx: &Cx) -> Vec<Node> {
    let core = &cx.core;
    let widgets = core.registry.widgets();
    if widgets.is_empty() {
        return Vec::new();
    }
    let Some(views) = core.app_views.as_ref() else {
        return Vec::new();
    };
    let base = core.options.base();
    // Who is signed in, read once and only when a card is drawn.
    let mut user: Option<Value> = None;
    let mut drawn: Vec<Node> = Vec::new();
    for entry in widgets {
        let widget = &entry.widget;
        if widget
            .can
            .as_ref()
            .is_some_and(|(action, subject)| !cx.can(action, subject, None))
        {
            continue;
        }
        let data = match &widget.data {
            Some(f) => match f(crate::PageContext::new(cx.inner(entry.module, Map::new()))).await {
                Ok(data) => data,
                Err(error) => {
                    tracing::warn!(
                        target: "navree::helm",
                        widget = %widget.key,
                        status = error.status.as_u16(),
                        "a widget failed"
                    );
                    continue;
                }
            },
            None => Value::Null,
        };
        if user.is_none() {
            user = Some(user_json(cx).await);
        }
        let template = widget.template.as_deref().unwrap_or_default();
        let rendered = match views.render(
            template,
            json!({
                "title": widget.title,
                "user": user,
                "path": base,
                "data": data,
            }),
        ) {
            Ok(rendered) => rendered,
            Err(error) => {
                tracing::warn!(
                    target: "navree::helm",
                    widget = %widget.key,
                    %error,
                    "a widget failed"
                );
                continue;
            }
        };
        // The app's own template, rendered by navree-view with its escaping.
        drawn.push(
            el("section")
                .attr("aria-label", widget.title.clone())
                .attr("data-helm-widget", widget.key.clone())
                .child(
                    Card::titled(widget.title.clone())
                        .body(Node::Raw(SafeHtml::trusted(rendered))),
                )
                .into(),
        );
    }
    drawn
}

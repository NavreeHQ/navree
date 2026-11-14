//! Saved views: a name and a query string. Applying one is following a link, so a
//! stale view goes through the same allow-list as any URL, and a view can't hold
//! a selection, since a query string doesn't.

use std::borrow::Cow;

use http::StatusCode;
use navree_http::body::{BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};
use serde::de::DeserializeOwned;

use super::{FilterBar, NoFilter, TableLabels, TableQuery, TableRow};
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons;
use crate::node::{Element, Node, el};
use crate::op::{Op, Reply, ToastTone};

/// One saved view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedView {
    /// Its key.
    pub id: String,
    /// What the user called it.
    pub name: String,
    /// The table's state, as a query string.
    pub query: String,
    /// Whether the table opens with it when the URL names no state.
    pub is_default: bool,
}

/// What a saved-view form asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewOp {
    /// Save the state as a view.
    Save,
    /// Delete the view.
    Delete,
}

/// A saved-view form's post, read through the table's definition.
#[derive(Clone, Debug)]
pub struct ViewPost<R, F = NoFilter> {
    op: ViewOp,
    name: String,
    view: String,
    is_default: bool,
    table: String,
    query: TableQuery<R, F>,
}

impl<R: TableRow, F: FilterBar> ViewPost<R, F> {
    /// Takes columns away from the person asking, as [`TableQuery::without_columns`]
    /// does for the table they were drawn in, so that a view they save holds nothing
    /// they could not have set.
    #[must_use]
    pub fn without_columns<S: AsRef<str>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.query = self.query.without_columns(names);
        self
    }

    /// Lets the person asking use only these columns, as [`TableQuery::only_columns`]
    /// does for the table they were drawn in.
    #[must_use]
    pub fn only_columns<S: AsRef<str>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.query = self.query.only_columns(names);
        self
    }

    /// What's asked.
    pub fn op(&self) -> ViewOp {
        self.op
    }

    /// The name given.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The view named.
    pub fn view(&self) -> &str {
        &self.view
    }

    /// Whether the table should open with this view.
    pub fn is_default(&self) -> bool {
        self.is_default
    }

    /// The state to save.
    pub fn query(&self) -> &TableQuery<R, F> {
        &self.query
    }

    /// Says so, and redraws the table.
    pub fn done(&self, text: impl Into<String>) -> Reply {
        Reply::ops_or_redirect(self.query.href())
            .toast(ToastTone::Success, text)
            .op(Op::Invalidate {
                target: self.table.clone(),
            })
    }

    /// Refuses, saving nothing: says why, with `status`, and redraws the table. A person
    /// who already keeps the most views a table allows (`TooManyViews`) is answered this way.
    pub fn refused(&self, status: StatusCode, text: impl Into<String>) -> Reply {
        Reply::ops_or_redirect(self.query.href())
            .toast(ToastTone::Danger, text)
            .op(Op::Invalidate {
                target: self.table.clone(),
            })
            .status(status)
    }
}

impl<R, F> FromRequest for ViewPost<R, F>
where
    R: TableRow + Send + 'static,
    F: FilterBar + Default + DeserializeOwned + Send + 'static,
{
    type Rejection = StatusCode;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (parts, body) = request.into_parts();
        if let Some(status) = crate::form::refusal(&parts).await {
            return Err(status);
        }
        let limit = BodyLimit::of(&parts.extensions);
        let bytes = read_body(body, limit)
            .await
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        let (mut op, mut name, mut view, mut is_default, mut table, mut path, mut state) = (
            ViewOp::Save,
            String::new(),
            String::new(),
            false,
            String::new(),
            String::new(),
            String::new(),
        );
        for (key, value) in urlencoded::parse(&bytes) {
            match key.as_ref() {
                "_op" if value == "delete" => op = ViewOp::Delete,
                "_name" => name = value.into_owned(),
                "_view" => view = value.into_owned(),
                "_default" => is_default = value == "1",
                "_table" => table = value.into_owned(),
                "_path" => path = value.into_owned(),
                "_q" => state = value.into_owned(),
                _ => {}
            }
        }
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['\\', '\r', '\n']) {
            path = "/".to_owned();
        }
        if !table
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            table.clear();
        }
        if op == ViewOp::Save && name.trim().is_empty() {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
        Ok(Self {
            op,
            name,
            view,
            is_default,
            table,
            query: TableQuery::parse(path, &state),
        })
    }
}

/// The Views popover: a link for each view, a form to save this one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn menu(
    table: &str,
    query: &str,
    views: &[SavedView],
    url: &Href,
    path: &str,
    labels: &TableLabels,
    csrf: Option<&str>,
    popover: impl Fn(String, Element, String, Vec<Node>) -> Node,
) -> Node {
    let hidden = |name: &'static str, value: String| {
        el("input")
            .attr("type", "hidden")
            .attr("name", name)
            .attr("value", value)
    };
    let common = || {
        Node::list((
            hidden("_table", table.to_owned()),
            hidden("_path", path.to_owned()),
            hidden("_q", query.to_owned()),
            csrf.map(|token| csrf_field(token.to_owned())),
        ))
    };
    let rows: Vec<Element> = views
        .iter()
        .map(|view| {
            let href = if view.query.is_empty() {
                path.to_owned()
            } else {
                format!("{path}?{}", view.query)
            };
            let active = view.query == query;
            let delete: Cow<'static, str> =
                labels.delete_view.replace("{label}", &view.name).into();
            el("div").slot("view-row").children((
                el("a")
                    .slot("menu-link")
                    .href(href)
                    .flag("data-ui-in-region", true)
                    .flag("data-active", active)
                    .attr_opt("aria-current", active.then_some("true"))
                    .children((
                        el("bdi").child(view.name.clone()),
                        view.is_default.then(|| {
                            el("span")
                                .slot("view-default")
                                .child(labels.default_view.to_string())
                        }),
                    )),
                el("form")
                    .attr("method", "post")
                    .attr("action", url.clone())
                    .flag("data-ui-form", true)
                    .children((
                        common(),
                        hidden("_view", view.id.clone()),
                        el("button")
                            .slot("button")
                            .attr("data-variant", "ghost")
                            .attr("data-size", "xs")
                            .flag("data-icon-only", true)
                            .attr("type", "submit")
                            .attr("name", "_op")
                            .attr("value", "delete")
                            .attr("aria-label", delete.to_string())
                            .child(crate::components::icon(icons::X)),
                    )),
            ))
        })
        .collect();
    let save = el("form")
        .slot("view-save")
        .attr("method", "post")
        .attr("action", url.clone())
        .flag("data-ui-form", true)
        .children((
            common(),
            el("input")
                .slot("input")
                .attr("data-size", "sm")
                .attr("type", "text")
                .attr("name", "_name")
                .attr("required", "")
                .attr("maxlength", 80_i64)
                .attr("placeholder", labels.view_name.to_string())
                .attr("aria-label", labels.view_name.to_string()),
            el("label").slot("filter-option").children((
                el("input")
                    .slot("checkbox")
                    .attr("type", "checkbox")
                    .attr("name", "_default")
                    .attr("value", "1"),
                labels.make_default.to_string(),
            )),
            el("button")
                .slot("button")
                .attr("data-variant", "outline")
                .attr("data-size", "sm")
                .attr("type", "submit")
                .attr("name", "_op")
                .attr("value", "save")
                .child(labels.save_view.to_string()),
        ));
    let trigger = el("button")
        .slot("button")
        .attr("data-variant", "outline")
        .attr("data-size", "sm")
        .children((
            crate::components::icon(icons::STAR),
            labels.views.to_string(),
        ));
    popover(
        format!("{table}-views"),
        trigger,
        labels.views.to_string(),
        vec![el("div").slot("view-list").child(rows).into(), save.into()],
    )
}

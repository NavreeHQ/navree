//! The rows of a [`DataTable`] for a native client.
//!
//! For a browser a table's rows skip the tree: they are the bulk of a page,
//! and each is written straight to one string. A string can't be read into
//! native parts, so when a native client asked, the rows are also built as a
//! tree like everything else on the page: each cell is the component its
//! column draws, or its text; a box to select the row and a menu of its actions
//! are the same parts the string has. The detail of a row is built for every
//! row, shut: a client opens it itself, with no request.
//!
//! The tree is not put in the page. The page keeps the string, so a table built
//! for a native client and written for a browser is the table it always was; the
//! tree is kept with the request, by the string it stands for, and the reader of
//! the table finds it there.

use serde::de::DeserializeOwned;

use super::DataTable;
use crate::components::icon;
use crate::icons;
use crate::node::{Node, el};
use crate::table::{ColumnRole, FilterBar, TableRow};

/// A column role's word on a cell, for the reader of the table.
fn role_name(role: ColumnRole) -> &'static str {
    match role {
        ColumnRole::Title => "title",
        ColumnRole::Subtitle => "subtitle",
        ColumnRole::Status => "status",
        ColumnRole::Trailing => "trailing",
    }
}

impl<R: TableRow, F: FilterBar + Default + DeserializeOwned> DataTable<'_, R, F> {
    /// The rows as a tree, when a native client asked. `None` for a browser.
    pub(super) fn native_rows(&self) -> Option<Node> {
        if !crate::native::Client::current().is_native() {
            return None;
        }
        let state = &self.query.state;
        let roles = R::column_roles();
        let role_of = |name: &str| {
            roles
                .iter()
                .find(|(column, _)| *column == name)
                .map(|(_, role)| role_name(*role))
        };
        // A subtitle may be a column the table does not show: it comes after
        // the shown ones, with its name and its heading.
        let hidden: Vec<usize> = roles
            .iter()
            .filter(|(_, role)| *role == ColumnRole::Subtitle)
            .filter_map(|(name, _)| R::COLUMNS.iter().position(|column| column.name == *name))
            .filter(|at| !state.cols.contains(at))
            .collect();
        let span = self.span();
        let mut rows = Vec::with_capacity(self.data.rows.len());
        let mut text = String::new();
        for row in self.data.rows {
            let key = row.key();
            let mut cells: Vec<Node> = Vec::with_capacity(state.cols.len() + hidden.len() + 3);
            if self.selectable() {
                text.clear();
                if let Some(first) = state.cols.first() {
                    row.write_text(*first, &mut text);
                }
                let mut box_ = el("input")
                    .slot("checkbox")
                    .attr("type", "checkbox")
                    .flag("data-ui-select-row", true)
                    .attr("name", "ids")
                    .attr("value", key.clone().unwrap_or_default())
                    .attr("form", self.bulk_id())
                    .attr(
                        "aria-label",
                        self.labels.select_row.replace("{label}", &text),
                    );
                if self.selectable_if.as_ref().is_some_and(|can| !can(row)) {
                    box_ = box_.flag("disabled", true);
                }
                cells.push(el("td").slot("cell-select").child(box_).into());
            }
            for at in &state.cols {
                let draw = self
                    .cells
                    .iter()
                    .find(|(name, _)| *name == self.query.column(*at).name)
                    .map(|(_, draw)| draw);
                cells.push(
                    el("td")
                        .attr_opt("data-role", role_of(self.query.column(*at).name))
                        .child(match draw {
                            Some(draw) => draw(row),
                            None => {
                                text.clear();
                                row.write_text(*at, &mut text);
                                Node::Text(text.clone().into())
                            }
                        })
                        .into(),
                );
            }
            // The menu is the last column of the table; a subtitle the table does not show
            // comes after it, with the columns that are not drawn.
            if self.has_actions() {
                text.clear();
                if let Some(first) = state.cols.first() {
                    row.write_text(*first, &mut text);
                }
                cells.push(self.native_actions(key.as_deref().unwrap_or_default(), row, &text));
            }
            for at in &hidden {
                text.clear();
                row.write_text(*at, &mut text);
                let column = &R::COLUMNS[*at];
                cells.push(
                    el("td")
                        .attr("data-role", "subtitle")
                        .attr("data-col", column.name)
                        .attr("data-label", column.label)
                        .child(Node::Text(text.clone().into()))
                        .into(),
                );
            }
            rows.push(
                el("tr")
                    .attr_opt("data-key", key.clone())
                    .attr_opt(
                        "data-ui-row-href",
                        self.row_href.as_ref().map(|href| href(row)),
                    )
                    .attr_opt(
                        "data-ui-row-opens",
                        self.row_opens.as_ref().map(|opens| opens(row)),
                    )
                    .child(cells),
            );
            if let Some(detail) = &self.detail {
                rows.push(
                    el("tr")
                        .slot("row-detail")
                        .attr_opt("data-key", key.as_ref().map(|key| format!("{key}-detail")))
                        .child(el("td").attr("colspan", span as i64).child(detail(row))),
                );
            }
        }
        Some(Node::list(rows))
    }

    /// A row's menu: the button named for the row and the menu it opens.
    fn native_actions(&self, key: &str, row: &R, label: &str) -> Node {
        let Some(actions) = &self.row_actions else {
            return Node::Empty;
        };
        let actions = actions(row);
        let cell = el("td").slot("cell-actions");
        if actions.is_empty() {
            return cell.into();
        }
        let menu_id = format!("{}-a-{key}", self.id);
        let trigger_id = format!("{menu_id}-t");
        let items: Vec<Node> = actions
            .into_iter()
            .map(|action| self.action_item(action))
            .collect();
        let trigger = el("button")
            .slot("button")
            .attr("data-variant", "ghost")
            .attr("data-size", "sm")
            .flag("data-icon-only", true)
            .attr("id", trigger_id.clone())
            .attr("type", "button")
            .attr("popovertarget", menu_id.clone())
            .attr("aria-haspopup", "menu")
            .attr("aria-expanded", "false")
            .attr("data-ui-keep-attrs", "aria-expanded")
            .attr(
                "aria-label",
                self.labels.row_actions.replace("{label}", label),
            )
            .child(icon(icons::ELLIPSIS));
        let menu = crate::components::menu_element(
            &menu_id,
            &trigger_id,
            None,
            "bottom",
            crate::components::PopupAlign::End,
            false,
        )
        .child(items);
        cell.child(Node::list((trigger, menu))).into()
    }
}

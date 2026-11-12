//! The rows of a [`DataTable`] for a native client.
//!
//! For a browser a table's rows skip the tree: they are the bulk of a page,
//! and each is written straight to one string. A string can't be read into
//! native parts, so when a native client asked, the rows are built as a tree
//! like everything else on the page: each cell is the component its column
//! draws, or its text.

use serde::de::DeserializeOwned;

use super::DataTable;
use crate::node::{Node, el};
use crate::table::{FilterBar, TableRow};

impl<R: TableRow, F: FilterBar + Default + DeserializeOwned> DataTable<'_, R, F> {
    /// The rows as a tree, when a native client asked and the table has
    /// nothing a native row can't hold yet: a checkbox to select it, a menu
    /// of actions, a detail that opens under it. `None` leaves the rows to
    /// the string a browser gets.
    pub(super) fn native_rows(&self) -> Option<Node> {
        if !crate::native::Client::current().is_native()
            || self.selectable()
            || self.has_actions()
            || self.has_detail()
        {
            return None;
        }
        let state = &self.query.state;
        let mut rows = Vec::with_capacity(self.data.rows.len());
        let mut text = String::new();
        for row in self.data.rows {
            let mut cells = Vec::with_capacity(state.cols.len());
            for at in &state.cols {
                let draw = self
                    .cells
                    .iter()
                    .find(|(name, _)| *name == self.query.column(*at).name)
                    .map(|(_, draw)| draw);
                cells.push(el("td").child(match draw {
                    Some(draw) => draw(row),
                    None => {
                        text.clear();
                        row.write_text(*at, &mut text);
                        Node::Text(text.clone().into())
                    }
                }));
            }
            rows.push(
                el("tr")
                    .attr_opt("data-key", row.key())
                    .attr_opt(
                        "data-ui-row-href",
                        self.row_href.as_ref().map(|href| href(row)),
                    )
                    .child(cells),
            );
        }
        Some(Node::list(rows))
    }
}

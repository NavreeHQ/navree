//! Builders for the changes a route answers a native client with when it
//! answers ops and not a screen: a list's next rows, what changed in a list,
//! a table's next records, a part drawn again.

use super::{IntoParts, list_of};
use crate::node::{Node, TableRow};
use crate::op::{Edge, Op};

impl Op {
    /// Puts `part` in place of the part `target`, keeping what the reader did
    /// to the parts that are in both: what they typed, what is open.
    pub fn morph(target: impl Into<String>, part: impl Into<Node>) -> Self {
        Self::Morph {
            target: target.into(),
            node: part.into(),
        }
    }

    /// Puts `part` in place of the part `target`, as new: nothing is kept.
    pub fn replace(target: impl Into<String>, part: impl Into<Node>) -> Self {
        Self::Replace {
            target: target.into(),
            node: part.into(),
        }
    }

    /// Adds `parts` at the end of the part `target`.
    pub fn append(target: impl Into<String>, parts: impl IntoParts) -> Self {
        Self::Append {
            target: target.into(),
            nodes: list_of(parts),
            more: None,
        }
    }

    /// Adds `parts` at the start of the part `target`.
    pub fn prepend(target: impl Into<String>, parts: impl IntoParts) -> Self {
        Self::Prepend {
            target: target.into(),
            nodes: list_of(parts),
            before: None,
        }
    }

    /// Takes the part or the table row `target` out.
    pub fn remove(target: impl Into<String>) -> Self {
        Self::Remove {
            target: target.into(),
        }
    }

    /// The next rows of the list `target`, by cursor. `more` is where the rows
    /// after these are, or none when these are the last.
    pub fn next_rows(
        target: impl Into<String>,
        rows: impl IntoParts,
        more: Option<String>,
    ) -> Self {
        Self::Append {
            target: target.into(),
            nodes: list_of(rows),
            more,
        }
    }

    /// The rows before the first of the list `target`, by cursor. `before` is
    /// where the rows before these are, or none when these are the first.
    pub fn earlier_rows(
        target: impl Into<String>,
        rows: impl IntoParts,
        before: Option<String>,
    ) -> Self {
        Self::Prepend {
            target: target.into(),
            nodes: list_of(rows),
            before,
        }
    }

    /// Brings the list `target` up to date. `rows` are the rows that are new
    /// or changed: each takes the place of the row with its id, or joins the
    /// list at `at` when the list has none. `gone` are the ids of the rows that
    /// left, and `since` is the revision the list is at now.
    pub fn merge(
        target: impl Into<String>,
        since: impl Into<String>,
        at: Edge,
        rows: impl IntoParts,
        gone: impl IntoIterator<Item = String>,
    ) -> Self {
        Self::Merge {
            target: target.into(),
            nodes: list_of(rows),
            gone: gone.into_iter().collect(),
            since: since.into(),
            at,
            more: false,
        }
    }

    /// The next records of the table `target`, by cursor. `more` is where the
    /// records after these are, or none when these are the last.
    pub fn next_records(
        target: impl Into<String>,
        rows: impl IntoIterator<Item = TableRow>,
        more: Option<String>,
    ) -> Self {
        Self::Rows {
            target: target.into(),
            rows: rows.into_iter().collect(),
            at: Edge::End,
            more,
            prev: None,
        }
    }

    /// Puts `row` in place of the table row `target`, keeping whether it is
    /// chosen and whether its detail is open.
    pub fn record(target: impl Into<String>, row: TableRow) -> Self {
        Self::Row {
            target: target.into(),
            row,
        }
    }
}

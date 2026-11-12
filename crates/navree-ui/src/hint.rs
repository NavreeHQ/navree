//! What a part means, in words that mean the same on the web: a button's
//! [`Role`] and a table column's [`ColumnRole`].
//!
//! A browser places a page's parts where the page puts them, so it needs
//! none of this. A native client places them by what they mean, as its own
//! system does: the action that creates goes where creation goes, the column
//! that names a record is the title of its row. The words are fields of the
//! component; with the `native-preview` feature they are written into the
//! tree only while a native client asks, as a mark that writes no HTML, so a
//! page's markup never changes by a byte.

/// What a button is for, when its look does not say it.
///
/// ```
/// use navree_ui::{Button, Role};
///
/// let new = Button::primary("New order").href("/orders/new").role(Role::Create);
/// assert!(navree_ui::render(&new.into()).starts_with("<a "));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Role {
    /// The one thing the screen or the form is for.
    Primary,
    /// Something else a reader may do here.
    Secondary,
    /// It deletes, or can't be undone.
    Destructive,
    /// It leaves without doing anything.
    Cancel,
    /// It agrees to what a dialog asks.
    Confirm,
    /// It starts a new thing.
    Create,
    /// It goes somewhere else in the app.
    Navigation,
}

/// What a table's column is to a record, for a screen too narrow for a table,
/// where each record is a row of a list: `#[col(role = title)]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ColumnRole {
    /// The record's name: the row's title.
    Title,
    /// A line about it, under the name. It may be a hidden column.
    Subtitle,
    /// Its state, drawn as a badge.
    Status,
    /// What stands at the row's end: an amount, a time.
    Trailing,
}

#[cfg(feature = "native-preview")]
pub(crate) use marks::{mark, role_of};

/// The marks a hint is written into the tree with: a zero-length slice of a
/// text of the toolkit's own, which writes nothing wherever it stands, so the
/// HTML of a node that carries one is the HTML of the node without it. Each
/// mark is told apart by where its slice starts.
#[cfg(feature = "native-preview")]
mod marks {
    use super::Role;
    use crate::node::Node;

    /// The text the marks are slices of. Its first byte is no mark, so a slice
    /// that ends another text just before it is never read as one.
    static MARKS: &str = "--------";

    /// The mark of `role`.
    pub(crate) fn mark(role: Role) -> Node {
        let at = match role {
            Role::Primary => 1,
            Role::Secondary => 2,
            Role::Destructive => 3,
            Role::Cancel => 4,
            Role::Confirm => 5,
            Role::Create => 6,
            Role::Navigation => 7,
        };
        Node::Static(&MARKS[at..at])
    }

    /// The role `text` marks, when it is a mark.
    pub(crate) fn role_of(text: &str) -> Option<Role> {
        if !text.is_empty() {
            return None;
        }
        let at = (text.as_ptr() as usize).checked_sub(MARKS.as_ptr() as usize)?;
        Some(match at {
            1 => Role::Primary,
            2 => Role::Secondary,
            3 => Role::Destructive,
            4 => Role::Cancel,
            5 => Role::Confirm,
            6 => Role::Create,
            7 => Role::Navigation,
            _ => return None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::render::render;

        #[test]
        fn a_mark_writes_nothing_and_says_its_role() {
            for role in [
                Role::Primary,
                Role::Secondary,
                Role::Destructive,
                Role::Cancel,
                Role::Confirm,
                Role::Create,
                Role::Navigation,
            ] {
                let mark = mark(role);
                assert_eq!(render(&mark), "");
                let Node::Static(text) = mark else {
                    panic!("not a mark");
                };
                assert_eq!(role_of(text), Some(role));
            }
            // Text that is not a mark says nothing.
            assert_eq!(role_of(""), None);
            assert_eq!(role_of("create"), None);
            assert_eq!(role_of(&MARKS[0..0]), None);
            assert_eq!(role_of(&MARKS[8..]), None);
        }
    }
}

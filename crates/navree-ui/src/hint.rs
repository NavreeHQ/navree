//! What a part means, in words that mean the same on the web: a button's
//! [`Role`], how the screen a link leads to is [`Presented`], a [`Standard`]
//! action, and a table column's [`ColumnRole`].
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

/// How the screen a link leads to is presented by a native client, when the
/// page knows: a link says nothing of it, and the shell's routes decide.
///
/// ```
/// use navree_ui::{Button, Presented};
///
/// let new = Button::primary("New order").href("/orders/new").presented(Presented::Task);
/// assert!(navree_ui::render(&new.into()).starts_with("<a "));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Presented {
    /// On top of this screen, with a way back.
    Push,
    /// In place of this screen.
    Replace,
    /// As the first screen of its destination, with nothing under it.
    Root,
    /// In a sheet over this one: a glance at something, put away with a swipe.
    Sheet,
    /// Over this one as a task the person finishes or cancels: a form to fill in.
    Task,
    /// Over the whole screen.
    Full,
}

/// An action every system has its own symbol and words for, said by its meaning: a
/// native client draws the system's own symbol and label for it (a share button
/// looks like every other share button there), whatever icon and words the page gave.
///
/// ```
/// use navree_ui::{Button, Standard};
///
/// let share = Button::outline("Send the invoice")
///     .href("/invoices/7/share")
///     .standard(Standard::Share);
/// assert!(navree_ui::render(&share.into()).starts_with("<a "));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Standard {
    /// Sends something to another app or person.
    Share,
    /// Deletes something.
    Delete,
    /// Adds a new thing.
    Add,
    /// Changes something.
    Edit,
    /// Looks for something.
    Search,
    /// Finishes a task, keeping what was done.
    Done,
    /// Leaves a task, keeping nothing.
    Cancel,
    /// Offers more actions.
    More,
}

/// What an element says to a native client beyond what its markup does. Written into
/// the tree only while a native client asks, and never into the HTML: a page's markup
/// does not change by a byte.
#[cfg(feature = "native-preview")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hints {
    /// How the screen a link leads to is presented.
    pub(crate) presented: Option<Presented>,
    /// Whether what the control does can be undone.
    pub(crate) undoable: bool,
    /// The standard action it is.
    pub(crate) standard: Option<Standard>,
    /// Why it can't be used now.
    pub(crate) reason: Option<String>,
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
pub(crate) use marks::{KEYS, key_mark, key_of, mark, role_of};

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

    /// How many keys of what an element says there can be in one request.
    pub(crate) const KEYS: usize = 1 << 16;

    static KEY_BYTES: [u8; KEYS] = [b'-'; KEYS];

    /// The text the keys are slices of: a key is where its slice starts.
    static KEY_TEXT: &str = match std::str::from_utf8(&KEY_BYTES) {
        Ok(text) => text,
        Err(_) => "",
    };

    /// The mark that carries `key`, which is at least 1. `None` past the keys there are.
    pub(crate) fn key_mark(key: u32) -> Option<Node> {
        let at = usize::try_from(key)
            .ok()
            .filter(|at| (1..KEYS).contains(at))?;
        Some(Node::Static(&KEY_TEXT[at..at]))
    }

    /// The key a node carries, when it is a mark of one.
    pub(crate) fn key_of(node: &Node) -> Option<u32> {
        let Node::Static(text) = node else {
            return None;
        };
        if !text.is_empty() {
            return None;
        }
        let at = (text.as_ptr() as usize).checked_sub(KEY_TEXT.as_ptr() as usize)?;
        (1..KEYS)
            .contains(&at)
            .then(|| u32::try_from(at).ok())
            .flatten()
    }

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
            assert_eq!(role_of(&MARKS[8..8]), None);
        }
    }
}

//! [`Op`]: one change the server makes to the screen in front. The same closed
//! list as the web's ops, with a node where the web has HTML.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::action::{Pair, Present};
use crate::node::{Node, TableRow, Tone, is_default, is_false};

/// How a change of address enters the client's history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlMode {
    /// A new entry.
    #[default]
    Push,
    /// In place of the current one.
    Replace,
}

/// What a toast's button does.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastAction {
    /// What the button says.
    pub label: String,
    /// Where it leads or posts.
    pub href: String,
    /// Whether it posts.
    #[serde(default, skip_serializing_if = "is_false")]
    pub post: bool,
    /// What goes with the post: the page's token among them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Pair>,
}

/// Which end of a list or a table new rows join.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Edge {
    /// The start: the top of a list with the newest first.
    Start,
    /// The end.
    #[default]
    End,
}

/// A short message that leaves by itself: a snackbar, a banner, a toast, as the
/// system has it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toast {
    /// What it means.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub tone: Tone,
    /// What it says.
    pub text: String,
    /// A heading over the words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A name: a later toast of the same name takes this one's place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Its button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ToastAction>,
}

/// One change to the screen in front. A client applies ops in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Op {
    /// Puts `node` in place of the node `target`, keeping what the reader did
    /// to the nodes that are in both: typed values, the focus, the scroll, what
    /// is open.
    Morph {
        /// The id of the node to change.
        target: String,
        /// What it becomes.
        node: Node,
    },
    /// Puts `node` in place of the node `target`, as new: nothing is kept.
    Replace {
        /// The id of the node to change.
        target: String,
        /// What takes its place.
        node: Node,
    },
    /// Adds nodes at the end of `target`'s children.
    Append {
        /// The id of the node to add to.
        target: String,
        /// What is added.
        nodes: Vec<Node>,
        /// For a list: where the rows after these are, or none when these are the last.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        more: Option<String>,
    },
    /// Adds nodes at the start of `target`'s children.
    Prepend {
        /// The id of the node to add to.
        target: String,
        /// What is added.
        nodes: Vec<Node>,
        /// For a list: where the rows before these are, or none when these are
        /// the first.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
    },
    /// Takes the node or the table row `target` out.
    Remove {
        /// The id of the node or the row.
        target: String,
    },
    /// Brings a list up to date with what changed since its revision: each
    /// node takes the place of the row with its id where it stands, or joins
    /// the list at `at` when the list has none; each id in `gone` leaves. What
    /// the reader did to a row that stays (a caret, the focus) is kept.
    Merge {
        /// The list's id.
        target: String,
        /// The rows that are new or changed, as they are now.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        nodes: Vec<Node>,
        /// The ids of the rows that left the list.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        gone: Vec<String>,
        /// The revision the list is at now: what its next ask for changes says.
        since: String,
        /// Where new rows join.
        #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
        at: Edge,
        /// Whether more changes follow this revision: the client asks again at once.
        #[serde(default, skip_serializing_if = "is_false")]
        more: bool,
    },
    /// Adds records to a table: the next ones by cursor, or new ones.
    Rows {
        /// The table's id.
        target: String,
        /// The records, in the table's order.
        rows: Vec<TableRow>,
        /// Which end they join.
        #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
        at: Edge,
        /// Where the records after these are, by cursor, or none when these are
        /// the last: the table's `more` from now on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        more: Option<String>,
    },
    /// Puts `row` in place of the table row `target`, keeping what the reader
    /// did to it: whether it is chosen, whether its detail is open.
    Row {
        /// The id of the row to change.
        target: String,
        /// What it becomes.
        row: TableRow,
    },
    /// Shows messages at the fields of a form, or clears them when `fields` is empty.
    Errors {
        /// The form's id.
        form: String,
        /// Messages by field name.
        fields: BTreeMap<String, Vec<String>>,
    },
    /// Shows a toast.
    Toast(Toast),
    /// Presents the sheet, dialog or popover `target`, or opens a disclosure.
    Open {
        /// The node's id.
        target: String,
    },
    /// Dismisses the sheet, dialog or popover `target`, or closes a disclosure.
    Close {
        /// The node's id.
        target: String,
    },
    /// Moves the focus to `target`.
    Focus {
        /// The control's id.
        target: String,
    },
    /// Changes the address the screen in front answers to, loading nothing.
    Url {
        /// The address.
        href: String,
        /// How it enters history.
        #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
        mode: UrlMode,
    },
    /// Goes to a screen of the app.
    Navigate {
        /// The address.
        href: String,
        /// How the screen is presented. When not said, the client's own rules
        /// for where a post leads decide, then the shell's routes.
        #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
        present: Present,
    },
    /// Leaves for another site, in the system's browser.
    NavigateExternal {
        /// The address.
        href: String,
    },
    /// Fetches the screen in front again.
    Reload,
    /// Fetches the region `target` again.
    Invalidate {
        /// The region's id.
        target: String,
    },
    /// Starts a device call that needs no tap: a haptic, the app's badge, a
    /// cancelled notification, the store's review prompt. The type has no
    /// other calls, so a reply can't open the camera or ask for a location.
    Device {
        /// The call.
        call: crate::device::ReplyCall,
    },
    /// An op from a newer server, which this build does not know. A client
    /// skips it and applies the rest.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_are_named_objects_with_a_node_where_the_web_has_markup() {
        let ops = vec![
            Op::Morph {
                target: "order".to_owned(),
                node: Node::text("Paid"),
            },
            Op::Append {
                target: "feed".to_owned(),
                nodes: vec![Node::text("Shipped")],
                more: Some("/feed?after=9".to_owned()),
            },
            Op::Errors {
                form: "refund".to_owned(),
                fields: BTreeMap::from([(
                    "amount".to_owned(),
                    vec!["Enter an amount above zero.".to_owned()],
                )]),
            },
            Op::Toast(Toast {
                tone: Tone::Success,
                text: "Refunded".to_owned(),
                title: Some("Done".to_owned()),
                ..Toast::default()
            }),
            Op::Close {
                target: "refund".to_owned(),
            },
            Op::Url {
                href: "/orders?page=2".to_owned(),
                mode: UrlMode::Replace,
            },
            Op::Navigate {
                href: "/orders/7".to_owned(),
                present: Present::Auto,
            },
            Op::NavigateExternal {
                href: "https://pay.example/7".to_owned(),
            },
            Op::Reload,
            Op::Invalidate {
                target: "figures".to_owned(),
            },
        ];
        let json = serde_json::to_string(&ops).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"[{"op":"morph","target":"order","node":{"t":"text","value":"Paid"}},"#,
                r#"{"op":"append","target":"feed","nodes":[{"t":"text","value":"Shipped"}],"more":"/feed?after=9"},"#,
                r#"{"op":"errors","form":"refund","fields":{"amount":["Enter an amount above zero."]}},"#,
                r#"{"op":"toast","tone":"success","text":"Refunded","title":"Done"},"#,
                r#"{"op":"close","target":"refund"},"#,
                r#"{"op":"url","href":"/orders?page=2","mode":"replace"},"#,
                r#"{"op":"navigate","href":"/orders/7"},"#,
                r#"{"op":"navigate_external","href":"https://pay.example/7"},"#,
                r#"{"op":"reload"},"#,
                r#"{"op":"invalidate","target":"figures"}]"#
            )
        );
        assert_eq!(serde_json::from_str::<Vec<Op>>(&json).unwrap(), ops);
    }

    #[test]
    fn a_navigation_after_a_post_may_say_how_it_is_presented() {
        let op = Op::Navigate {
            href: "/orders/7".to_owned(),
            present: Present::Replace,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(json, r#"{"op":"navigate","href":"/orders/7","present":"replace"}"#);
        assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
    }

    #[test]
    fn a_list_and_a_table_change_row_by_row() {
        let merge: Op = serde_json::from_str(
            r#"{"op":"merge","target":"inbox","nodes":[{"t":"row","id":"c-88","title":"Harpreet Sandhu"}],"gone":["c-12"],"since":"0000412","at":"start"}"#,
        )
        .unwrap();
        let Op::Merge { gone, since, at, more, .. } = &merge else {
            panic!("{merge:?}");
        };
        assert_eq!((gone.as_slice(), since.as_str(), *at, *more), (&["c-12".to_owned()][..], "0000412", Edge::Start, false));
        let rows: Op = serde_json::from_str(
            r#"{"op":"rows","target":"orders","rows":[{"id":"1043","cells":[{"t":"text","value":"CH-1043"}]}],"more":"/orders?after=1043"}"#,
        )
        .unwrap();
        let Op::Rows { rows, at, more, .. } = &rows else {
            panic!("{rows:?}");
        };
        assert_eq!((rows.len(), *at, more.as_deref()), (1, Edge::End, Some("/orders?after=1043")));
        assert!(serde_json::from_str::<Op>(r#"{"op":"row","target":"1043","row":{"id":"1043"}}"#).is_ok());
        // A revision is needed: without it the next ask could not say where it stands.
        assert!(serde_json::from_str::<Op>(r#"{"op":"merge","target":"inbox"}"#).is_err());
    }

    #[test]
    fn an_op_from_a_newer_server_is_read_as_unknown() {
        let ops: Vec<Op> =
            serde_json::from_str(r#"[{"op":"confetti","colours":3},{"op":"reload"}]"#).unwrap();
        assert_eq!(ops, [Op::Unknown, Op::Reload]);
    }
}

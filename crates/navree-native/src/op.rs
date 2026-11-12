//! [`Op`]: one change the server makes to the screen in front. The same closed
//! list as the web's ops, with a node where the web has HTML.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::node::{Node, Tone, is_default, is_false};

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
}

/// A short message that leaves by itself: a snackbar, a banner, a toast, as the
/// system has it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toast {
    /// What it means.
    #[serde(default, skip_serializing_if = "is_default")]
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
    },
    /// Takes the node `target` out.
    Remove {
        /// The node's id.
        target: String,
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
        #[serde(default, skip_serializing_if = "is_default")]
        mode: UrlMode,
    },
    /// Goes to a screen of the app.
    Navigate {
        /// The address.
        href: String,
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
    fn an_op_from_a_newer_server_is_read_as_unknown() {
        let ops: Vec<Op> =
            serde_json::from_str(r#"[{"op":"confetti","colours":3},{"op":"reload"}]"#).unwrap();
        assert_eq!(ops, [Op::Unknown, Op::Reload]);
    }
}

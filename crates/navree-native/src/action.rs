//! [`Action`]: what a control does, and [`Command`]: what a client changes on
//! its own, with no request.
//!
//! Both lists are closed. An action is data a client interprets; nothing in it
//! is code.

use serde::{Deserialize, Serialize};

use crate::node::{Tone, is_default};

/// How a screen a link leads to is presented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Present {
    /// On top of this one, with a way back.
    #[default]
    Push,
    /// In place of this one.
    Replace,
    /// As the first screen of its destination, with nothing under it.
    Root,
    /// In a sheet over this one.
    Sheet,
    /// Over the whole screen.
    Full,
    /// In the system's browser: it is not part of the app.
    External,
}

/// A value sent with a request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pair {
    /// The name.
    pub name: String,
    /// The value.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
}

/// What a control does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Action {
    /// Goes to a screen.
    Navigate {
        /// The screen's address.
        href: String,
        /// How it is presented. The shell's routes decide when this is the default.
        #[serde(default, skip_serializing_if = "is_default")]
        present: Present,
    },
    /// Sends a form.
    Submit {
        /// The form's id. The form the control stands in, when there is none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        form: Option<String>,
        /// Where to send it, when not to the form's own address.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        href: Option<String>,
        /// The control's own name and value, sent with the form's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        with: Option<Pair>,
    },
    /// Sends a request of its own: a button that posts.
    Request {
        /// `get` or `post`.
        #[serde(default, skip_serializing_if = "is_default")]
        method: crate::node::Method,
        /// Where to.
        href: String,
        /// What goes with it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fields: Vec<Pair>,
    },
    /// Changes the screen in front, with no request.
    Commands {
        /// The commands, run in order.
        list: Vec<Command>,
    },
    /// Goes back one screen.
    Back,
    /// Fetches the screen again.
    Refresh,
    /// Closes the sheet or dialog the control stands in.
    Dismiss,
    /// Starts a device call: a share sheet, a scanner, a confirmation. Only a
    /// person's tap runs an action, which is what most calls require.
    Device(crate::device::DeviceCall),
    /// An action from a newer server, which this build does not know. A client
    /// draws the control as one that can't be used now.
    #[serde(other)]
    Unknown,
}

impl Action {
    /// Goes to `href`, pushed.
    pub fn navigate(href: impl Into<String>) -> Self {
        Self::Navigate {
            href: href.into(),
            present: Present::Push,
        }
    }

    /// Sends the form the control stands in.
    pub fn submit() -> Self {
        Self::Submit {
            form: None,
            href: None,
            with: None,
        }
    }

    /// Runs one command.
    pub fn command(command: Command) -> Self {
        Self::Commands {
            list: vec![command],
        }
    }
}

/// A state a command sets on a node: the closed set a client keeps for every
/// node it draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum State {
    /// Shown open: a disclosure, a menu.
    Open,
    /// Not drawn.
    Hidden,
    /// Checked.
    Checked,
    /// Chosen among its neighbours.
    Selected,
    /// Can't be used.
    Disabled,
    /// The one in use among its neighbours.
    Active,
    /// Its value is wrong.
    Invalid,
    /// Waiting for an answer.
    Pending,
}

/// One change a client makes on its own. The same closed list as the web
/// runtime's commands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "c", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Command {
    /// Shows a hidden node, or hides a shown one.
    Toggle {
        /// The node's id.
        id: String,
    },
    /// Shows a node.
    Show {
        /// The node's id.
        id: String,
    },
    /// Hides a node.
    Hide {
        /// The node's id.
        id: String,
    },
    /// Presents a sheet, a dialog or a popover, or opens a disclosure.
    Open {
        /// The node's id.
        id: String,
    },
    /// Dismisses a sheet, a dialog or a popover, or closes a disclosure.
    Close {
        /// The node's id.
        id: String,
    },
    /// Copies text to the clipboard.
    Copy {
        /// The text.
        text: String,
    },
    /// Sets or clears a state of a node.
    Set {
        /// The node's id.
        id: String,
        /// The state.
        state: State,
        /// Whether it is set or cleared.
        on: bool,
    },
    /// Flips a state of a node.
    Flip {
        /// The node's id.
        id: String,
        /// The state.
        state: State,
    },
    /// Moves the focus to a control.
    Focus {
        /// The control's id.
        id: String,
    },
    /// Sends a form.
    Submit {
        /// The form's id.
        id: String,
    },
    /// Sets the words of a text node.
    Text {
        /// The node's id.
        id: String,
        /// The words.
        text: String,
    },
    /// Writes how many characters the control this runs on holds into a text
    /// node, as `12`, or `12/160` with a most.
    Count {
        /// The text node's id.
        id: String,
        /// The most the control takes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<u32>,
    },
    /// Sets a hidden value of a form.
    Value {
        /// The hidden value's id.
        id: String,
        /// The value.
        value: String,
    },
    /// Shows a toast.
    Toast {
        /// What it means.
        #[serde(default, skip_serializing_if = "is_default")]
        tone: Tone,
        /// What it says.
        text: String,
    },
    /// A command from a newer server, which this build does not know. A client
    /// skips it and runs the rest.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_and_commands_are_named_objects() {
        let action = Action::Commands {
            list: vec![
                Command::Open {
                    id: "refund".to_owned(),
                },
                Command::Set {
                    id: "menu".to_owned(),
                    state: State::Open,
                    on: true,
                },
                Command::Toast {
                    tone: Tone::Success,
                    text: "Copied".to_owned(),
                },
            ],
        };
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"t":"commands","list":[{"c":"open","id":"refund"},"#,
                r#"{"c":"set","id":"menu","state":"open","on":true},"#,
                r#"{"c":"toast","tone":"success","text":"Copied"}]}"#
            )
        );
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), action);
    }

    #[test]
    fn the_usual_presentation_and_an_enclosing_form_are_left_out() {
        assert_eq!(
            serde_json::to_string(&Action::navigate("/orders/7")).unwrap(),
            r#"{"t":"navigate","href":"/orders/7"}"#
        );
        assert_eq!(
            serde_json::to_string(&Action::submit()).unwrap(),
            r#"{"t":"submit"}"#
        );
        assert_eq!(
            serde_json::to_string(&Action::Back).unwrap(),
            r#"{"t":"back"}"#
        );
    }

    #[test]
    fn an_action_or_a_command_from_a_newer_server_is_read_as_unknown() {
        assert_eq!(
            serde_json::from_str::<Action>(r#"{"t":"teleport","to":"/orders"}"#).unwrap(),
            Action::Unknown
        );
        let action: Action = serde_json::from_str(
            r#"{"t":"commands","list":[{"c":"sparkle","id":"x"},{"c":"open","id":"refund"}]}"#,
        )
        .unwrap();
        assert_eq!(
            action,
            Action::Commands {
                list: vec![
                    Command::Unknown,
                    Command::Open {
                        id: "refund".to_owned()
                    }
                ]
            }
        );
    }
}

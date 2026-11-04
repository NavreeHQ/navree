//! The words an upload says, in one struct, so an app can translate every one.

use serde_json::json;

/// Every word an upload says: on the page, in the browser module and in the server's
/// refusals. The defaults are English. A word with `{name}`, `{max}`, `{types}` or
/// `{count}` in it gets that value; a word without one is plain text.
///
/// ```
/// use navree_ui::UploadLabels;
///
/// let labels = UploadLabels {
///     send: "भेजें".to_owned(),
///     ..UploadLabels::default()
/// };
/// assert_eq!(labels.send, "भेजें");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadLabels {
    /// The prompt in the drop area when several files can be sent.
    pub choose: String,
    /// The prompt when one file can be sent.
    pub choose_one: String,
    /// What follows the prompt, for several files.
    pub drop: String,
    /// What follows the prompt, for one file.
    pub drop_one: String,
    /// The Send button of a form that is sent by hand.
    pub send: String,
    /// The button that takes a file out of the list.
    pub remove: String,
    /// The button that stops a file on its way.
    pub cancel: String,
    /// The button that sends a file again after it failed.
    pub retry: String,
    /// The name of the list of files, for assistive technology.
    pub files: String,
    /// A file on its way: `{name}`.
    pub sending: String,
    /// A file that arrived.
    pub done: String,
    /// A file that could not be sent: `{name}`.
    pub failed: String,
    /// A file that could not be sent because the connection failed: `{name}`.
    pub offline: String,
    /// A file over the size limit: `{name}` and `{max}`.
    pub too_big: String,
    /// A file of a kind that is not accepted: `{name}` and `{types}`.
    pub wrong_type: String,
    /// A file with nothing in it: `{name}`.
    pub empty: String,
    /// More files than are allowed: `{count}`.
    pub too_many: String,
    /// A file taken out of the list: `{name}`.
    pub removed: String,
    /// A chunked upload that is no longer there.
    pub expired: String,
    /// One file arrived.
    pub sent_one: String,
    /// Several files arrived: `{count}`.
    pub sent_many: String,
    /// A form sent with no file in it.
    pub nothing: String,
    /// The size limit in the hint: `{size}`.
    pub up_to: String,
    /// The count limit in the hint: `{count}`.
    pub up_to_files: String,
    /// The word before the last kind of file in the hint (`PNG or JPG`).
    pub or: String,
    /// All images, in the hint.
    pub images: String,
    /// All videos, in the hint.
    pub videos: String,
    /// All audio, in the hint.
    pub audio: String,
    /// Text files, in the hint.
    pub text: String,
}

impl Default for UploadLabels {
    fn default() -> Self {
        let text = |words: &str| words.to_owned();
        Self {
            choose: text("Choose files"),
            choose_one: text("Choose a file"),
            drop: text("or drop them here"),
            drop_one: text("or drop it here"),
            send: text("Send"),
            remove: text("Remove"),
            cancel: text("Cancel"),
            retry: text("Try again"),
            files: text("Files"),
            sending: text("Sending {name}"),
            done: text("Sent"),
            failed: text("{name} could not be sent."),
            offline: text("{name} could not be sent. Check your connection and try again."),
            too_big: text("{name} is larger than {max}."),
            wrong_type: text("{name} is not an accepted kind of file. Accepted: {types}."),
            empty: text("{name} is empty."),
            too_many: text("You can send up to {count} files."),
            removed: text("{name} was removed"),
            expired: text("This upload has expired. Choose the file again."),
            sent_one: text("1 file sent."),
            sent_many: text("{count} files sent."),
            nothing: text("Choose a file to send."),
            up_to: text("up to {size}"),
            up_to_files: text("up to {count} files"),
            or: text("or"),
            images: text("images"),
            videos: text("videos"),
            audio: text("audio"),
            text: text("text"),
        }
    }
}

impl UploadLabels {
    /// The words the browser module speaks, as the JSON the markup carries.
    pub(crate) fn module_words(&self) -> String {
        json!({
            "sending": self.sending,
            "done": self.done,
            "failed": self.failed,
            "offline": self.offline,
            "too_big": self.too_big,
            "too_many": self.too_many,
            "empty": self.empty,
            "removed": self.removed,
        })
        .to_string()
    }
}

/// `template` with each `{key}` replaced by its value in `pairs`. A `{key}` that has no
/// value stays as it is, and a value is never read for keys again.
pub(crate) fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match pairs.iter().find(|(name, _)| *name == key) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_put_in_once_and_never_read_again() {
        assert_eq!(
            fill("{name} is larger than {max}.", &[("name", "{max}"), ("max", "1 MB")]),
            "{max} is larger than 1 MB."
        );
        assert_eq!(fill("no keys", &[("a", "b")]), "no keys");
        assert_eq!(fill("{a} {b}", &[("a", "x")]), "x {b}");
        assert_eq!(fill("open {brace", &[]), "open {brace");
        assert_eq!(fill("दो {a} शब्द", &[("a", "x")]), "दो x शब्द");
    }

    #[test]
    fn the_module_words_are_json() {
        let words: serde_json::Value =
            serde_json::from_str(&UploadLabels::default().module_words()).unwrap();
        assert_eq!(words["done"], "Sent");
    }
}

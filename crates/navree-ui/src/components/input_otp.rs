//! InputOtp: a field for a one-time code.
//!
//! It is one real text input, not a row of small ones. A phone can fill it in from a
//! text message, a paste lands whole, a screen reader reads one field, and it posts as
//! one value with no script. The boxes are drawn round it by the stylesheet: the
//! font is monospace, so a character is exactly one `ch` wide, the letter spacing
//! gives every character the width of one box, and the lines between the boxes are a
//! background that repeats at that width. The `otp` module keeps what is typed or
//! pasted to the characters a code may hold and tells the page when the code is whole.
//!
//! The server stays the authority: [`OtpRules::check`] reads a posted code under the
//! same rules the field draws.

use std::error::Error;
use std::fmt;

use crate::node::{AttrValue, Element, Node, Uses, el, into_node};

/// The longest text read as a code. A longer post is not a code and is not walked.
const LONGEST_POST: usize = 64;

/// What the characters of a code are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum OtpKind {
    /// The digits 0 to 9: the default.
    #[default]
    Digits,
    /// The digits and the letters A to Z. A small letter is read as its capital.
    Alphanumeric,
}

impl OtpKind {
    const fn id(self) -> &'static str {
        match self {
            Self::Digits => "digits",
            Self::Alphanumeric => "alnum",
        }
    }
}

/// Why a posted code is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OtpError {
    /// Nothing was posted, or only spaces and dashes.
    Empty,
    /// The code has too few or too many characters.
    WrongLength,
    /// The code has a character that a code of this kind never has.
    BadCharacter,
}

impl OtpError {
    /// The refusal in words for a person.
    pub const fn message(self) -> &'static str {
        match self {
            Self::Empty => "Enter the code.",
            Self::WrongLength => "The code is not the right length.",
            Self::BadCharacter => "The code has a character it cannot have.",
        }
    }
}

impl fmt::Display for OtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl Error for OtpError {}

/// How long a code is and what it is made of: what the field draws and what the server
/// checks a posted code against.
///
/// ```
/// use navree_ui::{OtpKind, OtpRules};
///
/// let rules = OtpRules::new(6, OtpKind::Digits);
/// assert_eq!(rules.check(" 482 915 ").as_deref(), Ok("482915"));
/// assert!(rules.check("48291").is_err());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OtpRules {
    length: u8,
    kind: OtpKind,
}

impl OtpRules {
    /// The fewest characters a code field draws.
    pub const MIN_LENGTH: u8 = 3;
    /// The most characters a code field draws.
    pub const MAX_LENGTH: u8 = 12;

    /// Rules for a code of `length` characters of `kind`. A length below 3 or above 12
    /// is held to it, in the field and in the check alike.
    pub fn new(length: u8, kind: OtpKind) -> Self {
        Self {
            length: length.clamp(Self::MIN_LENGTH, Self::MAX_LENGTH),
            kind,
        }
    }

    /// The number of characters.
    pub const fn length(self) -> u8 {
        self.length
    }

    /// What the characters are.
    pub const fn kind(self) -> OtpKind {
        self.kind
    }

    /// Reads a posted code: ASCII spaces and dashes inside it are dropped, and so is
    /// any white space around it. What is left must be exactly the right length of
    /// ASCII digits (or ASCII letters and digits, as capitals, for
    /// [`OtpKind::Alphanumeric`]).
    ///
    /// Nothing else is taken for a digit or a letter: not Arabic-Indic or fullwidth
    /// digits, not zero-width characters, not other kinds of space. Comparing the code
    /// with the one that was sent is the app's job; do it in constant time.
    ///
    /// # Errors
    ///
    /// [`OtpError::Empty`] for nothing, [`OtpError::WrongLength`] for too few or too
    /// many characters, and [`OtpError::BadCharacter`] for any other character.
    pub fn check(self, posted: &str) -> Result<String, OtpError> {
        if posted.len() > LONGEST_POST {
            return Err(OtpError::WrongLength);
        }
        let mut code = String::with_capacity(usize::from(self.length));
        for ch in posted
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .chars()
        {
            match ch {
                ' ' | '-' => {}
                '0'..='9' => code.push(ch),
                'a'..='z' | 'A'..='Z' if self.kind == OtpKind::Alphanumeric => {
                    code.push(ch.to_ascii_uppercase());
                }
                _ => return Err(OtpError::BadCharacter),
            }
        }
        if code.is_empty() {
            Err(OtpError::Empty)
        } else if code.len() == usize::from(self.length) {
            Ok(code)
        } else {
            Err(OtpError::WrongLength)
        }
    }

    /// What of `text` a field of these rules could hold: its allowed characters, as
    /// capitals, cut to the length.
    fn keep(self, text: &str) -> String {
        text.chars()
            .filter_map(|ch| match ch {
                '0'..='9' => Some(ch),
                'a'..='z' | 'A'..='Z' if self.kind == OtpKind::Alphanumeric => {
                    Some(ch.to_ascii_uppercase())
                }
                _ => None,
            })
            .take(usize::from(self.length))
            .collect()
    }
}

/// A field for a one-time code, such as the six digits a text message carries.
///
/// ```
/// use navree_ui::{InputOtp, render};
///
/// let html = render(&InputOtp::new("code", 6).group(3).into());
/// assert!(html.contains(r#"autocomplete="one-time-code""#));
/// assert!(html.contains(r#"maxlength="6""#));
/// assert!(html.contains(r#"data-group="3""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct InputOtp {
    el: Element,
    wrap: Element,
    rules: OtpRules,
    group: Option<u8>,
    submit: bool,
    value: Option<String>,
}

impl InputOtp {
    /// A field of `length` digits posting as `name`. A length below 3 or above 12 is
    /// held to it.
    pub fn new(name: impl Into<String>, length: u8) -> Self {
        Self {
            el: el("input")
                .slot("input-otp-field")
                .attr("type", "text")
                .attr("name", name.into()),
            wrap: el("span").slot("input-otp"),
            rules: OtpRules::new(length, OtpKind::Digits),
            group: None,
            submit: false,
            value: None,
        }
    }

    /// Sets what the characters are (digits by default).
    pub fn kind(mut self, kind: OtpKind) -> Self {
        self.rules = OtpRules::new(self.rules.length(), kind);
        self
    }

    /// Draws a heavier line after every `size` boxes, 2 to 4 of them, as in 482-915.
    /// A size that does not divide the code into groups is ignored.
    pub fn group(mut self, size: u8) -> Self {
        self.group = Some(size);
        self
    }

    /// Sets the code the field starts with. Only the characters the code can have are
    /// kept, and only as many as it has room for.
    pub fn value(mut self, value: impl AsRef<str>) -> Self {
        self.value = Some(value.as_ref().to_owned());
        self
    }

    /// Names the field for assistive technology when no visible label does.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.el = self.el.attr("aria-label", label.into());
        self
    }

    /// Requires a code.
    pub fn required(mut self) -> Self {
        self.el = self.el.flag("required", true);
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.el = self.el.flag("disabled", disabled);
        self
    }

    /// Shows the code and keeps it still.
    pub fn readonly(mut self) -> Self {
        self.el = self.el.flag("readonly", true);
        self
    }

    /// Focuses it when the page loads.
    pub fn autofocus(mut self) -> Self {
        self.el = self.el.flag("autofocus", true);
        self
    }

    /// Marks it invalid.
    pub fn invalid(mut self, invalid: bool) -> Self {
        if invalid {
            self.el = self.el.attr("aria-invalid", "true");
        }
        self
    }

    /// Sends the form as soon as the code is whole and the form is valid. The form
    /// still works with a button, and with no script.
    pub fn submit_when_complete(mut self) -> Self {
        self.submit = true;
        self
    }

    /// The rules this field draws, for the server to check a posted code against.
    pub fn rules(&self) -> OtpRules {
        self.rules
    }

    /// Sets the input's `id`.
    pub fn id(mut self, id: impl Into<AttrValue>) -> Self {
        self.el = self.el.id(id);
        self
    }

    /// Sets an attribute on the input.
    pub fn attr(mut self, name: &'static str, value: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr(name, value);
        self
    }

    /// Takes `columns` columns of a [`Grid`](crate::Grid).
    pub fn span(mut self, columns: u8) -> Self {
        self.wrap = self
            .wrap
            .attr("data-span", super::layout::span_name(columns));
        self
    }

    fn into_node(self) -> Node {
        let rules = self.rules;
        let length = i64::from(rules.length());
        let digits = rules.kind() == OtpKind::Digits;
        let pattern = if digits {
            format!("[0-9]{{{}}}", rules.length())
        } else {
            format!("[0-9A-Za-z]{{{}}}", rules.length())
        };
        let value = self
            .value
            .as_deref()
            .map(|text| rules.keep(text))
            .filter(|text| !text.is_empty());
        let input = self
            .el
            .attr("autocomplete", "one-time-code")
            .attr("inputmode", if digits { "numeric" } else { "text" })
            .attr("maxlength", length)
            .attr("minlength", length)
            .attr("pattern", pattern)
            .attr("spellcheck", "false")
            .attr("autocapitalize", if digits { "off" } else { "characters" })
            .attr("autocorrect", "off")
            .attr("enterkeyhint", "done")
            .attr("data-ui-otp", rules.kind().id())
            .flag("data-ui-submit", self.submit)
            .attr_opt("value", value);
        let group = self
            .group
            .filter(|size| (2..=4).contains(size) && *size < rules.length());
        self.wrap
            .attr("data-length", length)
            .attr_opt("data-group", group.map(i64::from))
            .uses(Uses::OTP)
            .child(input)
            .into()
    }
}

into_node!(InputOtp);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    #[test]
    fn the_field_is_a_wrapper_round_one_input() {
        let out = render(&InputOtp::new("code", 6).into());
        assert!(
            out.starts_with(r#"<span data-slot="input-otp" data-length="6"><input"#),
            "{out}"
        );
        assert_eq!(out.matches("<input").count(), 1, "{out}");
    }

    #[test]
    fn the_rules_read_a_code_the_way_the_field_draws_it() {
        let rules = OtpRules::new(6, OtpKind::Digits);
        assert_eq!(rules.check("482-915").as_deref(), Ok("482915"));
        assert_eq!(rules.check(""), Err(OtpError::Empty));
        assert_eq!(rules.check("48291"), Err(OtpError::WrongLength));
        assert_eq!(rules.check("48291x"), Err(OtpError::BadCharacter));
        assert_eq!(rules.keep("48 29-15 77"), "482915");
    }
}

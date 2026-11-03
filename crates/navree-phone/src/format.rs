//! Writing a number: E.164, international, national and RFC 3966, by the
//! format of its country calling code's main territory whose leading digits
//! and pattern fit the national significant number.

use crate::meta::{self, Format};
use crate::normalize::is_punctuation;
use crate::parse::expand;

/// How a number is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PhoneFormat {
    /// `+447400123456`: the form to store, compare and send to providers.
    E164,
    /// `+44 7400 123456`: for people anywhere.
    International,
    /// `07400 123456`: for people in the number's country.
    National,
    /// `tel:+44-7400-123456`: for `tel:` links.
    Rfc3966,
}

/// Writes the number of calling code `code`, national significant number
/// `nsn` and extension `extension` (empty for none).
pub(crate) fn format(code: u16, nsn: &str, extension: &str, how: PhoneFormat) -> String {
    let mut out = String::with_capacity(32);
    if how == PhoneFormat::E164 {
        out.push('+');
        out.push_str(&code.to_string());
        out.push_str(nsn);
        return out;
    }
    let Some(main) = meta::main_for(code) else {
        out.push_str(nsn);
        return out;
    };
    let formats = if how == PhoneFormat::National {
        main.formats()
    } else {
        main.intl_formats()
    };
    let body = match choose(formats, nsn.as_bytes()) {
        Some(format) => apply(nsn, format, how),
        None => nsn.to_owned(),
    };
    match how {
        PhoneFormat::International => {
            out.push('+');
            out.push_str(&code.to_string());
            out.push(' ');
        }
        PhoneFormat::Rfc3966 => {
            out.push_str("tel:+");
            out.push_str(&code.to_string());
            out.push('-');
        }
        PhoneFormat::National | PhoneFormat::E164 => {}
    }
    out.push_str(&body);
    if !extension.is_empty() {
        if how == PhoneFormat::Rfc3966 {
            out.push_str(";ext=");
        } else {
            // A country's own prefix (` x` in the United Kingdom) is a label
            // that reads at most 9 digits back; a longer extension takes the
            // explicit label, which reads 20.
            match main.extension_prefix.get() {
                prefix if !prefix.is_empty() && extension.len() <= 9 => out.push_str(prefix),
                _ => out.push_str(" ext. "),
            }
        }
        out.push_str(extension);
    }
    out
}

/// The first format whose leading digits start `nsn` and whose pattern
/// matches all of it.
pub(crate) fn choose(formats: &'static [Format], nsn: &[u8]) -> Option<&'static Format> {
    formats.iter().find(|format| {
        (!format.leading.exists() || format.leading.looking_at(nsn).is_some())
            && format.pattern.matches(nsn)
    })
}

/// `nsn` written by `format`.
fn apply(nsn: &str, format: &Format, how: PhoneFormat) -> String {
    let template = match format.national.get() {
        national if how == PhoneFormat::National && !national.is_empty() => national,
        _ => format.format.get(),
    };
    let written = replace_all(format, nsn.as_bytes(), template);
    if how != PhoneFormat::Rfc3966 {
        return written;
    }
    // Groups joined by single dashes, with no punctuation in front.
    let mut out = String::with_capacity(written.len());
    let mut separator = false;
    for c in written.chars() {
        if is_punctuation(c) {
            separator = !out.is_empty();
        } else {
            if separator {
                out.push('-');
                separator = false;
            }
            out.push(c);
        }
    }
    out
}

/// Every match of the format's pattern in `input` replaced by `template`,
/// as a regular expression engine's `replace_all` does.
fn replace_all(format: &Format, input: &[u8], template: &str) -> String {
    let mut out = String::with_capacity(template.len() + input.len());
    let mut pos = 0;
    while pos < input.len() {
        let found = (pos..input.len()).find_map(|start| {
            let rest = input.get(start..)?;
            let captures = format.pattern.captures(rest, false)?;
            (captures.end() > 0).then_some((start, captures))
        });
        let Some((start, captures)) = found else {
            break;
        };
        out.extend(
            input
                .get(pos..start)
                .unwrap_or(&[])
                .iter()
                .map(|b| char::from(*b)),
        );
        out.push_str(&expand(
            template,
            &captures,
            input.get(start..).unwrap_or(&[]),
        ));
        pos = start + captures.end();
    }
    out.extend(
        input
            .get(pos..)
            .unwrap_or(&[])
            .iter()
            .map(|b| char::from(*b)),
    );
    out
}

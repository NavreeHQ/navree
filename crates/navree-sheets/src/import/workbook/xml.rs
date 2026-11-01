//! What the XML readers of XLSX and ODS share: a reader over a part,
//! attributes as they are written, entities and Excel's `_xHHHH_` escapes.

use std::borrow::Cow;
use std::io::BufRead;

use quick_xml::events::{BytesRef, BytesStart};

use super::{Damaged, damaged};

/// A reader of a part's XML that turns `<a/>` into a start and an end, keeps
/// text as it is and does not check that end tags match their start.
pub(super) fn reader<R: BufRead>(source: R) -> quick_xml::Reader<R> {
    let mut reader = quick_xml::Reader::from_reader(source);
    let config = reader.config_mut();
    config.check_end_names = false;
    config.trim_text(false);
    config.check_comments = false;
    config.expand_empty_elements = true;
    reader
}

/// The attributes of `element` as they are written: keys with their prefix,
/// values with their entities.
pub(super) fn attributes<'a>(element: &'a BytesStart<'_>) -> Attributes<'a> {
    Attributes {
        raw: element.attributes_raw(),
        at: 0,
    }
}

/// The value of the attribute `key` of `element`, as it is written; the
/// first when it is given twice.
pub(super) fn attribute<'a>(
    element: &'a BytesStart<'_>,
    key: &str,
) -> Result<Option<&'a str>, Damaged> {
    for attribute in attributes(element) {
        let (name, value) = attribute?;
        if name == key {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// The values of the attributes `keys` of `element`, as they are written.
/// The attributes after the last one found are not read.
pub(super) fn attributes_of<'a, const N: usize>(
    element: &'a BytesStart<'_>,
    keys: [&str; N],
) -> Result<[Option<&'a str>; N], Damaged> {
    let mut values = [None; N];
    let mut found = 0;
    for attribute in attributes(element) {
        let (key, value) = attribute?;
        if let Some(slot) = keys
            .iter()
            .position(|wanted| *wanted == key)
            .and_then(|at| values.get_mut(at))
        {
            *slot = Some(value);
            found += 1;
            if found == N {
                break;
            }
        }
    }
    Ok(values)
}

/// Attributes as they are written. A key without `=` or a value without
/// quotes makes the element unreadable.
pub(super) struct Attributes<'a> {
    raw: &'a str,
    at: usize,
}

impl<'a> Iterator for Attributes<'a> {
    type Item = Result<(&'a str, &'a str), Damaged>;

    fn next(&mut self) -> Option<Self::Item> {
        let raw = self.raw;
        let rest = raw.get(self.at..)?.trim_start();
        if rest.is_empty() {
            self.at = raw.len();
            return None;
        }
        // Whatever comes next ends the attributes, read or not.
        self.at = raw.len();
        let Some((key, after)) = rest.split_once('=') else {
            return Some(Err(damaged("an attribute has no value")));
        };
        let after = after.trim_start();
        let Some(quote) = after.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            return Some(Err(damaged("an attribute value is not quoted")));
        };
        let value = &after[1..];
        let (value, rest) = value.split_once(quote).unwrap_or((value, ""));
        self.at = raw.len() - rest.len();
        Some(Ok((key.trim_end(), value)))
    }
}

/// An attribute value with its entities (`&amp;`, `&#233;`) turned into text.
pub(super) fn decode(value: &str) -> Result<Cow<'_, str>, Damaged> {
    quick_xml::escape::unescape(value)
        .map_err(|error| damaged(format_args!("an attribute value: {error}")))
}

/// Adds the text of an entity reference in text (`&lt;`, `&#xA0;`) to `out`.
pub(super) fn push_entity(entity: &BytesRef<'_>, out: &mut String) -> Result<(), Damaged> {
    if let Some(text) = quick_xml::escape::resolve_xml_entity(entity) {
        out.push_str(text);
        return Ok(());
    }
    match entity.resolve_char_ref() {
        Ok(Some(character)) => {
            out.push(character);
            Ok(())
        }
        Ok(None) => Err(damaged(format_args!("the unknown entity &{};", &**entity))),
        Err(error) => Err(damaged(error)),
    }
}

/// Text with Excel's escapes of characters XML cannot hold (`_x000D_` for a
/// carriage return) turned back into the characters. Only escapes of the
/// form `_x00HH_` are read.
pub(super) fn unescape_excel(text: &str) -> Cow<'_, str> {
    if !text.contains("_x00") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("_x00") {
        let (before, escape) = rest.split_at(at);
        out.push_str(before);
        let byte = escape
            .get(4..7)
            .filter(|tail| tail.ends_with('_'))
            .and_then(|tail| u8::from_str_radix(&tail[..2], 16).ok());
        match byte {
            Some(byte) => {
                out.push(char::from(byte));
                rest = &escape[7..];
            }
            None => {
                out.push('_');
                rest = &escape[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(raw: &str) -> Vec<Result<(&str, &str), String>> {
        Attributes { raw, at: 0 }
            .map(|attribute| attribute.map_err(|error| error.0))
            .collect()
    }

    #[test]
    fn attributes_are_read_as_written() {
        assert_eq!(
            all(r#" r="A1"  s = '3' t="s""#),
            [Ok(("r", "A1")), Ok(("s", "3")), Ok(("t", "s"))]
        );
        assert_eq!(all(r#" name="a &amp; b""#), [Ok(("name", "a &amp; b"))]);
        assert_eq!(all("  "), []);
        assert!(all(" r").first().is_some_and(Result::is_err));
        assert!(all(" r=A1").first().is_some_and(Result::is_err));
        // A value that is not closed runs to the end.
        assert_eq!(all(r#" r="A1"#), [Ok(("r", "A1"))]);
        // An error ends the attributes.
        assert_eq!(all(r#" r s="1""#).len(), 1);
    }

    #[test]
    fn excel_escapes_are_read() {
        assert_eq!(unescape_excel("a_x000D_b"), "a\rb");
        // An escaped `_` keeps the text after it as it is.
        assert_eq!(unescape_excel("_x005F_x000D_"), "_x000D_");
        assert_eq!(unescape_excel("_x00zz_ and _x00"), "_x00zz_ and _x00");
        assert_eq!(unescape_excel("_x00E9_"), "é");
        assert_eq!(unescape_excel("plain"), "plain");
    }
}

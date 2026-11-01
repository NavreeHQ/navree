//! `$ref` pointers: reading them, writing them, and the type a reference to
//! a part of the document becomes in the generated file.

use super::json::Value;
use super::ts::{Ty, named};

/// A pointer's segments, unescaped and percent-decoded, with empty ones
/// dropped.
pub(crate) fn parse_pointer(pointer: &str) -> Vec<String> {
    pointer
        .split('/')
        .map(unescape)
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// The pointer of a reference: what follows the first `#/`.
pub(crate) fn parse_ref(reference: &str) -> Vec<String> {
    let mut parts = reference.split("#/");
    let _file = parts.next();
    parse_pointer(parts.next().unwrap_or(""))
}

fn unescape(segment: &str) -> String {
    let text = segment.replace("~1", "/").replace("~0", "~");
    percent_decode(&text).unwrap_or(text)
}

/// `decodeURIComponent`: `None` where the text is not valid percent-encoded
/// UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_owned());
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let hex = text.get(at + 1..at + 3)?;
            out.push(
                u8::from_str_radix(hex, 16)
                    .ok()
                    .filter(|_| hex.bytes().all(|b| b.is_ascii_hexdigit()))?,
            );
            at += 3;
        } else {
            out.push(byte);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A segment as a pointer writes it.
pub(crate) fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// A pointer built from parts: a part that is itself a reference
/// contributes its segments, any other part is one segment.
pub(crate) fn create_ref(parts: &[&str]) -> String {
    let mut pointer = String::from("#");
    for part in parts {
        if part.is_empty() {
            continue;
        }
        let segments = parse_ref(part);
        if segments.is_empty() {
            pointer.push('/');
            pointer.push_str(&escape_pointer(part));
        } else {
            for segment in segments {
                pointer.push('/');
                pointer.push_str(&escape_pointer(&segment));
            }
        }
    }
    pointer
}

/// The type of what `reference` points at: `components["schemas"]["Pet"]`.
/// For a reference to a parameter (`resolved`), the last part is the
/// parameter's `in` and `name`.
pub(crate) fn oapi_ref(reference: &str, resolved: Option<&Value>) -> Result<Ty, String> {
    let pointer = parse_ref(reference);
    let Some((first, rest)) = pointer.split_first() else {
        return Err(format!(
            "Error parsing $ref: {reference}. Is this a valid $ref?"
        ));
    };
    let parameter = resolved.and_then(|resolved| {
        let in_ = resolved.get("in").filter(|v| v.truthy())?;
        if resolved.has("$ref") && resolved.get("$ref").is_some_and(Value::truthy) {
            return None;
        }
        Some((in_.js_string(), resolved.get("name").map(Value::js_string)?))
    });
    let mut ty = named(first);
    let (lead, tail) = rest.split_at(rest.len().min(2));
    for segment in lead {
        ty = Ty::Indexed(Box::new(ty), segment.clone());
    }
    for (index, segment) in tail.iter().enumerate() {
        if segment == "properties" {
            continue;
        }
        if let (Some((in_, name)), true) = (&parameter, index + 1 == tail.len()) {
            ty = Ty::Indexed(Box::new(ty), in_.clone());
            ty = Ty::Indexed(Box::new(ty), name.clone());
            continue;
        }
        ty = Ty::Indexed(Box::new(ty), segment.clone());
    }
    Ok(ty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pointers_unescaped_and_decoded() {
        assert_eq!(
            parse_ref("#/components/schemas/Foo"),
            ["components", "schemas", "Foo"]
        );
        assert_eq!(
            parse_ref("#/paths/~1pets~1{id}/get"),
            ["paths", "/pets/{id}", "get"]
        );
        assert_eq!(parse_ref("#/a/b%20c/~0x"), ["a", "b c", "~x"]);
        assert_eq!(parse_ref("#/a//b"), ["a", "b"]);
        assert_eq!(parse_ref("application/json"), Vec::<String>::new());
        // Text that is not valid percent-encoding stays as written.
        assert_eq!(parse_ref("#/a/100%"), ["a", "100%"]);
        assert_eq!(parse_ref("#/a/%FF"), ["a", "%FF"]);
    }

    #[test]
    fn builds_pointers_from_parts() {
        assert_eq!(
            create_ref(&["components", "schemas", "A/B"]),
            "#/components/schemas/A~1B"
        );
        assert_eq!(
            create_ref(&["#/paths/~1x/get", "responses", "200"]),
            "#/paths/~1x/get/responses/200"
        );
        assert_eq!(create_ref(&["", "x", ""]), "#/x");
    }

    #[test]
    fn turns_a_reference_into_an_indexed_type() {
        assert_eq!(
            oapi_ref("#/components/schemas/Pet", None).unwrap(),
            Ty::Indexed(
                Box::new(Ty::Indexed(
                    Box::new(named("components")),
                    "schemas".to_owned()
                )),
                "Pet".to_owned()
            )
        );
        assert!(oapi_ref("#/", None).is_err());
        assert!(oapi_ref("nowhere", None).is_err());
    }
}

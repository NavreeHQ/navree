//! Icons and flags for a native client: SVG shapes as path data, and the name
//! a system symbol can be found by.
//!
//! An icon is compile-time markup with no name in the page. Its name is found
//! again from the markup itself, in the toolkit's own table, so a client that
//! has a system symbol for `shopping-bag` can draw that and any other draws
//! the paths.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::OnceLock;

use navree_native::node as n;

/// The name of the toolkit's own icon whose shapes are `shapes`, if it is one.
pub(crate) fn name_of(shapes: &str) -> Option<&'static str> {
    static NAMES: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            crate::icons::ALL
                .iter()
                .map(|(name, icon)| (icon.shapes(), *name))
                .collect()
        })
        .get(shapes)
        .copied()
}

/// The code of the flag whose artwork is `shapes`, if it is one of the toolkit's.
pub(crate) fn flag_of(shapes: &str) -> Option<&'static str> {
    static CODES: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    CODES
        .get_or_init(|| {
            let mut codes = HashMap::new();
            for code in crate::flags::ALL {
                codes.insert(code.rect_shapes(), code.as_str());
                codes.insert(code.square_shapes(), code.as_str());
            }
            codes
        })
        .get(shapes)
        .copied()
}

/// The flag of a two-letter country code as the pair of regional indicators
/// every system draws as that flag. `None` for a code that is not two letters:
/// a region or a body.
pub(crate) fn flag_emoji(code: &str) -> Option<String> {
    let bytes = code.as_bytes();
    if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    bytes
        .iter()
        .map(|byte| char::from_u32(0x1F1E6 + u32::from(byte.to_ascii_uppercase() - b'A')))
        .collect()
}

/// An icon's shapes as a native icon: each shape as path data.
pub(crate) fn icon(shapes: &str, view_box: &str) -> Result<n::Icon, String> {
    let mut paths = Vec::new();
    let mut fills = Vec::new();
    let mut rest = shapes;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else {
            return Err("an unclosed tag".to_owned());
        };
        let tag = rest[..end].trim_end_matches('/').trim();
        rest = &rest[end + 1..];
        if tag.starts_with('/') {
            continue;
        }
        let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        let attrs = attributes(attrs)?;
        let get = |key: &str| {
            attrs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| *value)
        };
        let number = |key: &str| -> Result<f64, String> {
            match get(key) {
                Some(value) => value
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| format!("`{key}` of a {name} is not a number")),
                None => Ok(0.0),
            }
        };
        let path = match name {
            "path" => get("d")
                .ok_or_else(|| "a path with no data".to_owned())?
                .to_owned(),
            "circle" => round(number("cx")?, number("cy")?, number("r")?, number("r")?),
            "ellipse" => round(number("cx")?, number("cy")?, number("rx")?, number("ry")?),
            "line" => format!(
                "M{} {}L{} {}",
                num(number("x1")?),
                num(number("y1")?),
                num(number("x2")?),
                num(number("y2")?)
            ),
            "rect" => {
                let rx = match (get("rx"), get("ry")) {
                    (Some(_), _) => number("rx")?,
                    (None, Some(_)) => number("ry")?,
                    (None, None) => 0.0,
                };
                let ry = if get("ry").is_some() {
                    number("ry")?
                } else {
                    rx
                };
                rect(
                    number("x")?,
                    number("y")?,
                    number("width")?,
                    number("height")?,
                    rx,
                    ry,
                )
            }
            "polyline" | "polygon" => {
                let points = get("points").ok_or_else(|| format!("a {name} with no points"))?;
                let mut path = String::new();
                let mut values = points
                    .split(|ch: char| ch == ',' || ch.is_whitespace())
                    .filter(|part| !part.is_empty());
                let mut first = true;
                while let Some(x) = values.next() {
                    let y = values
                        .next()
                        .ok_or_else(|| format!("a {name} with half a point"))?;
                    let (x, y): (f64, f64) = (
                        x.parse()
                            .map_err(|_| format!("a point of a {name} is not a number"))?,
                        y.parse()
                            .map_err(|_| format!("a point of a {name} is not a number"))?,
                    );
                    let _ = write!(
                        path,
                        "{}{} {}",
                        if first { "M" } else { "L" },
                        num(x),
                        num(y)
                    );
                    first = false;
                }
                if name == "polygon" {
                    path.push('z');
                }
                path
            }
            other => return Err(format!("a <{other}> is not a shape this reads")),
        };
        // A shape says for itself whether it is filled; one that says nothing is stroked.
        if get("fill").is_some_and(|fill| fill != "none") {
            fills.push(path);
        } else {
            paths.push(path);
        }
    }
    if paths.is_empty() && fills.is_empty() {
        return Err("no shapes".to_owned());
    }
    Ok(n::Icon {
        name: name_of(shapes).map(str::to_owned),
        paths,
        fills,
        view_box: (view_box != "0 0 24 24").then(|| view_box.to_owned()),
        ..n::Icon::default()
    })
}

/// The `key="value"` pairs of a tag.
fn attributes(mut text: &str) -> Result<Vec<(&str, &str)>, String> {
    let mut out = Vec::new();
    loop {
        text = text.trim_start();
        if text.is_empty() {
            return Ok(out);
        }
        let Some((name, rest)) = text.split_once('=') else {
            return Err("an attribute with no value".to_owned());
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            return Err("an attribute whose value is not quoted".to_owned());
        };
        let Some((value, rest)) = rest.split_once('"') else {
            return Err("an attribute whose value does not end".to_owned());
        };
        out.push((name.trim(), value));
        text = rest;
    }
}

/// A number as path data writes it: no trailing zeros, no `-0`.
fn num(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        return "0".to_owned();
    }
    let mut text = format!("{rounded:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// A circle or an ellipse as two arcs.
fn round(cx: f64, cy: f64, rx: f64, ry: f64) -> String {
    format!(
        "M{} {}a{} {} 0 1 0 {} 0a{} {} 0 1 0 {} 0",
        num(cx - rx),
        num(cy),
        num(rx),
        num(ry),
        num(2.0 * rx),
        num(rx),
        num(ry),
        num(-2.0 * rx)
    )
}

/// A rectangle, its corners rounded when it has radii.
fn rect(x: f64, y: f64, width: f64, height: f64, rx: f64, ry: f64) -> String {
    let rx = rx.min(width / 2.0).max(0.0);
    let ry = ry.min(height / 2.0).max(0.0);
    if rx == 0.0 || ry == 0.0 {
        return format!(
            "M{} {}h{}v{}h{}z",
            num(x),
            num(y),
            num(width),
            num(height),
            num(-width)
        );
    }
    let (rxs, rys) = (num(rx), num(ry));
    format!(
        "M{} {}h{}a{rxs} {rys} 0 0 1 {rxs} {rys}v{}a{rxs} {rys} 0 0 1 -{rxs} {rys}h{}a{rxs} {rys} 0 0 1 -{rxs} -{rys}v{}a{rxs} {rys} 0 0 1 {rxs} -{rys}z",
        num(x + rx),
        num(y),
        num(width - 2.0 * rx),
        num(height - 2.0 * ry),
        num(-(width - 2.0 * rx)),
        num(-(height - 2.0 * ry)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons;

    #[test]
    fn every_icon_of_the_toolkit_becomes_paths_and_keeps_its_name() {
        for (name, glyph) in icons::ALL {
            let icon = icon(glyph.shapes(), glyph.view_box())
                .unwrap_or_else(|why| panic!("{name}: {why}"));
            assert!(!icon.paths.is_empty() || !icon.fills.is_empty(), "{name}");
            assert!(icon.name.is_some(), "{name}");
            assert!(icon.view_box.is_none(), "{name}");
            for path in icon.paths.iter().chain(&icon.fills) {
                assert!(
                    path.starts_with('M') || path.starts_with('m'),
                    "{name}: {path}"
                );
            }
        }
        // An icon Lucide renamed shares its shapes with the new name; either is its name.
        let bag = icon(icons::SHOPPING_BAG.shapes(), icons::SHOPPING_BAG.view_box()).unwrap();
        assert_eq!(bag.name.as_deref(), Some("shopping-bag"));
        assert!(bag.fills.is_empty());
        // An outline with solid dots in it keeps both kinds of shape.
        let both = icon(
            icons::BRAIN_CIRCUIT.shapes(),
            icons::BRAIN_CIRCUIT.view_box(),
        )
        .unwrap();
        assert!(!both.paths.is_empty() && !both.fills.is_empty());
    }

    #[test]
    fn shapes_other_than_paths_are_written_as_path_data() {
        let icon = icon(
            r#"<circle cx="12" cy="12" r="10"/><rect width="18" height="14" x="3" y="5" rx="2"/><line x1="2" x2="22" y1="10" y2="10"/><polyline points="4 6 8 10 12 6"/><polygon points="1,1 2,2 3,1"/><ellipse cx="12" cy="5" rx="9" ry="3"/><rect x="1" y="2" width="3" height="4"/>"#,
            "0 0 24 24",
        )
        .unwrap();
        assert_eq!(
            icon.paths,
            [
                "M2 12a10 10 0 1 0 20 0a10 10 0 1 0 -20 0",
                "M5 5h14a2 2 0 0 1 2 2v10a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-10a2 2 0 0 1 2 -2z",
                "M2 10L22 10",
                "M4 6L8 10L12 6",
                "M1 1L2 2L3 1z",
                "M3 5a9 3 0 1 0 18 0a9 3 0 1 0 -18 0",
                "M1 2h3v4h-3z",
            ]
        );
        assert_eq!(icon.name, None);
    }

    #[test]
    fn markup_that_is_not_shapes_is_refused() {
        assert!(icon("", "0 0 24 24").is_err());
        assert!(icon(r#"<text x="1">a</text>"#, "0 0 24 24").is_err());
        assert!(icon(r#"<g><path d="M0 0"/></g>"#, "0 0 24 24").is_err());
        assert!(icon(r#"<circle cx="a" cy="1" r="1"/>"#, "0 0 24 24").is_err());
        let filled = icon(
            r#"<path d="M0 0h4v4z" fill="currentColor"/><path d="M1 1" fill="none"/>"#,
            "0 0 16 16",
        )
        .unwrap();
        assert_eq!(filled.fills, ["M0 0h4v4z"]);
        assert_eq!(filled.paths, ["M1 1"]);
        assert_eq!(filled.view_box.as_deref(), Some("0 0 16 16"));
    }

    #[test]
    fn a_flag_is_found_by_its_artwork_and_drawn_as_its_emoji() {
        let india = crate::flags::CountryCode::parse("IN").unwrap();
        assert_eq!(flag_of(india.rect_shapes()), Some("IN"));
        assert_eq!(flag_of(india.square_shapes()), Some("IN"));
        assert_eq!(flag_of("<path/>"), None);
        assert_eq!(flag_emoji("IN").as_deref(), Some("🇮🇳"));
        assert_eq!(flag_emoji("ae").as_deref(), Some("🇦🇪"));
        assert_eq!(flag_emoji("GB-SCT"), None);
        assert_eq!(flag_emoji("1N"), None);
    }
}

//! The CSS the native engine reads: inline `style` declarations (lengths,
//! colors, font families) and `@page { size; margin }` rules.

use super::model::Color;
use crate::print::Paper;

/// The declarations of a `style` attribute, as (lower-case property,
/// value without `!important`).
pub(crate) fn declarations(style: &str) -> Vec<(String, String)> {
    style
        .split(';')
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            let property = property.trim().to_ascii_lowercase();
            let value = value.trim();
            let value = value
                .strip_suffix("!important")
                .map(str::trim_end)
                .unwrap_or(value);
            (!property.is_empty() && !value.is_empty()).then(|| (property, value.to_owned()))
        })
        .collect()
}

/// A length in points: `12pt`, `16px`, `5mm`, `1cm`, `0.5in`, `1.2em`
/// (of `font_size`), `50%` (of `percent_of`), a bare number as pixels.
pub(crate) fn length(value: &str, font_size: f32, percent_of: Option<f32>) -> Option<f32> {
    let (number, unit) = number_and_unit(value)?;
    let number = number as f32;
    Some(match unit.as_str() {
        "pt" => number,
        "px" | "" => number * 0.75,
        "mm" => number * 72.0 / 25.4,
        "cm" => number * 72.0 / 2.54,
        "in" => number * 72.0,
        "pc" => number * 12.0,
        "em" | "rem" => number * font_size,
        "%" => number / 100.0 * percent_of?,
        _ => return None,
    })
}

/// A length in millimeters (page sizes and margins); relative units are
/// not lengths here.
pub(crate) fn length_mm(value: &str) -> Option<f64> {
    let (number, unit) = number_and_unit(value)?;
    Some(match unit.as_str() {
        "mm" => number,
        "cm" => number * 10.0,
        "in" => number * 25.4,
        "pt" => number * 25.4 / 72.0,
        "pc" => number * 25.4 / 6.0,
        "px" | "" => number * 25.4 / 96.0,
        _ => return None,
    })
}

/// `12.5mm` as (12.5, "mm"), the unit in lower case.
fn number_and_unit(value: &str) -> Option<(f64, String)> {
    let value = value.trim().to_ascii_lowercase();
    let split = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(value.len());
    let number: f64 = value[..split].parse().ok()?;
    number
        .is_finite()
        .then(|| (number, value[split..].trim().to_owned()))
}

/// A `font-size`: a length, a percentage or `em` of `parent`, or a
/// keyword.
pub(crate) fn font_size(value: &str, parent: f32) -> Option<f32> {
    let keyword = match value.trim().to_ascii_lowercase().as_str() {
        "xx-small" => Some(7.0),
        "x-small" => Some(7.5),
        "small" => Some(10.0),
        "medium" => Some(12.0),
        "large" => Some(13.5),
        "x-large" => Some(18.0),
        "xx-large" => Some(24.0),
        "smaller" => Some(parent / 1.2),
        "larger" => Some(parent * 1.2),
        _ => None,
    };
    keyword
        .or_else(|| length(value, parent, Some(parent)))
        .filter(|size| *size > 0.0)
}

/// A color: `#rgb`, `#rrggbb`, `rgb(r, g, b)` (`rgba` ignores alpha) or a
/// named color.
pub(crate) fn color(value: &str) -> Option<Color> {
    let value = value.trim().to_ascii_lowercase();
    if value.starts_with('#') {
        let hex = value.trim_start_matches('#');
        // #rrggbbaa and #rgba: the alpha is dropped.
        let hex = match hex.len() {
            8 => &hex[..6],
            4 => &hex[..3],
            _ => hex,
        };
        return Color::hex(hex);
    }
    if let Some(inner) = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
    {
        let inner = inner.trim_end_matches(')');
        let parts: Vec<&str> = inner
            .split([',', ' ', '/'])
            .filter(|p| !p.trim().is_empty())
            .collect();
        if parts.len() < 3 {
            return None;
        }
        let channel = |part: &str| -> Option<u8> {
            let part = part.trim();
            let value = match part.strip_suffix('%') {
                Some(percent) => percent.parse::<f32>().ok()? * 2.55,
                None => part.parse::<f32>().ok()?,
            };
            Some(value.round().clamp(0.0, 255.0) as u8)
        };
        return Some(Color::rgb(
            channel(parts[0])?,
            channel(parts[1])?,
            channel(parts[2])?,
        ));
    }
    Some(match value.as_str() {
        "black" => Color::rgb(0, 0, 0),
        "white" => Color::rgb(255, 255, 255),
        "red" => Color::rgb(255, 0, 0),
        "green" => Color::rgb(0, 128, 0),
        "blue" => Color::rgb(0, 0, 255),
        "gray" | "grey" => Color::rgb(128, 128, 128),
        "darkgray" | "darkgrey" => Color::rgb(169, 169, 169),
        "lightgray" | "lightgrey" => Color::rgb(211, 211, 211),
        "silver" => Color::rgb(192, 192, 192),
        "dimgray" | "dimgrey" => Color::rgb(105, 105, 105),
        "maroon" => Color::rgb(128, 0, 0),
        "navy" => Color::rgb(0, 0, 128),
        "teal" => Color::rgb(0, 128, 128),
        "olive" => Color::rgb(128, 128, 0),
        "purple" => Color::rgb(128, 0, 128),
        "orange" => Color::rgb(255, 165, 0),
        "yellow" => Color::rgb(255, 255, 0),
        "lime" => Color::rgb(0, 255, 0),
        "aqua" | "cyan" => Color::rgb(0, 255, 255),
        "fuchsia" | "magenta" => Color::rgb(255, 0, 255),
        "darkred" => Color::rgb(139, 0, 0),
        "darkgreen" => Color::rgb(0, 100, 0),
        "darkblue" => Color::rgb(0, 0, 139),
        "crimson" => Color::rgb(220, 20, 60),
        "firebrick" => Color::rgb(178, 34, 34),
        "gold" => Color::rgb(255, 215, 0),
        "goldenrod" => Color::rgb(218, 165, 32),
        "royalblue" => Color::rgb(65, 105, 225),
        "steelblue" => Color::rgb(70, 130, 180),
        "slategray" | "slategrey" => Color::rgb(112, 128, 144),
        "whitesmoke" => Color::rgb(245, 245, 245),
        "gainsboro" => Color::rgb(220, 220, 220),
        "beige" => Color::rgb(245, 245, 220),
        "ivory" => Color::rgb(255, 255, 240),
        "seagreen" => Color::rgb(46, 139, 87),
        "forestgreen" => Color::rgb(34, 139, 34),
        "indigo" => Color::rgb(75, 0, 130),
        "brown" => Color::rgb(165, 42, 42),
        "pink" => Color::rgb(255, 192, 203),
        "tomato" => Color::rgb(255, 99, 71),
        "coral" => Color::rgb(255, 127, 80),
        _ => return None,
    })
}

/// The families of a `font-family` value, quotes removed.
pub(crate) fn families(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|family| {
            family
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .trim()
                .to_owned()
        })
        .filter(|family| !family.is_empty())
        .collect()
}

/// What `@page` rules set.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PageRule {
    pub(crate) paper: Option<Paper>,
    pub(crate) landscape: Option<bool>,
    /// Top, right, bottom, left, in millimeters.
    pub(crate) margins: [Option<f64>; 4],
}

impl PageRule {
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.paper.is_none() && self.landscape.is_none() && self.margins.iter().all(Option::is_none)
    }
}

/// The `@page` rules in a stylesheet (later declarations win).
pub(crate) fn page_rules(css: &str) -> PageRule {
    let mut rule = PageRule::default();
    let mut rest = css;
    while let Some(at) = rest.find("@page") {
        rest = &rest[at + 5..];
        let Some(open) = rest.find('{') else {
            break;
        };
        // `@page :first { }` and named pages: only the plain rule counts.
        let selector = rest[..open].trim();
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let body = &rest[open + 1..open + close];
        rest = &rest[open + close..];
        if !selector.is_empty() {
            continue;
        }
        for (property, value) in declarations(body) {
            match property.as_str() {
                "size" => size(&value, &mut rule),
                "margin" => {
                    let values: Vec<Option<f64>> =
                        value.split_whitespace().map(length_mm).collect();
                    let [top, right, bottom, left] = match values.as_slice() {
                        [all] => [*all; 4],
                        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
                        [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
                        [top, right, bottom, left, ..] => [*top, *right, *bottom, *left],
                        [] => [None; 4],
                    };
                    rule.margins = [top, right, bottom, left];
                }
                "margin-top" => rule.margins[0] = length_mm(&value),
                "margin-right" => rule.margins[1] = length_mm(&value),
                "margin-bottom" => rule.margins[2] = length_mm(&value),
                "margin-left" => rule.margins[3] = length_mm(&value),
                _ => {}
            }
        }
    }
    rule
}

/// `size: A4 landscape`, `size: letter`, `size: 210mm 148mm`.
fn size(value: &str, rule: &mut PageRule) {
    let mut lengths = Vec::new();
    for word in value.split_whitespace() {
        match word.to_ascii_lowercase().as_str() {
            "a3" => rule.paper = Some(Paper::A3),
            "a4" => rule.paper = Some(Paper::A4),
            "a5" => rule.paper = Some(Paper::A5),
            "letter" => rule.paper = Some(Paper::Letter),
            "legal" => rule.paper = Some(Paper::Legal),
            "landscape" => rule.landscape = Some(true),
            "portrait" => rule.landscape = Some(false),
            other => {
                if let Some(mm) = length_mm(other) {
                    lengths.push(mm);
                }
            }
        }
    }
    match lengths.as_slice() {
        [side] => {
            rule.paper = Some(Paper::Custom {
                width_mm: *side,
                height_mm: *side,
            });
        }
        [width, height, ..] => {
            rule.paper = Some(Paper::Custom {
                width_mm: *width,
                height_mm: *height,
            });
        }
        [] => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_split_and_drop_important() {
        assert_eq!(
            declarations("color: red; FONT-WEIGHT: bold !important;; bad; text-align:center"),
            vec![
                ("color".to_owned(), "red".to_owned()),
                ("font-weight".to_owned(), "bold".to_owned()),
                ("text-align".to_owned(), "center".to_owned()),
            ]
        );
    }

    #[test]
    fn lengths_in_points() {
        assert_eq!(length("12pt", 10.0, None), Some(12.0));
        assert_eq!(length("16px", 10.0, None), Some(12.0));
        assert_eq!(length("2em", 10.0, None), Some(20.0));
        assert_eq!(length("50%", 10.0, Some(300.0)), Some(150.0));
        assert_eq!(length("50%", 10.0, None), None);
        assert!((length("25.4mm", 10.0, None).unwrap() - 72.0).abs() < 0.001);
        assert_eq!(length("1in", 10.0, None), Some(72.0));
        assert_eq!(length("wide", 10.0, None), None);
        assert_eq!(font_size("small", 11.0), Some(10.0));
        assert_eq!(font_size("150%", 10.0), Some(15.0));
        assert_eq!(font_size("-3pt", 10.0), None);
    }

    #[test]
    fn colors_in_every_form() {
        assert_eq!(color("#1a5fb4"), Some(Color::rgb(0x1a, 0x5f, 0xb4)));
        assert_eq!(color("#FFF"), Some(Color::WHITE));
        assert_eq!(color("#11223380"), Some(Color::rgb(0x11, 0x22, 0x33)));
        assert_eq!(color("rgb(10, 20, 30)"), Some(Color::rgb(10, 20, 30)));
        assert_eq!(color("rgba(10 20 30 / 0.5)"), Some(Color::rgb(10, 20, 30)));
        assert_eq!(color("rgb(100%, 0%, 50%)"), Some(Color::rgb(255, 0, 128)));
        assert_eq!(color("Navy"), Some(Color::rgb(0, 0, 128)));
        assert_eq!(color("chartreuse-ish"), None);
    }

    #[test]
    fn font_families_lose_their_quotes() {
        assert_eq!(
            families("\"Noto Serif\", 'Noto Sans Arabic', serif"),
            vec!["Noto Serif", "Noto Sans Arabic", "serif"]
        );
    }

    #[test]
    fn page_rules_set_size_orientation_and_margins() {
        let rule = page_rules(
            "body { margin: 0 } @page { size: A5 landscape; margin: 15mm 10mm; } \
             @page :first { margin: 50mm }",
        );
        assert_eq!(rule.paper, Some(Paper::A5));
        assert_eq!(rule.landscape, Some(true));
        assert_eq!(
            rule.margins,
            [Some(15.0), Some(10.0), Some(15.0), Some(10.0)]
        );
        let custom = page_rules("@page{size:100mm 50mm;margin-top:1in}");
        assert_eq!(
            custom.paper,
            Some(Paper::Custom {
                width_mm: 100.0,
                height_mm: 50.0
            })
        );
        assert!((custom.margins[0].unwrap() - 25.4).abs() < 0.001);
        assert!(page_rules("p { color: red }").is_empty());
    }
}

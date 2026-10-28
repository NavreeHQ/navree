//! SVG output of a [`QrCode`].
//!
//! The markup is the one the two-factor setup screens already embed (they
//! put it in the page as it is): an XML prolog, an `<svg>` with `width`,
//! `height` and a matching `viewBox` in whole pixels and
//! `shape-rendering="crispEdges"`, a white background `<path>` and one
//! black `<path>` for all dark modules. Neighbouring dark modules in a row
//! are one rectangle, which keeps the document about a third of the size.

use std::fmt::Write as _;

use super::QrCode;

/// Quiet zone around the symbol, in modules (ISO/IEC 18004 §6.3.8).
const QUIET_ZONE: usize = 4;

pub(super) fn render(code: &QrCode, min_size: u32) -> String {
    let modules = code.size();
    let across = modules + 2 * QUIET_ZONE;
    // The smallest whole number of pixels per module that reaches `min_size`.
    let unit = usize::try_from(min_size)
        .unwrap_or(usize::MAX)
        .div_ceil(across)
        .max(1);
    let side = across * unit;
    let mut svg = String::with_capacity(512 + modules * modules * 2);
    // Writing to a String does not fail.
    let _ = write!(
        svg,
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1""#,
            r#" width="{side}" height="{side}" viewBox="0 0 {side} {side}""#,
            r#" shape-rendering="crispEdges">"#,
            r##"<path d="M0 0h{side}v{side}H0z" fill="#fff"/>"##,
            r##"<path fill="#000" d=""##
        ),
        side = side
    );
    for y in 0..modules {
        let mut x = 0;
        while x < modules {
            if !code.is_dark(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < modules && code.is_dark(x, y) {
                x += 1;
            }
            let width = (x - start) * unit;
            let _ = write!(
                svg,
                "M{} {}h{width}v{unit}h-{width}z",
                (start + QUIET_ZONE) * unit,
                (y + QUIET_ZONE) * unit
            );
        }
    }
    svg.push_str(r#""/></svg>"#);
    svg
}

//! The steps of the heat maps, measured.
//!
//! A heat map mixes one ink into the page's surface in steps, one set for the light palette
//! and one for the dark, written in `styles/extra/chart-grids.css`. Here those steps are read
//! out of the sheet and measured in every built-in theme, both palettes and both surfaces a
//! chart sits on, for every colour an ink can be:
//!
//! - each step is told from the one before it (zero, the faintest step, up to the deepest);
//! - the deepest step of the calendar reaches 3:1 on the surface, as any chart mark must;
//! - a value written in a matrix cell reads at 4.5:1 on every step it can sit on.

use navree_ui::theme::{Palette, Rgb, Side, Theme, Token};

/// The least distance between the calendar's steps, in OKLab times 100.
const CALENDAR_APART: f64 = 5.5;
/// The least contrast of a value in a cell: black or white by luminance reads at 4.58:1 at worst,
/// and the colour a browser mixes differs a little from the one measured here.
const READS: f64 = 4.5;
/// The least distance between the matrix's steps.
const MATRIX_APART: f64 = 3.5;

/// One mixing of the ink: its share in percent, and what it is mixed into.
#[derive(Clone, Copy)]
struct Mix(f64, Token);

/// The steps of a ramp as the sheet writes them: for `attr` (`level` or `m`), each step's
/// number with its mixing in the light palette and in the dark one.
fn steps(attr: &str) -> Vec<(u8, Mix, Mix)> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/extra/chart-grids.css");
    let css = std::fs::read_to_string(path).unwrap();
    let mut found = Vec::new();
    for line in css.lines() {
        let Some(rest) = line.strip_prefix(&format!("[data-{attr}=\"")) else {
            continue;
        };
        let number: u8 = rest[..1].parse().unwrap();
        let mixes: Vec<Mix> = line
            .split("color-mix(in oklab,var(--c) ")
            .skip(1)
            .map(|part| {
                let (share, into) = part.split_once("%,var(--").unwrap();
                let into = match into.split(')').next().unwrap() {
                    "surface" => Token::Ink0,
                    "fg" => Token::Fg,
                    other => panic!("{line}: mixed into {other}"),
                };
                Mix(share.parse().unwrap(), into)
            })
            .collect();
        match mixes.as_slice() {
            [] => found.push((number, Mix(100.0, Token::Ink0), Mix(100.0, Token::Ink0))),
            [light, dark] => found.push((number, *light, *dark)),
            other => panic!("{line}: {} mixes", other.len()),
        }
    }
    found.sort_by_key(|step| step.0);
    found
}

/// A step's colour: the ink mixed into the surface (or into the text ink), by palette.
fn colour(palette: &Palette, side: Side, surface: Token, ink: Token, step: &(u8, Mix, Mix)) -> Rgb {
    let Mix(share, into) = if side == Side::Light { step.1 } else { step.2 };
    let into = if into == Token::Ink0 { surface } else { into };
    palette.get(ink).mix(palette.get(into), 1.0 - share / 100.0)
}

fn apart(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (a.to_oklab(), b.to_oklab());
    100.0 * ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

const INKS: [Token; 9] = [
    Token::Chart1,
    Token::Chart2,
    Token::Chart3,
    Token::Chart4,
    Token::Chart5,
    Token::Chart6,
    Token::Chart7,
    Token::Chart8,
    Token::Accent,
];

#[test]
fn the_calendars_steps_are_told_apart_and_the_deepest_reads() {
    let steps = steps("level");
    assert_eq!(steps.iter().map(|s| s.0).collect::<Vec<_>>(), [1, 2, 3, 4]);
    let mut found = Vec::new();
    for theme in Theme::BUILT_IN {
        for side in Side::BOTH {
            let palette = theme.spec().palette(side);
            for surface in [Token::Ink0, Token::Ink1] {
                let under = palette.get(surface);
                for ink in INKS {
                    let mut ramp = vec![palette.get(Token::Ink3)];
                    ramp.extend(steps.iter().map(|s| colour(palette, side, surface, ink, s)));
                    for (at, pair) in ramp.windows(2).enumerate() {
                        let distance = apart(pair[0], pair[1]);
                        if distance < CALENDAR_APART {
                            found.push(format!(
                                "{} {} {} on {}: step {} to {} is {distance:.1} apart",
                                theme.id(),
                                side.as_str(),
                                ink.name(),
                                surface.name(),
                                at,
                                at + 1
                            ));
                        }
                    }
                    let ratio = ramp[4].contrast(under);
                    if ratio < 3.0 {
                        found.push(format!(
                            "{} {} {} on {}: the deepest step is {ratio:.2}:1",
                            theme.id(),
                            side.as_str(),
                            ink.name(),
                            surface.name()
                        ));
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn the_matrixs_steps_are_told_apart_and_a_value_in_a_cell_reads() {
    let steps = steps("m");
    assert_eq!(
        steps.iter().map(|s| s.0).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6]
    );
    let mut found = Vec::new();
    for theme in Theme::BUILT_IN {
        for side in Side::BOTH {
            let palette = theme.spec().palette(side);
            for surface in [Token::Ink0, Token::Ink1] {
                // The default pair, and the success and danger tones a matrix may be given.
                for ink in [Token::Chart1, Token::Chart2, Token::Success, Token::Danger] {
                    let ramp: Vec<Rgb> = steps
                        .iter()
                        .map(|s| colour(palette, side, surface, ink, s))
                        .collect();
                    for (at, pair) in ramp.windows(2).enumerate() {
                        let distance = apart(pair[0], pair[1]);
                        if distance < MATRIX_APART {
                            found.push(format!(
                                "{} {} {}: step {} to {} is {distance:.1} apart",
                                theme.id(),
                                side.as_str(),
                                ink.name(),
                                at + 1,
                                at + 2
                            ));
                        }
                    }
                    // The sheet writes a value in black or white, by the luminance of the step.
                    for (at, cell) in ramp.iter().enumerate() {
                        let text = if cell.luminance() > 0.179 {
                            Rgb::BLACK
                        } else {
                            Rgb::WHITE
                        };
                        let ratio = text.contrast(*cell);
                        if ratio < READS {
                            found.push(format!(
                                "{} {} {} on {}: a value on step {} is {ratio:.2}:1",
                                theme.id(),
                                side.as_str(),
                                ink.name(),
                                surface.name(),
                                at + 1
                            ));
                        }
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

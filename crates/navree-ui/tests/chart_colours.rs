//! The chart colours of every built-in theme, measured.
//!
//! A chart tells its series apart by colour first, so the eight chart colours
//! are checked as a set, in both palettes of every built-in theme:
//!
//! - each reaches 3:1 on every surface: the page and cards, where a chart sits,
//!   and the raised tooltip, where a series' key sits;
//! - each sits in one band of lightness, so no series looks heavier than another,
//!   and has enough colour not to read as grey;
//! - each stays apart from the series next to it for readers with protanopia or
//!   deuteranopia, the two common kinds of colour blindness, and for everyone
//!   else. Neighbours are taken around a ring, the last next to the first, as a
//!   donut draws them.
//!
//! Distances are Euclidean in OKLab, times 100. Colour blindness is simulated on
//! linear sRGB with the matrices of Machado, Oliveira and Fernandes (2009) at full
//! severity; the thresholds are set for that model.
//!
//! The funnel steps through `chart-1` mixed into the surface, faintest first; the
//! faintest still reaches 2:1, the least for a step of a one-hue scale.

use navree_ui::theme::{Rgb, SURFACES, Side, Theme, Token};

/// The least distance between neighbours as a protanope or deuteranope sees them.
const COLOUR_BLIND: f64 = 8.0;
/// The least distance between neighbours with full colour vision.
const NORMAL: f64 = 15.0;
/// The least OKLCH chroma: below it a colour reads as grey.
const CHROMA: f64 = 0.10;
/// The least contrast on every surface.
const CONTRAST: f64 = 3.0;
/// The least contrast of the funnel's faintest step on the page and on cards.
const FAINTEST: f64 = 2.0;

/// The OKLCH lightness band of each palette.
fn band(side: Side) -> (f64, f64) {
    match side {
        Side::Light => (0.43, 0.77),
        Side::Dark => (0.48, 0.67),
    }
}

/// How a reader sees colour.
#[derive(Clone, Copy, Debug)]
enum Vision {
    Normal,
    Protan,
    Deutan,
}

const PROTAN: [[f64; 3]; 3] = [
    [0.152_286, 1.052_583, -0.204_868],
    [0.114_503, 0.786_281, 0.099_216],
    [-0.003_882, -0.048_116, 1.051_998],
];

const DEUTAN: [[f64; 3]; 3] = [
    [0.367_322, 0.860_646, -0.227_968],
    [0.280_085, 0.672_501, 0.047_413],
    [-0.011_820, 0.042_940, 0.968_881],
];

fn linear(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The colour in OKLab as `vision` sees it.
fn oklab(colour: Rgb, vision: Vision) -> [f64; 3] {
    let rgb = [linear(colour.r), linear(colour.g), linear(colour.b)];
    let seen = match vision {
        Vision::Normal => rgb,
        Vision::Protan | Vision::Deutan => {
            let m = if matches!(vision, Vision::Protan) {
                PROTAN
            } else {
                DEUTAN
            };
            m.map(|row| (row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2]).clamp(0.0, 1.0))
        }
    };
    let [r, g, b] = seen;
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

/// How far apart two colours look to `vision`: OKLab distance times 100.
fn distance(a: Rgb, b: Rgb, vision: Vision) -> f64 {
    let (a, b) = (oklab(a, vision), oklab(b, vision));
    100.0 * ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn lightness_and_chroma(colour: Rgb) -> (f64, f64) {
    let [l, a, b] = oklab(colour, Vision::Normal);
    (l, a.hypot(b))
}

/// What falls short in one palette of one theme.
fn shortfalls(theme: &Theme, side: Side) -> Vec<String> {
    let palette = theme.spec().palette(side);
    let charts = Token::CHARTS.map(|token| palette.get(token));
    let mut out = Vec::new();
    let (low, high) = band(side);
    for (at, colour) in charts.iter().enumerate() {
        let (lightness, chroma) = lightness_and_chroma(*colour);
        if !(low..=high).contains(&lightness) {
            out.push(format!(
                "chart-{} {colour} has lightness {lightness:.3}, outside {low}-{high}",
                at + 1
            ));
        }
        if chroma < CHROMA {
            out.push(format!(
                "chart-{} {colour} has chroma {chroma:.3}, under {CHROMA}",
                at + 1
            ));
        }
        for surface in SURFACES {
            let ratio = colour.contrast(palette.get(surface));
            if ratio < CONTRAST {
                out.push(format!(
                    "chart-{} {colour} is {ratio:.2}:1 on {}",
                    at + 1,
                    surface.name()
                ));
            }
        }
    }
    for at in 0..charts.len() {
        let next = (at + 1) % charts.len();
        let (a, b) = (charts[at], charts[next]);
        for (vision, least) in [
            (Vision::Protan, COLOUR_BLIND),
            (Vision::Deutan, COLOUR_BLIND),
            (Vision::Normal, NORMAL),
        ] {
            let apart = distance(a, b, vision);
            if apart < least {
                out.push(format!(
                    "chart-{} {a} and chart-{} {b} are {apart:.1} apart to {vision:?} vision, under {least}",
                    at + 1,
                    next + 1
                ));
            }
        }
    }
    out
}

#[test]
fn every_built_in_theme_keeps_its_chart_colours_apart_and_readable() {
    let mut found = Vec::new();
    for theme in Theme::BUILT_IN {
        for side in Side::BOTH {
            for shortfall in shortfalls(&theme, side) {
                found.push(format!("{} {}: {shortfall}", theme.id(), side.as_str()));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The least distance between neighbours in the default theme, to see how much room there is.
#[test]
fn the_default_set_clears_each_threshold() {
    for side in Side::BOTH {
        let palette = Theme::NAVREE.spec().palette(side);
        let charts = Token::CHARTS.map(|token| palette.get(token));
        let least = |vision| {
            (0..charts.len())
                .map(|at| distance(charts[at], charts[(at + 1) % charts.len()], vision))
                .fold(f64::INFINITY, f64::min)
        };
        let blind = least(Vision::Protan).min(least(Vision::Deutan));
        assert!(blind >= COLOUR_BLIND, "{} {blind:.1}", side.as_str());
        assert!(
            least(Vision::Normal) >= NORMAL,
            "{} {:.1}",
            side.as_str(),
            least(Vision::Normal)
        );
    }
}

/// The measure itself: white stays white to every reader, and an orange and a
/// green that anyone with full colour vision tells apart run together for a
/// deuteranope.
#[test]
fn the_simulation_sees_what_colour_blind_readers_see() {
    let white = Rgb::hex(0xffffff);
    for vision in [Vision::Protan, Vision::Deutan] {
        assert!(distance(white, white, vision) < 1e-9);
        let [l, a, b] = oklab(white, vision);
        assert!((l - 1.0).abs() < 0.001 && a.abs() < 0.001 && b.abs() < 0.001);
    }
    let (orange, green) = (Rgb::hex(0xd95926), Rgb::hex(0x5b8f00));
    let normal = distance(orange, green, Vision::Normal);
    let deutan = distance(orange, green, Vision::Deutan);
    let protan = distance(orange, green, Vision::Protan);
    assert!((normal - 23.9).abs() < 0.1, "{normal}");
    assert!((deutan - 4.2).abs() < 0.1, "{deutan}");
    assert!((protan - 7.4).abs() < 0.1, "{protan}");
    // The same pair as two neighbours of a palette fails the check.
    let mut theme = *Theme::NAVREE.spec();
    theme.dark.set(Token::Chart1, orange);
    theme.dark.set(Token::Chart2, green);
    let changed = Theme::custom(
        navree_ui::theme::ThemeId::parse("probe").unwrap(),
        "Probe",
        theme,
    )
    .unwrap();
    let found = shortfalls(&changed, Side::Dark);
    assert!(
        found
            .iter()
            .any(|s| s.contains("chart-1") && s.contains("Deutan")),
        "{found:?}"
    );
}

/// The share of `chart-1` in each of the funnel's steps, as the stylesheet writes them.
fn funnel_steps() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/38-charts.css");
    let css = std::fs::read_to_string(path).unwrap();
    let mut steps = Vec::new();
    for line in css.lines() {
        if let Some(rest) = line.split("fill:color-mix(in oklab,var(--chart-1) ").nth(1)
            && line.contains("[data-step=")
            && let Some((percent, _)) = rest.split_once("%,var(--surface))")
        {
            steps.push(percent.parse().unwrap());
        }
    }
    steps
}

#[test]
fn the_funnels_faintest_step_still_reads_on_its_surface() {
    let steps = funnel_steps();
    assert_eq!(steps.len(), 5, "{steps:?}");
    assert!(steps.windows(2).all(|w| w[0] < w[1]), "{steps:?}");
    let faintest = steps[0];
    let mut found = Vec::new();
    for theme in Theme::BUILT_IN {
        for side in Side::BOTH {
            let palette = theme.spec().palette(side);
            for surface in [Token::Ink0, Token::Ink1] {
                let under = palette.get(surface);
                let step = palette
                    .get(Token::Chart1)
                    .mix(under, 1.0 - f64::from(faintest) / 100.0);
                let ratio = step.contrast(under);
                if ratio < FAINTEST {
                    found.push(format!(
                        "{} {}: chart-1 at {faintest}% on {} is {ratio:.2}:1",
                        theme.id(),
                        side.as_str(),
                        surface.name()
                    ));
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

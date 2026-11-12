use http::HeaderMap;
use http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use sha2::{Digest, Sha256};

use super::*;
use crate::charts::{Gauge, Ink, Ring, RingValue, Rings, Sparkline};
use crate::components::Tone;
use crate::node::el;

fn ring() -> Node {
    Ring::new("Recovery", 82).max(100).unit("%").into()
}

fn tile(size: TileSize) -> Tile {
    Tile::new(size)
        .title("Today")
        .note("Updated 07:30")
        .block(Block::ring("Recovery", 82, 100).unit("%"))
        .block(Block::sparkline("Resting HR", [62, 61, 60, 59, 61, 58]).unit("bpm"))
        .block(
            Block::stat("Sleep", "7h 42m")
                .note("+18 min vs 7-day avg")
                .tone(Tone::Success),
        )
        .block(Block::columns("Stress", [22, 35, 48, 30, 26, 18]).ink(Ink::slot(4)))
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The hex colours of the drawing, in the order written.
fn colours(svg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(at) = rest.find("=\"#") {
        let value = &rest[at + 2..at + 9];
        out.push(value.to_owned());
        rest = &rest[at + 9..];
    }
    out
}

#[test]
fn a_drawing_is_written_with_every_colour_resolved_and_nothing_of_the_page() {
    let svg = ChartImage::new(240, 240).svg(&ring()).unwrap();
    assert!(svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240" font-family="Geist">"#), "{svg}");
    for page in [
        "var(",
        "data-",
        "aria-",
        "currentColor",
        "<div",
        "<span",
        "count-up",
    ] {
        assert!(!svg.contains(page), "{page} in {svg}");
    }
    let palette = Theme::NAVREE.spec().palette(Side::Dark);
    // The page's colour under it, the ring's track in the text colour at 9%, its arc in
    // the first chart colour, the ends round.
    assert!(
        svg.contains(&format!(r#"fill="{}""#, palette.get(Token::Ink0).to_hex())),
        "{svg}"
    );
    assert!(
        svg.contains(&format!(
            r#"stroke="{}" stroke-opacity="0.09""#,
            palette.get(Token::Fg).to_hex()
        )),
        "{svg}"
    );
    assert!(
        svg.contains(&format!(
            r#"stroke="{}" stroke-linecap="round""#,
            palette.get(Token::Chart1).to_hex()
        )),
        "{svg}"
    );
    // The drawing fills the picture: the ring's own box is stretched to it.
    assert!(
        svg.contains(r#"viewBox="0 0 120 120" width="100%" height="100%""#),
        "{svg}"
    );
}

#[test]
fn light_and_dark_take_their_own_palettes_and_the_theme_its_own() {
    let dark = ChartImage::new(120, 120)
        .mode(Mode::Dark)
        .svg(&ring())
        .unwrap();
    let light = ChartImage::new(120, 120)
        .mode(Mode::Light)
        .svg(&ring())
        .unwrap();
    let system = ChartImage::new(120, 120)
        .mode(Mode::System)
        .svg(&ring())
        .unwrap();
    assert_ne!(dark, light);
    assert_eq!(
        dark, system,
        "a picture can't follow the device: system is dark"
    );
    let light_ink = Theme::NAVREE
        .spec()
        .palette(Side::Light)
        .get(Token::Ink0)
        .to_hex();
    assert!(light.contains(&light_ink), "{light}");
    let ocean = ChartImage::new(120, 120)
        .theme(&Theme::OCEAN)
        .svg(&Ring::new("Focus", 40).ink(Ink::accent()).into())
        .unwrap();
    let accent = Theme::OCEAN
        .spec()
        .palette(Side::Dark)
        .get(Token::Accent)
        .to_hex();
    assert!(ocean.contains(&accent), "{ocean}");
}

#[test]
fn an_accent_of_ones_own_colours_what_is_drawn_in_the_accent() {
    let cyan = Rgb::hex(0x22d3ee);
    let node: Node = Ring::new("Recovery", 82)
        .ink(Ink::accent())
        .fill(crate::charts::Gradient::sheen(Ink::accent()))
        .into();
    let svg = ChartImage::new(120, 120).accent(cyan).svg(&node).unwrap();
    // A sheen's first stop is the accent; its far end the accent mixed toward the text.
    assert!(
        svg.contains(r##"stop offset="0" stop-color="#22d3ee""##),
        "{svg}"
    );
    let fg = Theme::NAVREE.spec().palette(Side::Dark).get(Token::Fg);
    let vivid = cyan.mix(fg, 0.38).to_hex();
    assert!(
        svg.contains(&format!(r#"stop offset="1" stop-color="{vivid}""#)),
        "{svg}"
    );
    // A slot colour keeps its own.
    let slot = ChartImage::new(120, 120)
        .accent(cyan)
        .svg(&Ring::new("Recovery", 82).ink(Ink::slot(3)).into())
        .unwrap();
    assert!(!slot.contains("#22d3ee"), "{slot}");
}

#[test]
fn a_sparkline_keeps_its_line_two_pixels_wide_when_its_drawing_stretches() {
    let line: Node = Sparkline::new("Resting HR", [62, 61, 60, 59, 61, 58])
        .height(64)
        .area()
        .into();
    let svg = ChartImage::new(400, 64).svg(&line).unwrap();
    // The wash is stretched as a group; the line's points are moved to the picture's
    // width instead, so its stroke stays 2 px whatever the width.
    assert!(
        svg.contains(r#"<g transform="scale(0.4 1)"><path"#),
        "{svg}"
    );
    assert!(
        svg.contains(r#"d="M0 4L80 18L160 32L240 46L320 18L400 60""#),
        "{svg}"
    );
    assert!(
        svg.contains(r#"stroke-width="2" stroke-linejoin="round" stroke-linecap="round""#),
        "{svg}"
    );
    assert!(!svg.contains("vector-effect"), "{svg}");
    // The end dot on the surface, at the end of the line.
    assert!(svg.contains(r#"cx="100%" cy="60" r="3""#), "{svg}");
}

#[test]
fn a_columns_rounded_clip_becomes_its_own_shape() {
    let columns: Node = Sparkline::new("Steps", [6200, 8100, 7400]).columns().into();
    let svg = ChartImage::new(300, 32).svg(&columns).unwrap();
    assert!(!svg.contains("inset("), "{svg}");
    assert!(svg.contains(r#"x="7.333%" width="18.666%""#), "{svg}");
    // Rounded as a pill, each end half the narrower side.
    assert!(
        svg.contains(r##"height="18.4" fill="#3987e5" rx="9.2" ry="9.2""##),
        "{svg}"
    );
}

#[test]
fn every_part_a_ring_gauge_rings_and_sparkline_draw_has_its_colour() {
    let nodes: Vec<Node> = vec![
        ring(),
        Gauge::new("Stress", 40).into(),
        Rings::new("Activity")
            .ring(RingValue::new("Move", 540).max(600).unit("kcal"))
            .ring(RingValue::new("Exercise", 38).max(30).unit("min"))
            .into(),
        Sparkline::new("HR", [1, 3, 2]).points().fade().into(),
        Sparkline::new("HR", [1, 3, 2]).tone(Tone::Danger).into(),
    ];
    for node in nodes {
        let svg = ChartImage::new(200, 200).svg(&node).unwrap();
        // Every shape says how it is painted: a renderer's default is black.
        for shape in ["<path", "<circle", "<rect"] {
            for (at, _) in svg.match_indices(shape) {
                let end = at + svg[at..].find('>').unwrap();
                let tag = &svg[at..end];
                assert!(
                    tag.contains("fill=") || tag.contains("stroke="),
                    "{tag} in {svg}"
                );
            }
        }
    }
    // A lap past the goal is the vivid end of its colour.
    let lap = ChartImage::new(200, 200)
        .svg(&Ring::new("Move", 130).max(100).ink(Ink::slot(2)).into())
        .unwrap();
    let chart2 = Theme::NAVREE.spec().palette(Side::Dark).get(Token::Chart2);
    assert!(lap.contains(&chart2.vivid(Side::Dark).to_hex()), "{lap}");
    // A tone colours as its token.
    let danger = ChartImage::new(200, 40)
        .svg(&Sparkline::new("HR", [1, 3, 2]).tone(Tone::Danger).into())
        .unwrap();
    let red = Theme::NAVREE
        .spec()
        .palette(Side::Dark)
        .get(Token::Danger)
        .to_hex();
    assert!(danger.contains(&red), "{danger}");
}

#[test]
fn nothing_outside_the_drawing_is_read_or_kept() {
    let hostile: Node = el("svg")
        .attr("viewBox", "0 0 10 10")
        .children((
            el("image").attr("href", "/etc/passwd"),
            el("use").attr("href", "https://elsewhere.test/a.svg#x"),
            el("use").attr("href", "#own"),
            el("foreignObject").child(el("div").child("html")),
            el("text").child("<script>alert(1)</script> & \"quoted\""),
            el("rect")
                .attr("fill", "var(--nope, #123456)")
                .attr("clip-path", "polygon(0 0)"),
        ))
        .into();
    let svg = ChartImage::new(10, 10).svg(&hostile).unwrap();
    for gone in [
        "<image",
        "passwd",
        "elsewhere",
        "foreignObject",
        "<div",
        "polygon",
    ] {
        assert!(!svg.contains(gone), "{gone} in {svg}");
    }
    assert!(svg.contains(r##"<use href="#own"/>"##), "{svg}");
    assert!(
        svg.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;quoted&quot;"),
        "{svg}"
    );
    assert!(
        svg.contains(r##"fill="#123456""##),
        "a fallback colour is kept: {svg}"
    );
    // It still draws.
    assert!(ChartImage::new(10, 10).png(&hostile).is_ok());
}

#[test]
fn a_node_with_no_drawing_or_a_size_out_of_range_is_refused() {
    let none: Node = el("div").child("words").into();
    assert_eq!(
        ChartImage::new(10, 10).svg(&none),
        Err(ImageError::NoDrawing)
    );
    assert_eq!(
        ChartImage::new(10, 10).png(&none).unwrap_err(),
        ImageError::NoDrawing
    );
    assert_eq!(ChartImage::new(0, 10).svg(&ring()), Err(ImageError::Size));
    assert_eq!(
        ChartImage::new(MAX_SIDE + 1, 10).svg(&ring()),
        Err(ImageError::Size)
    );
    assert_eq!(
        ChartImage::new(MAX_SIDE, 10).scale(2).svg(&ring()),
        Err(ImageError::Size)
    );
    assert!(ImageError::NoDrawing.to_string().contains("SVG"));
}

#[test]
fn the_same_inputs_make_the_same_bytes_and_the_same_tag() {
    let make = || {
        ChartImage::for_tile(&tile(TileSize::Medium))
            .accent(Rgb::hex(0x34d399))
            .png(&tile(TileSize::Medium).into())
            .unwrap()
    };
    let first = make();
    let again = make();
    let threaded = std::thread::spawn(make).join().unwrap();
    assert_eq!(sha(first.bytes()), sha(again.bytes()));
    assert_eq!(first.bytes(), threaded.bytes());
    assert_eq!(first.etag(), again.etag());
    assert_eq!(first.etag().len(), 34);
    assert!(first.etag().starts_with('"') && first.etag().ends_with('"'));
    // The tag is known before anything is drawn, and is the picture's.
    let image = ChartImage::for_tile(&tile(TileSize::Medium)).accent(Rgb::hex(0x34d399));
    assert_eq!(
        image.etag(&tile(TileSize::Medium).into()).unwrap(),
        first.etag()
    );
    // Anything that draws differently tags differently.
    let other_mode = ChartImage::for_tile(&tile(TileSize::Medium))
        .accent(Rgb::hex(0x34d399))
        .mode(Mode::Light)
        .png(&tile(TileSize::Medium).into())
        .unwrap();
    assert_ne!(other_mode.etag(), first.etag());
    assert_ne!(sha(other_mode.bytes()), sha(first.bytes()));
    let other_accent = image.clone().accent(Rgb::hex(0xa78bfa));
    assert_ne!(
        other_accent.etag(&tile(TileSize::Medium).into()).unwrap(),
        first.etag()
    );
    let other_scale = image.scale(2);
    assert_ne!(
        other_scale.etag(&tile(TileSize::Medium).into()).unwrap(),
        first.etag()
    );
}

#[test]
fn the_words_are_drawn_in_the_built_in_font() {
    let stat = |value: &str| {
        let tile = Tile::new(TileSize::Small).block(Block::stat("Spent", value));
        ChartImage::for_tile(&tile).png(&tile.into()).unwrap()
    };
    // A picture with other words is another picture: the words are drawn, not lost.
    assert_ne!(stat("1,20,000").bytes(), stat("1,20,001").bytes());
    // The rupee sign, which Geist lacks, is drawn from the font beside it.
    let with = stat("\u{20b9}1,20,000");
    let without = stat("1,20,000");
    assert_ne!(with.bytes(), without.bytes());
}

#[test]
fn a_tile_is_its_size_and_scales() {
    for (size, pixels) in [
        (TileSize::Small, (474, 474)),
        (TileSize::Medium, (1014, 474)),
        (TileSize::Large, (1014, 1014)),
        (TileSize::Custom(10, 9000), (64, 4096)),
    ] {
        assert_eq!(size.pixels(), pixels);
        let png = ChartImage::for_tile(&tile(size))
            .png(&tile(size).into())
            .unwrap();
        assert_eq!((png.width(), png.height()), pixels);
        // The PNG's own header says so too.
        assert_eq!(&png.bytes()[16..20], &pixels.0.to_be_bytes());
        assert_eq!(&png.bytes()[20..24], &pixels.1.to_be_bytes());
    }
    let twice = ChartImage::for_tile(&tile(TileSize::Small))
        .scale(2)
        .png(&tile(TileSize::Small).into())
        .unwrap();
    assert_eq!((twice.width(), twice.height()), (948, 948));
    assert_eq!(TileSize::from_name("large"), Some(TileSize::Large));
    assert_eq!(TileSize::from_name("huge"), None);
    assert_eq!(TileSize::Medium.name(), "medium");
}

#[test]
fn a_tile_lays_its_blocks_nearest_to_square_and_draws_no_more_than_fit() {
    let cells = |size: TileSize, n: usize| {
        let mut t = Tile::new(size);
        for i in 0..n {
            t = t.block(Block::stat(format!("B{i}"), "1"));
        }
        let svg = ChartImage::for_tile(&t).svg(&t.into()).unwrap();
        // Each block sits on a panel of its own.
        let panel = Theme::NAVREE
            .spec()
            .palette(Side::Dark)
            .get(Token::Ink2)
            .to_hex();
        svg.matches(&format!(r#"fill="{panel}""#)).count()
    };
    assert_eq!(cells(TileSize::Small, 2), 2);
    assert_eq!(cells(TileSize::Small, 5), 2, "a small tile draws two");
    assert_eq!(cells(TileSize::Medium, 6), 4);
    assert_eq!(cells(TileSize::Large, 9), 6);
    assert_eq!(super::tile::grid(2, 400.0, 400.0, 0.0), (1, 2));
    assert_eq!(super::tile::grid(4, 900.0, 380.0, 0.0), (4, 1));
    assert_eq!(super::tile::grid(4, 870.0, 800.0, 0.0), (2, 2));
    assert_eq!(super::tile::grid(3, 400.0, 400.0, 0.0), (2, 2));
    // An empty tile says so, in words of its own.
    let empty = Tile::new(TileSize::Small);
    assert!(
        ChartImage::for_tile(&empty)
            .svg(&empty.into())
            .unwrap()
            .contains("No blocks yet")
    );
}

#[test]
fn a_tiles_words_are_text_and_long_ones_are_cut() {
    let t = Tile::new(TileSize::Small)
        .title("Simran Kaur's morning summary for Tuesday")
        .block(Block::stat("<b>Sleep</b>", "7h 42m"));
    let svg = ChartImage::for_tile(&t).svg(&t.into()).unwrap();
    assert!(svg.contains("&lt;B&gt;SLEEP&lt;/B&gt;"), "{svg}");
    assert!(
        svg.contains('\u{2026}'),
        "a long title ends in an ellipsis: {svg}"
    );
}

#[test]
fn a_route_answers_304_for_the_picture_it_holds_and_the_picture_otherwise() {
    let image = ChartImage::new(120, 120).cache_control("private, no-cache");
    let node = ring();
    let tag = image.etag(&node).unwrap();
    let mut held = HeaderMap::new();
    held.insert(IF_NONE_MATCH, format!("\"old\", W/{tag}").parse().unwrap());
    let response = image.respond(&node, &held);
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(response.headers()[ETAG], tag.as_str());
    assert_eq!(response.headers()[CACHE_CONTROL], "private, no-cache");

    let response = image.respond(&node, &HeaderMap::new());
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CONTENT_TYPE], "image/png");
    assert_eq!(response.headers()[ETAG], tag.as_str());

    let mut any = HeaderMap::new();
    any.insert(IF_NONE_MATCH, "*".parse().unwrap());
    assert_eq!(
        image.respond(&node, &any).status(),
        StatusCode::NOT_MODIFIED
    );
    let png = image.png(&node).unwrap();
    assert_eq!(
        png.clone().respond(&held).status(),
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(png.respond(&HeaderMap::new()).status(), StatusCode::OK);

    let none: Node = el("p").into();
    assert_eq!(
        image.respond(&none, &HeaderMap::new()).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[test]
fn the_shorthand_draws_as_the_builder_does() {
    let quick = png(ring(), 160, 160, &Theme::ROSE, Mode::Light).unwrap();
    let built = ChartImage::new(160, 160)
        .theme(&Theme::ROSE)
        .mode(Mode::Light)
        .png(&ring())
        .unwrap();
    assert_eq!(quick, built);
    // A clear picture has no colour under it.
    let clear = ChartImage::new(160, 160)
        .backdrop(Backdrop::Clear)
        .svg(&ring())
        .unwrap();
    assert!(
        !clear.contains(r#"<rect x="0" y="0" width="100%""#),
        "{clear}"
    );
    let inline = ChartImage::new(160, 160).svg_node(&ring()).unwrap();
    assert!(crate::render(&inline).starts_with("<svg xmlns"));
}

#[test]
fn the_colours_are_the_palettes_and_nothing_else() {
    // Every colour written into a tile is one of the theme's, an accent's derivations
    // or a mix the stylesheets make: no colour is written by hand.
    let svg = ChartImage::for_tile(&tile(TileSize::Large))
        .svg(&tile(TileSize::Large).into())
        .unwrap();
    let palette = Theme::NAVREE.spec().palette(Side::Dark);
    let known: Vec<String> = palette.iter().map(|(_, c)| c.to_hex()).collect();
    let mut odd: Vec<String> = colours(&svg)
        .into_iter()
        .filter(|c| !known.contains(c))
        .collect();
    odd.sort();
    odd.dedup();
    // The vivid ends of the accent and of chart colour 4, nothing else.
    let fg = palette.get(Token::Fg);
    let vivid_accent = palette.get(Token::Accent).mix(fg, 0.38).to_hex();
    let vivid_4 = palette.get(Token::Chart4).vivid(Side::Dark).to_hex();
    for c in &odd {
        assert!(*c == vivid_accent || *c == vivid_4, "{c} in {svg}");
    }
}

#[test]
fn two_drawings_in_one_page_share_no_id() {
    let dark = ChartImage::for_tile(&tile(TileSize::Medium))
        .svg_node(&tile(TileSize::Medium).into())
        .unwrap();
    let light = ChartImage::for_tile(&tile(TileSize::Medium))
        .mode(Mode::Light)
        .svg_node(&tile(TileSize::Medium).into())
        .unwrap();
    let ids = |node: &Node| {
        let html = crate::render(node);
        html.match_indices(" id=\"")
            .map(|(at, _)| {
                html[at + 5..]
                    .split('"')
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (ids(&dark), ids(&light));
    assert!(!a.is_empty());
    assert!(a.iter().all(|id| !b.contains(id)), "{a:?} {b:?}");
    // Every reference points at an id of its own drawing.
    let html = crate::render(&dark);
    for (at, _) in html.match_indices("url(#") {
        let id = html[at + 5..].split(')').next().unwrap();
        assert!(a.iter().any(|own| own == id), "{id}");
    }
    assert!(
        html.contains(r#"width="100%" viewBox="0 0 1014 474""#),
        "{html}"
    );
}

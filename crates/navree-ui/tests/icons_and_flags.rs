//! Icons and flags: what ships is safe, the pins hold, names are checked while
//! compiling, and a flag or icon sits wherever a node does.

use std::collections::BTreeSet;

use navree_ui::components::{icon, icon_labelled};
use navree_ui::flags::{self, CodeKind, CountryCode};
use navree_ui::icons::{self, Icon};
use navree_ui::{
    Badge, Flag, FlagShape, FlagSize, Node, Row, TableCell, el, flag, icon_file, lucide,
    render,
};
use sha2::{Digest, Sha256};

// ------------------------------------------------------------------- the pins

fn crate_file(path: &str) -> String {
    let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("{}: {e}", full.display()))
        .replace("\r\n", "\n")
}

fn lock() -> serde_json::Value {
    serde_json::from_str(&crate_file("icons.lock.json")).expect("icons.lock.json is JSON")
}

#[test]
fn the_generated_files_are_the_ones_the_lock_records() {
    let lock = lock();
    let outputs = lock["outputs"].as_object().expect("outputs");
    assert_eq!(outputs.len(), 3, "{outputs:?}");
    for (path, recorded) in outputs {
        let digest = Sha256::digest(crate_file(path).as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            recorded.as_str(),
            Some(hex.as_str()),
            "{path} was changed by hand or the lock is stale. Run: node crates/navree-ui/runtime/icons.mjs"
        );
    }
}

#[test]
fn every_source_is_pinned_by_version_and_checksum() {
    let lock = lock();
    let sources = lock["sources"].as_object().expect("sources");
    for name in ["lucide", "flags", "cldr-territories", "cldr-subdivisions"] {
        let pin = &sources[name];
        let integrity = pin["integrity"].as_str().expect(name);
        assert!(integrity.starts_with("sha512-") && integrity.len() > 80, "{name}");
        let version = pin["version"].as_str().expect(name);
        assert!(version.chars().next().is_some_and(|c| c.is_ascii_digit()), "{name}");
        assert!(
            pin["url"].as_str().expect(name).starts_with("https://registry.npmjs.org/"),
            "{name}"
        );
        assert!(
            ["ISC", "MIT", "Unicode-3.0"].contains(&pin["licence"].as_str().expect(name)),
            "{name}"
        );
    }
}

#[test]
fn third_party_names_the_pinned_versions_and_licences() {
    let notices = crate_file("THIRD_PARTY.md");
    let lock = lock();
    for (name, needles) in [
        ("lucide", &["ISC", "Lucide"][..]),
        ("flags", &["flag-icons", "MIT", "Panayiotis Lipiridis"][..]),
        ("cldr-territories", &["CLDR", "Unicode"][..]),
    ] {
        let version = lock["sources"][name]["version"].as_str().expect(name);
        assert!(notices.contains(version), "THIRD_PARTY.md does not name {name} {version}");
        for needle in needles {
            assert!(notices.contains(needle), "THIRD_PARTY.md lacks {needle}");
        }
    }
}

// ------------------------------------------------------- what ships is safe

/// Everything in the shipped artwork that could run, load or style something.
const FORBIDDEN: [&str; 17] = [
    "<script", "<style", "style=", "<foreignObject", "<image", "<a ", "<iframe", "<text", "<animate",
    "javascript:", "data:", "http:", "https:", "xlink:", "onload", "onclick", "<!",
];

/// Element names in some markup.
fn tags(markup: &str) -> BTreeSet<String> {
    markup
        .split('<')
        .skip(1)
        .map(|part| {
            part.trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect()
        })
        .collect()
}

const SHAPE_TAGS: [&str; 8] = ["path", "circle", "rect", "line", "ellipse", "polyline", "polygon", "g"];

#[test]
fn every_icon_is_shapes_and_nothing_that_loads_or_runs() {
    for (name, shipped) in icons::ALL {
        let markup = shipped.shapes();
        for needle in FORBIDDEN {
            assert!(!markup.contains(needle), "{name} contains {needle}");
        }
        for tag in tags(markup) {
            assert!(SHAPE_TAGS.contains(&tag.as_str()), "{name} has <{tag}>");
        }
        assert!(!markup.contains('\''), "{name}");
        assert!(!markup.contains("  "), "{name}");
    }
}

const FLAG_TAGS: [&str; 15] = [
    "path", "circle", "rect", "line", "ellipse", "polyline", "polygon", "g", "use", "defs",
    "clipPath", "linearGradient", "radialGradient", "stop", "marker",
];

#[test]
fn every_flag_is_shapes_and_nothing_that_loads_or_runs() {
    for code in flags::ALL {
        for (shape, markup) in [("rect", code.rect_shapes()), ("square", code.square_shapes())] {
            for needle in FORBIDDEN {
                assert!(!markup.contains(needle), "{code} {shape} contains {needle}");
            }
            for tag in tags(markup) {
                assert!(FLAG_TAGS.contains(&tag.as_str()), "{code} {shape} has <{tag}>");
            }
        }
    }
}

/// `id="x"` values in some markup.
fn ids(markup: &str) -> Vec<&str> {
    markup
        .split(" id=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap_or_default())
        .collect()
}

/// `url(#x)` and `href="#x"` targets.
fn references(markup: &str) -> Vec<&str> {
    let mut found: Vec<&str> = markup
        .split("url(#")
        .skip(1)
        .map(|rest| rest.split(')').next().unwrap_or_default())
        .collect();
    found.extend(
        markup
            .split("href=\"#")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap_or_default()),
    );
    found
}

#[test]
fn ids_inside_flags_belong_to_one_flag_and_one_shape() {
    let mut every_id = BTreeSet::new();
    for code in flags::ALL {
        let lower = code.as_str().to_ascii_lowercase();
        for (prefix, markup) in [(format!("f4-{lower}-"), code.rect_shapes()), (format!("f1-{lower}-"), code.square_shapes())] {
            let own = ids(markup);
            for id in &own {
                assert!(id.starts_with(&prefix), "{code}: id {id} should start with {prefix}");
                assert!(every_id.insert((*id).to_owned()), "id {id} is in two flags");
            }
            for reference in references(markup) {
                assert!(own.contains(&reference), "{code}: a reference to {reference} that nothing defines");
            }
        }
    }
    assert!(every_id.len() > 200, "{}", every_id.len());
}

#[test]
fn the_set_has_every_iso_country_and_the_regions_it_promises() {
    assert_eq!(flags::ALL.len(), 270);
    assert_eq!(CountryCode::countries().count(), 250);
    let regions: Vec<_> = flags::ALL.iter().filter(|c| c.kind() == CodeKind::Region).map(|c| c.as_str()).collect();
    for code in ["GB-ENG", "GB-NIR", "GB-SCT", "GB-WLS", "ES-CT", "ES-GA", "ES-PV", "IC", "CP", "DG"] {
        assert!(regions.contains(&code), "{code} is not among {regions:?}");
    }
    let bodies: Vec<_> = flags::ALL.iter().filter(|c| c.kind() == CodeKind::Organisation).map(|c| c.as_str()).collect();
    assert_eq!(bodies, ["ARAB", "ASEAN", "CEFTA", "EAC", "EU", "PC", "UN"]);
}

// -------------------------------------------------------------------- icons

#[test]
fn an_icon_draws_as_a_hidden_svg_with_its_box() {
    let html = render(&icon(icons::MAIL_OPEN));
    assert!(html.starts_with(r#"<svg data-slot="icon" viewBox="0 0 24 24" aria-hidden="true">"#), "{html}");
    assert!(html.ends_with("</svg>"));
}

#[test]
fn a_lone_icon_can_carry_a_name() {
    let html = render(&icon_labelled(icons::CIRCLE_CHECK, "Delivered"));
    assert!(html.contains(r#"role="img""#) && html.contains(r#"aria-label="Delivered""#), "{html}");
    assert!(!html.contains("aria-hidden"), "{html}");
    let html = render(&icon_labelled(icons::CIRCLE_CHECK, r#"a "quoted" <name>"#));
    assert!(html.contains("&quot;") && html.contains("&lt;name"), "{html}");
}

#[test]
fn three_ways_to_name_an_icon_find_the_same_one() {
    assert_eq!(lucide!("mail-open"), icons::MAIL_OPEN);
    assert_eq!(Icon::lookup("mail-open"), Some(icons::MAIL_OPEN));
    assert_eq!(lucide!("trash-2"), icons::TRASH);
}

#[test]
#[allow(deprecated)]
fn an_old_name_still_compiles_and_draws_the_new_icon() {
    assert_eq!(icons::ALERT_CIRCLE, icons::CIRCLE_ALERT);
    assert_eq!(lucide!("alert-triangle"), icons::TRIANGLE_ALERT);
}

#[test]
fn a_page_of_every_icon_stays_small() {
    let all: Vec<Node> = icons::ALL.iter().map(|(_, shipped)| icon(*shipped)).collect();
    let html = render(&Node::list(all));
    assert!(html.len() < 1_100_000, "{}", html.len());
    assert!(!html.contains("style="));
}

// -------------------------------------------------------------------- flags

#[test]
fn a_flag_is_an_image_with_the_countrys_name() {
    let html = render(&Flag::new(flags::AE).into());
    assert!(html.starts_with(r#"<svg data-slot="flag" viewBox="0 0 640 480" focusable="false" role="img" aria-label="United Arab Emirates">"#), "{html}");
    assert!(html.contains(r##"fill="#00732f""##));
    assert!(!html.contains("data-size") && !html.contains("data-shape"), "{html}");
}

#[test]
fn sizes_and_shapes_are_attributes() {
    let html = render(&Flag::new(flags::US).size(FlagSize::Xl).shape(FlagShape::Square).into());
    assert!(html.contains(r#"data-size="xl""#) && html.contains(r#"data-shape="square""#), "{html}");
    assert!(html.contains(r#"viewBox="0 0 512 512""#), "{html}");
    let html = render(&Flag::new(flags::US).round().into());
    assert!(html.contains(r#"data-shape="round""#) && html.contains(r#"viewBox="0 0 512 512""#), "{html}");
    for (size, name) in [(FlagSize::Xs, "xs"), (FlagSize::Md, "md"), (FlagSize::Lg, "lg")] {
        assert!(render(&Flag::new(flags::US).size(size).into()).contains(&format!(r#"data-size="{name}""#)));
    }
    assert!(!render(&Flag::new(flags::US).size(FlagSize::Sm).into()).contains("data-size"));
}

#[test]
fn a_flag_beside_its_name_can_be_hidden_or_renamed() {
    let html = render(&flag!("de").decorative().into());
    assert!(html.contains(r#"aria-hidden="true""#) && !html.contains("aria-label") && !html.contains("role="), "{html}");
    let html = render(&flag!("de").label("Deutschland").into());
    assert!(html.contains(r#"aria-label="Deutschland""#), "{html}");
}

#[test]
fn a_code_from_data_finds_its_flag_or_draws_a_placeholder() {
    for text in ["ae", "AE", "Ae", "  ae  "] {
        assert_eq!(Flag::country(text).code(), Some(flags::AE), "{text:?}");
    }
    assert_eq!(Flag::country("gb_sct").code(), Some(flags::GB_SCT));
    let unknown = render(&Flag::country("zz").into());
    assert!(unknown.contains(r#"data-state="unknown""#) && unknown.contains(r#"aria-label="Unknown country""#), "{unknown}");
    assert!(unknown.contains("currentColor"));
    let shaped = render(&Flag::country("zz").round().label("Elsewhere").into());
    assert!(shaped.contains(r#"viewBox="0 0 512 512""#) && shaped.contains("Elsewhere"), "{shaped}");
}

#[test]
fn hostile_codes_never_panic_and_never_reach_the_page() {
    let long = "AE".repeat(50_000);
    for text in ["", "\0", "<script>", "\"><img src=x onerror=alert(1)>", "../../etc", "🇦🇪", &long, "ae\n", "a\u{0}e"] {
        let html = render(&Flag::country(text).into());
        assert!(html.contains(r#"data-state="unknown""#) || text.trim() == "ae", "{text:.10?}");
        assert!(!html.contains("<img") && !html.contains("<script"), "{text:.10?}");
        assert!(html.len() < 2_000, "{}", html.len());
    }
}

#[test]
fn a_flag_sits_in_a_badge_a_row_and_a_table_cell() {
    let badge = render(&Badge::new(Row::gap(2).children((flag!("AE").decorative(), "UAE"))).into());
    assert!(badge.contains(r#"data-slot="badge""#) && badge.contains(r#"data-slot="flag""#) && badge.contains("UAE"), "{badge}");
    let cell = render(&TableCell::new(Row::gap(2).children((Flag::new(flags::FR).decorative(), "France"))).into());
    assert!(cell.contains(r#"data-slot="flag""#) && cell.contains("France"), "{cell}");
    let wrapped = render(&el("li").children((Flag::country("jp"), " Japan")).into());
    assert!(wrapped.contains("Japan") && wrapped.contains("aria-label=\"Japan\""), "{wrapped}");
}

#[test]
fn a_flag_page_keeps_the_strict_content_security_policy() {
    let mut page = Vec::new();
    for code in flags::ALL.iter().take(40) {
        page.push(Node::from(Flag::new(*code)));
        page.push(Node::from(Flag::new(*code).round()));
    }
    let html = render(&Node::list(page));
    for needle in ["style=", "<style", "<script", "xlink", "href=\"http"] {
        assert!(!html.contains(needle), "{needle}");
    }
}

#[test]
fn the_heaviest_flags_are_known() {
    let mut weights: Vec<(usize, &str)> = flags::ALL.iter().map(|c| (c.rect_shapes().len(), c.as_str())).collect();
    weights.sort_unstable_by(|a, b| b.cmp(a));
    let names: Vec<_> = weights.iter().take(3).map(|(_, n)| *n).collect();
    assert_eq!(names, ["RS", "BO", "MX"], "{:?}", &weights[..5]);
    let median = weights[weights.len() / 2].0;
    assert!(median < 2_000, "the median flag is {median} bytes");
}

// --------------------------------------------------------------- own icons

mod brand {
    //! The icons of `tests/own-icons/`, checked while this test compiles.
    navree_ui::icon_set!("tests/own-icons");
}

#[test]
fn a_directory_of_svg_files_becomes_constants() {
    let names: Vec<_> = brand::ALL.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["arrow-left", "brand-mark"]);
    assert_eq!(brand::ARROW_LEFT.view_box(), "0 0 24 24");
    assert_eq!(brand::BRAND_MARK.view_box(), "0 0 512 512");
}

#[test]
fn own_icons_are_sanitised_and_draw_like_built_in_ones() {
    assert_eq!(
        brand::ARROW_LEFT.shapes(),
        r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5"/><path d="m12 19-7-7 7-7"/></g>"#
    );
    let html = render(&icon(brand::BRAND_MARK));
    assert!(html.contains(r#"viewBox="0 0 512 512""#), "{html}");
    assert!(html.contains(r##"fill="#ffffff""##) && html.contains("fill-opacity"), "{html}");
    assert!(!html.contains("<title") && !html.contains("<svg xmlns"), "{html}");
}

#[test]
fn a_single_file_is_a_constant_too() {
    const ARROW: Icon = icon_file!("tests/own-icons/arrow-left.svg");
    assert_eq!(ARROW, brand::ARROW_LEFT);
}

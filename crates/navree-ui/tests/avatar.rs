//! An avatar's initials, tone, fill and shape: the letters it takes from a name in any
//! script, what a screen reader is told, and that an avatar with none of them is drawn and
//! loads what it did before.

use navree_ui::*;

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

/// What a page with `node` links beyond the toolkit's own sheet and the core script.
fn uses(node: impl Into<Node>) -> Uses {
    measure(&node.into()).uses
}

/// The source of the parts stylesheet, which an avatar's options are drawn by.
fn parts_sheet() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/extra/parts.css"),
    )
    .unwrap()
    .replace("\r\n", "\n")
}

/// The letters an avatar for `name` shows.
fn letters(name: &str) -> String {
    let out = html(Avatar::initials(name));
    let open = out.find('>').unwrap() + 1;
    out[open..out.rfind("</span>").unwrap()].to_owned()
}

#[test]
fn the_initials_are_the_first_letters_of_the_first_and_last_words() {
    assert_eq!(letters("Dalbir Oberoi"), "DO");
    assert_eq!(letters("Narmada Roadways Private Limited"), "NL");
    assert_eq!(letters("narmada"), "N");
    assert_eq!(letters("  Simran   Kaur  "), "SK");
    // A word's first letter or digit, past the marks before it.
    assert_eq!(letters("(Harjot) Singh-Gill"), "HS");
    assert_eq!(letters("3M India"), "3I");
    assert_eq!(letters("o'brien ltd"), "OL");
}

#[test]
fn the_initials_come_from_a_name_in_any_script() {
    assert_eq!(letters("ਗੁਰਪ੍ਰੀਤ ਸਿੰਘ"), "ਗਸ");
    assert_eq!(letters("प्रिया शर्मा"), "पश");
    assert_eq!(letters("王小明"), "王");
    assert_eq!(letters("김 민수"), "김민");
    assert_eq!(letters("محمد علي"), "مع");
    assert_eq!(letters("Ölçer Şahin"), "ÖŞ");
    assert_eq!(letters("ßeta"), "S");
}

#[test]
fn a_screen_reader_hears_the_whole_name_and_hostile_text_stays_text() {
    let out = html(Avatar::initials("Dalbir Oberoi"));
    assert_eq!(
        out,
        r#"<span data-slot="avatar" role="img" aria-label="Dalbir Oberoi" data-initials>DO</span>"#
    );
    let hostile = html(Avatar::initials(r#"<img src=x onerror=alert(1)> "Ltd""#));
    assert!(!hostile.contains("<img"), "{hostile}");
    assert!(hostile.contains("&lt;img"), "{hostile}");
    assert!(hostile.contains(">IL</span>"), "{hostile}");
}

#[test]
fn a_name_with_no_letter_draws_the_person_icon() {
    for name in ["", "   ", "🙂", "-- --"] {
        let out = html(Avatar::initials(name));
        assert_eq!(out, html(Avatar::placeholder(name)), "{name:?}");
        assert!(out.contains("<svg"), "{out}");
        assert!(!out.contains("data-initials"), "{out}");
    }
}

#[test]
fn a_tone_is_one_attribute_and_neutral_or_a_hostile_name_writes_nothing() {
    let out = html(Avatar::initials("Narmada Roadways").tone(Tone::Info));
    assert!(out.contains(r#" data-tone="info""#), "{out}");
    let named = html(Avatar::initials("Narmada").tone(Tone::named("teal")));
    assert!(named.contains(r#" data-tone="teal""#), "{named}");
    for tone in [Tone::Neutral, Tone::named("x\" onmouseover=\"y")] {
        let out = html(Avatar::initials("Narmada").tone(tone));
        assert!(!out.contains("data-tone"), "{out}");
    }
    // A tone reaches the person icon too.
    let icon = html(Avatar::placeholder("Simran Kaur").tone(Tone::Success));
    assert!(icon.contains(r#"data-tone="success""#), "{icon}");
}

#[test]
fn a_filled_avatar_takes_the_accent_and_no_tone() {
    let out = html(Avatar::initials("Staff").solid().tone(Tone::Info));
    assert!(out.contains(" data-solid"), "{out}");
    assert!(
        !out.contains("data-tone"),
        "a filled avatar is the accent's: {out}"
    );
    let sheet = parts_sheet();
    assert!(
        sheet.contains(
            "[data-slot=avatar][data-solid]{background:var(--accent);color:var(--accent-ink)}"
        ),
        "the primary button's pair, which every theme is checked for"
    );
}

#[test]
fn a_circle_is_round_whatever_the_theme_says_and_rounded_is_the_theme_corner() {
    let circle = html(Avatar::initials("Dalbir Oberoi").shape(AvatarShape::Circle));
    assert!(circle.contains(r#" data-shape="circle""#), "{circle}");
    let rounded = html(Avatar::initials("Dalbir Oberoi").shape(AvatarShape::Rounded));
    assert!(!rounded.contains("data-shape"), "{rounded}");
    assert_eq!(AvatarShape::default(), AvatarShape::Rounded);
    assert!(parts_sheet().contains("[data-slot=avatar][data-shape=circle]{border-radius:50%}"));
}

#[test]
fn an_avatar_with_no_new_option_is_drawn_and_loads_what_it_did() {
    // The example shells' pins hold these byte for byte too (`tests/shell.rs`).
    let placeholder = html(Avatar::placeholder("Anu"));
    assert!(
        placeholder.starts_with(r#"<span data-slot="avatar" role="img" aria-label="Anu"><svg "#),
        "{placeholder}"
    );
    assert_eq!(
        html(Avatar::image("/a.webp", "Anu").size(Size::Sm)),
        r#"<span data-slot="avatar" data-size="sm"><img src="/a.webp" alt="Anu" loading="lazy" width="32" height="32"></span>"#
    );
    assert!(uses(Avatar::placeholder("Anu")).is_none());
    assert!(
        uses(
            Avatar::placeholder("Anu")
                .shape(AvatarShape::Rounded)
                .tone(Tone::Neutral)
        )
        .is_none()
    );
    // Each option that draws something brings the parts stylesheet, and only it.
    for avatar in [
        Avatar::initials("Dalbir Oberoi"),
        Avatar::placeholder("Anu").tone(Tone::Info),
        Avatar::placeholder("Anu").solid(),
        Avatar::placeholder("Anu").shape(AvatarShape::Circle),
    ] {
        assert_eq!(uses(avatar), Uses::PARTS_STYLES);
    }
}

#[test]
fn the_initials_are_sized_with_the_avatar() {
    let sheet = parts_sheet();
    let at = sheet.find("[data-slot=avatar][data-initials]{").unwrap();
    let rule = &sheet[at..at + sheet[at..].find('\n').unwrap()];
    for size in [
        "font-size:12px",
        "[data-size=xs]{font-size:9px}",
        "[data-size=sm]{font-size:10px}",
        "[data-size=lg]{font-size:15px}",
    ] {
        assert!(rule.contains(size), "{size} in {rule}");
    }
    let out = html(Avatar::initials("Dalbir Oberoi").size(Size::Lg));
    assert!(out.contains(r#"data-size="lg""#), "{out}");
}

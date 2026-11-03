//! A program that names three icons and two flags holds those and no others.
//!
//! The test reads its own executable and looks for the markup of every icon and
//! flag in it. Whether the linker drops what nothing uses depends on the
//! profile, so this runs where the speed budgets do, in a release build:
//! `cargo test --release -p navree-ui --test only_what_is_named`.

use navree_ui::{Button, Flag, Row, flag, flags, icons, lucide, render};

/// The `r##"..."##` strings that follow `marker` in `text`.
fn raw_strings(text: &str, marker: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let Some(end) = rest.find("\"##") else { break };
        found.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    found
}

fn source(path: &str) -> String {
    let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(full)
        .expect("source")
        .replace("\r\n", "\n")
}

fn present(binary: &[u8], needle: &str) -> bool {
    binary
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[test]
fn a_binary_holds_the_icons_and_flags_it_names_and_no_others() {
    // What this program names. It never touches `ALL`, `Icon::lookup`,
    // `CountryCode::parse` or `Flag::country`, which would bring in the lot.
    let page = Row::gap(2).children((
        Button::primary("Add").icon(icons::PLUS),
        Button::outline("Open").icon(lucide!("mail-open")),
        Button::icon_only(icons::TRASH, "Delete"),
        flag!("gb-sct"),
        Flag::new(flags::AE),
    ));
    let html = render(&page.into());
    assert!(html.contains("Open") && html.contains("United Arab Emirates"));

    if cfg!(debug_assertions) {
        // A debug link keeps what nothing uses; the claim is about release builds.
        return;
    }

    let binary = std::fs::read(std::env::current_exe().expect("current exe")).expect("read exe");

    let named = [icons::PLUS, icons::TRASH, icons::MAIL_OPEN];
    let all_icons = raw_strings(&source("src/icons/lucide.rs"), "Icon::new(r##\"");
    assert!(all_icons.len() > 2000, "{}", all_icons.len());
    // An icon's markup can be a piece of another's: a minus is half of a plus.
    let inside_a_named_icon =
        |markup: &str| named.iter().any(|icon| icon.shapes().contains(markup));
    let mut held = Vec::new();
    for markup in &all_icons {
        if present(&binary, markup) && !inside_a_named_icon(markup) {
            held.push(markup.clone());
        }
    }
    for icon in named {
        assert!(present(&binary, icon.shapes()), "a named icon is missing");
    }
    // Components that draw icons of their own (an alert, a dialog's close button) are
    // not in this program, so their icons are not either.
    let named_markup: Vec<&str> = named.iter().map(|icon| icon.shapes()).collect();
    held.retain(|markup| !named_markup.contains(&markup.as_str()));
    assert!(
        held.is_empty(),
        "{} other icons are in the binary",
        held.len()
    );

    let flags_source = source("src/flags/data.rs");
    let art = raw_strings(&flags_source, "    r##\"");
    assert_eq!(art.len(), 270 * 2);
    let named_art = [
        flags::AE.rect_shapes(),
        flags::AE.square_shapes(),
        flags::GB_SCT.rect_shapes(),
        flags::GB_SCT.square_shapes(),
    ];
    let mut flags_held = 0;
    for markup in &art {
        let is_named = named_art.contains(&markup.as_str());
        let inside = named_art
            .iter()
            .any(|named| named.contains(markup.as_str()));
        if present(&binary, markup) && !is_named && !inside {
            flags_held += 1;
        }
    }
    assert_eq!(flags_held, 0, "other flags are in the binary");
    assert!(named_art.iter().all(|markup| present(&binary, markup)));
}

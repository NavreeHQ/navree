//! The gallery renders every component, and every page obeys the content
//! security policy (no inline scripts or styles). Set `NAVREE_UI_GALLERY_DIR`
//! to also write the pages there, with the assets, to open in a browser.

use std::collections::BTreeSet;

use navree_ui::gallery::{self, GALLERY_TEMPLATES, Links, PAGES};

/// The public macros of `ui/components.html` (helpers excepted).
fn component_macros() -> BTreeSet<String> {
    let source = navree_ui::TEMPLATES
        .iter()
        .find(|(name, _)| *name == "ui/components.html")
        .map(|(_, source)| *source)
        .unwrap();
    const HELPERS: [&str; 6] = [
        "attributes",
        "thousands",
        "field_messages",
        "field_id",
        "page_link",
        "table_cell",
    ];
    source
        .split("{%- macro ")
        .skip(1)
        .filter_map(|rest| rest.split('(').next())
        .map(str::trim)
        .filter(|name| !HELPERS.contains(name))
        .map(str::to_owned)
        .collect()
}

/// Components the gallery shows through other components rather than by
/// calling them: the shell renders the breadcrumb, menus, toasts, icons and
/// the mark; `data_table` renders sort links, row actions, pagination,
/// skeletons, empty states and action buttons; `stat` and `chart_card`
/// render sparklines, legends and charts.
const RENDERED_INSIDE_OTHERS: [&str; 14] = [
    "breadcrumb",
    "menu",
    "toast",
    "icon",
    "mark",
    "sort_link",
    "row_actions",
    "pagination",
    "skeleton",
    "empty_state",
    "action_button",
    "sparkline",
    "legend",
    "chart",
];

#[test]
fn the_gallery_uses_every_component() {
    let gallery: String = GALLERY_TEMPLATES
        .iter()
        .map(|(_, source)| *source)
        .collect();
    let components = component_macros();
    assert!(components.len() > 30, "{components:?}");
    let missing: Vec<&String> = components
        .iter()
        .filter(|name| !RENDERED_INSIDE_OTHERS.contains(&name.as_str()))
        .filter(|name| !gallery.contains(&format!("ui.{name}(")))
        .collect();
    assert!(
        missing.is_empty(),
        "components missing from the gallery: {missing:?}"
    );
}

#[test]
fn every_gallery_page_renders_without_inline_scripts_or_styles() {
    let views = gallery::views().expect("the gallery compiles");
    let links = Links::files();
    for page in PAGES {
        let html = gallery::render(&views, page.slug, "assets", &links)
            .unwrap_or_else(|error| panic!("{}: {error:#}", page.file));
        assert!(html.starts_with("<!doctype html>"), "{}", page.file);
        assert!(
            !html.contains(" style="),
            "{}: inline style attribute",
            page.file
        );
        assert!(!html.contains("<style"), "{}: inline stylesheet", page.file);
        for script in html.split("<script").skip(1) {
            assert!(
                script.starts_with(" src=\"assets/navree-ui.js\""),
                "{}: inline script <script{}",
                page.file,
                &script[..script.len().min(60)]
            );
        }
        assert!(
            !has_event_handler(&html),
            "{}: inline event handler",
            page.file
        );
        assert!(!html.contains("javascript:"), "{}", page.file);
    }
}

/// Whether `html` has an attribute such as ` onclick="`.
fn has_event_handler(html: &str) -> bool {
    html.match_indices(" on").any(|(at, _)| {
        let rest = &html[at + 3..];
        let name_len = rest.bytes().take_while(u8::is_ascii_lowercase).count();
        name_len > 0 && rest[name_len..].starts_with("=\"")
    })
}

#[test]
fn the_components_page_shows_every_kind_of_component() {
    let views = gallery::views().unwrap();
    let html = gallery::render(&views, "", "assets", &Links::files()).unwrap();
    for marker in [
        "ui-btn--primary",
        "ui-badge--success",
        "ui-status",
        "ui-stat",
        "ui-spark",
        "ui-chart__bar--accent",
        "ui-chart__line",
        "data-ui-table",
        "aria-sort=\"ascending\"",
        "data-ui-select-all",
        "ui-pagination",
        "data-ui-per-page",
        "ui-empty",
        "ui-skeleton",
        "aria-invalid=\"true\"",
        "ui-field__error",
        "ui-select",
        "ui-checkbox",
        "role=\"switch\"",
        "role=\"tablist\"",
        "ui-tabs--line",
        "<dialog class=\"ui-dialog",
        "role=\"alertdialog\"",
        "data-ui-toast-trigger",
        "id=\"ui-command\"",
        "ui-menu",
        "ui-alert--warning",
        "<progress",
        "ui-dl",
        "ui-kbd",
        "ui-avatar",
    ] {
        assert!(html.contains(marker), "the components page lacks {marker}");
    }
}

#[test]
fn writes_the_gallery_to_disk() {
    let dir = match std::env::var_os("NAVREE_UI_GALLERY_DIR") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::env::temp_dir().join(format!("navree-test-ui-gallery-{}", std::process::id())),
    };
    gallery::write(&dir).expect("the gallery is written");
    for page in PAGES {
        assert!(dir.join(page.file).is_file(), "{}", page.file);
    }
    assert!(dir.join("assets/navree-ui.css").is_file());
    assert!(dir.join("assets/fonts/geist.woff2").is_file());
    if std::env::var_os("NAVREE_UI_GALLERY_DIR").is_none() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

//! The website's UI docs are what the toolkit draws today: a change to a
//! component, an example or the toolkit's assets that isn't written out fails
//! here, as `node build.mjs --check` does for the runtime.

#[test]
fn the_websites_ui_docs_are_current() {
    let files = ui_docs::generate();
    let stale = ui_docs::stale(&files);
    assert!(
        stale.is_empty(),
        "stale in {}: {stale:?}\n{}",
        ui_docs::OUT,
        ui_docs::RERUN
    );
}

/// A preview is a static page with no server behind it: none follows an event stream, and
/// none loads the module that opens one, or the site would watch an address it doesn't have.
#[test]
fn no_preview_follows_a_stream() {
    let files = ui_docs::generate();
    let module = navree::ui::assets::LIVE_URL
        .rsplit('/')
        .next()
        .expect("the live module has a file name");
    let mut seen = 0;
    for (name, html) in files
        .iter()
        .filter(|(name, _)| name.starts_with("previews/"))
    {
        seen += 1;
        for stream in ["data-ui-live-src", "data-ui-live-event", module] {
            assert!(!html.contains(stream), "{name} has {stream}");
        }
    }
    assert!(seen > 100, "{seen} previews");
}

/// A preview links the stylesheets its parts use, as a page with those parts does: the
/// theme builder's own sheet, without which its choices are plain text.
#[test]
fn a_preview_links_every_stylesheet_its_page_uses() {
    let files = ui_docs::generate();
    let sheet = navree::ui::assets::THEME_BUILDER_URL
        .rsplit('/')
        .next()
        .expect("the builder's sheet has a file name");
    let builder = &files["previews/theme-builder/default.html"];
    assert!(
        builder.contains(&format!("<link rel=\"stylesheet\" href=\"../_toolkit/{sheet}\">")),
        "{builder}"
    );
    let button = &files["previews/button/variants.html"];
    assert!(!button.contains(sheet), "{button}");
}

/// A page is drawn in its own look: the theme builder's page in its draft's theme and
/// mode, as the app would send it.
#[test]
fn the_theme_builder_preview_is_drawn_in_its_draft() {
    let files = ui_docs::generate();
    let builder = &files["previews/theme-builder/default.html"];
    assert!(
        builder.starts_with("<!doctype html>\n<html lang=\"en\" data-theme=\"ocean\" data-mode=\"dark\">"),
        "{builder}"
    );
    assert!(
        builder.contains("<meta name=\"color-scheme\" content=\"dark\">"),
        "{builder}"
    );
    // A theme of your own links its sheet, which is written next to the toolkit's files.
    for (name, html) in files
        .iter()
        .filter(|(name, _)| name.starts_with("previews/") && name.ends_with(".html"))
    {
        for link in html.split("<link ").filter(|link| link.contains("data-ui-theme")) {
            let href = link
                .split("href=\"../")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("a theme link has an address");
            assert!(
                files.contains_key(&format!("previews/{href}")),
                "{name} links {href}, which is not written"
            );
        }
    }
}

/// A preview draws a drawer open with CSS, so its scrim stands in for the
/// dialog's `::backdrop`: a sheet asked for a light backdrop gets the toolkit's
/// own light value, and the default one the toolkit's default.
#[test]
fn a_previews_scrim_is_the_toolkits_backdrop() {
    // The theme's scrim, and part of it for the light backdrop.
    const SCRIM: &str = "var(--scrim,rgb(1 3 2/.72))";
    const LIGHT: &str = "color-mix(in srgb,var(--scrim,rgb(1 3 2/.72)) 55%,transparent)";
    let preview = include_str!("../frame/preview.css");
    let toolkit = include_str!("../../../crates/navree-ui/styles/30-overlays.css");
    for (selector, toolkit_rule) in [
        (
            format!(
                "dialog:not([open]))::after{{content:\"\";position:fixed;inset:0;z-index:40;background:{SCRIM}}}"
            ),
            format!("&::backdrop{{background:{SCRIM}}}"),
        ),
        (
            format!("dialog[data-backdrop=light]:not([open]))::after{{background:{LIGHT}}}"),
            format!("&[data-backdrop=light]::backdrop{{background:{LIGHT}}}"),
        ),
    ] {
        assert!(
            preview.contains(&selector),
            "the preview's scrim: {selector}"
        );
        assert!(
            toolkit.contains(&toolkit_rule),
            "the toolkit's backdrop: {toolkit_rule}"
        );
    }
}

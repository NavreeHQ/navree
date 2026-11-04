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

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

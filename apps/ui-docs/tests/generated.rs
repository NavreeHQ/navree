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

/// The previews' demo data is Indian: prices in rupees, and Indian people and companies,
/// never real people's names.
#[test]
fn no_preview_shows_dollars_or_western_demo_names() {
    let files = ui_docs::generate();
    for (name, html) in files
        .iter()
        .filter(|(name, _)| name.starts_with("previews/") && name.ends_with(".html"))
    {
        for word in [
            "Ada Lovelace",
            "Grace Hopper",
            "Acme",
            "Globex",
            ">Ada<",
            ">Grace<",
            ">Alan<",
            "ada@",
        ] {
            assert!(!html.contains(word), "{name} shows {word}");
        }
        let dollars = html
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0] == b'$' && pair[1].is_ascii_digit());
        assert!(!dollars, "{name} shows a price in dollars");
    }
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
        builder.contains(&format!(
            "<link rel=\"stylesheet\" href=\"../_toolkit/{sheet}\">"
        )),
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
        builder.starts_with(
            "<!doctype html>\n<html lang=\"en\" data-theme=\"ocean\" data-mode=\"dark\">"
        ),
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
        for link in html
            .split("<link ")
            .filter(|link| link.contains("data-ui-theme"))
        {
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

/// A list in a preview has no server to ask for its next page: the pages it would get are
/// drawn into the preview, for a source the preview's own list names.
#[test]
fn a_previews_lists_are_answered_from_the_preview() {
    let files = ui_docs::generate();
    let mut answered = 0;
    for (name, html) in files
        .iter()
        .filter(|(name, _)| name.starts_with("previews/") && name.ends_with(".html"))
    {
        for template in html.split("<template data-preview-answer").skip(1) {
            answered += 1;
            let src = template
                .split("data-src=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("an answer names its source");
            assert!(
                html.contains(&format!("data-ui-src=\"{src}\"")),
                "{name} answers {src}, which none of its lists asks"
            );
            assert!(
                template.contains("data-key="),
                "{name}: an answer for {src} holds no items"
            );
        }
    }
    for list in ["lazy-list/default", "lazy-list/conversation"] {
        let html = &files[&format!("previews/{list}.html")];
        assert!(html.contains("<template data-preview-answer"), "{list}");
    }
    assert!(answered >= 2, "{answered} answers");
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

/// The methods a macro writes for a family of types are in the props tables: the Select
/// family's builders (`picker_methods`, `typed_methods`) and the charts' (`common_methods`).
#[test]
fn a_props_table_lists_the_methods_a_macro_wrote() {
    let api = ui_docs::api::read(|file| {
        ui_docs::read_repo(&format!("crates/navree-ui/src/{file}"))
    });
    let names = |type_name: &str| -> Vec<String> {
        let doc = api.get(type_name).unwrap_or_else(|| panic!("no table for {type_name}"));
        doc.methods.iter().map(|method| method.name.clone()).collect()
    };
    for type_name in ["Select", "Combobox", "MultiSelect"] {
        let methods = names(type_name);
        for wanted in ["option", "options", "group", "placeholder", "required", "disabled", "size", "labels"] {
            assert!(methods.iter().any(|m| m == wanted), "{type_name} lacks {wanted}: {methods:?}");
        }
        assert!(methods.len() > 12, "{type_name}: {methods:?}");
    }
    // The two typed pickers add a search endpoint.
    for type_name in ["Combobox", "MultiSelect"] {
        let methods = names(type_name);
        assert!(methods.iter().any(|m| m == "search"), "{type_name}: {methods:?}");
    }
    assert!(!names("Select").iter().any(|m| m == "search"), "Select is not searched on the server");
    // The charts, as before.
    assert!(names("Sparkline").iter().any(|m| m == "heading"));
    // A new component is in the tables too.
    for type_name in ["ColorPicker", "Trace", "Questionnaire", "ReportTable"] {
        assert!(api.contains_key(type_name), "no table for {type_name}");
    }
}

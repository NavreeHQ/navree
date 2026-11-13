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
            "Anu Lakhotia",
            "Gauri Hundal",
            "Agra",
            "Gulati",
            ">Anu<",
            ">Gauri<",
            ">Arun<",
            "anu@",
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
    let api =
        ui_docs::api::read(|file| ui_docs::read_repo(&format!("crates/navree-ui/src/{file}")));
    let names = |type_name: &str| -> Vec<String> {
        let doc = api
            .get(type_name)
            .unwrap_or_else(|| panic!("no table for {type_name}"));
        doc.methods
            .iter()
            .map(|method| method.name.clone())
            .collect()
    };
    for type_name in ["Select", "Combobox", "MultiSelect"] {
        let methods = names(type_name);
        for wanted in [
            "option",
            "options",
            "group",
            "placeholder",
            "required",
            "disabled",
            "size",
            "labels",
        ] {
            assert!(
                methods.iter().any(|m| m == wanted),
                "{type_name} lacks {wanted}: {methods:?}"
            );
        }
        assert!(methods.len() > 12, "{type_name}: {methods:?}");
    }
    // The two typed pickers add a search endpoint.
    for type_name in ["Combobox", "MultiSelect"] {
        let methods = names(type_name);
        assert!(
            methods.iter().any(|m| m == "search"),
            "{type_name}: {methods:?}"
        );
    }
    assert!(
        !names("Select").iter().any(|m| m == "search"),
        "Select is not searched on the server"
    );
    // The charts, as before.
    assert!(names("Sparkline").iter().any(|m| m == "heading"));
    // A new component is in the tables too.
    for type_name in ["ColorPicker", "Trace", "Questionnaire", "ReportTable"] {
        assert!(api.contains_key(type_name), "no table for {type_name}");
    }
}

/// `Zone`, the band a line chart shades and a gauge lays on its track, has a props table in
/// the written data, and the two pages that use it list it.
#[test]
fn a_zone_has_a_props_table_and_the_pages_that_use_it_list_it() {
    let files = ui_docs::generate();
    let api: serde_json::Value = serde_json::from_str(
        files
            .get("api.json")
            .unwrap_or_else(|| panic!("no api.json in the generated files")),
    )
    .unwrap_or_default();
    let zone = &api["Zone"];
    assert_eq!(zone["name"], "Zone", "{zone}");
    let constructors = zone["constructors"].as_array().cloned().unwrap_or_default();
    assert!(
        constructors.iter().any(|c| c["name"] == "new"),
        "Zone::new is documented: {zone}"
    );
    let methods: Vec<&str> = zone["methods"]
        .as_array()
        .map(|m| m.iter().filter_map(|m| m["name"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        methods.contains(&"label") && methods.contains(&"ink"),
        "{methods:?}"
    );
    let catalog = &files["catalog.json"];
    let catalog: serde_json::Value = serde_json::from_str(catalog).unwrap_or_default();
    for page in ["line-chart", "gauge"] {
        let types = catalog["components"]
            .as_array()
            .and_then(|all| all.iter().find(|c| c["id"] == page))
            .map(|c| c["types"].clone())
            .unwrap_or_default();
        assert!(
            types
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t == "Zone")),
            "{page} lists Zone: {types}"
        );
    }
}

/// The parts of a conversation with an assistant have a table each, and the page of each is
/// in the catalog.
#[test]
fn the_conversations_parts_have_tables_and_pages() {
    let api =
        ui_docs::api::read(|file| ui_docs::read_repo(&format!("crates/navree-ui/src/{file}")));
    for type_name in [
        "Conversation",
        "ChatLabels",
        "ChatMessage",
        "ChatState",
        "PromptInput",
        "ToolCallCard",
        "ToolState",
        "Reasoning",
        "ApprovalCard",
        "ApprovalState",
        "SurfaceFrame",
        "FrameLabels",
        "Suggestions",
        "Sources",
        "Citation",
        "ThreadList",
    ] {
        assert!(api.contains_key(type_name), "no table for {type_name}");
    }
    for id in [
        "conversation",
        "chat-message",
        "prompt-input",
        "tool-call-card",
        "approval-card",
        "surface-frame",
        "suggestions",
        "sources",
        "thread-list",
    ] {
        assert!(
            ui_docs::catalog::COMPONENTS.iter().any(|c| c.id == id),
            "no catalog entry for {id}"
        );
    }
}

/// The phone-width shell is drawn twice: with its sidebar closed behind the button, and open
/// over the page as it is after the button is pressed.
#[test]
fn the_phone_shell_is_drawn_with_its_sidebar_closed_and_open() {
    let files = ui_docs::generate();
    let opened = "data-slot=\"sidebar-wrapper\" data-mobile-open";
    let closed = &files["previews/app-shell/phone.html"];
    let open = &files["previews/app-shell/phone-open.html"];
    assert!(!closed.contains(opened), "the closed preview is open");
    assert_eq!(
        open.matches(opened).count(),
        1,
        "the open preview is not open"
    );
    // Both show the same page.
    assert!(closed.contains("Sutlej CRM") && open.contains("Sutlej CRM"));
}

/// A docs page asks for a props table by the type's name, and the website stops building
/// when the generated tables hold no such type: every name a page asks for is written.
#[test]
fn every_props_table_a_docs_page_asks_for_is_written() {
    let files = ui_docs::generate();
    let api: serde_json::Value =
        serde_json::from_str(&files["api.json"]).expect("api.json is JSON");
    let mut pages = Vec::new();
    mdx_pages(&ui_docs::repo().join("docs"), &mut pages);
    let mut asked = 0;
    for page in pages {
        let text = std::fs::read_to_string(&page).expect("a docs page is text");
        for tag in text.split("<UiProps ").skip(1) {
            let name = tag
                .split_once("type=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(name, _)| name)
                .unwrap_or_else(|| panic!("{}: a props table with no type", page.display()));
            asked += 1;
            assert!(
                api.get(name).is_some(),
                "{} asks for the props of {name}, which no component or guide in the catalog lists",
                page.display()
            );
        }
    }
    assert!(asked > 300, "{asked} props tables");
}

/// Every `.mdx` file under `dir`, at any depth.
fn mdx_pages(dir: &std::path::Path, pages: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("the docs folder is readable") {
        let path = entry.expect("a docs entry").path();
        if path.is_dir() {
            mdx_pages(&path, pages);
        } else if path.extension().is_some_and(|ext| ext == "mdx") {
            pages.push(path);
        }
    }
}

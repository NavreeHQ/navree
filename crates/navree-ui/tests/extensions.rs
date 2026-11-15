//! UI extensions in a page and in an app: the files an element names reach the head, rows and
//! replies included; the toolkit serves contributed files and the boot checks whose they are;
//! the authoring kit and the separators. A test binary of its own: it reads the warnings the
//! boot logs.

use std::sync::{Arc, Mutex};

use http::Method;
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, PluginInfo, ProviderKey, Resolver,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_ui::table::NoFilter;
use navree_ui::{
    Button, DataTable, ExtensionFile, Input, Markdown, Node, Reply, SafeHtml, TableQuery,
    TableRow, TestView, UiExtension, UiModule, Uses, View, assets, el,
};

/// Captures formatted tracing output written while the guard is alive (on this thread: use a
/// current-thread runtime).
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(self.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.0.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(HttpModule::for_root(Default::default()))
}

/// A map with a script and a stylesheet, its route prefix and one attribute it keeps, and a
/// badge with a stylesheet only. Both are scoped for the namespace `geo`.
static MAP: UiExtension = UiExtension::new("geo", "map")
    .script(ExtensionFile::__new(
        b"export const contract = 1; export function mount() {}",
        "1a2b3c4d5e6f",
        None,
    ))
    .sheet(ExtensionFile::__new(
        br#":where([data-slot^="geo:"]){.pin{color:red}}"#,
        "0f9e8d7c6b5a",
        Some("geo"),
    ))
    .contract(1)
    .keeps(&["data-moving"])
    .routes(&["/geo/"]);
static BADGE: UiExtension = UiExtension::new("geo", "badge").sheet(ExtensionFile::__new(
    br#":where([data-slot^="geo:"]){}"#,
    "aaaaaaaaaaaa",
    Some("geo"),
));

/// The link a page writes for a stylesheet an element names.
fn sheet_link(extension: &UiExtension) -> String {
    format!(
        r#"<link rel="stylesheet" href="{}" data-ui-ext-link>"#,
        extension.sheet_url().expect("a stylesheet")
    )
}

/// The preload a page writes for a script an element names.
fn script_preload(extension: &UiExtension) -> String {
    format!(
        r#"<link rel="modulepreload" href="{}">"#,
        extension.script_url().expect("a script")
    )
}

/// Where `needle` first stands in `html`.
fn at(html: &str, needle: &str) -> usize {
    html.find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not in {html}"))
}

/// Drivers of a delivery company in Ludhiana, drawn as a table.
#[derive(Clone, TableRow)]
#[table(sort = "title", per = 25)]
struct Driver {
    #[col(skip)]
    id: i64,
    #[col(sortable, label = "Driver")]
    title: String,
    #[col(label = "City")]
    city: String,
}

fn driver(id: i64, title: &str, city: &str) -> Driver {
    Driver {
        id,
        title: title.to_owned(),
        city: city.to_owned(),
    }
}

fn drivers() -> Vec<Driver> {
    vec![
        driver(1, "Harpreet Sandhu", "Ludhiana"),
        driver(2, "Meera Sharma", "Jalandhar"),
        driver(3, "Simran Kaur", "Amritsar"),
    ]
}

fn query(raw: &str) -> TableQuery<Driver, NoFilter> {
    TableQuery::parse("/drivers", raw)
}

#[test]
fn a_page_links_the_sheets_and_preloads_the_scripts_its_elements_name_once_each() {
    let page = View::new("Deliveries").body((
        el("div").slot("geo:map").extension(&MAP),
        el("div").slot("geo:map").extension(&MAP),
        el("span").slot("geo:badge").extension(&BADGE),
    ));
    let html = page.to_html();
    for link in [sheet_link(&MAP), sheet_link(&BADGE), script_preload(&MAP)] {
        assert_eq!(html.matches(&link).count(), 1, "{link} in {html}");
    }
    // The sheets come after the toolkit's own and before the icon; the preload, after the icon
    // and before the first script.
    let toolkit = format!(
        r#"<link rel="stylesheet" href="{}">"#,
        assets::NAVREE_UI_URL
    );
    let icon = r#"<link rel="icon""#;
    for sheet in [sheet_link(&MAP), sheet_link(&BADGE)] {
        assert!(at(&html, &toolkit) < at(&html, &sheet), "{html}");
        assert!(at(&html, &sheet) < at(&html, icon), "{html}");
    }
    assert!(at(&html, icon) < at(&html, &script_preload(&MAP)), "{html}");
    assert!(at(&html, &script_preload(&MAP)) < at(&html, "<script"), "{html}");
    // A sheet-only extension is not preloaded.
    assert_eq!(html.matches("modulepreload").count(), 1, "{html}");
}

#[test]
fn a_sheet_named_inside_a_table_row_is_linked_in_the_head() {
    let rows = drivers();
    let page = View::new("Drivers").body(
        DataTable::new("drivers", &query(""), &rows)
            .cell("city", |_| el("span").slot("geo:badge").extension(&BADGE)),
    );
    let html = page.to_html();
    assert_eq!(html.matches(&sheet_link(&BADGE)).count(), 1, "{html}");
    assert!(!html.contains(&sheet_link(&MAP)), "{html}");
}

#[test]
fn a_sheet_named_in_a_detail_row_is_linked_in_the_head() {
    let rows = drivers();
    let page = |raw: &str| {
        View::new("Drivers")
            .body(
                DataTable::new("drivers", &query(raw), &rows)
                    .detail(|_| el("span").slot("geo:badge").extension(&BADGE).into()),
            )
            .to_html()
    };
    // Only the rows that are open are drawn, so only an open row names its sheet.
    let open = page("open=1");
    assert_eq!(open.matches(&sheet_link(&BADGE)).count(), 1, "{open}");
    assert!(
        open.contains(BADGE.sheet_url().expect("a stylesheet")),
        "{open}"
    );
    let closed = page("");
    assert!(!closed.contains("data-ui-ext-link"), "{closed}");
}

#[test]
fn a_sheet_value_outside_the_grammar_is_not_linked() {
    let page = View::new("Deliveries").body((
        el("div")
            .uses(Uses::EXTENSION_SHEETS)
            .attr("data-ui-ext-sheet", "https://cdn.example/x.css"),
        el("div")
            .uses(Uses::EXTENSION_SHEETS)
            .attr("data-ui-ext-sheet", "/_navree/ui/ext/geo.map-0f9e8d7c6b5a.css?v=2"),
        el("div")
            .uses(Uses::EXTENSION_SHEETS)
            .attr("data-ui-ext-sheet", "/_navree/ui/ext/../app/x.css"),
        // A row string written by a cell, naming an island's script as if it were an extension's.
        el("div")
            .uses(Uses::EXTENSIONS)
            .child(Node::Raw(SafeHtml::trusted(
                r#"<b data-ui-ext="/_navree/ui/island/clock-0123456789ab.js"></b>"#,
            ))),
    ));
    let html = page.to_html();
    assert!(!html.contains("data-ui-ext-link"), "{html}");
    assert!(!html.contains("modulepreload"), "{html}");
}

#[test]
fn a_page_with_no_extension_is_written_as_before() {
    let rows = drivers();
    let page = View::new("Deliveries").body((
        el("form")
            .attr("method", "post")
            .attr("action", "/deliveries")
            .child((Input::text("city"), Button::primary("Save").submit())),
        DataTable::new("drivers", &query(""), &rows),
        Markdown::new("Harpreet Sandhu delivers in **Ludhiana**."),
    ));
    let html = page.to_html();
    for absent in [
        "/_navree/ui/ext/",
        "data-ui-ext",
        "modulepreload",
        "data-ui-fresh",
    ] {
        assert!(!html.contains(absent), "`{absent}` in {html}");
    }
    // The scripts are the toolkit's modules for what the page holds, in their order, and no more.
    let scripts: String = assets::scripts(page.uses())
        .map(|url| format!(r#"<script type="module" src="{url}"></script>"#))
        .collect();
    assert!(!scripts.is_empty(), "the page needs the runtime");
    assert!(html.contains(&scripts), "{html}");
    assert_eq!(
        html.matches("<script").count(),
        assets::scripts(page.uses()).count(),
        "{html}"
    );
}

#[test]
fn markdown_and_cleaned_rich_text_carry_no_extension_and_link_nothing() {
    // Markdown is stored text: whatever it says of an extension is text, and no page acts on it.
    let page = View::new("Notes").body(Markdown::new(
        r#"<div data-ui-ext="/_navree/ui/ext/geo.map-1a2b3c4d5e6f.js" data-ui-ext-sheet="/_navree/ui/ext/geo.map-0f9e8d7c6b5a.css">Ludhiana</div>"#,
    ));
    let html = page.to_html();
    assert!(!html.contains("data-ui-ext-link"), "{html}");
    assert!(!html.contains("modulepreload"), "{html}");
    assert!(!html.contains("data-ui-ext=\""), "{html}");
    assert!(!html.contains("data-ui-ext-sheet=\""), "{html}");
    assert!(
        !page
            .uses()
            .meets(Uses::EXTENSIONS.with(Uses::EXTENSION_SHEETS))
    );
    assert!(!TestView::of(&page).uses_extension(&MAP));

    // Rich text is cleaned by a policy that drops every `data-ui-` attribute, and no policy can
    // be written to keep one of an extension's four (the sanitizer's own tests clean markup).
    for attribute in [
        "data-ui-ext",
        "data-ui-ext-sheet",
        "data-ui-ext-routes",
        "data-ui-props",
    ] {
        assert!(
            navree_validation::HtmlPolicy::PAGE_ATTRIBUTE_PREFIXES
                .iter()
                .any(|prefix| attribute.starts_with(prefix)),
            "{attribute}"
        );
        let policy = navree_validation::HtmlPolicy {
            generic_attributes: [attribute.to_owned()].into_iter().collect(),
            ..navree_validation::HtmlPolicy::default()
        };
        assert!(policy.check().is_err(), "{attribute}");
    }
}

/// The pages of the replies below.
struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "!/pin", || async {
                Reply::ops(Vec::new()).morph("pin", el("div").slot("geo:map").extension(&MAP))
            })
            .public(),
            RouteDef::on(Method::POST, "!/badge", || async {
                Reply::ops(Vec::new()).morph("badge", el("span").slot("geo:badge").extension(&BADGE))
            })
            .public(),
        ]
    }
}

struct PagesModule;

impl Module for PagesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<Pages>();
    }
}

/// The module files a reply to the runtime lists.
async fn modules_of(path: &str) -> Vec<String> {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder().module(UiModule).module(PagesModule).build().await;
    let response = app
        .client()
        .post(path)
        .header("x-navree-ui", "1")
        .send()
        .await
        .assert_status(200);
    response.json_value()["modules"]
        .as_array()
        .expect("the reply lists its module files")
        .iter()
        .filter_map(|module| module.as_str().map(str::to_owned))
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn a_reply_that_morphs_in_an_extension_brings_the_extensions_module() {
    let modules = modules_of("/pin").await;
    assert!(modules.iter().any(|url| url == assets::CORE_URL), "{modules:?}");
    assert!(
        modules.iter().any(|url| url == assets::EXTENSIONS_URL),
        "{modules:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_reply_that_morphs_in_a_sheet_only_extension_brings_the_extensions_module() {
    let modules = modules_of("/badge").await;
    assert!(
        modules.iter().any(|url| url == assets::EXTENSIONS_URL),
        "{modules:?}"
    );
}

#[test]
fn test_view_says_which_extensions_a_page_uses_including_in_table_rows() {
    let rows = drivers();
    let table = View::new("Drivers").body(
        DataTable::new("drivers", &query(""), &rows)
            .cell("city", |_| el("span").slot("geo:badge").extension(&BADGE)),
    );
    let table = TestView::of(&table);
    assert!(table.uses_extension(&BADGE));
    assert!(!table.uses_extension(&MAP));

    let direct = View::new("Map").body(el("div").slot("geo:map").extension(&MAP));
    let direct = TestView::of(&direct);
    assert!(direct.uses_extension(&MAP));
    assert!(!direct.uses_extension(&BADGE));

    let none = TestView::of(&View::new("Plain").body(el("p").child("Ludhiana")));
    assert!(!none.uses_extension(&MAP) && !none.uses_extension(&BADGE));
}

/// The plugin that ships the map: its namespace is `geo`.
fn geo() -> PluginInfo {
    PluginInfo::new(
        "navree-plugin-geo",
        env!("CARGO_CRATE_NAME"),
        "geo",
        "1.0.0",
    )
}

/// An extension of another namespace, one of the app's own, a second map with other files, and
/// extensions whose declarations the boot refuses.
static PIN: UiExtension = UiExtension::new("maps", "pin").sheet(ExtensionFile::__new(
    b"/* scoped for maps */",
    "bbbbbbbbbbbb",
    Some("maps"),
));
static RATING: UiExtension = UiExtension::new("app", "rating").sheet(ExtensionFile::__new(
    b"/* scoped for app */",
    "cccccccccccc",
    Some("app"),
));
static OTHER_MAP: UiExtension = UiExtension::new("geo", "map")
    .script(ExtensionFile::__new(
        b"export const contract = 1; /* another map */",
        "eeeeeeeeeeee",
        None,
    ))
    .contract(1);
static TOUR: UiExtension = UiExtension::new("geo", "tour").script(ExtensionFile::__new(
    b"export const contract = 1;",
    "dddddddddddd",
    None,
));
static NEWER_TOUR: UiExtension = UiExtension::new("geo", "tour")
    .script(ExtensionFile::__new(
        b"export const contract = 2;",
        "dddddddddddd",
        None,
    ))
    .contract(2);
static EMPTY: UiExtension = UiExtension::new("geo", "empty");

/// A module of the plugin `geo` that contributes extensions.
macro_rules! plugin_module {
    ($name:ident, $($extension:expr),+) => {
        struct $name;

        impl Module for $name {
            fn register(&self, m: &mut ModuleBuilder) {
                m.plugin(geo());
                $(m.contribute($extension);)+
            }
        }
    };
}

/// A module that names no plugin and contributes extensions.
macro_rules! app_module {
    ($name:ident, $($extension:expr),+) => {
        struct $name;

        impl Module for $name {
            fn register(&self, m: &mut ModuleBuilder) {
                $(m.contribute($extension);)+
            }
        }
    };
}

plugin_module!(GeoModule, MAP, BADGE);
plugin_module!(MapsModule, PIN);
plugin_module!(GeoMapModule, MAP);
plugin_module!(NoContractModule, TOUR);
plugin_module!(NewerContractModule, NEWER_TOUR);
plugin_module!(EmptyModule, EMPTY);
plugin_module!(BadgeModule, BADGE);
app_module!(Shop, BADGE);
app_module!(ShopOwn, RATING);
app_module!(FirstMapModule, MAP);
app_module!(SecondMapModule, OTHER_MAP);
app_module!(SameMapModule, MAP);

/// What the log says, one line each, with every `WARN` and `INFO` line that is about the files.
fn said_about_extensions(capture: &LogCapture) -> Vec<String> {
    capture
        .lines()
        .into_iter()
        .filter(|line| line.contains("UI extension"))
        .collect()
}

/// Asks `app` for `path` and checks it is not found.
async fn not_found(app: &TestApp, path: &str) {
    app.client().get(path).send().await.assert_status(404);
}

#[tokio::test(flavor = "current_thread")]
async fn the_extension_route_serves_current_hashes_immutable_and_old_ones_uncached() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder().module(UiModule).module(GeoModule).build().await;
    let script = MAP.script_url().expect("a script");
    let sheet = MAP.sheet_url().expect("a stylesheet");

    let served = app.client().get(script).send().await.assert_status(200);
    assert_eq!(
        served.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(
        served.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(
        &served.bytes()[..],
        MAP.script_file().expect("a script").bytes()
    );
    let packed = app
        .client()
        .get(script)
        .header("accept-encoding", "br")
        .send()
        .await
        .assert_status(200);
    assert_eq!(packed.header("content-encoding"), Some("br"));

    let served = app.client().get(sheet).send().await.assert_status(200);
    assert_eq!(
        served.header("content-type"),
        Some("text/css; charset=utf-8")
    );
    assert_eq!(
        served.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(
        &served.bytes()[..],
        MAP.sheet_file().expect("a stylesheet").bytes()
    );

    // An older hash still gets the current file, uncached, so a page open across a deploy works.
    let old = app
        .client()
        .get("/_navree/ui/ext/geo.map-000000000000.js")
        .send()
        .await
        .assert_status(200);
    assert_eq!(old.header("cache-control"), Some("no-cache"));
    assert_eq!(
        &old.bytes()[..],
        MAP.script_file().expect("a script").bytes()
    );

    // A client that holds the current file is told so.
    app.client()
        .get(script)
        .header("if-none-match", "\"1a2b3c4d5e6f\"")
        .send()
        .await
        .assert_status(304);

    let routes = navree_http::route_table(app.ctx())
        .await
        .expect("the route table");
    let route = routes
        .iter()
        .find(|route| route.name.as_deref() == Some("ui.extensions"))
        .expect("the extension route");
    assert!(route.access.public);
    assert_eq!(
        route.api.as_ref().and_then(|api| api.summary.as_deref()),
        Some("A UI extension's script or stylesheet")
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_with_no_extension_has_no_extension_route() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder().module(UiModule).module(PagesModule).build().await;
    let routes = navree_http::route_table(app.ctx())
        .await
        .expect("the route table");
    assert!(
        routes
            .iter()
            .all(|route| route.name.as_deref() != Some("ui.extensions"))
    );
    app.client()
        .get(MAP.script_url().expect("a script"))
        .send()
        .await
        .assert_status(404);
    assert!(said_about_extensions(&capture).is_empty());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn an_unknown_extension_file_is_warned_only_for_the_apps_namespaces_and_at_most_64_times() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder().module(UiModule).module(GeoModule).build().await;
    let warned = || {
        capture
            .lines()
            .into_iter()
            .filter(|line| line.contains("was asked for"))
            .collect::<Vec<_>>()
    };

    // A name in a namespace of the plugin: once, however often it is asked for.
    for _ in 0..3 {
        not_found(&app, "/_navree/ui/ext/geo.pin-0123456789ab.js").await;
    }
    let lines = warned();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("WARN")
            && lines[0].contains(
                "UI extension file `geo.pin-0123456789ab.js` was asked for, but `geo` has no extension of that name: contribute it with m.contribute(..)"
            ),
        "{}",
        lines[0]
    );
    // A name in the app's own namespace warns too; another namespace, a name of no shape and a
    // name that does have an extension (of the other kind of file) do not.
    not_found(&app, "/_navree/ui/ext/app.x-0123456789ab.css").await;
    assert_eq!(warned().len(), 2);
    for silent in [
        "/_navree/ui/ext/other.map-0123456789ab.js",
        "/_navree/ui/ext/geo.map.js",
        "/_navree/ui/ext/geo.map-0123456789ab.mjs",
        "/_navree/ui/ext/geo.badge-0123456789ab.js",
        "/_navree/ui/ext/Geo.pin-0123456789ab.js",
    ] {
        not_found(&app, silent).await;
    }
    assert_eq!(warned().len(), 2, "{:?}", warned());
    // Anyone can ask for any name: the log holds 64 of them and no more.
    for number in 0..70 {
        not_found(&app, &format!("/_navree/ui/ext/geo.name{number}-0123456789ab.js")).await;
    }
    assert_eq!(warned().len(), 64);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_extension_outside_its_namespace_fails_the_boot() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let Err(error) = builder()
        .module(UiModule)
        .module(MapsModule)
        .try_build()
        .await
    else {
        panic!("a plugin's extension is under its plugin's namespace")
    };
    assert!(
        error.to_string().contains(
            "plugin `navree-plugin-geo` contributes the UI extension `maps.pin`, which is not under its namespace: build it with UiExtension::new(\"geo\", \"pin\")"
        ),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_module_taking_a_plugins_namespace_for_an_extension_fails_the_boot() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let Err(error) = builder()
        .module(UiModule)
        .module(GeoMapModule)
        .module(Shop)
        .try_build()
        .await
    else {
        panic!("an app's own extension is under a namespace no plugin has")
    };
    assert!(
        error.to_string().contains(
            "module `Shop` contributes the UI extension `geo.badge` under `geo`, the namespace of plugin `navree-plugin-geo`; an app's own extensions use a namespace no plugin has, such as app"
        ),
        "{error}"
    );

    // The same module with an extension under `app` boots, and its file is served.
    let app = builder()
        .module(UiModule)
        .module(GeoMapModule)
        .module(ShopOwn)
        .build()
        .await;
    app.client()
        .get(RATING.sheet_url().expect("a stylesheet"))
        .send()
        .await
        .assert_status(200);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn two_modules_contributing_one_extension_name_fail_the_boot() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let Err(error) = builder()
        .module(UiModule)
        .module(FirstMapModule)
        .module(SecondMapModule)
        .try_build()
        .await
    else {
        panic!("two different files can not share one name")
    };
    let error = error.to_string();
    for part in [
        "UI extension `geo.map` is contributed by both `",
        "FirstMapModule",
        "SecondMapModule",
        "` with different files; give one another name",
    ] {
        assert!(error.contains(part), "{part}: {error}");
    }

    // The same files from both are one extension: it boots, is served and is logged once.
    let app = builder()
        .module(UiModule)
        .module(FirstMapModule)
        .module(SameMapModule)
        .build()
        .await;
    app.client()
        .get(MAP.script_url().expect("a script"))
        .send()
        .await
        .assert_status(200);
    let said = said_about_extensions(&capture);
    assert_eq!(
        said.iter()
            .filter(|line| line.contains("UI extension geo.map "))
            .count(),
        1,
        "{said:?}"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_script_extension_without_a_contract_or_with_an_unknown_one_fails_the_boot() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let Err(error) = builder()
        .module(UiModule)
        .module(NoContractModule)
        .try_build()
        .await
    else {
        panic!("a script names its contract")
    };
    assert!(
        error.to_string().contains(
            "UI extension `geo.tour` has a script but declares no contract: add .contract(1)"
        ),
        "{error}"
    );

    let Err(error) = builder()
        .module(UiModule)
        .module(NewerContractModule)
        .try_build()
        .await
    else {
        panic!("a contract this toolkit does not run stops the boot")
    };
    assert!(
        error.to_string().contains(
            "UI extension `geo.tour` is written for extension contract 2, and this navree-ui runs contract 1: update navree, or use a release of `navree-plugin-geo` written for contract 1"
        ),
        "{error}"
    );

    let Err(error) = builder()
        .module(UiModule)
        .module(EmptyModule)
        .try_build()
        .await
    else {
        panic!("an extension has a file")
    };
    assert!(
        error
            .to_string()
            .contains("UI extension `geo.empty` has no script and no stylesheet"),
        "{error}"
    );

    // A stylesheet alone needs no contract.
    let app = builder()
        .module(UiModule)
        .module(BadgeModule)
        .build()
        .await;
    app.client()
        .get(BADGE.sheet_url().expect("a stylesheet"))
        .send()
        .await
        .assert_status(200);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_booted_app_logs_each_extension_once_with_the_size_of_its_files() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder().module(UiModule).module(GeoModule).build().await;
    let said = said_about_extensions(&capture);
    assert!(
        said.iter().any(|line| line.contains("INFO")
            && line.contains(
                "UI extension geo.map (plugin `navree-plugin-geo`): script 1 KB, stylesheet 1 KB, contract 1"
            )),
        "{said:?}"
    );
    assert!(
        said.iter().any(|line| line.contains(
            "UI extension geo.badge (plugin `navree-plugin-geo`): stylesheet 1 KB"
        )),
        "{said:?}"
    );
    app.shutdown().await.unwrap();
}

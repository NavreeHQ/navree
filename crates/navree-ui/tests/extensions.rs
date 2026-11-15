//! UI extensions in a page and in an app: the files an element names reach the head, rows and
//! replies included; the toolkit serves contributed files and the boot checks whose they are;
//! the authoring kit and the separators. A test binary of its own: it reads the warnings the
//! boot logs.

use std::sync::{Arc, Mutex};

use http::Method;
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
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

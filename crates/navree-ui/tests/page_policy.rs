//! The page policy: nothing allowed until the app's configuration says so, one policy for every
//! page, and pages that start clean. A test binary of its own: it reads the warnings and lines
//! the boot logs.

use std::sync::{Arc, Mutex};

use http::Method;
use http::header::CONTENT_SECURITY_POLICY;
use navree_config::{Config, ConfigSection, ConfigSources, FRAMEWORK_SECTIONS};
use navree_http::{AppError, Controller, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, PluginInfo, ProviderKey, Resolver,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_ui::app_files::IMAGE_CSP;
use navree_ui::pages::region_answer;
use navree_ui::{
    AppImage, Directive, LazyItem, LazyPage, PAGE_CSP, PagePolicy, PagePolicyOptions, PageSources,
    Text, UiModule, View, el,
};
use serde_json::json;

/// Captures formatted tracing output written while the guard is alive (on this thread: use a
/// current-thread runtime).
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
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

/// A browser's `Accept`.
const BROWSER: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

/// The policy with the app's tile host allowed, as the toolkit composes it.
const WITH_TILES: &str = "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data: https://tile.openstreetmap.org; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// A delivery company's logo, served by the toolkit under a policy of its own.
static MARK: AppImage = AppImage::new(
    "mark",
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><circle cx="16" cy="16" r="14"/></svg>"#,
    8 * 1024,
);

/// An app with the toolkit and the pages below.
fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(PagesModule)
}

/// The same, with the tile host of the plugin `geo` allowed.
fn with_tiles() -> TestAppBuilder {
    builder()
        .module(Geo)
        .config(
            "ui.policy.allow.geo.img_src",
            json!(["https://tile.openstreetmap.org"]),
        )
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

/// What the plugin `geo` asks for: the host of its map tiles.
fn tiles() -> PageSources {
    PageSources::new().ask(
        Directive::ImgSrc,
        "https://tile.openstreetmap.org",
        "map tiles",
    )
}

/// The plugin `geo`, asking for its tile host.
struct Geo;

impl Module for Geo {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(geo()).contribute(tiles());
    }
}

/// The plugin `geo`, asking for its tile host and for WebAssembly.
struct GeoWithScanner;

impl Module for GeoWithScanner {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(geo()).contribute(tiles().wasm("barcode decoder"));
    }
}

/// The plugin `geo`, asking for a source outside the rules.
struct GeoAskingForAnything;

impl Module for GeoAskingForAnything {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(geo())
            .contribute(PageSources::new().ask(Directive::ImgSrc, "*", "map tiles"));
    }
}

/// A module of the app that serves a picture of its own.
struct LogoModule;

impl Module for LogoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(MARK);
    }
}

/// The pages and answers below.
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
            RouteDef::on(Method::GET, "!/deliveries", || async {
                View::new("Deliveries")
                    .body(el("main").child("Harpreet Sandhu delivers in Ludhiana"))
            })
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/fragments/zones", || async {
                region_answer(
                    el("div")
                        .id("zones")
                        .child("Model Town and Sarabha Nagar"),
                )
            })
            .public(),
            RouteDef::on(Method::GET, "!/rows", || async {
                let rows = [LazyItem::new("1", "0001", Text::new("Meera Sharma"))];
                LazyPage::new(rows, false)
            })
            .public(),
            RouteDef::on(Method::GET, "!/missing", || async {
                Err::<View, _>(AppError::not_found())
            })
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/own-policy", || async {
                (
                    [(CONTENT_SECURITY_POLICY, "default-src 'none'")],
                    "no page",
                )
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

/// The policy `path` is answered with when a browser asks, with the headers the runtime adds.
async fn policy_with(app: &TestApp, path: &str, headers: &[(&str, &str)]) -> String {
    let mut request = app.client().get(path).header("accept", BROWSER);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request.send().await;
    response
        .header("content-security-policy")
        .unwrap_or_else(|| panic!("`{path}` carries no policy"))
        .to_owned()
}

/// The policy `path` is answered with when a browser asks.
async fn policy_of(app: &TestApp, path: &str) -> String {
    policy_with(app, path, &[]).await
}

/// The error a boot ends in.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the boot should have failed"),
        Err(error) => error.to_string(),
    }
}

/// Whether the log holds a line of `level` with `text` in it.
fn logged(capture: &LogCapture, level: &str, text: &str) -> bool {
    capture
        .lines()
        .iter()
        .any(|line| line.contains(level) && line.contains(text))
}

#[tokio::test(flavor = "current_thread")]
async fn with_nothing_allowed_every_response_keeps_the_toolkits_policy_byte_for_byte() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    // An app with no module that asks, and an app whose plugin asks for a source while the
    // configuration allows nothing: every page, fragment and error page of both carries the
    // toolkit's policy, and the first says nothing about the page policy in its log.
    let plain = builder().build().await;
    let lines = capture.lines();
    assert!(
        lines.iter().all(|line| !line.contains("page policy")),
        "{lines:?}"
    );
    let asking = builder().module(Geo).build().await;
    for app in [plain, asking] {
        for path in ["/deliveries", "/fragments/zones", "/rows", "/missing"] {
            assert_eq!(policy_of(&app, path).await, PAGE_CSP, "{path}");
        }
        let region = policy_with(
            &app,
            "/fragments/zones",
            &[("x-navree-ui", "1"), ("x-navree-region", "zones")],
        )
        .await;
        assert_eq!(region, PAGE_CSP);
        let policy = app.get::<PagePolicy>().await;
        assert!(policy.is_the_toolkits());
        assert_eq!(policy.header(), PAGE_CSP);
        app.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn allowed_sources_reach_every_page_region_fragment_and_error_page() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = with_tiles().build().await;
    for path in ["/deliveries", "/fragments/zones", "/rows", "/missing"] {
        assert_eq!(policy_of(&app, path).await, WITH_TILES, "{path}");
    }
    let region = policy_with(
        &app,
        "/fragments/zones",
        &[("x-navree-ui", "1"), ("x-navree-region", "zones")],
    )
    .await;
    assert_eq!(region, WITH_TILES);
    let policy = app.get::<PagePolicy>().await;
    assert!(!policy.is_the_toolkits());
    assert_eq!(policy.header(), WITH_TILES);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_response_with_its_own_policy_is_left_alone() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = with_tiles().module(LogoModule).build().await;
    // The picture keeps the policy that runs nothing, and a route that writes a policy of its
    // own keeps it, while the app's pages carry the composed one.
    let logo = app.client().get(&MARK.url()).send().await.assert_status(200);
    assert_eq!(logo.header("content-security-policy"), Some(IMAGE_CSP));
    assert_eq!(policy_of(&app, "/own-policy").await, "default-src 'none'");
    assert_eq!(policy_of(&app, "/deliveries").await, WITH_TILES);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_source_outside_the_grammar_fails_the_boot_naming_the_key() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let forms = "'self', data:, blob: or an https origin such as https://tile.openstreetmap.org";

    // A path, a key that is neither a namespace nor app, and more than 32 sources.
    let error = boot_error(
        builder()
            .module(Geo)
            .config(
                "ui.policy.allow.geo.img_src",
                json!(["https://tiles.example/a.png"]),
            ),
    )
    .await;
    assert!(
        error.contains(&format!(
            "ui.policy.allow.geo.img_src[0] is not a source the page policy takes for img_src: {forms}"
        )),
        "{error}"
    );
    assert!(!error.contains("tiles.example/a.png"), "{error}");

    let error = boot_error(
        builder()
            .module(Geo)
            .config("ui.policy.allow.Geo.img_src", json!(["https://tiles.example"])),
    )
    .await;
    assert!(
        error.contains("ui.policy.allow.Geo: `Geo` is neither a namespace nor app"),
        "{error}"
    );

    let many: Vec<String> = (0..33).map(|n| format!("https://t{n}.tiles.example")).collect();
    let error = boot_error(
        builder()
            .module(Geo)
            .config("ui.policy.allow.geo.img_src", json!(many)),
    )
    .await;
    assert!(
        error.contains("ui.policy.allow.geo.img_src lists more than 32 sources"),
        "{error}"
    );
    let few: Vec<String> = (0..32).map(|n| format!("https://t{n}.tiles.example")).collect();
    builder()
        .module(Geo)
        .config("ui.policy.allow.geo.img_src", json!(few))
        .build()
        .await
        .shutdown()
        .await
        .unwrap();

    // A plugin that asks for a source outside the rules is named.
    let error = boot_error(builder().module(GeoAskingForAnything)).await;
    assert!(
        error.contains(&format!(
            "plugin `navree-plugin-geo` asks for img-src `*`, which is not a source the page policy takes ({forms})"
        )),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn script_style_worker_and_frame_sources_cannot_be_allowed() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    // The four kinds of source a configuration can allow, and no more.
    assert_eq!(
        Directive::ALL.to_vec(),
        vec![
            Directive::ImgSrc,
            Directive::MediaSrc,
            Directive::ConnectSrc,
            Directive::FontSrc
        ]
    );
    for key in [
        "script_src",
        "style_src",
        "worker_src",
        "frame_src",
        "object_src",
        "form_action",
        "base_uri",
        "frame_ancestors",
        "wasm",
    ] {
        let value = if key == "wasm" {
            json!(true)
        } else {
            json!(["'self'"])
        };
        let error = boot_error(
            builder()
                .module(Geo)
                .config(&format!("ui.policy.allow.geo.{key}"), value),
        )
        .await;
        assert!(
            error.contains(&format!(
                "ui.policy.allow.geo: unknown key `{key}` (known: img_src, media_src, connect_src, font_src); a page's scripts, styles, workers and frames come from this app only"
            )),
            "{key}: {error}"
        );
    }
    // A keyword that unlocks code is no source of any directive either.
    let error = boot_error(
        builder()
            .module(Geo)
            .config("ui.policy.allow.geo.connect_src", json!(["'unsafe-eval'"])),
    )
    .await;
    assert!(
        error.contains("ui.policy.allow.geo.connect_src[0] is not a source the page policy takes for connect_src: 'self', an https origin or a wss origin"),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn localhost_is_refused_in_production_and_staging() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let local = || {
        builder()
            .module(Geo)
            .config("ui.policy.allow.geo.img_src", json!(["http://localhost:8090"]))
    };
    for environment in ["production", "staging"] {
        let error = boot_error(local().config("app.env", environment)).await;
        assert!(
            error.contains("ui.policy.allow.geo.img_src[0] points at this machine (localhost), which is refused in production and staging"),
            "{environment}: {error}"
        );
    }
    let app = local().config("app.env", "local").build().await;
    assert!(
        policy_of(&app, "/deliveries")
            .await
            .contains("img-src 'self' data: http://localhost:8090;")
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn every_wildcard_source_is_warned_about_by_name() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder()
        .module(Geo)
        .config(
            "ui.policy.allow.geo.img_src",
            json!(["https://*.tiles.example", "https://*.r2.dev"]),
        )
        .build()
        .await;
    for source in ["https://*.tiles.example", "https://*.r2.dev"] {
        let text = format!(
            "ui.policy.allow.geo.img_src allows the wildcard {source}: anyone who can hold a name under it can receive what a page sends there"
        );
        let said = capture
            .lines()
            .iter()
            .filter(|line| line.contains("WARN") && line.contains(&text))
            .count();
        assert_eq!(said, 1, "{source}: {:?}", capture.lines());
    }
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn wasm_is_one_app_switch_that_adds_wasm_unsafe_eval_and_nothing_else() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let with_wasm = PAGE_CSP.replacen(
        "script-src 'self';",
        "script-src 'self' 'wasm-unsafe-eval';",
        1,
    );
    assert_ne!(with_wasm, PAGE_CSP);

    // Nobody asks for it: one switch, and the info line says so.
    let app = builder().config("ui.policy.wasm", true).build().await;
    assert_eq!(policy_of(&app, "/deliveries").await, with_wasm);
    assert_eq!(policy_of(&app, "/missing").await, with_wasm);
    assert!(app.get::<PagePolicy>().await.allows_wasm());
    assert!(logged(
        &capture,
        "INFO",
        "page policy: WebAssembly is allowed for every script on every page (ui.policy.wasm); asked for by: none"
    ));
    app.shutdown().await.unwrap();

    // A plugin asks for it, and the line names the plugin's namespace.
    let app = builder()
        .module(GeoWithScanner)
        .config("ui.policy.wasm", true)
        .build()
        .await;
    assert_eq!(policy_of(&app, "/deliveries").await, with_wasm);
    assert!(logged(
        &capture,
        "INFO",
        "page policy: WebAssembly is allowed for every script on every page (ui.policy.wasm); asked for by: geo"
    ));
    app.shutdown().await.unwrap();

    // Asking changes nothing: the page keeps the toolkit's policy and the boot warns.
    let app = builder().module(GeoWithScanner).build().await;
    assert_eq!(policy_of(&app, "/deliveries").await, PAGE_CSP);
    assert!(!app.get::<PagePolicy>().await.allows_wasm());
    assert!(logged(
        &capture,
        "WARN",
        "page policy: plugin `geo` asks for WebAssembly (barcode decoder), which the app does not allow (allow it with ui.policy.wasm = true, which opens it for every script)"
    ));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn the_boot_says_what_each_plugin_asked_for_and_what_the_app_allowed() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    // Asked and not allowed: a warning that says how to allow it, and the policy is unchanged.
    let app = builder().module(Geo).build().await;
    assert!(logged(
        &capture,
        "WARN",
        "page policy: plugin `geo` asks for img-src https://tile.openstreetmap.org (map tiles), which the app does not allow; what needs it shows its fallback (allow it with ui.policy.allow.geo.img_src)"
    ));
    assert!(!logged(&capture, "INFO", "every page but the fresh ones is served with"));
    app.shutdown().await.unwrap();

    // Asked and allowed: an info line for the ask, and one for the header the pages carry.
    let app = with_tiles().build().await;
    assert!(logged(
        &capture,
        "INFO",
        "page policy: plugin `geo` asks for img-src https://tile.openstreetmap.org (map tiles): allowed"
    ));
    assert!(logged(
        &capture,
        "INFO",
        &format!("page policy: every page but the fresh ones is served with: {WITH_TILES}")
    ));
    assert!(!logged(&capture, "WARN", "which plugin `geo` does not ask for"));
    app.shutdown().await.unwrap();

    // Allowed and not asked: a warning.
    let app = builder()
        .module(Geo)
        .config(
            "ui.policy.allow.geo.img_src",
            json!(["https://tile.openstreetmap.org", "https://cdn.ludhiana.example"]),
        )
        .build()
        .await;
    assert!(logged(
        &capture,
        "WARN",
        "page policy: ui.policy.allow.geo allows img-src https://cdn.ludhiana.example, which plugin `geo` does not ask for"
    ));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn an_allowance_for_a_namespace_no_plugin_has_is_applied_and_warned() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder()
        .module(Geo)
        .config("ui.policy.allow.maps.img_src", json!(["https://maps.example"]))
        .config(
            "ui.policy.allow.app.img_src",
            json!(["https://cdn.ludhiana.example"]),
        )
        .build()
        .await;
    // The app's own sources come first, then the namespaces in name order, all applied.
    assert_eq!(
        policy_of(&app, "/deliveries").await,
        "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data: https://cdn.ludhiana.example https://maps.example; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"
    );
    assert!(logged(
        &capture,
        "WARN",
        "page policy: ui.policy.allow.maps names no plugin of this app; its sources are allowed on every page all the same"
    ));
    assert!(!logged(&capture, "WARN", "ui.policy.allow.app names no plugin"));
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn page_policy_allows_answers_as_the_header_does() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder()
        .module(Geo)
        .config(
            "ui.policy.allow.geo.img_src",
            json!(["https://*.tiles.example", "https://tile.openstreetmap.org"]),
        )
        .build()
        .await;
    let policy = app.get::<PagePolicy>().await;
    // A wildcard stands for one label or more, a name for itself, and a path is left out.
    assert!(policy.allows(Directive::ImgSrc, "https://a.tiles.example"));
    assert!(policy.allows(Directive::ImgSrc, "https://a.b.tiles.example"));
    assert!(!policy.allows(Directive::ImgSrc, "https://tiles.example"));
    assert!(!policy.allows(Directive::ImgSrc, "https://atiles.example"));
    assert!(policy.allows(Directive::ImgSrc, "https://tile.openstreetmap.org/15/1/2.png"));
    // A port the listing does not name is another origin; the scheme's own port is the same.
    assert!(!policy.allows(Directive::ImgSrc, "https://tile.openstreetmap.org:8443"));
    assert!(policy.allows(Directive::ImgSrc, "https://tile.openstreetmap.org:443"));
    assert!(!policy.allows(Directive::ImgSrc, "http://tile.openstreetmap.org"));
    // The toolkit's own sources are always there for pictures, and nothing allowed for pictures
    // is allowed for another directive.
    assert!(policy.allows(Directive::ImgSrc, "'self'"));
    assert!(policy.allows(Directive::ImgSrc, "data:"));
    assert!(!policy.allows(Directive::ConnectSrc, "data:"));
    assert!(policy.allows(Directive::ConnectSrc, "'self'"));
    assert!(!policy.allows(Directive::FontSrc, "https://tile.openstreetmap.org"));
    assert!(!policy.allows(Directive::MediaSrc, "https://tile.openstreetmap.org"));
    app.shutdown().await.unwrap();
}

#[test]
fn ui_policy_is_a_framework_section() {
    assert_eq!(<PagePolicyOptions as ConfigSection>::KEY, "ui.policy");
    assert!(FRAMEWORK_SECTIONS.contains(&"ui.policy"));
}

#[test]
fn the_source_grammar_matches_the_shared_cases() {
    let cases = include_str!("fixtures/policy-sources.txt");
    let mut read = 0;
    for line in cases.lines().filter(|line| !line.trim().is_empty()) {
        let parts: Vec<&str> = line.split('\t').collect();
        let &[key, source, verdict] = parts.as_slice() else {
            panic!("a case is a key, a source and a verdict, apart by tabs: {line:?}");
        };
        let sources = ConfigSources::new().set(&format!("ui.policy.allow.app.{key}"), json!([source]));
        let config = Config::load(&sources).expect("the configuration loads");
        let taken = config.typed::<PagePolicyOptions>().is_ok();
        assert_eq!(taken, verdict == "accept", "{key} takes {source:?}: {verdict}");
        read += 1;
    }
    assert!(read >= 100, "{read} cases");
}

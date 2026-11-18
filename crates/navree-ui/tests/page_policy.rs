//! The page policy: nothing allowed until the app's configuration says so, one policy for every
//! page, and pages that start clean. A test binary of its own: it reads the warnings and lines
//! the boot logs.

use std::sync::{Arc, Mutex};

use http::Method;
use http::header::CONTENT_SECURITY_POLICY;
use navree_config::{Config, ConfigSection, ConfigSources, FRAMEWORK_SECTIONS};
use navree_http::{AppError, Controller, HttpModule, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_plugin_geo::{Geo, GeoAskingForAnything, GeoWithClock, GeoWithMap, GeoWithScanner};
use navree_testing::{TestApp, TestAppBuilder, TestResponse};
use navree_ui::app_files::IMAGE_CSP;
use navree_ui::pages::region_answer;
use navree_ui::{
    AppImage, AppShell, Directive, ExtensionFile, IslandScript, LazyItem, LazyPage, Node, PAGE_CSP,
    PagePolicy, PagePolicyOptions, PageSources, Reply, SafeHtml, Shell, Text, UiExtension,
    UiModule, View, assets, el,
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

/// A map component of the plugin `geo`: a script and a stylesheet, scoped for its namespace.
static MAP: UiExtension = UiExtension::new("geo", "map")
    .script(ExtensionFile::__new(
        b"export const contract = 1; export function mount() {}",
        "e1613860bbf0",
        None,
    ))
    .sheet(ExtensionFile::__new(
        br#":where([data-slot^="geo:"]){.pin{color:red}}"#,
        "cdc93e5e5466",
        Some("geo"),
    ))
    .contract(1)
    .routes(&["/geo/"]);

/// A clock drawn by a script, once under the namespace of the plugin `geo` and once with no
/// namespace, as the app's own scripts are.
static CLOCK: IslandScript = IslandScript::new(
    "clock",
    b"export function mount(body) { body.textContent = 'IST'; }",
    1024,
)
.namespace("geo");
static PLAIN_CLOCK: IslandScript = IslandScript::new(
    "clock",
    b"export function mount(body) { body.textContent = 'IST'; }",
    1024,
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
    builder().module(Geo).config(
        "ui.policy.allow.geo.img_src",
        json!(["https://tile.openstreetmap.org"]),
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

/// The plugin `geo`: its crate is this module, so the app's modules at the top of the file
/// (`PagesModule`, `AppClock`, `LogoModule`) are in no crate it names.
mod navree_plugin_geo {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_ui::{Directive, PageSources};

    use super::{CLOCK, MAP, tiles};

    /// The plugin that ships the map: its namespace is `geo`.
    fn geo() -> PluginInfo {
        PluginInfo::new("navree-plugin-geo", module_path!(), "geo", "1.0.0")
    }

    /// The plugin `geo`, asking for its tile host.
    pub struct Geo;

    impl Module for Geo {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(geo()).contribute(tiles());
        }
    }

    /// The plugin `geo`, asking for its tile host and for WebAssembly.
    pub struct GeoWithScanner;

    impl Module for GeoWithScanner {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(geo()).contribute(tiles().wasm("barcode decoder"));
        }
    }

    /// The plugin `geo`, asking for a source outside the rules.
    pub struct GeoAskingForAnything;

    impl Module for GeoAskingForAnything {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(geo())
                .contribute(PageSources::new().ask(Directive::ImgSrc, "*", "map tiles"));
        }
    }

    /// The plugin `geo` with its map component: an app with a UI extension.
    pub struct GeoWithMap;

    impl Module for GeoWithMap {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(geo());
            m.contribute(MAP);
        }
    }

    /// The plugin `geo` with an island script of its own, under its namespace.
    pub struct GeoWithClock;

    impl Module for GeoWithClock {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(geo());
            m.contribute(CLOCK);
        }
    }
}

/// A module of the app with an island script of its own: what apps of today have.
struct AppClock;

impl Module for AppClock {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(PLAIN_CLOCK);
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
                region_answer(el("div").id("zones").child("Model Town and Sarabha Nagar"))
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
                ([(CONTENT_SECURITY_POLICY, "default-src 'none'")], "no page")
            })
            .public(),
            // A map of the plugin `geo`, a sign-in page that is fresh and one that is not, an
            // account page of the app's shell that is fresh and one that is not.
            RouteDef::on(Method::GET, "!/map", || async {
                View::new("Map").body(el("main").child(el("div").slot("geo:map").extension(&MAP)))
            })
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/sign-in", || async { sign_in(true) })
                .public()
                .page(),
            RouteDef::on(Method::GET, "!/sign-in-plain", || async { sign_in(false) })
                .public()
                .page(),
            RouteDef::on(Method::GET, "!/account", |shell: AppShell| async move {
                shell
                    .page("Account")
                    .fresh()
                    .body(el("p").child("Meera Sharma, Jalandhar"))
            })
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/account-plain",
                |shell: AppShell| async move {
                    shell
                        .page("Account")
                        .body(el("p").child("Meera Sharma, Jalandhar"))
                },
            )
            .public()
            .page(),
            // Pages that are not fresh and take a password or a code, one under a pattern, and
            // one that writes its field as a string, which is not read.
            RouteDef::on(Method::GET, "!/pin", || async {
                View::new("PIN").body(el("form").child(password("pin")))
            })
            .public()
            .page(),
            RouteDef::on(
                Method::GET,
                "!/pin/{id}",
                |Path(id): Path<String>| async move {
                    View::new(format!("PIN {id}")).body(el("form").child(password("pin")))
                },
            )
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/otp", || async {
                View::new("Code").body(
                    el("form").child(
                        el("input")
                            .attr("type", "text")
                            .attr("name", "code")
                            .attr("autocomplete", "one-time-code"),
                    ),
                )
            })
            .public()
            .page(),
            RouteDef::on(Method::GET, "!/pin-raw", || async {
                let field = Node::Raw(SafeHtml::trusted(r#"<input type="password" name="pin">"#));
                View::new("PIN").body(el("form").child(field))
            })
            .public()
            .page(),
            // A reply that draws a password field into the page it answers.
            RouteDef::on(Method::POST, "!/workspace/delete", || async {
                Reply::ops(Vec::new()).morph(
                    "confirm",
                    el("div").id("confirm").child(password("password")),
                )
            })
            .public(),
        ]
    }
}

/// A password input.
fn password(name: &'static str) -> navree_ui::Element {
    el("input").attr("type", "password").attr("name", name)
}

/// The sign-in page of an app with a map: a password field and the map component, marked fresh
/// or not.
fn sign_in(fresh: bool) -> View {
    let page = View::new("Sign in").body(
        el("form")
            .attr("method", "post")
            .attr("action", "/sign-in")
            .child((
                password("password"),
                el("div").slot("geo:map").extension(&MAP),
            )),
    );
    if fresh { page.fresh() } else { page }
}

struct PagesModule;

impl Module for PagesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        // The app's shell, which the account pages are drawn in.
        m.factory(Factory::<Shell>::new(|_| async {
            Ok(Arc::new(Shell::new("Agra")))
        }))
        .exports::<Shell>();
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

/// How many warnings in the log hold `text`.
fn warnings(capture: &LogCapture, text: &str) -> usize {
    capture
        .lines()
        .iter()
        .filter(|line| line.contains("WARN") && line.contains(text))
        .count()
}

/// The page at `path`, asked for as a browser does.
async fn page(app: &TestApp, path: &str) -> TestResponse {
    app.client()
        .get(path)
        .header("accept", BROWSER)
        .send()
        .await
        .assert_status(200)
}

/// The opening tag of the root element of `html`.
fn root_tag(html: &str) -> &str {
    let start = html.find("<html").expect("a root element");
    let length = html[start..].find('>').expect("a closed tag");
    &html[start..start + length]
}

/// Every header of `response` but the two that differ from one answer to the next, in order.
fn headers_of(response: &TestResponse) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "x-request-id" | "date"))
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    headers.sort();
    headers
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
    let logo = app
        .client()
        .get(&MARK.url())
        .send()
        .await
        .assert_status(200);
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
    let error = boot_error(builder().module(Geo).config(
        "ui.policy.allow.geo.img_src",
        json!(["https://tiles.example/a.png"]),
    ))
    .await;
    assert!(
        error.contains(&format!(
            "ui.policy.allow.geo.img_src[0] is not a source the page policy takes for img_src: {forms}"
        )),
        "{error}"
    );
    assert!(!error.contains("tiles.example/a.png"), "{error}");

    let error = boot_error(builder().module(Geo).config(
        "ui.policy.allow.Geo.img_src",
        json!(["https://tiles.example"]),
    ))
    .await;
    assert!(
        error.contains("ui.policy.allow.Geo: `Geo` is neither a namespace nor app"),
        "{error}"
    );

    let many: Vec<String> = (0..33)
        .map(|n| format!("https://t{n}.tiles.example"))
        .collect();
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
    let few: Vec<String> = (0..32)
        .map(|n| format!("https://t{n}.tiles.example"))
        .collect();
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
        builder().module(Geo).config(
            "ui.policy.allow.geo.img_src",
            json!(["http://localhost:8090"]),
        )
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
    assert!(!logged(
        &capture,
        "INFO",
        "every page but the fresh ones is served with"
    ));
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
    assert!(!logged(
        &capture,
        "WARN",
        "which plugin `geo` does not ask for"
    ));
    app.shutdown().await.unwrap();

    // Allowed and not asked: a warning.
    let app = builder()
        .module(Geo)
        .config(
            "ui.policy.allow.geo.img_src",
            json!([
                "https://tile.openstreetmap.org",
                "https://cdn.ludhiana.example"
            ]),
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
        .config(
            "ui.policy.allow.maps.img_src",
            json!(["https://maps.example"]),
        )
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
    assert!(!logged(
        &capture,
        "WARN",
        "ui.policy.allow.app names no plugin"
    ));
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
    assert!(policy.allows(
        Directive::ImgSrc,
        "https://tile.openstreetmap.org/15/1/2.png"
    ));
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
        let sources =
            ConfigSources::new().set(&format!("ui.policy.allow.app.{key}"), json!([source]));
        let config = Config::load(&sources).expect("the configuration loads");
        let taken = config.typed::<PagePolicyOptions>().is_ok();
        assert_eq!(
            taken,
            verdict == "accept",
            "{key} takes {source:?}: {verdict}"
        );
        read += 1;
    }
    assert!(read >= 100, "{read} cases");
}

#[tokio::test(flavor = "current_thread")]
async fn a_fresh_page_keeps_the_toolkits_policy_when_the_app_allows_sources() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = with_tiles().build().await;
    // The sign-in page and the account page keep the toolkit's policy; a page of the app that is
    // not fresh carries the composed one.
    assert_eq!(policy_of(&app, "/sign-in").await, PAGE_CSP);
    assert_eq!(policy_of(&app, "/account").await, PAGE_CSP);
    assert_eq!(policy_of(&app, "/map").await, WITH_TILES);
    assert_eq!(policy_of(&app, "/sign-in-plain").await, WITH_TILES);
    // A fresh page answered to a region's fetch keeps it too.
    let region = policy_with(
        &app,
        "/sign-in",
        &[("x-navree-ui", "1"), ("x-navree-region", "security")],
    )
    .await;
    assert_eq!(region, PAGE_CSP);
    // A policy the app widened turns pages that start clean on, though it has no extension.
    let sign_in = page(&app, "/sign-in").await;
    assert_eq!(sign_in.header("x-navree-fresh"), Some("1"));
    assert!(root_tag(&sign_in.text()).ends_with(" data-ui-fresh-page"));
    assert!(page(&app, "/map").await.header("x-navree-fresh").is_none());
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_fresh_page_writes_its_mark_header_and_no_extension_link_in_an_app_with_extensions() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder().module(GeoWithMap).build().await;

    // The sign-in page is fresh: its mark is the last attribute of the root element, its answer
    // says so, and it links no extension file and loads no extensions module.
    let sign_in = page(&app, "/sign-in").await;
    let html = sign_in.text();
    assert!(
        html.starts_with(r#"<!doctype html><html lang="en""#),
        "{html}"
    );
    assert!(root_tag(&html).ends_with(" data-ui-fresh-page"), "{html}");
    assert_eq!(sign_in.header("x-navree-fresh"), Some("1"));
    assert_eq!(sign_in.header("content-security-policy"), Some(PAGE_CSP));
    for absent in ["data-ui-ext-link", "modulepreload", assets::EXTENSIONS_URL] {
        assert!(!html.contains(absent), "`{absent}` in {html}");
    }

    // The same component on a page that is not fresh has its links and its module, and no mark.
    let map = page(&app, "/map").await;
    let html = map.text();
    assert!(!root_tag(&html).contains("data-ui-fresh"), "{html}");
    assert!(map.header("x-navree-fresh").is_none());
    for present in [
        format!(
            r#"<link rel="stylesheet" href="{}" data-ui-ext-link>"#,
            MAP.sheet_url().expect("a stylesheet")
        ),
        format!(
            r#"<link rel="modulepreload" href="{}">"#,
            MAP.script_url().expect("a script")
        ),
        assets::EXTENSIONS_URL.to_owned(),
    ] {
        assert!(html.contains(&present), "`{present}` is not in {html}");
    }

    // Written outside a request, a fresh page never carries the mark.
    assert!(
        !View::new("Sign in")
            .fresh()
            .to_html()
            .contains("data-ui-fresh-page")
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn in_an_app_without_extensions_a_fresh_page_is_written_and_navigated_as_before() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    // No extension, no plugin island and nothing allowed: a page marked fresh is the page it
    // always was, byte for byte and header for header.
    let app = builder().build().await;
    for (marked, unmarked) in [
        ("/sign-in", "/sign-in-plain"),
        ("/account", "/account-plain"),
    ] {
        let marked = page(&app, marked).await;
        let unmarked = page(&app, unmarked).await;
        assert_eq!(marked.text(), unmarked.text(), "the same bytes");
        assert_eq!(headers_of(&marked), headers_of(&unmarked));
        assert!(marked.header("x-navree-fresh").is_none());
        assert_eq!(marked.header("content-security-policy"), Some(PAGE_CSP));
        assert!(
            !marked.text().contains("data-ui-fresh"),
            "{}",
            marked.text()
        );
    }
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_page_with_a_password_field_that_is_not_fresh_is_warned_once_per_route() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let said = |path: &str| {
        warnings(
            &capture,
            &format!(
                "page {path} holds a password field and is not fresh: call .fresh() on its View or AppPage"
            ),
        )
    };
    let app = builder().module(GeoWithMap).build().await;
    // A page asked twice is named once, a code field is named, a fresh page is not, and the name
    // is the route's pattern, never the address asked for.
    for _ in 0..2 {
        page(&app, "/pin").await;
    }
    page(&app, "/otp").await;
    page(&app, "/sign-in").await;
    page(&app, "/pin/42").await;
    page(&app, "/pin/43").await;
    assert_eq!(said("/pin"), 1);
    assert_eq!(said("/otp"), 1);
    assert_eq!(said("/sign-in"), 0);
    assert_eq!(said("/pin/{id}"), 1);
    assert_eq!(said("/pin/42") + said("/pin/43"), 0);
    // Only elements are read: a field written inside a string is not seen, and a page with no
    // field is not named.
    page(&app, "/pin-raw").await;
    page(&app, "/map").await;
    assert_eq!(said("/pin-raw") + said("/map"), 0);
    app.shutdown().await.unwrap();

    // An app with no extension and nothing allowed warns nothing.
    let before = warnings(&capture, "holds a password field");
    let app = builder().build().await;
    page(&app, "/pin").await;
    page(&app, "/otp").await;
    assert_eq!(warnings(&capture, "holds a password field"), before);
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_reply_that_sends_a_password_field_as_ops_is_warned_about_in_an_app_with_extensions() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let text = "route /workspace/delete sends a password field as ops into a page that may hold plugin code: draw it on a fresh page and answer with a redirect to it";
    let delete = |app: &TestApp| {
        app.client()
            .post("/workspace/delete")
            .header("x-navree-ui", "1")
            .send()
    };
    let app = builder().module(GeoWithMap).build().await;
    delete(&app).await.assert_status(200);
    delete(&app).await.assert_status(200);
    assert_eq!(warnings(&capture, text), 1);
    app.shutdown().await.unwrap();

    // An app with no extension and nothing allowed warns nothing.
    let app = builder().build().await;
    delete(&app).await.assert_status(200);
    assert_eq!(warnings(&capture, text), 1, "no new warning");
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugins_island_script_turns_fresh_pages_on() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    // A plugin's island script is one built under a namespace: with it the app has pages that
    // start clean, though it has no UI extension and allows nothing.
    let app = builder().module(GeoWithClock).build().await;
    let sign_in = page(&app, "/sign-in").await;
    assert!(
        root_tag(&sign_in.text()).ends_with(" data-ui-fresh-page"),
        "{}",
        sign_in.text()
    );
    assert_eq!(sign_in.header("x-navree-fresh"), Some("1"));
    assert_eq!(sign_in.header("content-security-policy"), Some(PAGE_CSP));
    assert!(
        page(&app, "/sign-in-plain")
            .await
            .header("x-navree-fresh")
            .is_none()
    );
    app.shutdown().await.unwrap();

    // The app's own island script, with no namespace, as apps of today have: nothing changes.
    let app = builder().module(AppClock).build().await;
    let sign_in = page(&app, "/sign-in").await;
    assert!(
        !sign_in.text().contains("data-ui-fresh"),
        "{}",
        sign_in.text()
    );
    assert!(sign_in.header("x-navree-fresh").is_none());
    app.shutdown().await.unwrap();
}

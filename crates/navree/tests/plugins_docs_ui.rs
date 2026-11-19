//! Proves `docs/plugins/ui-components.mdx`: an inline `geo` plugin embeds its
//! script and scoped stylesheet with `extension_script!` and `extension_sheet!`,
//! contributes `MAP` and `PageSources`, draws its element on a page route,
//! serves its files at hashed URLs, logs the exact page-policy warning when the
//! tile host is not allowed, and includes the host in `Content-Security-Policy`
//! once `ui.policy.allow.geo.img_src` allows it.

#![allow(missing_docs)]

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use http::Method;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, Resolver};
use navree::prelude::*;
use navree::testing::TestApp;
use navree::ui::{UiModule, View, el};
use serde_json::{Value, json};

/// Inline module standing for the `navree-plugin-geo` crate (a test binary is
/// one crate, so `PluginInfo::new` names `module_path!()` as the plugin's crate).
mod geo_page {
    use navree::plugin::PluginInfo;
    use navree::prelude::*;
    use navree::ui::{Directive, PageSources, UiExtension, extension_script, extension_sheet};

    pub static MAP: UiExtension = UiExtension::new("geo", "map")
        .script(extension_script!(
            "tests/fixtures/plugins_docs_ui/map.js",
            4096
        ))
        .sheet(extension_sheet!(
            "tests/fixtures/plugins_docs_ui/map.css",
            "geo",
            4096
        ))
        .contract(1)
        .routes(&["/geo/"]);

    pub struct GeoPageModule;

    impl Module for GeoPageModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-geo",
                module_path!(),
                "geo",
                "0.1.0",
            ));
            m.contribute(MAP);
            m.contribute(PageSources::new().ask(
                Directive::ImgSrc,
                "https://tile.openstreetmap.org",
                "map tiles",
            ));
        }
    }
}

struct DeliveriesController;

impl Injectable for DeliveriesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, std::result::Result<Self, KernelError>> {
        Box::pin(async { Ok(DeliveriesController) })
    }
}

impl Controller for DeliveriesController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/deliveries", || async {
                let props = json!({
                    "city": "Ludhiana",
                    "place": "Sarabha Nagar, Ludhiana",
                    "zoom": 15,
                });
                View::new("Deliveries").body(
                    el("main").child(
                        el("div")
                            .slot("geo:map")
                            .extension_with(&geo_page::MAP, &props)
                            .child(
                                el("div")
                                    .slot("geo:map-body")
                                    .flag("data-ui-keep", true)
                                    .child("Map unavailable"),
                            ),
                    ),
                )
            })
            .public()
            .page(),
        ]
    }
}

struct DeliveriesAppModule;

impl Module for DeliveriesAppModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DeliveriesController>();
    }
}

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

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
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

fn assert_ui_components_page_slices_match_source() {
    let mdx_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/ui-components.mdx");
    let mdx = fs::read_to_string(&mdx_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", mdx_path.display()))
        .replace("\r\n", "\n");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");

    let mut open: Option<(usize, usize, String)> = None;
    let mut code = String::new();
    let mut checked = 0usize;

    for (idx, line) in mdx.split('\n').enumerate() {
        let trimmed = line.trim_start();
        let ticks = trimmed.chars().take_while(|c| *c == '`').count();
        match &open {
            None if ticks >= 3 => {
                open = Some((idx + 1, ticks, trimmed[ticks..].trim().to_owned()));
                code.clear();
            }
            None => {}
            Some((start_line, opened_ticks, info)) => {
                if ticks >= *opened_ticks && trimmed.trim_end().len() == ticks {
                    let lang = info.split([',', ' ']).next().unwrap_or_default();
                    let prefix = match lang {
                        "rust" => Some("// "),
                        "toml" => Some("# "),
                        _ => None,
                    };
                    if let Some(prefix) = prefix {
                        let mut lines = code.lines();
                        if let Some(first) = lines.next()
                            && let Some(rel) = first
                                .trim()
                                .strip_prefix(prefix)
                                .map(str::trim)
                                .filter(|p| p.starts_with("plugins/") || p.starts_with("crates/"))
                        {
                            let target = fs::read_to_string(repo_root.join(rel))
                                .unwrap_or_else(|err| {
                                    panic!("line {start_line}: cannot read `{rel}`: {err}")
                                })
                                .replace("\r\n", "\n");
                            let hay: Vec<&str> = target.lines().map(str::trim_end).collect();
                            let needle: Vec<&str> = lines.map(str::trim_end).collect();
                            assert!(
                                hay.windows(needle.len()).any(|w| w == needle.as_slice()),
                                "ui-components.mdx block at line {start_line} is not a verbatim slice of `{rel}`"
                            );
                            checked += 1;
                        }
                    }
                    open = None;
                } else {
                    code.push_str(line);
                    code.push('\n');
                }
            }
        }
    }
    assert!(
        checked >= 3,
        "expected ui-components.mdx to quote `plugins/geo` slices, checked {checked}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_ui_components_page_example_boots_and_draws_its_extension() {
    assert_ui_components_page_slices_match_source();

    let capture = LogCapture::default();
    let _guard = capture.install();

    // 1. Boot without allowing `https://tile.openstreetmap.org`: warns and keeps CSP strict.
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(UiModule)
        .module(geo_page::GeoPageModule)
        .module(DeliveriesAppModule)
        .build()
        .await;

    let expected_warning = "page policy: plugin `geo` asks for img-src https://tile.openstreetmap.org (map tiles), which the app does not allow; what needs it shows its fallback (allow it with ui.policy.allow.geo.img_src)";
    let logs = capture.text();
    assert!(
        logs.contains(expected_warning),
        "expected page-policy warning in boot logs:\n{logs}"
    );

    let script_url = geo_page::MAP.script_url().expect("script url");
    let sheet_url = geo_page::MAP.sheet_url().expect("sheet url");

    let page = app
        .client()
        .get("/deliveries")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
    let csp = page
        .header("content-security-policy")
        .expect("CSP header on View");
    assert!(
        !csp.contains("https://tile.openstreetmap.org"),
        "unallowed host must not appear in CSP: {csp}"
    );

    let html = page.text();
    assert!(
        html.contains(script_url),
        "HTML names script address:\n{html}"
    );
    assert!(
        html.contains(sheet_url),
        "HTML names sheet address:\n{html}"
    );
    let head_link = format!(r#"<link rel="stylesheet" href="{sheet_url}" data-ui-ext-link>"#);
    assert!(
        html.contains(&head_link),
        "head links the extension stylesheet:\n{html}"
    );

    let served_script = app.client().get(script_url).send().await.assert_status(200);
    assert_eq!(
        served_script.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(
        &served_script.bytes()[..],
        geo_page::MAP.script_file().expect("script file").bytes()
    );

    let served_sheet = app.client().get(sheet_url).send().await.assert_status(200);
    assert_eq!(
        served_sheet.header("content-type"),
        Some("text/css; charset=utf-8")
    );
    let sheet_text = served_sheet.text();
    assert!(
        sheet_text.contains(r#":where([data-slot^="geo:"])"#),
        "served sheet is scoped to `geo:`:\n{sheet_text}"
    );

    app.shutdown().await.expect("shutdown");

    // 2. Boot with `ui.policy.allow.geo.img_src = ["https://tile.openstreetmap.org"]`.
    let allowed_app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config(
            "ui.policy.allow.geo.img_src",
            json!(["https://tile.openstreetmap.org"]),
        )
        .module(UiModule)
        .module(geo_page::GeoPageModule)
        .module(DeliveriesAppModule)
        .build()
        .await;

    let allowed_page = allowed_app
        .client()
        .get("/deliveries")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
    let allowed_csp = allowed_page
        .header("content-security-policy")
        .expect("CSP header on View");
    assert!(
        allowed_csp.contains("img-src 'self' data: https://tile.openstreetmap.org"),
        "expected allowed tile host in img-src:\n{allowed_csp}"
    );

    allowed_app.shutdown().await.expect("shutdown");
}

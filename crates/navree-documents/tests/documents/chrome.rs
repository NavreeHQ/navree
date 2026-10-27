//! The Chrome engine against a mock DevTools endpoint: templates and
//! data, assets from the template tree, blocked origins, print settings,
//! locales, the pool, timeouts, crashes and errors.

use std::time::Duration;

use navree_documents::{
    ChromeEngine, ChromeOptions, DOCUMENT_ORIGIN, DocumentsError, FONTS_STYLESHEET, Margins,
    PrintOptions, Render, Templates,
};
use serde_json::json;

use crate::cdp_mock::{Behavior, MockChrome};
use crate::support::fixture_templates;

fn options(mock: &MockChrome) -> ChromeOptions {
    ChromeOptions {
        endpoint: Some(mock.ws_url()),
        timeout_secs: 5,
        ..ChromeOptions::default()
    }
}

fn engine(mock: &MockChrome) -> ChromeEngine {
    ChromeEngine::new(fixture_templates(), options(mock))
}

#[tokio::test]
async fn a_template_renders_with_its_data_and_prints() {
    let mock = MockChrome::start(Behavior::default()).await;
    let engine = engine(&mock);
    let pdf = engine
        .render(&Render::template("invoices/show").data(json!({
            "number": 42,
            "customer": "Ada & Co",
            "total_cents": 123456,
            "currency": "EUR",
        })))
        .await
        .unwrap();
    let text = pdf.text().unwrap();
    assert!(text.contains("Invoice 42"), "{text}");
    assert!(text.contains("Billed to Ada & Co"), "{text}");
    // The document was served escaped, from the private origin (the mock
    // prints Latin-1 text only, so the euro sign is checked here).
    let documents = mock.seen().documents.clone();
    assert!(documents[0].contains("Billed to Ada &amp; Co"), "{}", documents[0]);
    assert!(documents[0].contains("Total €1,234.56"), "{}", documents[0]);
    let navigate = &mock.params_of("Page.navigate")[0];
    assert_eq!(navigate["url"], format!("{DOCUMENT_ORIGIN}/invoices/show.html"));
    // A4 portrait with 10 mm margins, in inches.
    let print = &mock.params_of("Page.printToPDF")[0];
    assert!((print["paperWidth"].as_f64().unwrap() - 8.2677).abs() < 0.001);
    assert_eq!(print["landscape"], false);
    assert_eq!(print["printBackground"], true);
    // Every document prints in its own context, disposed of afterwards.
    mock.wait_for_contexts_disposed().await;
    let methods = mock.methods();
    assert_eq!(methods[0], "Target.createBrowserContext");
    assert!(methods.contains(&"Fetch.enable".to_owned()));
}

#[tokio::test]
async fn templates_extend_layouts_and_can_be_named_with_the_extension() {
    let mock = MockChrome::start(Behavior::default()).await;
    let pdf = engine(&mock)
        .render(&Render::template("certificates/classic.html").data(json!({
            "name": "Grace Hopper",
            "course": "Compilers",
            "completed_on": "9 December 1906",
        })))
        .await
        .unwrap();
    let text = pdf.text().unwrap();
    assert!(text.contains("Certificate of completion"), "{text}");
    assert!(
        text.contains("This certifies that Grace Hopper completed Compilers on 9 December 1906."),
        "{text}"
    );
}

#[tokio::test]
async fn inline_html_prints_as_it_is_with_the_render_s_page_settings() {
    let mock = MockChrome::start(Behavior::default()).await;
    let print = PrintOptions::letter()
        .landscape()
        .margin(Margins::all(25.4))
        .footer("<span class=\"pageNumber\"></span>");
    let pdf = engine(&mock)
        .render(&Render::html("<h1>{{ not a template }}</h1>").print(print).title("Inline"))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("{{ not a template }}"));
    assert_eq!(pdf.title().unwrap().as_deref(), Some("Inline"));
    let params = &mock.params_of("Page.printToPDF")[0];
    assert_eq!(params["paperWidth"], 8.5);
    assert_eq!(params["landscape"], true);
    assert_eq!(params["marginLeft"], 1.0);
    assert_eq!(params["displayHeaderFooter"], true);
    assert_eq!(
        mock.params_of("Page.navigate")[0]["url"],
        format!("{DOCUMENT_ORIGIN}/inline.html")
    );
}

#[tokio::test]
async fn the_configured_print_settings_apply_by_default() {
    let mock = MockChrome::start(Behavior::default()).await;
    let options = ChromeOptions {
        print: PrintOptions::letter(),
        ..options(&mock)
    };
    ChromeEngine::new(fixture_templates(), options)
        .render(&Render::html("<p>x</p>"))
        .await
        .unwrap();
    assert_eq!(mock.params_of("Page.printToPDF")[0]["paperWidth"], 8.5);
}

#[tokio::test]
async fn assets_come_from_the_template_tree_and_other_origins_are_blocked() {
    let mock = MockChrome::start(Behavior::default()).await;
    engine(&mock)
        .render(&Render::template("reports/assets").data(json!({"title": "Assets"})))
        .await
        .unwrap();
    let seen = mock.seen();
    let fulfilled: Vec<(&str, u16)> = seen
        .fulfilled
        .iter()
        .map(|(url, status, _)| (url.as_str(), *status))
        .collect();
    let css = format!("{DOCUMENT_ORIGIN}/styles/document.css");
    let logo = format!("{DOCUMENT_ORIGIN}/images/logo.png");
    let missing = format!("{DOCUMENT_ORIGIN}/reports/missing.png");
    assert!(fulfilled.contains(&(css.as_str(), 200)), "{fulfilled:?}");
    assert!(fulfilled.contains(&(logo.as_str(), 200)), "{fulfilled:?}");
    assert!(fulfilled.contains(&(missing.as_str(), 404)), "{fulfilled:?}");
    let png = &seen.fulfilled.iter().find(|(url, ..)| *url == logo).unwrap().2;
    assert!(png.starts_with(b"\x89PNG"));
    assert_eq!(seen.blocked, vec!["https://tracker.example.com/pixel.png"]);
    assert!(seen.continued.is_empty());
}

#[tokio::test]
async fn the_bundled_fonts_are_served_from_the_reserved_prefix() {
    let mock = MockChrome::start(Behavior::default()).await;
    engine(&mock)
        .render(&Render::template("certificates/multilingual").data(json!({
            "title": "t", "name": "n", "course": "c",
        })))
        .await
        .unwrap();
    let seen = mock.seen();
    let css_url = format!("{DOCUMENT_ORIGIN}{FONTS_STYLESHEET}");
    let (_, status, css) = seen
        .fulfilled
        .iter()
        .find(|(url, ..)| *url == css_url)
        .expect("the stylesheet was requested");
    assert_eq!(*status, 200);
    let css = String::from_utf8(css.clone()).unwrap();
    assert!(css.contains("font-family: \"Noto Sans Arabic\""), "{css}");
    assert!(css.contains("url(\"/_navree/fonts/NotoSansDevanagari-Bold.ttf\")"), "{css}");
}

#[tokio::test]
async fn the_network_can_be_allowed() {
    let mock = MockChrome::start(Behavior::default()).await;
    let options = ChromeOptions {
        allow_network: true,
        ..options(&mock)
    };
    ChromeEngine::new(fixture_templates(), options)
        .render(&Render::template("reports/assets").data(json!({"title": "Assets"})))
        .await
        .unwrap();
    let seen = mock.seen();
    assert!(seen.blocked.is_empty());
    assert_eq!(seen.continued, vec!["https://tracker.example.com/pixel.png"]);
}

#[tokio::test]
async fn hidden_files_and_paths_outside_the_tree_are_not_served() {
    let mock = MockChrome::start(Behavior::default()).await;
    let templates = Templates::from_files([
        ("doc.html", "<img src=\"/.env\"><img src=\"../secret.txt\"><p>ok</p>"),
        (".env", "SECRET=1"),
    ]);
    ChromeEngine::new(templates, options(&mock))
        .render(&Render::template("doc"))
        .await
        .unwrap();
    for (url, status, body) in &mock.seen().fulfilled {
        assert_eq!(*status, 404, "{url}");
        assert!(body.is_empty());
    }
}

#[tokio::test]
async fn the_locale_sets_the_direction_and_the_browser_locale() {
    let mock = MockChrome::start(Behavior::default()).await;
    let engine = engine(&mock);
    let render = Render::template("reports/summary")
        .data(json!({"title": "تقرير التدقيق", "summary": "كل الضوابط ناجحة"}))
        .locale("ar");
    let html = engine.render_html(&render).await.unwrap();
    assert!(html.contains(r#"<html lang="ar" dir="rtl">"#), "{html}");
    engine.render(&render).await.unwrap();
    assert!(mock.seen().documents[0].contains("كل الضوابط ناجحة"));
    assert_eq!(
        mock.params_of("Emulation.setLocaleOverride"),
        vec![json!({"locale": "ar"})]
    );
    // Without a locale: English, left to right, no override.
    let html = engine
        .render_html(&Render::template("reports/summary").data(json!({"title": "t", "summary": "s"})))
        .await
        .unwrap();
    assert!(html.contains(r#"<html lang="en" dir="ltr">"#), "{html}");
}

#[tokio::test]
async fn render_html_needs_no_browser() {
    let engine = ChromeEngine::new(
        fixture_templates(),
        ChromeOptions {
            path: Some("/no/such/chrome".into()),
            ..ChromeOptions::default()
        },
    );
    let html = engine
        .render_html(&Render::template("invoices/show").data(json!({
            "number": 7, "customer": "Lin", "total_cents": 500, "currency": "USD",
        })))
        .await
        .unwrap();
    assert!(html.contains("<h1>Invoice 7</h1>"), "{html}");
    assert!(html.contains("Total $5.00"), "{html}");
    assert!(!engine.is_running().await);
}

#[tokio::test]
async fn the_pool_limits_pages_printing_at_once() {
    for (pool_size, expected) in [(1, 1), (3, 3)] {
        let mock = MockChrome::start(Behavior {
            print_delay: Duration::from_millis(200),
            ..Behavior::default()
        })
        .await;
        let engine = ChromeEngine::new(
            fixture_templates(),
            ChromeOptions {
                pool_size,
                ..options(&mock)
            },
        );
        let renders = (0..3).map(|i| {
            let engine = engine.clone();
            async move { engine.render(&Render::html(format!("<p>{i}</p>"))).await }
        });
        for result in futures_util::future::join_all(renders).await {
            result.unwrap();
        }
        assert_eq!(mock.seen().max_printing, expected, "pool_size {pool_size}");
        // One browser connection serves every render.
        assert_eq!(mock.seen().connections, 1);
    }
}

#[tokio::test]
async fn a_slow_render_times_out_and_its_context_is_disposed_of() {
    let mock = MockChrome::start(Behavior {
        print_delay: Duration::from_secs(3),
        ..Behavior::default()
    })
    .await;
    let engine = ChromeEngine::new(
        fixture_templates(),
        ChromeOptions {
            timeout_secs: 1,
            ..options(&mock)
        },
    );
    let error = engine.render(&Render::html("<p>slow</p>")).await.unwrap_err();
    assert!(
        matches!(error, DocumentsError::Timeout { engine: "chrome", secs: 1 }),
        "{error:?}"
    );
    assert_eq!(error.status(), http::StatusCode::SERVICE_UNAVAILABLE);
    mock.wait_for_contexts_disposed().await;
}

#[tokio::test]
async fn a_lost_browser_is_reconnected_on_the_next_render() {
    let mock = MockChrome::start(Behavior {
        crash_on_first_print: true,
        ..Behavior::default()
    })
    .await;
    let engine = engine(&mock);
    let error = engine.render(&Render::html("<p>one</p>")).await.unwrap_err();
    assert!(
        matches!(&error, DocumentsError::Engine { engine: "chrome", .. }),
        "{error:?}"
    );
    let pdf = engine.render(&Render::html("<p>two</p>")).await.unwrap();
    assert!(pdf.text().unwrap().contains("two"));
    assert_eq!(mock.seen().connections, 2);
}

#[tokio::test]
async fn an_http_endpoint_is_resolved_through_json_version() {
    let mock = MockChrome::start(Behavior::default()).await;
    let engine = ChromeEngine::new(
        fixture_templates(),
        ChromeOptions {
            endpoint: Some(mock.http_url()),
            ..options(&mock)
        },
    );
    engine.render(&Render::html("<p>x</p>")).await.unwrap();
    assert_eq!(mock.seen().version_requests, 1);
    assert!(engine.is_running().await);
    assert_eq!(engine.executable(), None);
    engine.shutdown().await;
    assert!(!engine.is_running().await);
}

#[tokio::test]
async fn template_errors_name_the_template() {
    let mock = MockChrome::start(Behavior::default()).await;
    let engine = engine(&mock);
    match engine.render(&Render::template("invoices/missing")).await {
        Err(DocumentsError::TemplateNotFound { name, tried }) => {
            assert_eq!(name, "invoices/missing");
            assert_eq!(tried, vec!["invoices/missing.html"]);
        }
        other => panic!("{other:?}"),
    }
    // An undefined variable is an error, not a blank.
    match engine
        .render(&Render::template("invoices/show").data(json!({"number": 1})))
        .await
    {
        Err(DocumentsError::Template { file, line, message, .. }) => {
            assert_eq!(file, "invoices/show.html");
            assert_eq!(line, Some(13));
            assert!(message.contains("undefined"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    match engine.render(&Render::typst("= Typst")).await {
        Err(DocumentsError::EngineUnavailable { engine: "chrome", .. }) => {}
        other => panic!("{other:?}"),
    }
    // Nothing reached the browser.
    assert!(mock.seen().calls.is_empty());
}

#[tokio::test]
async fn a_syntax_error_names_the_file_and_line() {
    let mock = MockChrome::start(Behavior::default()).await;
    let templates = Templates::from_files([("broken.html", "<p>\n{{ name }\n</p>")]);
    let error = ChromeEngine::new(templates, options(&mock))
        .render(&Render::template("broken").data(json!({"name": "x"})))
        .await
        .unwrap_err();
    match error {
        DocumentsError::Template { file, line, .. } => {
            assert_eq!(file, "broken.html");
            assert_eq!(line, Some(2));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn no_chrome_is_an_unavailable_engine() {
    let engine = ChromeEngine::new(
        fixture_templates(),
        ChromeOptions {
            path: Some(std::env::temp_dir().join("no-such-chrome-navree.exe")),
            timeout_secs: 5,
            ..ChromeOptions::default()
        },
    );
    let error = engine.render(&Render::html("<p>x</p>")).await.unwrap_err();
    assert!(
        matches!(&error, DocumentsError::EngineUnavailable { engine: "chrome", reason } if reason.contains("cannot start")),
        "{error:?}"
    );
    assert_eq!(error.status(), http::StatusCode::SERVICE_UNAVAILABLE);
}

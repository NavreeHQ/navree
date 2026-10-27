//! The Chrome engine with a real browser (`NAVREE_BROWSER_CHROME`).

use navree_documents::{ChromeEngine, ChromeOptions, Render};
use serde_json::json;

use crate::support::{chrome_or_skip, fixture_templates};

fn engine(path: std::path::PathBuf) -> ChromeEngine {
    ChromeEngine::new(
        fixture_templates(),
        ChromeOptions {
            path: Some(path),
            timeout_secs: 60,
            ..ChromeOptions::default()
        },
    )
}

#[tokio::test]
async fn chrome_prints_a_template_with_its_data() {
    let Some(chrome) = chrome_or_skip("chrome_prints_a_template_with_its_data") else {
        return;
    };
    let engine = engine(chrome);
    let pdf = engine
        .render(
            &Render::template("invoices/show")
                .data(json!({
                    "number": 42,
                    "customer": "Ada & Co",
                    "total_cents": 123456,
                    "currency": "EUR",
                }))
                .title("Invoice 42"),
        )
        .await
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(pdf.page_count().unwrap(), 1);
    let text = pdf.text().unwrap();
    assert!(text.contains("Invoice 42"), "{text}");
    assert!(text.contains("Billed to Ada & Co"), "{text}");
    assert!(text.contains("Total €1,234.56"), "{text}");
    assert_eq!(pdf.title().unwrap().as_deref(), Some("Invoice 42"));

    // Assets load from the template tree; the blocked tracker does not
    // hold the document up.
    let pdf = engine
        .render(&Render::template("reports/assets").data(json!({"title": "Assets"})))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("Assets"));
    engine.shutdown().await;
    assert!(!engine.is_running().await);
}

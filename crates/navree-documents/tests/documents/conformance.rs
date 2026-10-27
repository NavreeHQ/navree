//! The `PdfEngine` conformance suite, run on every engine: Typst, Chrome
//! (against the mock endpoint) and the fake.

use navree_documents::{
    DocumentsError, DocumentsFake, EngineKind, PdfEngine, Render, TemplateSource,
};
use serde_json::json;

use crate::support::fixture_templates;

/// What one engine is checked with.
struct Case<'a> {
    engine: &'a dyn PdfEngine,
    /// Inline markup for this engine, and text its PDF must contain.
    inline: Render,
    inline_text: &'a str,
    /// Whether the engine prints the data (the fake prints `key: value`).
    prints_sentence: bool,
}

async fn conformance(case: Case<'_>) {
    let engine = case.engine;
    let kind = engine.kind();
    let extension = kind.extension();
    let data = json!({
        "name": "Ada Lovelace",
        "course": "Analytical Engines",
        "completed_on": "26 October 2026",
    });

    // A named template renders with its data, with or without extension.
    for name in [
        "certificates/classic".to_owned(),
        format!("certificates/classic.{extension}"),
    ] {
        let pdf = engine
            .render(&Render::template(&name).data(&data))
            .await
            .unwrap_or_else(|error| panic!("{kind}: {name}: {error}"));
        assert!(pdf.starts_with(b"%PDF-"), "{kind}");
        assert!(pdf.page_count().unwrap() >= 1, "{kind}");
        let text = pdf.text().unwrap();
        assert!(text.contains("Ada Lovelace"), "{kind}: {text}");
        if case.prints_sentence {
            assert!(
                text.contains("completed Analytical Engines on 26 October 2026"),
                "{kind}: {text}"
            );
        }
    }

    // Inline markup renders.
    let pdf = engine.render(&case.inline).await.unwrap();
    assert!(pdf.text().unwrap().contains(case.inline_text), "{kind}");

    // An unknown name names the file tried.
    match engine
        .render(&Render::template("certificates/missing"))
        .await
    {
        Err(DocumentsError::TemplateNotFound { name, tried }) => {
            assert_eq!(name, "certificates/missing", "{kind}");
            assert_eq!(
                tried,
                vec![format!("certificates/missing.{extension}")],
                "{kind}"
            );
        }
        other => panic!("{kind}: {other:?}"),
    }
    // A path out of the tree is not found either.
    assert!(
        matches!(
            engine.render(&Render::template("../Cargo")).await,
            Err(DocumentsError::TemplateNotFound { .. })
        ),
        "{kind}"
    );

    // Data that is not JSON fails before rendering.
    let mut map = std::collections::BTreeMap::new();
    map.insert((1, 2), "tuple keys");
    assert!(
        matches!(
            engine
                .render(&Render::template("certificates/classic").data(map))
                .await,
            Err(DocumentsError::Data(_))
        ),
        "{kind}"
    );

    // The title lands in the metadata.
    let pdf = engine
        .render(
            &Render::template("certificates/classic")
                .data(&data)
                .title("Certificate 7"),
        )
        .await
        .unwrap();
    assert_eq!(
        pdf.title().unwrap().as_deref(),
        Some("Certificate 7"),
        "{kind}"
    );
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn the_typst_engine_conforms() {
    let engine = navree_documents::TypstEngine::new(fixture_templates());
    conformance(Case {
        engine: &engine,
        inline: Render::typst("Inline document"),
        inline_text: "Inline document",
        prints_sentence: true,
    })
    .await;
}

#[cfg(feature = "chrome")]
#[tokio::test]
async fn the_chrome_engine_conforms() {
    use crate::cdp_mock::{Behavior, MockChrome};
    let mock = MockChrome::start(Behavior::default()).await;
    let engine = navree_documents::ChromeEngine::new(
        fixture_templates(),
        navree_documents::ChromeOptions {
            endpoint: Some(mock.ws_url()),
            ..Default::default()
        },
    );
    conformance(Case {
        engine: &engine,
        inline: Render::html("<p>Inline document</p>"),
        inline_text: "Inline document",
        prints_sentence: true,
    })
    .await;
}

#[tokio::test]
async fn the_fake_engine_conforms() {
    let fake = DocumentsFake::with_templates(fixture_templates());
    for kind in [EngineKind::Typst, EngineKind::Chrome] {
        let engine = fake.engine(kind);
        let inline = match kind {
            EngineKind::Typst => Render::typst("Inline document"),
            EngineKind::Chrome => Render::html("<p>Inline document</p>"),
        };
        conformance(Case {
            engine: &engine,
            inline,
            inline_text: "inline",
            prints_sentence: false,
        })
        .await;
    }
    // Every render reached the fake's record.
    assert!(
        fake.rendered()
            .iter()
            .any(|r| matches!(r.source, TemplateSource::Html(_)))
    );
}

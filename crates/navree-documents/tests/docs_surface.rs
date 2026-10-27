//! Every public item the documents docs page names
//! (`apps/website/content/docs/packages/documents.mdx`): types, methods,
//! configuration keys, commands, the job and the constants. A rename fails
//! this file.

#![cfg(all(feature = "typst", feature = "chrome", feature = "docx"))]

use std::path::PathBuf;
use std::sync::Arc;

use navree_documents::docx::{Align, Image, Paragraph, Run, Table};
use navree_documents::{
    CHROME_ENV, ChromeEngine, ChromeOptions, DOCUMENT_ORIGIN, DOCX_CONTENT_TYPE, DocumentStatus,
    Documents, DocumentsError, DocumentsFake, DocumentsModule, DocumentsOptions, Docx, DocxFile,
    Download, EngineKind, FAILED_SUFFIX, FONTS_STYLESHEET, FakeEngine, HTML_EXTENSION, Margins,
    ModuleBuilderDocumentsExt, PDF_CONTENT_TYPE, Pages, Paper, Pdf, PdfEngine, Position,
    PrintOptions, QueuedDocument, QueuedRender, Render, RenderDocument, Stamp, StoredDocument,
    TYPST_EXTENSION, TemplateSource, Templates, TypstEngine, TypstOptions, is_rtl,
};
use navree_kernel::ModuleBuilder;
use serde_json::json;

/// The `Documents` methods the page names (compiled, never run).
#[allow(dead_code)]
async fn documents_methods(documents: &Documents, render: Render) -> Result<(), DocumentsError> {
    let _: Pdf = documents.pdf("certificates/course", json!({})).await?;
    let _: Pdf = documents.render(&render).await?;
    let _: EngineKind = documents.engine_for(&render)?;
    let stored: StoredDocument = documents.save(&render, "reports/q3.pdf").await?;
    let _: String = stored.temporary_url(chrono::Utc::now()).await?;
    let _: String = stored.url()?;
    let queued: QueuedDocument = documents.queue(render, "exports/q3.pdf").await?;
    let _: DocumentStatus = documents.status(&queued).await?;
    let _: Vec<String> = documents.font_families().await?;
    documents.shutdown().await;
    let _ = documents.disk()?;
    let _: &Templates = documents.templates();
    let _: Option<Arc<dyn PdfEngine>> = documents.engine(EngineKind::Chrome);
    Ok(())
}

/// `m.document_files(..)` (compiled, never run).
#[allow(dead_code)]
fn embeds(m: &mut ModuleBuilder) {
    m.document_files([
        ("certificates/course.typ", &b"x"[..]),
        ("images/seal.png", &b"x"[..]),
    ]);
}

#[test]
fn the_documented_api_exists() {
    let _ = DocumentsModule::for_root(DocumentsOptions::default());
    let _ = DocumentsModule::from_config();

    let templates = Templates::new(vec![PathBuf::from("resources/documents")], [("a.typ", "x")]);
    let documents = Documents::new(templates.clone())
        .default_engine(EngineKind::Typst)
        .with_engine(Arc::new(TypstEngine::new(templates.clone())));
    let _ = Documents::from_options(&DocumentsOptions::default(), templates.clone());
    let _ = documents;

    let render = Render::template("certificates/course")
        .data(json!({"name": "Ada"}))
        .locale("ar")
        .title("Certificate")
        .engine(EngineKind::Chrome)
        .print(PrintOptions::letter());
    assert_eq!(render.direction(), "rtl");
    assert_eq!(render.name(), "certificates/course");
    assert!(matches!(
        Render::typst("x").source,
        TemplateSource::Typst(_)
    ));
    assert!(matches!(Render::html("x").source, TemplateSource::Html(_)));
    assert!(matches!(render.source, TemplateSource::Template(_)));
    assert!(is_rtl("ar-EG"));
    assert_eq!((TYPST_EXTENSION, HTML_EXTENSION), ("typ", "html"));

    let print = PrintOptions::a4()
        .landscape()
        .margin(Margins::all(15.0))
        .footer("<span class=\"pageNumber\"></span>")
        .header("<span class=\"title\"></span>");
    assert_eq!(print.paper, Paper::A4);
    let _ = [Paper::A3, Paper::A5, Paper::Letter, Paper::Legal];

    let chrome = ChromeEngine::new(templates.clone(), ChromeOptions::default());
    let _ = chrome.executable();
    let _ = TypstEngine::new(templates)
        .bundled_fonts(true)
        .font_dirs(vec![])
        .packages_dir(None)
        .timeout(std::time::Duration::from_secs(60));

    assert_eq!(CHROME_ENV, "NAVREE_BROWSER_CHROME");
    assert_eq!(DOCUMENT_ORIGIN, "https://documents.navree.invalid");
    assert_eq!(FONTS_STYLESHEET, "/_navree/fonts.css");
    assert_eq!(FAILED_SUFFIX, ".failed");
    assert_eq!(PDF_CONTENT_TYPE, "application/pdf");
    assert!(DOCX_CONTENT_TYPE.ends_with("wordprocessingml.document"));
    assert_eq!(
        <RenderDocument as navree_contracts::Job>::NAME,
        "documents.render"
    );

    let pdf = Pdf::from_lines(["Invoice 42"]);
    let both = Pdf::merge([&pdf, &pdf]).unwrap().append(&pdf).unwrap();
    let stamped = both
        .stamp(
            &Stamp::text("PAID")
                .color(0.0, 0.5, 0.0)
                .opacity(0.4)
                .size(60.0)
                .rotate(30.0)
                .pages(Pages::Only(vec![2, 3])),
        )
        .unwrap()
        .stamp(
            &Stamp::text("Copy 1 of 2")
                .at(Position::TopRight)
                .pages(Pages::First),
        )
        .unwrap()
        .overlay(&pdf, Pages::Last)
        .unwrap()
        .with_metadata("Report", Some("Navree"))
        .unwrap();
    assert_eq!(stamped.page_count().unwrap(), 3);
    assert_eq!(stamped.page_texts().unwrap().len(), 3);
    assert!(stamped.text().unwrap().contains("PAID"));
    assert_eq!(stamped.title().unwrap().as_deref(), Some("Report"));
    let _ = [
        Position::Center,
        Position::Top,
        Position::Bottom,
        Position::TopLeft,
    ];
    let _ = [Position::BottomLeft, Position::BottomRight];
    let _ = Pages::All;
    let _ = Pages::Last;
    let _: Download = pdf.clone().download("a.pdf");
    let _: Download = pdf.inline("a.pdf");
    let _ = Download::new(&b"a,b"[..], "text/csv", "deals.csv");

    let file: DocxFile = Docx::new()
        .title("t")
        .author("a")
        .locale("en")
        .font("Noto Sans")
        .paper(Paper::Letter)
        .landscape()
        .margins(Margins::all(20.0))
        .header("h")
        .footer("f")
        .footer_page_numbers()
        .heading(1, "Report")
        .paragraph(
            Paragraph::new()
                .text("a")
                .bold("b")
                .italic("c")
                .run(Run::new("d").size(9.0).color("C00000").underline())
                .align(Align::Center),
        )
        .bullets(["x"])
        .numbered(["y"])
        .table(Table::new().header(["a"]).row(["b"]))
        .image(
            Image::new(
                std::fs::read(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/templates/images/logo.png"
                ))
                .unwrap(),
            )
            .width_mm(40.0),
        )
        .page_break()
        .build()
        .unwrap();
    assert!(file.text().unwrap().contains("Report"));
    let _: Download = file.download("report.docx");
    let _ = [Align::Start, Align::End, Align::Justify];

    let fake = DocumentsFake::new().fail("reports/audit", "broken");
    let _ = DocumentsFake::with_templates(Templates::default());
    let _: Documents = fake.documents();
    let _: FakeEngine = fake.engine(EngineKind::Typst);
    let _: Vec<Render> = fake.rendered();
    let _: Vec<String> = fake.rendered_names();
    let _: Vec<QueuedRender> = fake.queued();
    fake.assert_nothing_rendered();
    fake.assert_not_rendered("reports/audit");
}

#[test]
fn every_documented_configuration_key_deserializes() {
    let options: DocumentsOptions = serde_json::from_value(json!({
        "templates": ["resources/documents"],
        "engine": "typst",
        "disk": "documents",
        "typst": {
            "bundled_fonts": true,
            "font_dirs": ["resources/fonts"],
            "packages_dir": "resources/typst-packages",
            "timeout_secs": 60,
        },
        "chrome": {
            "path": "/usr/bin/chromium",
            "endpoint": "ws://chrome:9222/devtools/browser/x",
            "pool_size": 2,
            "timeout_secs": 30,
            "no_sandbox": false,
            "allow_network": false,
            "args": [],
            "print": {
                "paper": "a4",
                "landscape": false,
                "margin": {"top": 10, "right": 10, "bottom": 10, "left": 10},
                "print_background": true,
                "prefer_css_page_size": true,
                "scale": 1.0,
            },
        },
    }))
    .unwrap();
    assert_eq!(options.disk.as_deref(), Some("documents"));
    let _: TypstOptions = options.typst;
    assert_eq!(options.chrome.pool_size, 2);
}

#[test]
fn errors_and_statuses() {
    let errors = [
        DocumentsError::TemplateNotFound {
            name: "x".into(),
            tried: vec![],
        },
        DocumentsError::Template {
            file: "x.typ".into(),
            line: Some(1),
            column: Some(1),
            message: "m".into(),
        },
        DocumentsError::Data("d".into()),
        DocumentsError::EngineUnavailable {
            engine: "chrome",
            reason: "r".into(),
        },
        DocumentsError::Engine {
            engine: "chrome",
            message: "m".into(),
        },
        DocumentsError::Timeout {
            engine: "chrome",
            secs: 30,
        },
        DocumentsError::Pdf("p".into()),
        DocumentsError::Docx("d".into()),
        DocumentsError::Queue("q".into()),
    ];
    let statuses: Vec<u16> = errors.iter().map(|e| e.status().as_u16()).collect();
    assert_eq!(statuses, vec![500, 500, 500, 503, 500, 503, 500, 500, 500]);
    let _ = [
        DocumentStatus::Pending,
        DocumentStatus::Ready { size: 1 },
        DocumentStatus::Failed { error: "e".into() },
    ];
}

#[tokio::test]
async fn the_documented_commands_are_registered() {
    let app = navree_testing::TestApp::builder()
        .module(DocumentsModule::from_config())
        .build()
        .await;
    let names: Vec<&str> = app.ctx().commands().iter().map(|c| c.name).collect();
    assert!(names.contains(&"documents:render"), "{names:?}");
    assert!(names.contains(&"documents:fonts"), "{names:?}");
}

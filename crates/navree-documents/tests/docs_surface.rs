//! Every public item the documents docs page names
//! (`apps/website/content/docs/packages/documents.mdx`): types, methods,
//! configuration keys, commands, the job and the constants. A rename fails
//! this file.

#![cfg(all(feature = "native", feature = "chrome", feature = "docx"))]

use std::path::PathBuf;
use std::sync::Arc;

use navree_documents::docx::{Align, Image, Paragraph, Run, Table};
use navree_documents::native;
use navree_documents::{
    CHROME_ENV, ChromeEngine, ChromeOptions, DOCUMENT_ORIGIN, DOCX_CONTENT_TYPE, DocumentStatus,
    Documents, DocumentsError, DocumentsFake, DocumentsModule, DocumentsOptions, Docx, DocxFile,
    Download, EngineKind, FAILED_SUFFIX, FONTS_STYLESHEET, FakeEngine, HTML_EXTENSION, Margins,
    ModuleBuilderDocumentsExt, NativeEngine, NativeOptions, PDF_CONTENT_TYPE, Pages, Paper, Pdf,
    PdfDocument, PdfEngine, Position, PrintOptions, QueuedDocument, QueuedRender,
    RENDER_FAILED_MESSAGE, Render, RenderDocument, Stamp, StoredDocument, TemplateSource,
    Templates, is_rtl,
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
    let _: Option<uuid::Uuid> = queued.operation_id;
    let _: DocumentStatus = documents.status(&queued).await?;
    let _: Vec<String> = documents.font_families().await?;
    let _: Pdf = documents.render_document(&PdfDocument::new()).await?;
    documents.shutdown().await;
    let _ = documents.disk()?;
    let _: &Templates = documents.templates();
    let _: Option<Arc<dyn PdfEngine>> = documents.engine(EngineKind::Chrome);
    Ok(())
}

/// `queue_operation` (compiled, never run).
#[allow(dead_code)]
async fn behind_an_operation(
    documents: &Documents,
    operations: &navree_queue::Operations,
    uow: &navree_db::Uow,
    render: Render,
) -> Result<(), DocumentsError> {
    let op: navree_queue::OperationRecord = documents
        .queue_operation(
            operations,
            uow,
            navree_queue::Operation::new("audit.export"),
            render,
            "exports/audit-q3.pdf",
        )
        .await?;
    let _ = op.result;
    Ok(())
}

/// `m.document_files(..)` (compiled, never run).
#[allow(dead_code)]
fn embeds(m: &mut ModuleBuilder) {
    m.document_files([
        ("certificates/course.html", &b"x"[..]),
        ("images/seal.png", &b"x"[..]),
    ]);
}

#[test]
fn the_documented_api_exists() {
    let _ = DocumentsModule::for_root(DocumentsOptions::default());
    let _ = DocumentsModule::from_config();

    let templates = Templates::new(
        vec![PathBuf::from("resources/documents")],
        [("a.html", "x")],
    );
    let documents = Documents::new(templates.clone())
        .default_engine(EngineKind::Native)
        .with_engine(Arc::new(NativeEngine::new(templates.clone())));
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
    assert!(matches!(Render::html("x").source, TemplateSource::Html(_)));
    assert!(matches!(render.source, TemplateSource::Template(_)));
    assert!(is_rtl("ar-EG"));
    assert_eq!(HTML_EXTENSION, "html");

    let print = PrintOptions::a4()
        .landscape()
        .margin(Margins::all(15.0))
        .footer("<span class=\"pageNumber\"></span>")
        .header("<span class=\"title\"></span>");
    assert_eq!(print.paper, Paper::A4);
    let _ = [Paper::A3, Paper::A5, Paper::Letter, Paper::Legal];

    let chrome = ChromeEngine::new(templates.clone(), ChromeOptions::default());
    let _ = chrome.executable();
    let native = NativeEngine::new(templates.clone())
        .bundled_fonts(true)
        .font_dirs(vec![])
        .print(PrintOptions::a4())
        .clock(Arc::new(navree_contracts::SystemClock))
        .timeout(std::time::Duration::from_secs(60));
    let _ = NativeEngine::with_options(templates, NativeOptions::default());
    let _: Vec<String> = native.font_families();
    let _: &NativeOptions = native.options();

    let built: Pdf = PdfDocument::new()
        .title("Quarterly audit report")
        .author("Acme")
        .locale("en")
        .font("Noto Serif")
        .font_size(10.5)
        .paper(Paper::A4)
        .landscape()
        .margins(Margins::all(20.0))
        .header(
            native::Paragraph::new()
                .run(native::Run::title())
                .align(native::Align::End),
        )
        .footer_page_numbers()
        .heading(1, "Report")
        .paragraph(
            native::Paragraph::new()
                .text("a")
                .bold("b")
                .italic("c")
                .link("d", "https://example.com")
                .run(
                    native::Run::new("e")
                        .size(9.0)
                        .color("C00000")
                        .underline()
                        .strike()
                        .font("Noto Sans Mono"),
                )
                .line_break()
                .align(native::Align::Justify),
        )
        .bullets(["x"])
        .numbered(["y"])
        .list(
            native::List::new(native::ListKind::LowerRoman)
                .start(3)
                .item("z"),
        )
        .table(
            native::Table::new()
                .header(["a", "b"])
                .row([
                    native::Cell::new("1").colspan(1).align(native::Align::End),
                    native::Cell::new("2")
                        .valign(native::VAlign::Middle)
                        .background(native::Color::rgb(0xF2, 0xF4, 0xF7)),
                ])
                .widths([1.0, 1.0])
                .borders(native::Borders::Grid)
                .padding_mm(1.5, 2.0),
        )
        .image(
            native::Image::new(
                std::fs::read(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/templates/images/acme-logo.png"
                ))
                .unwrap(),
            )
            .width_mm(40.0)
            .alt("Acme"),
        )
        .rule()
        .space_mm(5.0)
        .page_break()
        .paragraph(native::Paragraph::from("End").direction(native::Direction::Ltr))
        .build()
        .unwrap();
    assert!(built.text().unwrap().contains("Report"));
    let _ = [native::Borders::Rows, native::Borders::None];
    let _ = [native::ListKind::Bullets, native::ListKind::Decimal];
    let _ = [native::VAlign::Top, native::VAlign::Bottom];

    assert_eq!(CHROME_ENV, "NAVREE_BROWSER_CHROME");
    assert_eq!(DOCUMENT_ORIGIN, "https://documents.navree.invalid");
    assert_eq!(FONTS_STYLESHEET, "/_navree/fonts.css");
    assert_eq!(FAILED_SUFFIX, ".failed");
    assert_eq!(RENDER_FAILED_MESSAGE, "The document could not be rendered.");
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
    let _: FakeEngine = fake.engine(EngineKind::Native);
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
        "engine": "native",
        "disk": "documents",
        "native": {
            "bundled_fonts": true,
            "font_dirs": ["resources/fonts"],
            "timeout_secs": 60,
            "print": {"paper": "a4", "margin": {"top": 20, "right": 20, "bottom": 20, "left": 20}},
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
    let _: NativeOptions = options.native;
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
            file: "x.html".into(),
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
        DocumentsError::TooLarge("t".into()),
        DocumentsError::Docx("d".into()),
        DocumentsError::Queue("q".into()),
    ];
    let statuses: Vec<u16> = errors.iter().map(|e| e.status().as_u16()).collect();
    // A PDF that cannot be read is the caller's: 400; one over a limit: 413.
    assert_eq!(
        statuses,
        vec![500, 500, 500, 503, 500, 503, 400, 413, 500, 500]
    );
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

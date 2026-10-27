//! The native engine: templates and data, the HTML subset, page setup,
//! fonts and scripts, the PDF it writes (fonts, tags, metadata), the
//! builder, and the samples (an invoice, a long report, Arabic, Hebrew and
//! Hindi). With `NAVREE_DOCUMENTS_SAMPLES=<dir>`, the samples are written
//! there.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use lopdf::{Dictionary, Document, Object};
use navree_contracts::FixedClock;
use navree_documents::native::{Align, Cell, Image, List, Paragraph, Run, Table};
use navree_documents::{
    Documents, DocumentsError, EngineKind, Margins, NativeEngine, Paper, Pdf, PdfDocument,
    PrintOptions, Render, Templates,
};
use serde_json::{Value, json};

use crate::support::{assert_golden, fixture_templates, fixtures_dir};

fn engine() -> NativeEngine {
    NativeEngine::new(fixture_templates())
}

fn load(pdf: &Pdf) -> Document {
    Document::load_mem(pdf).unwrap()
}

fn catalog(document: &Document) -> &Dictionary {
    let root = document.trailer.get(b"Root").unwrap().as_reference().unwrap();
    document.get_dictionary(root).unwrap()
}

fn resolve<'a>(document: &'a Document, object: &'a Object) -> &'a Object {
    document.dereference(object).map(|(_, o)| o).unwrap_or(object)
}

/// A string of the Info dictionary.
fn info(document: &Document, key: &[u8]) -> Option<String> {
    let info = resolve(document, document.trailer.get(b"Info").ok()?).as_dict().ok()?;
    let bytes = info.get(key).ok()?.as_str().ok()?;
    Some(match bytes.strip_prefix(&[0xfe, 0xff]) {
        Some(utf16) => String::from_utf16_lossy(
            &utf16
                .chunks(2)
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        ),
        None => String::from_utf8_lossy(bytes).into_owned(),
    })
}

/// Every font the document uses: (base font, embedded font file).
fn fonts(document: &Document) -> Vec<(String, bool)> {
    let mut fonts = Vec::new();
    for object in document.objects.values() {
        let Ok(dict) = object.as_dict() else { continue };
        if dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"Font".as_slice())
            || dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type0".as_slice())
        {
            continue;
        }
        let name = String::from_utf8_lossy(dict.get(b"BaseFont").unwrap().as_name().unwrap()).into_owned();
        let descendant = resolve(document, dict.get(b"DescendantFonts").unwrap())
            .as_array()
            .unwrap()[0]
            .clone();
        let descendant = resolve(document, &descendant).as_dict().unwrap();
        let descriptor = resolve(document, descendant.get(b"FontDescriptor").unwrap())
            .as_dict()
            .unwrap();
        let embedded = [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"]
            .iter()
            .any(|key| descriptor.has(key));
        fonts.push((name, embedded));
    }
    fonts.sort();
    fonts.dedup();
    fonts
}

/// (width, height) of page `index` (from 0).
fn page_size(document: &Document, index: usize) -> (f32, f32) {
    let id = document.get_pages()[&(index as u32 + 1)];
    let page = document.get_dictionary(id).unwrap();
    let media = resolve(document, page.get(b"MediaBox").unwrap()).as_array().unwrap();
    let n = |i: usize| resolve(document, &media[i]).as_float().unwrap();
    (n(2) - n(0), n(3) - n(1))
}

/// How many images the document draws (an image's alpha channel, its
/// soft mask, is an image too and is not counted).
fn images(document: &Document) -> usize {
    let streams = || document.objects.iter().filter_map(|(id, o)| Some((*id, o.as_stream().ok()?)));
    let masks: std::collections::HashSet<lopdf::ObjectId> = streams()
        .filter_map(|(_, s)| s.dict.get(b"SMask").and_then(Object::as_reference).ok())
        .collect();
    streams()
        .filter(|(id, s)| {
            s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image".as_slice())
                && !masks.contains(id)
        })
        .count()
}

/// The URIs of every link annotation.
fn links(document: &Document) -> Vec<String> {
    let mut uris = Vec::new();
    for object in document.objects.values() {
        let Ok(dict) = object.as_dict() else { continue };
        if dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Link".as_slice()) {
            continue;
        }
        if let Ok(action) = dict.get(b"A")
            && let Ok(action) = resolve(document, action).as_dict()
            && let Ok(uri) = action.get(b"URI").and_then(Object::as_str)
        {
            uris.push(String::from_utf8_lossy(uri).into_owned());
        }
    }
    uris
}

/// Writes a sample PDF where `NAVREE_DOCUMENTS_SAMPLES` points.
fn save_sample(name: &str, pdf: &Pdf) {
    if let Some(dir) = std::env::var_os("NAVREE_DOCUMENTS_SAMPLES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), pdf.bytes()).unwrap();
    }
}

#[tokio::test]
async fn a_template_renders_with_its_data_and_its_layout() {
    let pdf = engine()
        .render(&Render::template("certificates/classic").data(json!({
            "name": "Ada Lovelace",
            "course": "Analytical Engines",
            "completed_on": "26 October 2026",
        })))
        .await
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(pdf.page_count().unwrap(), 1);
    assert_eq!(
        pdf.text().unwrap(),
        "Certificate of completion\n\
         This certifies that Ada Lovelace completed Analytical Engines on 26 October 2026."
    );
    // `{% extends %}` brought the layout's <title> and @page rule.
    assert_eq!(pdf.title().unwrap().as_deref(), Some("Certificate"));
    let (w, h) = page_size(&load(&pdf), 0);
    assert!((w - 595.3).abs() < 1.0 && (h - 841.9).abs() < 1.0, "{w} x {h}");
    // The name may carry the extension.
    let pdf = engine()
        .render(&Render::template("certificates/classic.html").data(json!({
            "name": "Grace Hopper", "course": "Compilers", "completed_on": "today",
        })))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("Grace Hopper completed Compilers"));
}

#[tokio::test]
async fn unknown_and_broken_templates_name_the_file() {
    match engine().render(&Render::template("certificates/missing")).await {
        Err(DocumentsError::TemplateNotFound { name, tried }) => {
            assert_eq!(name, "certificates/missing");
            assert_eq!(tried, vec!["certificates/missing.html"]);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        engine().render(&Render::template("../escape")).await,
        Err(DocumentsError::TemplateNotFound { .. })
    ));
    // An undefined variable: the file and the line.
    match engine()
        .render(&Render::template("broken").data(json!({"name": "Ada"})))
        .await
    {
        Err(DocumentsError::Template {
            file,
            line,
            message,
            ..
        }) => {
            assert_eq!(file, "broken.html");
            assert_eq!(line, Some(3));
            assert!(message.contains("undefined"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    // A syntax error.
    let engine = NativeEngine::new(Templates::from_files([("bad.html", "<p>{{ name </p>")]));
    match engine.render(&Render::template("bad")).await {
        Err(DocumentsError::Template { file, .. }) => assert_eq!(file, "bad.html"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn embedded_templates_render_and_files_override_them() {
    let templates = Templates::new(
        vec![fixtures_dir()],
        [
            ("receipts/simple.html", "<p>Receipt for {{ name }}</p>"),
            ("certificates/classic.html", "<p>EMBEDDED</p>"),
        ],
    );
    let engine = NativeEngine::new(templates);
    let pdf = engine
        .render(&Render::template("receipts/simple").data(json!({"name": "Ada"})))
        .await
        .unwrap();
    assert_eq!(pdf.text().unwrap(), "Receipt for Ada");
    let pdf = engine
        .render(&Render::template("certificates/classic").data(json!({
            "name": "Ada", "course": "x", "completed_on": "y",
        })))
        .await
        .unwrap();
    assert!(!pdf.text().unwrap().contains("EMBEDDED"));
}

#[tokio::test]
async fn the_html_subset_prints() {
    let html = r#"
        <h1>Quarterly <em>audit</em></h1>
        <p>Prepared for <strong>Acme Ltd</strong> by <a href="https://audit.example/q3">the audit team</a>.<br>Second line.</p>
        <ul><li>Every control was tested<li>Two findings are open
            <ol><li>Rotate the keys</li><li>Restore a backup</li></ol></li></ul>
        <hr>
        <table>
          <thead><tr><th>Control</th><th>Status</th></tr></thead>
          <tbody>
            <tr><td>Multi-factor sign-in</td><td>Passed</td></tr>
            <tr><td colspan="2" style="text-align: center">All systems in scope</td></tr>
          </tbody>
        </table>
        <p>Run <code>queue:work</code> on every node.</p>
        <img src="/images/acme-logo.png" alt="Acme">
        <img src="/images/photo.jpg" width="50%">
        <img src="missing.png"><img src="https://tracker.example/pixel.png">
        <div class="page-break"></div>
        <p style="color: #c00000; font-size: 14pt; text-align: right">Signed off.</p>"#;
    let pdf = engine().render(&Render::html(html)).await.unwrap();
    let pages = pdf.page_texts().unwrap();
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(
        pages[0],
        "Quarterly audit\n\
         Prepared for Acme Ltd by the audit team.\n\
         Second line.\n\
         • Every control was tested\n\
         • Two findings are open\n\
         1. Rotate the keys\n\
         2. Restore a backup\n\
         Control Status\n\
         Multi-factor sign-in Passed\n\
         All systems in scope\n\
         Run queue:work on every node."
    );
    assert_eq!(pages[1], "Signed off.");
    let document = load(&pdf);
    assert_eq!(links(&document), vec!["https://audit.example/q3"]);
    assert_eq!(images(&document), 2, "the PNG and the JPEG; missing and remote ones are skipped");
    let names: Vec<String> = fonts(&document).into_iter().map(|(n, _)| n).collect();
    // The heading's <em> is bold italic.
    for face in [
        "NotoSans-Bold",
        "NotoSans-BoldItalic",
        "NotoSans-Regular",
        "NotoSansMono-Regular",
    ] {
        assert!(names.iter().any(|n| n.ends_with(face)), "{face}: {names:?}");
    }
}

#[tokio::test]
async fn fonts_are_embedded_as_subsets() {
    let pdf = engine()
        .render(&Render::html("<p>Invoice 42 — فاتورة — חשבונית — चालान</p>"))
        .await
        .unwrap();
    let fonts = fonts(&load(&pdf));
    assert_eq!(fonts.len(), 4, "{fonts:?}");
    for (name, embedded) in &fonts {
        assert!(embedded, "{name} is embedded");
        // A subset's name starts with six capitals and a plus sign.
        let (tag, _) = name.split_once('+').unwrap_or_else(|| panic!("{name}"));
        assert!(tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()), "{name}");
    }
    // Subsets: the whole document is far smaller than the four fonts.
    assert!(pdf.len() < 60_000, "{} bytes", pdf.len());
}

#[tokio::test]
async fn the_pdf_is_tagged_and_carries_its_metadata() {
    let html = r#"<html lang="en-GB"><head><title>Audit</title><meta name="author" content="Acme audit team"></head>
        <body><h1>Audit</h1><p>Body text.</p><ul><li>One</li></ul>
        <table><tr><th>A</th></tr><tr><td>1</td></tr></table><img src="/images/acme-logo.png" alt="Acme"></body></html>"#;
    let pdf = engine().render(&Render::html(html)).await.unwrap();
    let document = load(&pdf);
    let catalog = catalog(&document);
    assert!(catalog.has(b"StructTreeRoot"));
    let mark_info = resolve(&document, catalog.get(b"MarkInfo").unwrap()).as_dict().unwrap();
    assert_eq!(mark_info.get(b"Marked").unwrap().as_bool().unwrap(), true);
    assert_eq!(catalog.get(b"Lang").unwrap().as_str().unwrap(), b"en-GB");
    assert_eq!(pdf.title().unwrap().as_deref(), Some("Audit"));
    assert_eq!(info(&document, b"Author").as_deref(), Some("Acme audit team"));
    assert!(catalog.has(b"Outlines"), "headings make the outline");
    // The structure: every kind the HTML used.
    let mut roles: Vec<String> = document
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.get(b"Type").and_then(Object::as_name).ok() == Some(b"StructElem".as_slice()))
        .map(|d| String::from_utf8_lossy(d.get(b"S").unwrap().as_name().unwrap()).into_owned())
        .collect();
    roles.sort();
    roles.dedup();
    for role in ["H1", "P", "L", "LI", "Lbl", "LBody", "Table", "TR", "TH", "TD", "Figure"] {
        assert!(roles.iter().any(|r| r == role), "{role}: {roles:?}");
    }
    // A render's title wins over the template's.
    let pdf = engine()
        .render(&Render::html(html).title("Quarterly audit"))
        .await
        .unwrap();
    assert_eq!(pdf.title().unwrap().as_deref(), Some("Quarterly audit"));
}

#[tokio::test]
async fn the_locale_sets_the_language_and_the_direction() {
    let pdf = engine()
        .render(&Render::html("<p>مرحبا بالعالم 2026</p>").locale("ar"))
        .await
        .unwrap();
    assert_eq!(pdf.text().unwrap(), "مرحبا بالعالم 2026");
    let document = load(&pdf);
    assert_eq!(catalog(&document).get(b"Lang").unwrap().as_str().unwrap(), b"ar");
    // Templates get the locale and direction as variables.
    let engine = NativeEngine::new(Templates::from_files([(
        "x.html",
        "<html lang=\"{{ locale }}\" dir=\"{{ dir }}\"><p>{{ locale }} {{ dir }}</p></html>",
    )]));
    let pdf = engine.render(&Render::template("x").locale("he")).await.unwrap();
    assert_eq!(pdf.text().unwrap(), "he rtl");
}

#[tokio::test]
async fn arabic_renders_right_to_left_with_the_bundled_fonts() {
    let pdf = engine()
        .render(
            &Render::template("certificates/multilingual")
                .data(json!({
                    "title": "شهادة إتمام",
                    "name": "مريم العلي",
                    "course": "أساسيات الأمن السيبراني",
                }))
                .locale("ar"),
        )
        .await
        .unwrap();
    assert_golden("multilingual-ar.txt", &pdf.text().unwrap());
    // @page { size: A5 landscape } in the template.
    let (w, h) = page_size(&load(&pdf), 0);
    assert!((w - 595.3).abs() < 1.0 && (h - 419.5).abs() < 1.0, "{w} x {h}");
}

#[tokio::test]
async fn devanagari_and_hebrew_render_with_the_bundled_fonts() {
    let pdf = engine()
        .render(
            &Render::template("certificates/multilingual")
                .data(json!({
                    "title": "प्रमाण पत्र",
                    "name": "अनन्या शर्मा",
                    "course": "साइबर सुरक्षा की मूल बातें",
                }))
                .locale("hi"),
        )
        .await
        .unwrap();
    assert_golden("multilingual-hi.txt", &pdf.text().unwrap());
    let pdf = engine()
        .render(&Render::html("<p>שלום עולם, 42 תלמידים</p>").locale("he"))
        .await
        .unwrap();
    assert_eq!(pdf.text().unwrap(), "שלום עולם, 42 תלמידים");
}

#[tokio::test]
async fn mixed_scripts_fall_back_through_the_bundled_families() {
    let pdf = engine()
        .render(&Render::html("<p>Invoice 42 — فاتورة — चालान</p>"))
        .await
        .unwrap();
    assert_golden("mixed-scripts.txt", &pdf.text().unwrap());
}

#[tokio::test]
async fn fonts_come_from_the_bundle_and_the_font_directories() {
    let families = engine().font_families();
    for family in [
        "Noto Sans",
        "Noto Serif",
        "Noto Sans Mono",
        "Noto Sans Arabic",
        "Noto Sans Hebrew",
        "Noto Sans Devanagari",
    ] {
        assert!(families.iter().any(|f| f == family), "{families:?}");
    }
    let without = engine().bundled_fonts(false);
    assert!(without.font_families().is_empty());
    match without.render(&Render::html("<p>x</p>")).await {
        Err(DocumentsError::EngineUnavailable { engine, reason }) => {
            assert_eq!(engine, "native");
            assert!(reason.contains("font"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/NotoSansHebrew-Regular.ttf"),
        dir.path().join("Hebrew.ttf"),
    )
    .unwrap();
    std::fs::write(dir.path().join("broken.otf"), b"not a font").unwrap();
    let custom = engine()
        .bundled_fonts(false)
        .font_dirs(vec![dir.path().to_path_buf()]);
    assert_eq!(custom.font_families(), vec!["Noto Sans Hebrew".to_owned()]);
    let pdf = custom
        .render(&Render::html("<p style=\"font-family: 'Noto Sans Hebrew'\">שלום</p>"))
        .await
        .unwrap();
    assert_eq!(pdf.text().unwrap(), "שלום");
}

#[tokio::test]
async fn page_settings_come_from_the_render_then_the_template() {
    let letter = engine()
        .render(&Render::html("<p>x</p>").print(PrintOptions::letter().landscape()))
        .await
        .unwrap();
    let (w, h) = page_size(&load(&letter), 0);
    assert!((w - 792.0).abs() < 0.5 && (h - 612.0).abs() < 0.5, "{w} x {h}");
    // @page wins while prefer_css_page_size is on…
    let a5 = "<style>@page { size: A5; margin: 10mm }</style><p>x</p>";
    let pdf = engine()
        .render(&Render::html(a5).print(PrintOptions::letter()))
        .await
        .unwrap();
    let (w, h) = page_size(&load(&pdf), 0);
    assert!((w - 419.5).abs() < 1.0 && (h - 595.3).abs() < 1.0, "{w} x {h}");
    // …and not when it is off.
    let mut print = PrintOptions::letter();
    print.prefer_css_page_size = false;
    let pdf = engine().render(&Render::html(a5).print(print)).await.unwrap();
    let (w, _) = page_size(&load(&pdf), 0);
    assert!((w - 612.0).abs() < 0.5, "{w}");
    // A custom size from the engine's own settings.
    let custom = engine().print(PrintOptions {
        paper: Paper::Custom {
            width_mm: 100.0,
            height_mm: 50.0,
        },
        margin: Margins::all(5.0),
        ..PrintOptions::default()
    });
    let pdf = custom.render(&Render::html("<p>Label</p>")).await.unwrap();
    let (w, h) = page_size(&load(&pdf), 0);
    assert!((w - 283.5).abs() < 0.5 && (h - 141.7).abs() < 0.5, "{w} x {h}");
}

#[tokio::test]
async fn headers_and_footers_number_the_pages() {
    let rows: String = (1..=80)
        .map(|i| format!("<tr><td>Control {i}</td><td>Passed</td></tr>"))
        .collect();
    let html = format!(
        "<header><p>Acme audit</p></header>\
         <table><thead><tr><th>Control</th><th>Status</th></tr></thead><tbody>{rows}</tbody></table>\
         <footer><p>Page <span class=\"pageNumber\"></span> of <span class=\"totalPages\"></span></p></footer>"
    );
    let pdf = engine().render(&Render::html(html)).await.unwrap();
    let pages = pdf.page_texts().unwrap();
    let count = pages.len();
    assert!(count >= 2, "{count}");
    let mut seen = 0;
    for (index, page) in pages.iter().enumerate() {
        let lines: Vec<&str> = page.lines().collect();
        assert_eq!(lines[0], "Acme audit");
        assert_eq!(lines[1], "Control Status", "the header row repeats");
        assert_eq!(*lines.last().unwrap(), format!("Page {} of {count}", index + 1));
        seen += lines.iter().filter(|l| l.starts_with("Control ") && l.ends_with("Passed")).count();
    }
    assert_eq!(seen, 80, "every row once");
    // The print options' footer, with Chrome's classes, works the same.
    let pdf = engine()
        .render(&Render::html("<p>Body</p>").print(
            PrintOptions::a4().footer(
                "<div style=\"font-size: 8pt; text-align: right\">Page <span class=\"pageNumber\"></span> of <span class=\"totalPages\"></span></div>",
            ),
        ))
        .await
        .unwrap();
    assert_eq!(pdf.text().unwrap(), "Body\nPage 1 of 1");
}

#[tokio::test]
async fn dates_come_from_the_clock() {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 3, 14, 9, 26, 53).unwrap(),
    ));
    let pdf = engine()
        .clock(clock)
        .render(&Render::html(
            "<p>Body</p><footer><p>Printed <span class=\"date\"></span></p></footer>",
        ))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("Printed 2026-03-14"));
    let created = info(&load(&pdf), b"CreationDate").unwrap();
    assert!(created.starts_with("D:20260314092653"), "{created}");
}

#[tokio::test]
async fn images_that_do_not_decode_name_their_file() {
    let engine = NativeEngine::new(Templates::from_files([
        ("bad.html", &b"<img src=\"images/x.png\">"[..]),
        ("images/x.png", &b"not an image"[..]),
    ]));
    match engine.render(&Render::template("bad")).await {
        Err(DocumentsError::Template { file, message, .. }) => {
            assert_eq!(file, "images/x.png");
            assert!(message.contains("image"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    match PdfDocument::new().image(Image::new(&b"junk"[..])).build() {
        Err(DocumentsError::Engine { engine, .. }) => assert_eq!(engine, "native"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn data_that_is_not_json_fails_before_rendering() {
    let mut map = std::collections::BTreeMap::new();
    map.insert((1, 2), "tuple keys are not JSON");
    let error = engine()
        .render(&Render::template("certificates/classic").data(map))
        .await
        .unwrap_err();
    assert!(matches!(error, DocumentsError::Data(_)), "{error:?}");
}

#[tokio::test]
async fn renders_run_concurrently_and_time_out() {
    let engine = engine();
    let renders = (0..4).map(|i| {
        let engine = engine.clone();
        tokio::spawn(async move {
            engine
                .render(&Render::html(format!("<p>Document {i}</p>")))
                .await
                .unwrap()
                .text()
                .unwrap()
        })
    });
    for (i, handle) in renders.enumerate() {
        assert_eq!(handle.await.unwrap(), format!("Document {i}"));
    }
    let rows: String = (0..3000).map(|i| format!("<tr><td>{i}</td></tr>")).collect();
    let error = engine
        .timeout(Duration::ZERO)
        .render(&Render::html(format!("<table>{rows}</table>")))
        .await
        .unwrap_err();
    assert!(
        matches!(error, DocumentsError::Timeout { engine: "native", .. }),
        "{error:?}"
    );
    assert_eq!(error.status(), http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn documents_render_built_documents_with_the_configured_engine() {
    let documents = Documents::new(fixture_templates());
    assert_eq!(
        documents.engine_for(&Render::html("<p>x</p>")).unwrap(),
        EngineKind::Native
    );
    let pdf = documents
        .render_document(&PdfDocument::new().heading(1, "Built").paragraph("In code."))
        .await
        .unwrap();
    assert_eq!(pdf.text().unwrap(), "Built\nIn code.");
}

/// The invoice sample: an HTML template with a logo, layout tables, line
/// items, totals and a numbered footer.
#[tokio::test]
async fn sample_invoice() {
    let lines: Vec<Value> = [
        ("Security audit", "Quarterly review of 42 controls", 1, 450_000),
        ("Penetration test", "External perimeter, 3 days", 3, 120_000),
        ("Multi-factor rollout", "Configuration and training", 12, 9_500),
        ("Backup restore drill", "Database and object storage", 2, 35_000),
        ("Incident response retainer", "October 2026", 1, 80_000),
        ("Travel", "Two on-site visits", 2, 18_250),
    ]
    .iter()
    .map(|(description, detail, qty, unit)| {
        json!({
            "description": description,
            "detail": detail,
            "qty": qty,
            "unit_cents": unit,
            "amount_cents": qty * unit,
        })
    })
    .collect();
    let subtotal: i64 = lines.iter().map(|l| l["amount_cents"].as_i64().unwrap()).sum();
    let vat = subtotal * 20 / 100;
    let data = json!({
        "number": "INV-2026-0042",
        "issued": "27 October 2026",
        "due": "26 November 2026",
        "currency": "EUR",
        "company": {
            "name": "Acme Security Ltd",
            "street": "12 Harbour Road",
            "city": "Dublin D02 X285, Ireland",
            "email": "billing@acme.example",
            "iban": "IE29 AIBK 9311 5212 3456 78",
            "bic": "AIBKIE2D",
        },
        "customer": {
            "name": "Northwind Traders GmbH",
            "street": "Speicherstraße 7",
            "city": "20457 Hamburg, Germany",
            "vat": "DE 123 456 789",
        },
        "lines": lines,
        "subtotal_cents": subtotal,
        "vat_rate": 20,
        "vat_cents": vat,
        "total_cents": subtotal + vat,
        "pay_url": "https://pay.acme.example/INV-2026-0042",
        "terms": "Payment is due within 30 days of the invoice date. Late payments carry \
                  interest at 8 percentage points above the reference rate, as the late \
                  payment directive allows. Questions about this invoice go to \
                  billing@acme.example; quote the invoice number.",
    });
    let pdf = engine()
        .render(&Render::template("samples/invoice").data(&data))
        .await
        .unwrap();
    save_sample("invoice.pdf", &pdf);
    assert_eq!(pdf.page_count().unwrap(), 1);
    let text = pdf.text().unwrap();
    for expected in [
        "INVOICE",
        "INV-2026-0042",
        "Northwind Traders GmbH",
        "Security audit 1",
        "Total due",
        "Page 1 of 1",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    assert_eq!(pdf.title().unwrap().as_deref(), Some("Invoice INV-2026-0042"));
    let document = load(&pdf);
    assert_eq!(images(&document), 1);
    assert_eq!(links(&document), vec!["https://pay.acme.example/INV-2026-0042"]);
}

/// The report sample: a document built in code, with a table long enough
/// to take several pages.
fn report() -> PdfDocument {
    let photo = std::fs::read(fixtures_dir().join("images/photo.jpg")).unwrap();
    let owners = ["Platform", "Security", "IT operations", "Finance", "People"];
    let statuses = ["Passed", "Passed", "Passed", "Open", "Passed", "In progress"];
    let mut table = Table::new()
        .header([
            Cell::new("#"),
            Cell::new("Control"),
            Cell::new("Owner"),
            Cell::new("Status").align(Align::End),
        ])
        .widths([7.0, 55.0, 22.0, 16.0]);
    for i in 1..=120 {
        let control = match i % 4 {
            0 => format!("Access reviews for production systems, batch {i}"),
            1 => format!("Encryption at rest for storage volumes in region {}", i % 7 + 1),
            2 => format!("Multi-factor sign-in for administrators ({i})"),
            _ => format!("Restore test of backup set {i:03}"),
        };
        let status = statuses[i % statuses.len()];
        let mut status_cell = Cell::new(status).align(Align::End);
        if status == "Open" {
            status_cell = Cell::new(Run::new(status).bold().color("B42318")).align(Align::End);
        }
        table = table.row([
            Cell::new(i.to_string()),
            Cell::new(control),
            Cell::new(owners[i % owners.len()]),
            status_cell,
        ]);
    }
    PdfDocument::new()
        .title("Quarterly security review")
        .author("Acme audit team")
        .locale("en")
        .header(Paragraph::new().run(Run::title()).text(" · Q3 2026").align(Align::End))
        .footer_page_numbers()
        .heading(1, "Quarterly security review")
        .paragraph(
            Paragraph::new()
                .text("Prepared for ")
                .bold("Acme Ltd")
                .text(" by the audit team, ")
                .italic("27 October 2026")
                .text("."),
        )
        .paragraph(
            Paragraph::from(
                "This review covers every control in scope for the third quarter. Each \
                 control was tested against its written procedure, its evidence was \
                 sampled, and its owner confirmed the result. Controls that failed are \
                 listed with a remediation plan and a date; controls in progress carry \
                 the work that remains. The appendix lists the systems in scope and the \
                 people interviewed.",
            )
            .align(Align::Justify),
        )
        .heading(2, "Summary")
        .bullets([
            "117 of 120 controls passed",
            "Two findings are open and one is in progress",
            "No finding affects customer data",
        ])
        .list(
            List::numbered()
                .item("Rotate the signing keys")
                .item_with(
                    "Restore a backup end to end",
                    List::bullets().item("Database").item("Object storage"),
                )
                .item("Review access every quarter"),
        )
        .heading(2, "Controls")
        .table(table)
        .heading(2, "Evidence")
        .image(
            Image::new(photo)
                .width_mm(110.0)
                .align(Align::Center)
                .alt("The data centre at dusk"),
        )
        .paragraph(
            Paragraph::new()
                .text("The full evidence pack is at ")
                .link("security.acme.example/q3", "https://security.acme.example/q3")
                .text("."),
        )
        .rule()
        .paragraph(Paragraph::from("Signed off by the audit team.").align(Align::End))
}

#[tokio::test]
async fn sample_report() {
    let pdf = report().build().unwrap();
    save_sample("report.pdf", &pdf);
    let pages = pdf.page_texts().unwrap();
    let count = pages.len();
    assert!(count >= 4, "{count}");
    assert!(
        pages[0].contains("Prepared for Acme Ltd by the audit team, 27 October 2026."),
        "{}",
        pages[0]
    );
    let mut rows = 0;
    for (index, page) in pages.iter().enumerate() {
        let lines: Vec<&str> = page.lines().collect();
        assert_eq!(lines[0], "Quarterly security review · Q3 2026", "{page}");
        assert_eq!(*lines.last().unwrap(), format!("Page {} of {count}", index + 1));
        let body_rows = lines
            .iter()
            .filter(|l| l.split(' ').next().is_some_and(|n| n.parse::<u32>().is_ok()))
            .count();
        if body_rows > 0 && index > 0 {
            assert!(lines.contains(&"# Control Owner Status"), "the header repeats: {page}");
        }
        rows += body_rows;
    }
    assert!(rows >= 120, "{rows}");
    let document = load(&pdf);
    assert_eq!(info(&document, b"Author").as_deref(), Some("Acme audit team"));
    assert_eq!(images(&document), 1);
    assert_eq!(links(&document), vec!["https://security.acme.example/q3"]);
    assert!(fonts(&document).iter().all(|(_, embedded)| *embedded));
}

/// The multilingual sample: Arabic, Hebrew and Hindi in one right-to-left
/// document.
#[tokio::test]
async fn sample_multilingual() {
    let data = json!({
        "title": "شهادات إتمام",
        "issued": "27 October 2026",
        "arabic": {"name": "مريم العلي", "course": "أساسيات الأمن السيبراني", "score": 96},
        "hebrew": {"name": "נועה כהן", "course": "יסודות אבטחת מידע", "score": 91},
        "hindi": {"name": "अनन्या शर्मा", "course": "साइबर सुरक्षा की मूल बातें", "score": 88},
    });
    let pdf = engine()
        .render(&Render::template("samples/multilingual").data(&data))
        .await
        .unwrap();
    save_sample("multilingual.pdf", &pdf);
    let text = pdf.text().unwrap();
    for expected in [
        "شهادة إتمام",
        "مريم العلي",
        "תעודת סיום",
        "נועה כהן",
        "प्रमाण पत्र",
        "अनन्या शर्मा",
        "صفحة 1 من 1",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    let document = load(&pdf);
    assert_eq!(catalog(&document).get(b"Lang").unwrap().as_str().unwrap(), b"ar");
    let names: Vec<String> = fonts(&document).into_iter().map(|(n, _)| n).collect();
    for face in ["NotoSansArabic", "NotoSansHebrew", "NotoSansDevanagari"] {
        assert!(names.iter().any(|n| n.contains(face)), "{face}: {names:?}");
    }
}

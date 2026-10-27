//! The Typst engine: templates, data, imports, locales, fonts and errors.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use navree_contracts::FixedClock;
use navree_documents::{DocumentsError, Render, Templates, TypstEngine};
use serde_json::json;

use crate::support::{assert_golden, fixture_templates};

fn engine() -> TypstEngine {
    TypstEngine::new(fixture_templates())
}

#[tokio::test]
async fn a_template_renders_with_its_data_and_shared_imports() {
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
    let text = pdf.text().unwrap();
    assert!(text.contains("Certificate of completion"), "{text}");
    assert!(
        text.contains(
            "This certifies that Ada Lovelace completed Analytical Engines on 26 October 2026."
        ),
        "{text}"
    );
    // The sample data.json in the directory is replaced by the render's.
    assert!(!text.contains("Sample"), "{text}");
}

#[tokio::test]
async fn the_name_may_carry_the_extension() {
    let pdf = engine()
        .render(&Render::template("certificates/classic.typ").data(json!({
            "name": "Grace Hopper", "course": "Compilers", "completed_on": "today",
        })))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("Grace Hopper"));
}

#[tokio::test]
async fn inline_markup_reads_the_data_and_the_template_tree() {
    let pdf = engine()
        .render(
            &Render::typst(
                "#import \"/shared/layout.typ\": frame\n\
                 #let data = json(\"/data.json\")\n\
                 #show: frame.with(title: \"Inline\")\n\
                 Total: #data.total",
            )
            .data(json!({"total": "1,234.56 EUR"})),
        )
        .await
        .unwrap();
    let text = pdf.text().unwrap();
    assert!(text.contains("Inline"), "{text}");
    assert!(text.contains("Total: 1,234.56 EUR"), "{text}");
}

#[tokio::test]
async fn an_unknown_template_is_not_found() {
    let error = engine()
        .render(&Render::template("certificates/missing"))
        .await
        .unwrap_err();
    match error {
        DocumentsError::TemplateNotFound { name, tried } => {
            assert_eq!(name, "certificates/missing");
            assert_eq!(tried, vec!["certificates/missing.typ"]);
        }
        other => panic!("{other:?}"),
    }
    let error = engine()
        .render(&Render::template("../escape"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, DocumentsError::TemplateNotFound { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_broken_template_names_the_file_line_and_column() {
    let error = engine()
        .render(&Render::template("broken").data(json!({"name": "Ada"})))
        .await
        .unwrap_err();
    match &error {
        DocumentsError::Template {
            file,
            line,
            column,
            message,
        } => {
            assert_eq!(file, "broken.typ");
            assert_eq!(*line, Some(3));
            assert_eq!(*column, Some(13));
            assert!(message.contains("nmae"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        error.to_string().starts_with("broken.typ:3:13: "),
        "{error}"
    );

    let error = engine()
        .render(&Render::typst("#let x = \n= Title\n#(1 + )"))
        .await
        .unwrap_err();
    match error {
        DocumentsError::Template { file, line, .. } => {
            assert_eq!(file, "inline.typ");
            assert!(line.is_some());
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn files_outside_the_template_tree_cannot_be_read() {
    let error = engine()
        .render(&Render::typst("#read(\"../../Cargo.toml\")"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, DocumentsError::Template { .. }),
        "{error:?}"
    );
    let error = engine()
        .render(&Render::typst("#read(\"/missing.txt\")"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("missing.txt"), "{error}");
}

#[tokio::test]
async fn packages_come_from_the_packages_directory_only() {
    let error = engine()
        .render(&Render::typst("#import \"@preview/example:0.1.0\": *"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("packages_dir"), "{error}");

    let packages = tempfile::tempdir().unwrap();
    let package = packages.path().join("preview/greet/0.1.0");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("typst.toml"),
        "[package]\nname = \"greet\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\"\n",
    )
    .unwrap();
    std::fs::write(
        package.join("lib.typ"),
        "#let greet(name) = [Hello, #name!]",
    )
    .unwrap();
    let pdf = engine()
        .packages_dir(Some(packages.path().to_path_buf()))
        .render(&Render::typst(
            "#import \"@preview/greet:0.1.0\": greet\n#greet(\"Ada\")",
        ))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("Hello, Ada!"));
}

#[tokio::test]
async fn embedded_templates_render_and_files_override_them() {
    let templates = Templates::new(
        vec![crate::support::fixtures_dir()],
        [
            (
                "receipts/simple.typ",
                "Receipt for #json(\"/data.json\").name",
            ),
            ("certificates/classic.typ", "EMBEDDED"),
        ],
    );
    let engine = TypstEngine::new(templates);
    let pdf = engine
        .render(&Render::template("receipts/simple").data(json!({"name": "Ada"})))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("Receipt for Ada"));
    let pdf = engine
        .render(&Render::template("certificates/classic").data(json!({
            "name": "Ada", "course": "x", "completed_on": "y",
        })))
        .await
        .unwrap();
    assert!(!pdf.text().unwrap().contains("EMBEDDED"));
}

#[tokio::test]
async fn the_locale_sets_the_language_direction_and_inputs() {
    let pdf = engine()
        .render(
            &Render::typst("#sys.inputs.at(\"locale\") #sys.inputs.at(\"dir\") #context text.lang #context text.dir")
                .locale("ar"),
        )
        .await
        .unwrap();
    let text = pdf.text().unwrap();
    assert!(text.contains("ar"), "{text}");
    assert!(text.contains("rtl"), "{text}");
    let pdf = engine()
        .render(&Render::typst(
            "#sys.inputs.at(\"locale\", default: \"none\")",
        ))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("none"));
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
    let text = pdf.text().unwrap();
    assert!(text.contains("شهادة إتمام"), "{text}");
    assert!(text.contains("مريم العلي"), "{text}");
    assert_golden("multilingual-ar.txt", &text);
}

#[tokio::test]
async fn devanagari_renders_with_the_bundled_fonts() {
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
    let text = pdf.text().unwrap();
    assert!(text.contains("अनन्या शर्मा"), "{text}");
    assert_golden("multilingual-hi.txt", &text);
}

#[tokio::test]
async fn mixed_scripts_fall_back_through_the_bundled_families() {
    let pdf = engine()
        .render(&Render::typst("Invoice 42 — فاتورة — चालान"))
        .await
        .unwrap();
    let text = pdf.text().unwrap();
    assert!(text.contains("Invoice 42"), "{text}");
    assert!(text.contains("فاتورة"), "{text}");
    assert!(text.contains("चालान"), "{text}");
    assert_golden("mixed-scripts.txt", &text);
}

#[test]
fn the_bundled_families_are_listed() {
    let families = engine().font_families();
    for family in ["Noto Sans", "Noto Sans Arabic", "Noto Sans Devanagari"] {
        assert!(families.iter().any(|f| f == family), "{families:?}");
    }
    let without = engine().bundled_fonts(false).font_families();
    assert!(without.is_empty(), "{without:?}");
}

#[test]
fn font_directories_add_families_and_skip_other_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("nested")).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fonts/NotoSansArabic-Regular.ttf"
        ),
        dir.path().join("nested/Arabic.ttf"),
    )
    .unwrap();
    std::fs::write(dir.path().join("broken.ttf"), b"not a font").unwrap();
    std::fs::write(dir.path().join("readme.txt"), b"fonts").unwrap();
    let families = engine()
        .bundled_fonts(false)
        .font_dirs(vec![dir.path().to_path_buf()])
        .font_families();
    assert_eq!(families, vec!["Noto Sans Arabic".to_owned()]);
}

#[tokio::test]
async fn today_and_the_creation_date_come_from_the_clock() {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 3, 14, 9, 26, 53).unwrap(),
    ));
    let pdf = engine()
        .clock(clock)
        .render(&Render::typst(
            "#datetime.today().display(\"[year]-[month]-[day]\")",
        ))
        .await
        .unwrap();
    assert!(pdf.text().unwrap().contains("2026-03-14"));
}

#[tokio::test]
async fn the_title_goes_into_the_metadata() {
    let pdf = engine()
        .render(&Render::typst("Body").title("Quarterly audit report"))
        .await
        .unwrap();
    assert_eq!(
        pdf.title().unwrap().as_deref(),
        Some("Quarterly audit report")
    );
}

#[tokio::test]
async fn data_that_is_not_json_fails_before_compiling() {
    use std::collections::BTreeMap;
    let mut map = BTreeMap::new();
    map.insert((1, 2), "tuple keys are not JSON");
    let error = engine()
        .render(&Render::template("certificates/classic").data(map))
        .await
        .unwrap_err();
    assert!(matches!(error, DocumentsError::Data(_)), "{error:?}");
}

#[tokio::test]
async fn html_is_not_typst() {
    let error = engine().render(&Render::html("<p>")).await.unwrap_err();
    assert!(
        matches!(
            error,
            DocumentsError::EngineUnavailable {
                engine: "typst",
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn renders_run_concurrently() {
    let engine = engine();
    let renders = (0..4).map(|i| {
        let engine = engine.clone();
        tokio::spawn(async move {
            engine
                .render(&Render::typst(format!("Document {i}")))
                .await
                .unwrap()
                .text()
                .unwrap()
        })
    });
    for (i, handle) in renders.enumerate() {
        assert!(handle.await.unwrap().contains(&format!("Document {i}")));
    }
}

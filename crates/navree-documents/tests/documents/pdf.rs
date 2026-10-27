//! `Pdf`: merge, stamps, overlays, metadata and text.

use navree_documents::{DocumentsError, Pages, Pdf, Position, Stamp};

#[test]
fn merge_keeps_every_page_in_order() {
    let invoice = Pdf::from_lines(["Invoice 42"]);
    let terms = Pdf::from_lines((0..70).map(|i| format!("Term {i}")));
    let merged = Pdf::merge([&invoice, &terms, &invoice]).unwrap();
    assert_eq!(merged.page_count().unwrap(), 4);
    let pages = merged.page_texts().unwrap();
    assert_eq!(pages.len(), 4);
    assert!(pages[0].starts_with("Invoice 42"), "{pages:?}");
    assert!(pages[1].starts_with("Term 0"), "{pages:?}");
    assert!(pages[2].starts_with("Term 60"), "{pages:?}");
    assert!(pages[3].starts_with("Invoice 42"), "{pages:?}");
    assert_eq!(merged.text().unwrap().matches('\u{c}').count(), 3);
    assert_eq!(invoice.append(&terms).unwrap().page_count().unwrap(), 3);
    assert!(matches!(
        Pdf::merge(Vec::<&Pdf>::new()),
        Err(DocumentsError::Pdf(_))
    ));
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn merge_works_on_rendered_documents_with_fonts() {
    use navree_documents::{Render, TypstEngine};
    let engine = TypstEngine::new(crate::support::fixture_templates());
    let arabic = engine
        .render(&Render::typst("مرحبا بالعالم").locale("ar"))
        .await
        .unwrap();
    let english = engine
        .render(&Render::typst("Hello\n#pagebreak()\nWorld"))
        .await
        .unwrap();
    let merged = Pdf::merge([&arabic, &english]).unwrap();
    assert_eq!(merged.page_count().unwrap(), 3);
    let pages = merged.page_texts().unwrap();
    assert_eq!(pages, vec!["مرحبا بالعالم", "Hello", "World"]);
}

#[test]
fn a_watermark_goes_on_every_page_and_keeps_the_text() {
    let pdf = Pdf::merge([
        &Pdf::from_lines(["Quarterly report"]),
        &Pdf::from_lines(["Appendix"]),
    ])
    .unwrap();
    let stamped = pdf.stamp(&Stamp::text("DRAFT")).unwrap();
    let pages = stamped.page_texts().unwrap();
    assert_eq!(pages.len(), 2);
    for page in &pages {
        assert!(page.contains("DRAFT"), "{page}");
    }
    assert!(pages[0].contains("Quarterly report"));
    assert!(pages[1].contains("Appendix"));
}

#[test]
fn a_stamp_can_go_on_selected_pages_and_corners() {
    let pdf = Pdf::from_lines((0..130).map(|i| format!("Line {i}")));
    assert_eq!(pdf.page_count().unwrap(), 3);
    let stamped = pdf
        .stamp(
            &Stamp::text("Copy 1 of 2")
                .at(Position::TopRight)
                .pages(Pages::First),
        )
        .unwrap()
        .stamp(
            &Stamp::text("Confidential")
                .at(Position::Bottom)
                .pages(Pages::Last),
        )
        .unwrap();
    let pages = stamped.page_texts().unwrap();
    assert!(pages[0].contains("Copy 1 of 2"));
    assert!(!pages[1].contains("Copy 1 of 2") && !pages[1].contains("Confidential"));
    assert!(pages[2].contains("Confidential"));
    // Top-right: the stamp's line comes first, at the top of the page.
    assert!(
        pages[0].starts_with("Copy 1 of 2") || pages[0].lines().next().unwrap().contains("Copy"),
        "{}",
        pages[0]
    );
    // Bottom: the last line of the page.
    assert_eq!(pages[2].lines().last(), Some("Confidential"));
    let only = pdf
        .stamp(&Stamp::text("X").pages(Pages::Only(vec![2])))
        .unwrap();
    let pages = only.page_texts().unwrap();
    assert!(!pages[0].contains('X') && pages[1].contains('X') && !pages[2].contains('X'));
}

#[test]
fn a_stamp_in_another_script_points_to_overlays() {
    let error = Pdf::from_lines(["x"])
        .stamp(&Stamp::text("مدفوع"))
        .unwrap_err();
    assert!(error.to_string().contains("Pdf::overlay"), "{error}");
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn an_overlay_rendered_with_typst_carries_any_script() {
    use navree_documents::{Render, TypstEngine};
    let engine = TypstEngine::new(crate::support::fixture_templates());
    let seal = engine
        .render(&Render::typst(
            "#set page(paper: \"a4\", margin: 0pt)\n#place(bottom + right, dx: -2cm, dy: -2cm, text(size: 16pt)[مدفوع — PAID])",
        ))
        .await
        .unwrap();
    let invoice = Pdf::from_lines(["Invoice 42", "Total 120.00 EUR"]);
    let stamped = invoice.overlay(&seal, Pages::All).unwrap();
    assert_eq!(stamped.page_count().unwrap(), 1);
    let text = stamped.text().unwrap();
    assert!(text.contains("Invoice 42"), "{text}");
    assert!(text.contains("مدفوع"), "{text}");
    assert!(text.contains("PAID"), "{text}");
}

#[cfg(feature = "typst")]
#[tokio::test]
async fn an_overlay_of_another_size_is_scaled_to_fit() {
    use navree_documents::{Render, TypstEngine};
    let engine = TypstEngine::new(crate::support::fixture_templates());
    let small = engine
        .render(&Render::typst(
            "#set page(width: 6cm, height: 3cm, margin: 0pt)\n#align(center + horizon)[SEAL]",
        ))
        .await
        .unwrap();
    let pdf = Pdf::merge([&Pdf::from_lines(["one"]), &Pdf::from_lines(["two"])]).unwrap();
    let stamped = pdf.overlay(&small, Pages::Last).unwrap();
    let pages = stamped.page_texts().unwrap();
    assert_eq!(pages[0], "one");
    // Scaled to the page width and centred: below the first line.
    assert_eq!(pages[1], "two\nSEAL");
}

#[test]
fn metadata_and_conversions() {
    let pdf = Pdf::from_lines(["x"])
        .with_metadata("Audit report", Some("Navree"))
        .unwrap();
    assert_eq!(pdf.title().unwrap().as_deref(), Some("Audit report"));
    let bytes: bytes::Bytes = pdf.clone().into();
    assert_eq!(&bytes[..], &pdf[..]);
    let vec: Vec<u8> = pdf.clone().into();
    assert_eq!(vec.len(), pdf.len());
    assert_eq!(Pdf::from_bytes(vec).unwrap(), pdf);
    assert_eq!(format!("{pdf:?}"), format!("Pdf({} bytes)", pdf.len()));
}

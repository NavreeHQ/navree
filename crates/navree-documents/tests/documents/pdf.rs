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

#[test]
fn bytes_that_are_not_a_readable_pdf_are_a_client_error() {
    let garbage = Pdf::from_bytes(&b"not a pdf"[..]).unwrap_err();
    assert!(matches!(garbage, DocumentsError::Pdf(_)), "{garbage:?}");
    assert_eq!(garbage.status(), http::StatusCode::BAD_REQUEST);

    // A PDF cut short is unreadable too.
    let whole = Pdf::from_lines(["x"]);
    let cut = Pdf::from_bytes(whole.bytes().slice(..whole.len() / 2)).unwrap_err();
    assert_eq!(cut.status(), http::StatusCode::BAD_REQUEST, "{cut:?}");
}

/// A one-page PDF whose page tree sits in an object stream that inflates to
/// `inflated` bytes from a few kilobytes.
fn pdf_with_an_inflating_object_stream(inflated: usize) -> Vec<u8> {
    let plain = Pdf::from_lines(["x"]);
    let mut document = lopdf::Document::load_mem(&plain).unwrap();
    document.add_object(lopdf::Object::String(
        vec![b' '; inflated],
        lopdf::StringFormat::Literal,
    ));
    let options = lopdf::SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .compression_level(6)
        .build();
    let mut bytes = Vec::new();
    document.save_with_options(&mut bytes, options).unwrap();
    bytes
}

#[test]
fn a_pdf_whose_object_stream_inflates_past_the_limit_is_refused() {
    // 72 MiB of spaces: a few kilobytes on disk, over the 64 MiB a single
    // object stream may inflate to.
    let bomb = pdf_with_an_inflating_object_stream(72 * 1024 * 1024);
    assert!(
        bomb.len() < 256 * 1024,
        "the bomb is {} bytes, not small",
        bomb.len()
    );
    // Without a limit it is a perfectly good document.
    assert_eq!(
        lopdf::Document::load_mem(&bomb).unwrap().get_pages().len(),
        1
    );
    let error = Pdf::from_bytes(bomb).unwrap_err();
    assert!(matches!(error, DocumentsError::Pdf(_)), "{error:?}");
}

#[test]
fn an_object_stream_under_the_limit_loads() {
    let bytes = pdf_with_an_inflating_object_stream(1024 * 1024);
    let pdf = Pdf::from_bytes(bytes).unwrap();
    assert_eq!(pdf.page_count().unwrap(), 1);
    assert!(pdf.text().unwrap().contains('x'));
}

/// A one-page PDF whose page draws a form `fan_out` times, which draws the
/// next `fan_out` times, and so on for `levels` forms. The last one holds
/// `last`, a content stream.
fn pdf_with_forms_drawing_forms(fan_out: usize, levels: usize, last: &str) -> Vec<u8> {
    use lopdf::{Object, Stream, dictionary};

    let mut document = lopdf::Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let draws = "/Next Do ".repeat(fan_out);
    let mut inner: Option<lopdf::ObjectId> = None;
    for _ in 0..levels {
        let mut dict = dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
        };
        let content = match inner {
            Some(next) => {
                dict.set(
                    "Resources",
                    dictionary! { "XObject" => dictionary! { "Next" => next } },
                );
                draws.clone()
            }
            None => {
                dict.set(
                    "Resources",
                    dictionary! { "Font" => dictionary! { "F1" => font_id } },
                );
                last.to_owned()
            }
        };
        inner = Some(document.add_object(Stream::new(dict, content.into_bytes())));
    }
    let first = inner.expect("at least one form");
    let content_id = document.add_object(Stream::new(dictionary! {}, draws.into_bytes()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "XObject" => dictionary! { "Next" => first } },
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn forms_that_draw_forms_do_not_multiply_without_end() {
    // Eight levels of forms, each drawing the next 6 times: the last is
    // drawn 6^8 (1,679,616) times, from a file of a few hundred bytes.
    let bomb = pdf_with_forms_drawing_forms(6, 8, "q Q");
    assert!(bomb.len() < 4096, "{} bytes", bomb.len());
    let pdf = Pdf::from_bytes(bomb).unwrap();
    let started = std::time::Instant::now();
    let error = pdf.text().unwrap_err();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "gave up only after {:?}",
        started.elapsed()
    );
    assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
}

#[test]
fn a_form_drawn_ten_thousand_times_is_within_the_work_limit() {
    // A seal drawn on every row of a long table: 10,000 draws of one form.
    let pdf = Pdf::from_bytes(pdf_with_forms_drawing_forms(
        10_000,
        1,
        "BT /F1 12 Tf 1 0 0 1 50 700 Tm (Sealed) Tj ET",
    ))
    .unwrap();
    assert_eq!(pdf.text().unwrap(), "Sealed");
}

#[test]
fn forms_within_the_work_limit_still_show_their_text() {
    // The same shape, small: 3 levels drawing 4 times each is 84 draws.
    let pdf = Pdf::from_bytes(pdf_with_forms_drawing_forms(
        4,
        3,
        "BT /F1 12 Tf 1 0 0 1 50 700 Tm (Sealed) Tj ET",
    ))
    .unwrap();
    assert!(pdf.text().unwrap().contains("Sealed"));
}

/// A one-page PDF with `content` as its page, and font `/F1`: Helvetica, or
/// (with `zero_width`) a font whose `a` has no width.
fn pdf_with_page_content(content: &str, zero_width: bool) -> Vec<u8> {
    use lopdf::{Object, Stream, dictionary};

    let mut document = lopdf::Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let mut font = dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    };
    if zero_width {
        font.set("FirstChar", 97);
        font.set("LastChar", 97);
        font.set("Widths", vec![0.into()]);
    }
    let font_id = document.add_object(font);
    let content_id = document.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn a_very_long_line_of_glyphs_is_read_in_time_proportional_to_its_length() {
    // 60,000 glyphs on one baseline, shown by one operator: 60,000 squared
    // is 3.6 billion comparisons if every glyph is compared with every other.
    let glyphs = "a".repeat(60_000);
    let page = format!("BT /F1 10 Tf 1 0 0 1 10 700 Tm ({glyphs}) Tj ET");
    let pdf = Pdf::from_bytes(pdf_with_page_content(&page, false)).unwrap();
    let started = std::time::Instant::now();
    let text = pdf.text().unwrap();
    let took = started.elapsed();
    assert_eq!(text, glyphs);
    assert!(
        took < std::time::Duration::from_secs(2),
        "reading 60,000 glyphs took {took:?}"
    );
}

#[test]
fn marks_that_sit_on_nothing_are_refused_past_a_budget() {
    // 30,000 zero-width glyphs, 12 points apart: each one is a mark with no
    // letter to sit on, and each is compared with every mark before it.
    let shown = "(a) -1000 ".repeat(30_000);
    let page = format!("BT /F1 12 Tf 1 0 0 1 10 700 Tm [{shown}] TJ ET");
    let pdf = Pdf::from_bytes(pdf_with_page_content(&page, true)).unwrap();
    let started = std::time::Instant::now();
    let error = pdf.text().unwrap_err();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "gave up only after {:?}",
        started.elapsed()
    );
    assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
}

/// A one-page PDF showing one glyph of a composite font whose `ToUnicode`
/// map inflates to `inflated` bytes from a few kilobytes.
fn pdf_with_an_inflating_to_unicode_map(inflated: usize) -> Vec<u8> {
    use lopdf::{Object, Stream, dictionary};

    let mut document = lopdf::Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let mut map = Stream::new(dictionary! {}, vec![b' '; inflated]);
    map.compress().unwrap();
    let map_id = document.add_object(map);
    let descendant_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Sample",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
    });
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Sample",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![descendant_id.into()],
        "ToUnicode" => map_id,
    });
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 1 0 0 1 10 700 Tm <0041> Tj ET".to_vec(),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn a_to_unicode_map_that_inflates_past_the_limit_is_refused() {
    // 24 MiB of spaces from a few kilobytes, over the 8 MiB a map may have.
    let bomb = pdf_with_an_inflating_to_unicode_map(24 * 1024 * 1024);
    assert!(bomb.len() < 128 * 1024, "{} bytes", bomb.len());
    let pdf = Pdf::from_bytes(bomb).unwrap();
    let error = pdf.text().unwrap_err();
    assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
    // A map of ordinary size is read, and its font simply shows nothing here.
    let small = Pdf::from_bytes(pdf_with_an_inflating_to_unicode_map(1024)).unwrap();
    assert!(small.text().is_ok());
}

#[test]
fn a_few_marks_are_still_put_on_their_letters() {
    // A letter and a mark drawn apart over it, as Arabic fonts do for dots.
    let page = "BT /F1 12 Tf 1 0 0 1 10 700 Tm (b) Tj -6.67 0 Td (a) Tj ET";
    let pdf = Pdf::from_bytes(pdf_with_page_content(page, true)).unwrap();
    assert!(pdf.text().unwrap().contains('b'));
}

#[cfg(feature = "native")]
#[tokio::test]
async fn merge_works_on_rendered_documents_with_fonts() {
    use navree_documents::{NativeEngine, Render};
    let engine = NativeEngine::new(crate::support::fixture_templates());
    let arabic = engine
        .render(&Render::html("<p>مرحبا بالعالم</p>").locale("ar"))
        .await
        .unwrap();
    let english = engine
        .render(&Render::html(
            "<p>Hello</p><div class=\"page-break\"></div><p>World</p>",
        ))
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

#[cfg(feature = "native")]
#[tokio::test]
async fn an_overlay_rendered_natively_carries_any_script() {
    use navree_documents::{Margins, NativeEngine, PrintOptions, Render};
    let engine = NativeEngine::new(crate::support::fixture_templates());
    let seal = engine
        .render(
            &Render::html("<p style=\"text-align: right; font-size: 16pt\">مدفوع — PAID</p>")
                .print(PrintOptions::a4().margin(Margins::all(20.0))),
        )
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

#[cfg(feature = "native")]
#[tokio::test]
async fn an_overlay_of_another_size_is_scaled_to_fit() {
    use navree_documents::{NativeEngine, Render};
    let engine = NativeEngine::new(crate::support::fixture_templates());
    let small = engine
        .render(&Render::html(
            "<style>@page { size: 60mm 30mm; margin: 0 }</style><p style=\"text-align: center\">SEAL</p>",
        ))
        .await
        .unwrap();
    let pdf = Pdf::merge([&Pdf::from_lines(["one"]), &Pdf::from_lines(["two"])]).unwrap();
    let stamped = pdf.overlay(&small, Pages::Last).unwrap();
    let pages = stamped.page_texts().unwrap();
    assert_eq!(pages[0], "one");
    // Scaled to the page width and centered: below the first line.
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

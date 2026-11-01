//! `Pdf`: merge, stamps, overlays, metadata and text.

use navree_documents::{DocumentsError, Pages, Pdf, Position, Stamp};
use navree_pdf::{Document, Objects, ReadLimits};

/// The objects of `pdf`, to build an input from.
fn objects_of(pdf: &Pdf) -> Objects {
    let document = Document::load(pdf.bytes().clone(), ReadLimits::default()).unwrap();
    Objects::read(document).unwrap()
}

/// `pdf` read within the default limits.
fn read(pdf: &[u8]) -> Document {
    Document::load(pdf.to_vec(), ReadLimits::default()).unwrap()
}

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
    // Merging nothing is the app's mistake, not the input's.
    assert!(matches!(
        Pdf::merge(Vec::<&Pdf>::new()),
        Err(DocumentsError::Edit(_))
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
    let mut document = objects_of(&plain);
    document.add(navree_pdf::Object::String(
        vec![b' '; inflated],
        navree_pdf::StringFormat::Literal,
    ));
    document.save_with(true).unwrap()
}

#[test]
fn a_pdf_whose_object_stream_inflates_past_the_limit_is_refused() {
    // 16 MiB of spaces: a hundred kilobytes on disk, twice the 8 MiB a
    // single object stream may inflate to.
    let bomb = pdf_with_an_inflating_object_stream(16 * 1024 * 1024);
    assert!(
        bomb.len() < 256 * 1024,
        "the bomb is {} bytes, not small",
        bomb.len()
    );
    // Without a limit it is a perfectly good document.
    let unlimited = ReadLimits {
        object_stream: usize::MAX,
        ..ReadLimits::default()
    };
    assert_eq!(
        Document::load(bomb.clone(), unlimited)
            .unwrap()
            .pages()
            .len(),
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

/// A one-page PDF showing `x`, with an object stream that lists `members`
/// objects all at the same offset: one array of `zeros` zeros. Nothing is
/// compressed; the file is a few tens of kilobytes.
fn pdf_with_members_sharing_one_array(members: u32, zeros: usize) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &str| {
        offsets.push(out.len());
        out.extend_from_slice(body.as_bytes());
    };
    object(
        &mut out,
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
    );
    object(
        &mut out,
        "2 0 obj\n<< /Type /Pages /Kids [5 0 R] /Count 1 >>\nendobj\n",
    );
    let header: String = (10..10 + members).map(|n| format!("{n} 0 ")).collect();
    let body = format!("{header}[{}]", "0 ".repeat(zeros));
    object(
        &mut out,
        &format!(
            "3 0 obj\n<< /Type /ObjStm /N {members} /First {} /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
            header.len(),
            body.len()
        ),
    );
    object(
        &mut out,
        "5 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 6 0 R \
         /Resources << /Font << /F1 7 0 R >> >> >>\nendobj\n",
    );
    object(
        &mut out,
        "6 0 obj\n<< /Length 32 >>\nstream\nBT /F1 12 Tf 50 700 Td (x) Tj ET\nendstream\nendobj\n",
    );
    object(
        &mut out,
        "7 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
    );
    let at = out.len();
    let row = |kind: u8, second: usize, third: u16| {
        let mut row = vec![kind];
        row.extend_from_slice(&(second as u32).to_be_bytes());
        row.extend_from_slice(&third.to_be_bytes());
        row
    };
    // Objects 0 to 7 (4 is this cross-reference stream), then the members.
    let mut rows = row(0, 0, 65535);
    for offset in &offsets[..3] {
        rows.extend(row(1, *offset, 0));
    }
    rows.extend(row(1, at, 0));
    for offset in &offsets[3..] {
        rows.extend(row(1, *offset, 0));
    }
    for index in 0..members {
        rows.extend(row(2, 3, index as u16));
    }
    out.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /XRef /Size {} /Root 1 0 R /W [1 4 2] /Index [0 8 10 {members}] \
             /Length {} >>\nstream\n",
            10 + members,
            rows.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&rows);
    out.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{at}\n%%EOF\n").as_bytes());
    out
}

#[test]
fn objects_that_amplify_past_the_reading_budget_are_a_client_error() {
    // 1,100 objects that are the same array of 8,000 zeros: 16 KB of text
    // that, read as 1,100 objects, would hold about 270 MiB, over the 256
    // MiB a document's objects may hold.
    let bomb = pdf_with_members_sharing_one_array(1_100, 8_000);
    assert!(bomb.len() < 32 * 1024, "{} bytes", bomb.len());
    // Reading the pages reads none of them.
    let pdf = Pdf::from_bytes(bomb).unwrap();
    assert_eq!(pdf.page_count().unwrap(), 1);
    assert_eq!(pdf.text().unwrap(), "x");
    // Editing reads every object: refused once the budget runs out.
    let error = pdf.stamp(&Stamp::text("PAID")).unwrap_err();
    assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
    assert_eq!(error.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
    // A few such objects are a document like any other.
    let few = Pdf::from_bytes(pdf_with_members_sharing_one_array(3, 10_000)).unwrap();
    assert!(
        few.stamp(&Stamp::text("PAID"))
            .unwrap()
            .text()
            .unwrap()
            .contains("PAID")
    );
}

/// A one-page PDF whose page draws a form `fan_out` times, which draws the
/// next `fan_out` times, and so on for `levels` forms. The last one holds
/// `last`, a content stream.
fn pdf_with_forms_drawing_forms(fan_out: usize, levels: usize, last: &str) -> Vec<u8> {
    use navree_pdf::{Object, Stream, dictionary};

    let mut document = Objects::new();
    let pages_id = document.reserve();
    let font_id = document.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let draws = "/Next Do ".repeat(fan_out);
    let mut inner: Option<navree_pdf::ObjectId> = None;
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
        inner = Some(document.add(Stream::new(dict, content.into_bytes())));
    }
    let first = inner.expect("at least one form");
    let content_id = document.add(Stream::new(dictionary! {}, draws.into_bytes()));
    let page_id = document.add(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "XObject" => dictionary! { "Next" => first } },
    });
    document.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog_id = document.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document.save().unwrap()
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
    use navree_pdf::{Object, Stream, dictionary};

    let mut document = Objects::new();
    let pages_id = document.reserve();
    let mut font = dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    };
    if zero_width {
        font.set("FirstChar", 97);
        font.set("LastChar", 97);
        font.set("Widths", vec![0.into()]);
    }
    let font_id = document.add(font);
    let content_id = document.add(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let page_id = document.add(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
    });
    document.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog_id = document.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document.save().unwrap()
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
    use navree_pdf::{Object, Stream, dictionary};

    let mut document = Objects::new();
    let pages_id = document.reserve();
    let map_id = document.add(Stream::new(
        dictionary! { "Filter" => "FlateDecode" },
        crate::support::smallest_deflate(&vec![b' '; inflated]),
    ));
    let descendant_id = document.add(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Sample",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
    });
    let font_id = document.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Sample",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![descendant_id.into()],
        "ToUnicode" => map_id,
    });
    let content_id = document.add(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 1 0 0 1 10 700 Tm <0041> Tj ET".to_vec(),
    ));
    let page_id = document.add(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
    });
    document.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog_id = document.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document.compress();
    document.save().unwrap()
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
fn an_object_stream_over_the_load_limit_is_refused() {
    // 9 MiB of spaces, over the 8 MiB an object stream may inflate to.
    let error = Pdf::from_bytes(pdf_with_an_inflating_object_stream(9 * 1024 * 1024)).unwrap_err();
    assert!(matches!(error, DocumentsError::Pdf(_)), "{error:?}");
    let under = Pdf::from_bytes(pdf_with_an_inflating_object_stream(7 * 1024 * 1024)).unwrap();
    assert_eq!(under.page_count().unwrap(), 1);
}

/// Saves `document` with object streams and its streams compressed.
fn compact(mut document: Objects) -> Vec<u8> {
    document.compress();
    document.save_with(true).unwrap()
}

/// `pages` pages showing their number, with `fonts` font names for one
/// font: in resources the page tree passes down to every page, or (with
/// `shared`) in one resources object each page refers to.
fn pdf_with_many_fonts_on_many_pages(pages: usize, fonts: usize, shared: bool) -> Vec<u8> {
    use navree_pdf::{Dictionary, Object, Stream, dictionary};

    let mut document = Objects::new();
    let pages_id = document.reserve();
    let font = document.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let mut names = Dictionary::new();
    for i in 0..fonts {
        names.set(format!("F{i}"), font);
    }
    let resources = dictionary! { "Font" => names };
    let shared_id = shared.then(|| document.add(resources.clone()));
    let kids: Vec<Object> = (0..pages)
        .map(|i| {
            let content = format!("BT /F0 12 Tf 50 700 Td (Page {i}) Tj ET");
            let content_id = document.add(Stream::new(dictionary! {}, content.into_bytes()));
            let mut page = dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            };
            if let Some(id) = shared_id {
                page.set("Resources", id);
            }
            document.add(page).into()
        })
        .collect();
    let mut tree = dictionary! {
        "Type" => "Pages", "Kids" => kids, "Count" => pages as i64,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    };
    if !shared {
        tree.set("Resources", resources);
    }
    document.insert(pages_id, Object::Dictionary(tree));
    let catalog = document.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog);
    compact(document)
}

/// The names in each page's `category` resources (`Font`, `XObject`), as
/// a viewer finds them: following references and the page tree.
fn resource_names(pdf: &Pdf, category: &[u8]) -> Vec<std::collections::BTreeSet<Vec<u8>>> {
    use navree_pdf::Object;

    let document = read(pdf);
    let resolve = |object: &Object| -> Object {
        let mut object = object.clone();
        while let Object::Reference(id) = object {
            object = document.object(id).unwrap().clone();
        }
        object
    };
    document
        .pages()
        .into_iter()
        .map(|page_id| {
            let mut node = document.dictionary(page_id).unwrap();
            let resources = loop {
                if let Some(resources) = node.get(b"Resources") {
                    break resolve(resources);
                }
                let parent = node.get(b"Parent").unwrap().as_reference().unwrap();
                node = document.dictionary(parent).unwrap();
            };
            resources
                .as_dict()
                .unwrap()
                .get(category)
                .map(resolve)
                .map(|names| {
                    names
                        .as_dict()
                        .unwrap()
                        .iter()
                        .map(|(k, _)| k.to_vec())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}

fn names(fonts: usize, extra: &[&str]) -> std::collections::BTreeSet<Vec<u8>> {
    (0..fonts)
        .map(|i| format!("F{i}").into_bytes())
        .chain(extra.iter().map(|name| name.as_bytes().to_vec()))
        .collect()
}

#[test]
fn stamping_pages_that_inherit_resources_adds_them_once() {
    // 200 pages under one tree that gives each 200 font names. Copying the
    // names onto every page writes 40,000 of them.
    let edit = |fonts: usize| {
        let pdf = Pdf::from_bytes(pdf_with_many_fonts_on_many_pages(200, fonts, false)).unwrap();
        pdf.stamp(&Stamp::text("DRAFT")).unwrap()
    };
    let (few, many) = (edit(1), edit(200));
    let grown = many.len() - few.len();
    assert!(
        grown < 32 * 1024,
        "199 more names grew the stamped file by {grown} bytes"
    );
    // Every page still finds every font, and the stamp's.
    for page in resource_names(&many, b"Font") {
        assert_eq!(page, names(200, &["NavreeStamp"]));
    }
    let texts = many.page_texts().unwrap();
    assert_eq!(texts.len(), 200);
    assert!(
        texts[199].contains("Page 199") && texts[199].contains("DRAFT"),
        "{}",
        texts[199]
    );
}

#[test]
fn stamping_pages_that_share_one_resources_object_keeps_it_shared() {
    let edit = |fonts: usize| {
        let pdf = Pdf::from_bytes(pdf_with_many_fonts_on_many_pages(200, fonts, true)).unwrap();
        pdf.stamp(&Stamp::text("DRAFT")).unwrap()
    };
    let (few, many) = (edit(1), edit(200));
    let grown = many.len() - few.len();
    assert!(
        grown < 32 * 1024,
        "199 more names grew the stamped file by {grown} bytes"
    );
    for page in resource_names(&many, b"Font") {
        assert_eq!(page, names(200, &["NavreeStamp"]));
    }
    // A stamp on some pages leaves the others as they were.
    let pdf = Pdf::from_bytes(pdf_with_many_fonts_on_many_pages(3, 2, true)).unwrap();
    let first = pdf.stamp(&Stamp::text("X").pages(Pages::First)).unwrap();
    let fonts = resource_names(&first, b"Font");
    assert_eq!(fonts[0], names(2, &["NavreeStamp"]));
    assert_eq!(fonts[1], names(2, &[]));
    assert_eq!(fonts[2], names(2, &[]));
}

#[test]
fn merging_pages_that_inherit_resources_adds_them_once() {
    let edit = |fonts: usize| {
        let pdf = Pdf::from_bytes(pdf_with_many_fonts_on_many_pages(200, fonts, false)).unwrap();
        Pdf::merge([&pdf, &Pdf::from_lines(["The end"])]).unwrap()
    };
    let (few, many) = (edit(1), edit(200));
    let grown = many.len() - few.len();
    assert!(
        grown < 32 * 1024,
        "199 more names grew the merged file by {grown} bytes"
    );
    let fonts = resource_names(&many, b"Font");
    assert_eq!(fonts.len(), 201);
    for page in &fonts[..200] {
        assert_eq!(*page, names(200, &[]));
    }
    assert_eq!(fonts[200], [b"F1".to_vec()].into());
    assert_eq!(many.page_texts().unwrap()[200], "The end");
}

#[test]
fn an_overlay_on_pages_that_inherit_resources_adds_them_once() {
    let seal = Pdf::from_lines(["SEAL"]);
    let edit = |fonts: usize| {
        let pdf = Pdf::from_bytes(pdf_with_many_fonts_on_many_pages(200, fonts, false)).unwrap();
        pdf.overlay(&seal, Pages::All).unwrap()
    };
    let (few, many) = (edit(1), edit(200));
    let grown = many.len() - few.len();
    assert!(
        grown < 32 * 1024,
        "199 more names grew the file by {grown} bytes"
    );
    for page in resource_names(&many, b"Font") {
        assert_eq!(page, names(200, &[]));
    }
    for page in resource_names(&many, b"XObject") {
        assert_eq!(page, [b"NavreeOverlay".to_vec()].into());
    }
    assert!(many.page_texts().unwrap()[5].contains("SEAL"));
}

#[test]
fn a_page_listed_many_times_in_the_page_tree_is_stamped_once() {
    use navree_pdf::{Object, Stream, dictionary};

    // One page object listed 2,000 times. Wrapping its contents again for
    // each listing makes its list of contents grow by two every time.
    let mut document = Objects::new();
    let pages_id = document.reserve();
    let font = document.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let content = document.add(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 50 700 Td (Again) Tj ET".to_vec(),
    ));
    let page = document.add(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    // The page tree is read up to one listing per object in the file.
    for i in 0..2000 {
        document.add(Object::Integer(i));
    }
    document.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page); 2000], "Count" => 2000,
        }),
    );
    let catalog = document.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog);
    let pdf = Pdf::from_bytes(compact(document)).unwrap();
    assert!(pdf.page_count().unwrap() >= 1000);

    let stamped = pdf.stamp(&Stamp::text("DRAFT")).unwrap();
    let document = read(&stamped);
    let page_id = document.pages()[0];
    let contents = document
        .dictionary(page_id)
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .as_array()
        .unwrap()
        .len();
    assert_eq!(
        contents,
        3,
        "the page's contents were wrapped {} times",
        (contents - 1) / 2
    );
}

#[test]
fn a_merge_lists_a_page_the_tree_lists_many_times_once() {
    use navree_pdf::{Object, Stream, dictionary};

    // One page object listed 500 times, in a file with 500 objects besides:
    // the objects of the output are the page and what it leads to, and a
    // page tree that lists more pages than the file has objects reads short.
    let mut document = Objects::new();
    let pages_id = document.reserve();
    let content = document.add(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 50 700 Td (Again) Tj ET".to_vec(),
    ));
    let font = document.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let page = document.add(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    for i in 0..500 {
        document.add(Object::Integer(i));
    }
    document.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page); 500], "Count" => 500,
        }),
    );
    let catalog = document.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog);
    let pdf = Pdf::from_bytes(compact(document)).unwrap();
    assert!(pdf.page_count().unwrap() >= 400);
    let merged = Pdf::merge([&pdf, &Pdf::from_lines(["after"])]).unwrap();
    // The page once, and the page after it.
    assert_eq!(merged.page_count().unwrap(), 2);
    let text = merged.page_texts().unwrap();
    assert_eq!(text, ["Again", "after"]);
}

#[test]
fn a_stamp_on_some_pages_leaves_the_resources_of_the_others_as_they_were() {
    // Three pages sharing one resources object; the stamp goes on the second.
    let pdf = Pdf::from_bytes(pdf_with_many_fonts_on_many_pages(3, 2, true)).unwrap();
    let second = Stamp::text("X").pages(Pages::Only(vec![2]));
    let before = read(&pdf);
    let stamped = pdf.stamp(&second).unwrap();
    let after = read(&stamped);
    let resources = |document: &Document| -> Vec<navree_pdf::Object> {
        document
            .pages()
            .into_iter()
            .map(|page| {
                let page = document.dictionary(page).unwrap();
                page.get(b"Resources").unwrap().clone()
            })
            .collect()
    };
    let (was, now) = (resources(&before), resources(&after));
    let shared = was[0].as_reference().unwrap();
    assert!(
        was.iter().all(|r| r.as_reference().unwrap() == shared),
        "{was:?}"
    );
    // The other pages still refer to the object they shared, unchanged.
    assert_eq!(now[0], was[0]);
    assert_eq!(now[2], was[2]);
    assert_eq!(
        after.object(shared).unwrap(),
        before.object(shared).unwrap()
    );
    assert_ne!(now[1], was[1]);
    let fonts = resource_names(&stamped, b"Font");
    assert_eq!(
        fonts,
        [names(2, &[]), names(2, &["NavreeStamp"]), names(2, &[])]
    );
    let states = resource_names(&stamped, b"ExtGState");
    assert_eq!(states[0], names(0, &[]));
    assert_eq!(states[1], names(0, &["NavreeStampGs"]));
    assert_eq!(states[2], names(0, &[]));

    // The same when the pages inherit the resources from the page tree.
    let pdf = Pdf::from_bytes(pdf_with_many_fonts_on_many_pages(3, 2, false)).unwrap();
    let stamped = pdf.stamp(&second).unwrap();
    let fonts = resource_names(&stamped, b"Font");
    assert_eq!(
        fonts,
        [names(2, &[]), names(2, &["NavreeStamp"]), names(2, &[])]
    );
    let texts = stamped.page_texts().unwrap();
    assert!(!texts[0].contains('X') && texts[1].contains('X') && !texts[2].contains('X'));
}

#[test]
fn edits_the_app_cannot_ask_for_are_server_errors() {
    let nothing = Pdf::merge(Vec::<&Pdf>::new()).unwrap_err();
    assert_eq!(
        nothing.status(),
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "{nothing:?}"
    );
    let script = Pdf::from_lines(["x"])
        .stamp(&Stamp::text("مدفوع"))
        .unwrap_err();
    assert_eq!(
        script.status(),
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "{script:?}"
    );
    // Input that is not a PDF stays the client's error.
    let garbage = Pdf::from_bytes(&b"not a pdf"[..]).unwrap_err();
    assert_eq!(garbage.status(), http::StatusCode::BAD_REQUEST);
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

/// A page whose content stream has `DecodeParms` and no `Filter`: its bytes
/// are read as they are, whatever the parameters say.
fn pdf_with_parameters_and_no_filter(parameters: &str) -> Vec<u8> {
    let mut objects = Objects::new();
    let pages = objects.reserve();
    let font = objects.add(navree_pdf::dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let mut dict = navree_pdf::Dictionary::new();
    dict.set(
        parameters,
        navree_pdf::dictionary! { "Predictor" => 12, "Columns" => 4 },
    );
    let text = "BT /F1 12 Tf 72 700 Td (A stream with parameters and no filter, padded so                 that compressing it helps, padded, padded, padded, padded) Tj ET";
    let content = objects.add(navree_pdf::Stream::new(dict, text.as_bytes().to_vec()));
    let page = objects.add(navree_pdf::dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
        "Resources" => navree_pdf::dictionary! { "Font" => navree_pdf::dictionary! { "F1" => font } },
    });
    objects.insert(
        pages,
        navree_pdf::dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
    );
    let catalog = objects.add(navree_pdf::dictionary! { "Type" => "Catalog", "Pages" => pages });
    objects.trailer.set("Root", catalog);
    objects.save().unwrap()
}

#[test]
fn an_edit_leaves_a_stream_with_parameters_and_no_filter_readable() {
    for parameters in ["DecodeParms", "DP"] {
        let pdf = Pdf::from_bytes(pdf_with_parameters_and_no_filter(parameters)).unwrap();
        let before = pdf.text().unwrap();
        assert!(before.contains("A stream with parameters"), "{before:?}");
        for edited in [
            pdf.with_metadata("Title", None).unwrap(),
            pdf.stamp(&Stamp::text("PAID")).unwrap(),
            Pdf::merge([&pdf, &pdf]).unwrap(),
        ] {
            let after = edited.text().unwrap();
            assert!(
                after.contains("A stream with parameters"),
                "{parameters}: the text after an edit is {after:?}"
            );
        }
    }
}

#[test]
fn a_pdf_is_send_and_sync_and_can_be_held_across_an_await() {
    fn send_sync<T: Send + Sync + 'static>(_: &T) {}
    fn sendable<T: Send>(_: &T) {}
    let pdf = Pdf::from_lines(["a"]);
    send_sync(&pdf);
    send_sync(&std::sync::Arc::new(pdf.clone()));
    // Objects can move between threads, between one edit and the next.
    let objects = Objects::new();
    sendable(&objects);
    let future = async move {
        let stamped = pdf.stamp(&Stamp::text("x")).unwrap();
        std::future::ready(()).await;
        stamped.text().unwrap()
    };
    sendable(&future);
}

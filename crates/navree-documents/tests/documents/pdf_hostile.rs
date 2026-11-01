//! `Pdf`'s edits against files that amplify when they are written, and
//! against valid files the edits must not damage. What goes in and what comes
//! out are compared by size: an edit adds its own few objects to a page, and
//! holds what it reads to its limits.

use navree_documents::{DocumentsError, Pages, Pdf, Position, Stamp};
use navree_pdf::content::operations;
use navree_pdf::{Document, Object, ReadLimits};

/// `%PDF`, objects numbered from 1, a table, `trailer`.
fn file(objects: &[Vec<u8>], trailer: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", index + 1).bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n{trailer}\nstartxref\n{xref}\n%%EOF\n").bytes());
    out
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    object
}

const FONT: &[u8] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

/// The tree with `n` pages inheriting `inherited`.
fn inheriting(n: usize, inherited: &str) -> Vec<u8> {
    let kids: String = (0..n).map(|i| format!("{} 0 R ", i + 3)).collect();
    let mut objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        format!("<</Type/Pages/Count {n}{inherited}/Kids[{kids}]>>").into_bytes(),
    ];
    for _ in 0..n {
        objects.push(b"<</Type/Page/Parent 2 0 R>>".to_vec());
    }
    file(&objects, &format!("<</Root 1 0 R/Size {}>>", n + 3))
}

#[test]
fn a_long_string_a_tree_gives_every_page_is_not_copied_to_each() {
    let bytes = inheriting(500, &format!("/Rotate ({})", "x".repeat(16 * 1024)));
    let len = bytes.len();
    let pdf = Pdf::from_bytes(bytes).unwrap();
    let stamped = pdf.stamp(&Stamp::text("X")).unwrap();
    assert!(
        stamped.len() < 8 * len,
        "{len} bytes stamp to {}",
        stamped.len()
    );
}

#[test]
fn no_value_a_tree_gives_every_page_is_copied_to_each() {
    // Each of these is valid in a tree or at least read from one: an array of
    // four long strings, a long name, a box in a long array.
    let name = "n".repeat(16 * 1024);
    let long = format!("({})", "x".repeat(4 * 1024));
    for inherited in [
        format!("/CropBox [{long} {long} {long} {long}]"),
        format!("/CropBox /{name}"),
        format!("/MediaBox [{}]", "0 ".repeat(8 * 1024)),
    ] {
        let bytes = inheriting(500, &inherited);
        let len = bytes.len();
        let pdf = Pdf::from_bytes(bytes).unwrap();
        let stamped = pdf.stamp(&Stamp::text("X")).unwrap();
        assert!(
            stamped.len() < 8 * len,
            "{} of {inherited:.20}: {len} bytes stamp to {}",
            500,
            stamped.len()
        );
        let merged = Pdf::merge([&pdf]).unwrap();
        assert!(merged.len() < 8 * len, "{len} merge to {}", merged.len());
    }
}

/// A page, then `k` stream objects whose `/Length` runs to one `endstream`.
fn overlapping(k: usize) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![out.len()];
    out.extend_from_slice(b"1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n");
    offsets.push(out.len());
    out.extend_from_slice(b"2 0 obj\n<</Type/Pages/Kids[3 0 R]/Count 1>>\nendobj\n");
    offsets.push(out.len());
    out.extend_from_slice(b"3 0 obj\n<</Type/Page/Parent 2 0 R>>\nendobj\n");
    let mut holes = Vec::new();
    for index in 0..k {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n<</Filter/DCTDecode/Length ", index + 4).bytes());
        holes.push(out.len());
        out.extend_from_slice(b"0000000000>>\nstream\n");
    }
    out.resize(out.len() + 64 * 1024, b'x');
    let stop = out.len();
    for hole in &holes {
        let start = hole + b"0000000000>>\nstream\n".len();
        out[*hole..*hole + 10].copy_from_slice(format!("{:010}", stop - start).as_bytes());
    }
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", k + 4).bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<</Root 1 0 R/Size {}>>\nstartxref\n{xref}\n%%EOF\n",
            k + 4
        )
        .bytes(),
    );
    out
}

#[test]
fn streams_that_overlap_are_not_written_once_each() {
    let bytes = overlapping(1_000);
    let len = bytes.len();
    let pdf = Pdf::from_bytes(bytes).unwrap();
    let results = [
        ("with_metadata", pdf.with_metadata("T", None).unwrap()),
        ("stamp", pdf.stamp(&Stamp::text("X")).unwrap()),
        (
            "overlay",
            pdf.overlay(&Pdf::from_lines(["x"]), Pages::All).unwrap(),
        ),
        ("merge", Pdf::merge([&pdf]).unwrap()),
    ];
    for (edit, output) in results {
        assert!(
            output.len() < 2 * len,
            "{edit}: {len} bytes saved as {}",
            output.len()
        );
    }
}

#[test]
fn a_merge_of_many_copies_of_a_document_grows_with_the_copies_and_no_faster() {
    let pdf = Pdf::from_lines(["x"]);
    let once = Pdf::merge([&pdf]).unwrap().len();
    let many = Pdf::merge(std::iter::repeat_n(&pdf, 100)).unwrap();
    assert!(many.len() <= 100 * once, "{} against {once}", many.len());
}

#[test]
fn a_contents_array_many_pages_share_is_refused_before_it_is_copied() {
    // One array of 100,000 references to one stream, on 2,000 pages: stamping
    // would hold 200 million references, and write them.
    let pages = 2_000;
    let kids: String = (0..pages).map(|i| format!("{} 0 R ", i + 5)).collect();
    let mut objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        format!("<</Type/Pages/Kids[{kids}]/Count {pages}>>").into_bytes(),
        format!("[{}]", "4 0 R ".repeat(100_000)).into_bytes(),
        stream("", b""),
    ];
    for _ in 0..pages {
        objects.push(b"<</Type/Page/Parent 2 0 R/Contents 3 0 R>>".to_vec());
    }
    let bytes = file(&objects, &format!("<</Root 1 0 R/Size {}>>", pages + 5));
    let pdf = Pdf::from_bytes(bytes).unwrap();
    let error = pdf.stamp(&Stamp::text("X")).unwrap_err();
    assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
}

#[test]
fn an_overlay_over_its_limit_is_too_large() {
    // 65 MiB of content, stored as it is: over the 64 MiB a page may take.
    let bytes = file(
        &[
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
            b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R>>".to_vec(),
            stream("", &vec![b' '; 65 << 20]),
        ],
        "<</Root 1 0 R/Size 5>>",
    );
    let overlay = Pdf::from_bytes(bytes).unwrap();
    let error = Pdf::from_lines(["x"])
        .overlay(&overlay, Pages::All)
        .unwrap_err();
    assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
    assert_eq!(error.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
}

const ORIGINAL: &[u8] = b"BT /F 12 Tf 50 700 Td (Original page text) Tj ET";

fn indirect_contents() -> Vec<u8> {
    file(
        &[
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
            b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R/Resources<</Font<</F 6 0 R>>>>>>".to_vec(),
            b"[5 0 R]".to_vec(),
            stream("", ORIGINAL),
            FONT.to_vec(),
        ],
        "<</Root 1 0 R/Size 7>>",
    )
}

#[test]
fn a_stamp_keeps_the_text_of_an_indirect_contents_array() {
    let pdf = Pdf::from_bytes(indirect_contents()).unwrap();
    assert!(pdf.text().unwrap().contains("Original page text"));
    let stamped = pdf.stamp(&Stamp::text("PAID")).unwrap();
    let text = stamped.text().unwrap();
    assert!(text.contains("Original page text"), "{text:?}");
    assert!(text.contains("PAID"), "{text:?}");
    assert_contents_are_streams(&stamped);
}

#[test]
fn an_overlay_keeps_the_text_of_an_indirect_contents_array() {
    let pdf = Pdf::from_bytes(indirect_contents()).unwrap();
    let seal = Pdf::from_lines(["SEAL"]);
    let overlaid = pdf.overlay(&seal, Pages::All).unwrap();
    let text = overlaid.text().unwrap();
    assert!(text.contains("Original page text"), "{text:?}");
    assert!(text.contains("SEAL"), "{text:?}");
    assert_contents_are_streams(&overlaid);
}

/// Every page's `Contents` lists streams, as the format requires.
fn assert_contents_are_streams(pdf: &Pdf) {
    let document = Document::load(pdf.bytes().clone(), ReadLimits::default()).unwrap();
    for page in document.pages() {
        let contents = document.dictionary(page).unwrap().get(b"Contents").unwrap();
        for item in document.resolve(contents).unwrap().as_array().unwrap() {
            assert!(
                document.resolve(item).unwrap().as_stream().is_some(),
                "{item:?}"
            );
        }
    }
}

/// The point the last `Tm` of the page's last stream puts text at.
fn stamp_position(pdf: &Pdf) -> (f32, f32) {
    let document = Document::load(pdf.bytes().clone(), ReadLimits::default()).unwrap();
    let page = document.pages()[0];
    let content = document.page_content(page, 1 << 20).unwrap();
    let matrix = operations(&content)
        .map(Result::unwrap)
        .filter(|op| op.operator == "Tm")
        .last()
        .unwrap();
    (
        matrix.operands[4].as_float().unwrap(),
        matrix.operands[5].as_float().unwrap(),
    )
}

/// A one-page document on `media_box`, maybe with a `CropBox`.
fn on_box(media_box: &str, crop_box: &str) -> Pdf {
    let page = format!(
        "<</Type/Page/Parent 2 0 R/MediaBox{media_box}{crop_box}/Contents 4 0 R/Resources<</Font<</F 5 0 R>>>>>>"
    );
    Pdf::from_bytes(file(
        &[
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
            page.into_bytes(),
            stream("", b"BT /F 12 Tf 1050 1700 Td (Original page text) Tj ET"),
            FONT.to_vec(),
        ],
        "<</Root 1 0 R/Size 6>>",
    ))
    .unwrap()
}

#[test]
fn a_stamp_lands_on_a_page_whose_box_does_not_start_at_the_origin() {
    let pdf = on_box("[1000 1000 1612 1792]", "");
    for position in [
        Position::BottomLeft,
        Position::BottomRight,
        Position::TopLeft,
        Position::TopRight,
        Position::Center,
        Position::Top,
        Position::Bottom,
    ] {
        let stamped = pdf.stamp(&Stamp::text("PAID").at(position)).unwrap();
        let (x, y) = stamp_position(&stamped);
        assert!((1000.0..1612.0).contains(&x), "{position:?}: x {x}");
        assert!((1000.0..1792.0).contains(&y), "{position:?}: y {y}");
    }
    // A box written from its top right corner is the same box.
    let flipped = on_box("[1612 1792 1000 1000]", "");
    let stamped = flipped
        .stamp(&Stamp::text("PAID").at(Position::BottomLeft))
        .unwrap();
    let (x, y) = stamp_position(&stamped);
    assert!((1000.0..1612.0).contains(&x), "x {x}");
    assert!((1000.0..1792.0).contains(&y), "y {y}");
}

#[test]
fn a_stamp_lands_inside_the_crop_box_a_reader_shows() {
    let pdf = on_box("[0 0 2000 2000]", "/CropBox[1000 1000 1612 1792]");
    let stamped = pdf
        .stamp(&Stamp::text("PAID").at(Position::BottomLeft))
        .unwrap();
    let (x, y) = stamp_position(&stamped);
    assert!((1000.0..1612.0).contains(&x), "x {x}");
    assert!((1000.0..1792.0).contains(&y), "y {y}");
}

/// The `cm` an overlay's form is drawn with on the page's last stream.
fn overlay_matrix(pdf: &Pdf) -> [f32; 6] {
    let document = Document::load(pdf.bytes().clone(), ReadLimits::default()).unwrap();
    let page = document.pages()[0];
    let content = document.page_content(page, 1 << 20).unwrap();
    let matrix = operations(&content)
        .map(Result::unwrap)
        .filter(|op| op.operator == "cm")
        .last()
        .unwrap();
    let values: Vec<f32> = matrix
        .operands
        .iter()
        .map(|operand: &Object| operand.as_float().unwrap())
        .collect();
    values.try_into().unwrap()
}

#[test]
fn an_overlay_lands_on_a_page_whose_box_does_not_start_at_the_origin() {
    let pdf = on_box("[1000 1000 1612 1792]", "");
    let seal = Pdf::from_lines(["SEAL"]);
    let overlaid = pdf.overlay(&seal, Pages::All).unwrap();
    let [scale, _, _, _, dx, dy] = overlay_matrix(&overlaid);
    // The seal's page, 595 by 842, fit to the page and centered on it.
    let (width, height) = (595.0 * scale, 842.0 * scale);
    assert!(dx >= 999.0 && dx + width <= 1613.0, "{dx} {width}");
    assert!(dy >= 999.0 && dy + height <= 1793.0, "{dy} {height}");
}

#[test]
fn an_edit_refuses_a_file_it_would_lose_part_of() {
    // A font that does not parse: its page could only come out without it.
    let bytes = file(
        &[
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
            b"<</Type/Page/Parent 2 0 R/Contents 4 0 R/Resources<</Font<</F 5 0 R>>>>>>".to_vec(),
            stream("", ORIGINAL),
            b"<</Type/Font ( >>".to_vec(),
        ],
        "<</Root 1 0 R/Size 6>>",
    );
    let pdf = Pdf::from_bytes(bytes).unwrap();
    for error in [
        pdf.with_metadata("T", None).unwrap_err(),
        pdf.stamp(&Stamp::text("X")).unwrap_err(),
        Pdf::merge([&pdf]).unwrap_err(),
    ] {
        assert!(matches!(error, DocumentsError::Pdf(_)), "{error:?}");
    }
}

#[test]
fn edits_keep_a_font_with_a_value_the_reader_tolerates() {
    for font in [
        &b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica/X 99999999999999999999>>"[..],
        &b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica/X>>"[..],
    ] {
        let bytes = file(
            &[
                b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R/Resources<</Font<</F 5 0 R>>>>>>".to_vec(),
                stream("", ORIGINAL),
                font.to_vec(),
            ],
            "<</Root 1 0 R/Size 6>>",
        );
        let pdf = Pdf::from_bytes(bytes).unwrap();
        for edited in [
            pdf.with_metadata("T", None).unwrap(),
            pdf.stamp(&Stamp::text("X")).unwrap(),
            Pdf::merge([&pdf]).unwrap(),
        ] {
            let text = edited.text().unwrap();
            assert!(text.contains("Original page text"), "{text:?}");
        }
    }
}

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

/// Whether `run` on `make(4 * n)` takes less than eight times what it takes on
/// `make(n)`: work in proportion to the input takes four, in its square sixteen.
fn assert_linear(name: &str, n: u32, make: impl Fn(u32) -> Vec<u8>, run: impl Fn(Pdf)) {
    use std::time::{Duration, Instant};
    let timed = |n: u32| {
        let pdf = Pdf::from_bytes(make(n)).unwrap();
        let start = Instant::now();
        run(pdf);
        start.elapsed()
    };
    let mut seen = Vec::new();
    for _ in 0..3 {
        let small = timed(n).max(Duration::from_millis(10));
        let large = timed(4 * n);
        if large < small * 8 {
            return;
        }
        seen.push((small, large));
    }
    panic!("{name} is not linear in its input: {seen:?}");
}

/// A file with a cross-reference stream: `objects` are numbered from 1, and an
/// object stream holds objects `first` to `first + count`, all at the start of
/// `body`.
fn in_an_object_stream(objects: &[Vec<u8>], first: u32, count: u32, body: &[u8]) -> Vec<u8> {
    use navree_pdf::{Dictionary, Stream};
    use std::collections::BTreeMap;
    let deflate = |data: Vec<u8>| {
        let mut objects = navree_pdf::Objects::new();
        let id = objects.add(Stream::new(Dictionary::new(), data));
        objects.compress();
        objects.get(id).unwrap().as_stream().unwrap().clone()
    };
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut rows: BTreeMap<u32, (u8, u32, u32)> = BTreeMap::new();
    for (index, object) in objects.iter().enumerate() {
        let number = index as u32 + 1;
        rows.insert(number, (1, out.len() as u32, 0));
        out.extend(format!("{number} 0 obj\n").bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let container = first + count;
    let mut head = String::new();
    for index in 0..count {
        head.push_str(&format!("{} 0 ", first + index));
        rows.insert(first + index, (2, container, index));
    }
    let mut inside = head.clone().into_bytes();
    inside.extend_from_slice(body);
    let packed = deflate(inside);
    rows.insert(container, (1, out.len() as u32, 0));
    out.extend(format!("{container} 0 obj\n").bytes());
    out.extend_from_slice(&stream(
        &format!(
            "/Type/ObjStm/N {count}/First {}/Filter/FlateDecode",
            head.len()
        ),
        &packed.content,
    ));
    out.extend_from_slice(b"\nendobj\n");
    let own = container + 1;
    let xref = out.len() as u32;
    rows.insert(own, (1, xref, 0));
    let mut table = Vec::new();
    let mut index: Vec<(u32, u32)> = Vec::new();
    for (number, (kind, second, third)) in &rows {
        table.push(*kind);
        table.extend_from_slice(&second.to_be_bytes());
        table.extend_from_slice(&third.to_be_bytes()[1..]);
        match index.last_mut() {
            Some((start, length)) if *start + *length == *number => *length += 1,
            _ => index.push((*number, 1)),
        }
    }
    let list: String = index.iter().map(|(f, c)| format!("{f} {c} ")).collect();
    out.extend(format!("{own} 0 obj\n").bytes());
    out.extend_from_slice(&stream(
        &format!(
            "/Type/XRef/Size {}/W[1 4 3]/Index[{list}]/Root 1 0 R",
            own + 1
        ),
        &table,
    ));
    out.extend(format!("\nendobj\nstartxref\n{xref}\n%%EOF\n").bytes());
    out
}

/// `k` nodes of a page tree that all have the same `Kids`, which lists them
/// all, and one real page.
fn nodes_sharing_their_kids(k: u32) -> Vec<u8> {
    let kids: String = (0..k).map(|i| format!("{} 0 R ", 100 + i)).collect();
    let objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R 100 0 R]/Count 1>>".to_vec(),
        b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 600 800]>>".to_vec(),
        b"0".to_vec(),
        format!("[{kids}]").into_bytes(),
    ];
    in_an_object_stream(&objects, 100, k, b"<</Type/Pages/Kids 5 0 R/Count 1>>")
}

#[test]
fn nodes_that_share_their_kids_cost_an_edit_no_more_than_their_size() {
    assert_linear(
        "an edit of shared kids",
        1_000,
        nodes_sharing_their_kids,
        |pdf| {
            pdf.with_metadata("T", None).unwrap();
        },
    );
}

/// `k` pages whose `key` is the same array of `items` numbers.
fn pages_sharing_a_long_box(k: u32, items: usize, key: &str) -> Vec<u8> {
    let kids: String = (0..k).map(|i| format!("{} 0 R ", 100 + i)).collect();
    let objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        format!("<</Type/Pages/Kids[{kids}]/Count {k}/MediaBox[0 0 600 800]>>").into_bytes(),
        b"0".to_vec(),
        b"0".to_vec(),
        format!("[{}]", "1 ".repeat(items)).into_bytes(),
    ];
    let body = format!("<</Type/Page/Parent 2 0 R/{key} 5 0 R>>");
    in_an_object_stream(&objects, 100, k, body.as_bytes())
}

#[test]
fn pages_that_share_a_long_box_cost_a_stamp_no_more_than_their_size() {
    let stamp = Stamp::text("X");
    for key in ["CropBox", "MediaBox"] {
        assert_linear(
            key,
            100,
            |k| pages_sharing_a_long_box(k, 40 * k as usize, key),
            |pdf| {
                pdf.stamp(&stamp).unwrap();
            },
        );
    }
}

#[test]
fn a_box_is_four_numbers_or_absent() {
    let stamped = |media_box: &str| {
        let pdf = on_box(media_box, "");
        stamp_position(
            &pdf.stamp(&Stamp::text("PAID").at(Position::BottomLeft))
                .unwrap(),
        )
    };
    let (x, y) = stamped("[1000 1000 1612 1792]");
    assert!(x > 1000.0 && y > 1000.0, "{x} {y}");
    // Five numbers, or one that is not a number: the box is absent, and the
    // page is the default size.
    for other in [
        "[1000 1000 1612 1792 5]",
        "[1000 1000 1612 /x]",
        "[1000 1000]",
    ] {
        let (x, y) = stamped(other);
        assert!(x < 612.0 && y < 792.0, "{other}: {x} {y}");
    }
}

#[test]
fn what_a_page_lists_in_contents_that_is_no_stream_is_not_written() {
    let page = |contents: &str| {
        file(
            &[
                b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
                b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
                format!(
                    "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents {contents}                     /Resources<</Font<</F 6 0 R>>>>>>"
                )
                .into_bytes(),
                stream("", ORIGINAL),
                b"(no stream)".to_vec(),
                FONT.to_vec(),
            ],
            "<</Root 1 0 R/Size 7>>",
        )
    };
    let seal = Pdf::from_lines(["SEAL"]);
    for contents in [
        "99 0 R",
        "[99 0 R 4 0 R]",
        "[5 0 R 4 0 R 99 0 R]",
        "5 0 R",
        "null",
    ] {
        let pdf = Pdf::from_bytes(page(contents)).unwrap();
        assert_contents_are_streams(&pdf.stamp(&Stamp::text("X")).unwrap());
        assert_contents_are_streams(&pdf.overlay(&seal, Pages::All).unwrap());
    }
    let pdf = Pdf::from_bytes(page("[99 0 R 4 0 R]")).unwrap();
    let text = pdf.stamp(&Stamp::text("X")).unwrap().text().unwrap();
    assert!(text.contains("Original page text"), "{text:?}");
}

#[test]
fn an_edit_follows_a_parent_given_through_a_reference_to_a_reference() {
    let bytes = file(
        &[
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1/MediaBox[0 0 612 792]/Resources<</Font<</F 5 0 R>>>>>>".to_vec(),
            b"<</Type/Page/Parent 6 0 R/Contents 4 0 R>>".to_vec(),
            stream("", ORIGINAL),
            FONT.to_vec(),
            b"2 0 R".to_vec(),
        ],
        "<</Root 1 0 R/Size 7>>",
    );
    let pdf = Pdf::from_bytes(bytes).unwrap();
    assert!(pdf.text().unwrap().contains("Original page text"));
    for edited in [
        pdf.stamp(&Stamp::text("X")).unwrap(),
        Pdf::merge([&pdf]).unwrap(),
    ] {
        let text = edited.text().unwrap();
        assert!(text.contains("Original page text"), "{text:?}");
    }
}

#[test]
fn an_edit_does_without_what_only_an_annotation_refers_to() {
    let bytes = file(
        &[
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
            b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R/Annots[6 0 R]/Thumb 7 0 R/Resources<</Font<</F 5 0 R>>>>>>".to_vec(),
            stream("", ORIGINAL),
            FONT.to_vec(),
            b"<</A 1 ".to_vec(),
            Vec::new(),
        ],
        "<</Root 1 0 R/Size 8>>",
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

/// `pages` pages that are `<<>>` at one place of an object stream.
fn empty_pages(pages: u32) -> Vec<u8> {
    let kids: String = (0..pages).map(|i| format!("{} 0 R ", 100 + i)).collect();
    let objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        format!("<</Type/Pages/Kids[{kids}]/Count {pages}>>").into_bytes(),
    ];
    in_an_object_stream(&objects, 100, pages, b"<<>>")
}

#[test]
fn a_stamp_takes_time_in_proportion_to_its_pages() {
    assert_linear("a stamp", 2_000, empty_pages, |pdf| {
        pdf.stamp(&Stamp::text("X")).unwrap();
    });
}

/// A file of many stored streams, each too short to gain by compressing.
fn short_streams(count: u32) -> Vec<u8> {
    let list: String = (0..count).map(|i| format!("{} 0 R ", 4 + i)).collect();
    let mut objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
        format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 9 9]/Contents[{list}]>>").into_bytes(),
    ];
    for _ in 0..count {
        objects.push(stream("", b"q Q q Q q Q q Q q Q q Q q Q q Q q Q q Q"));
    }
    let size = objects.len() + 1;
    file(&objects, &format!("<</Root 1 0 R/Size {size}>>"))
}

#[test]
fn saving_many_short_streams_makes_no_compressor_for_each() {
    use std::time::Instant;
    // How long reading the text of a small document takes, for scale.
    let scale = Pdf::from_lines((0..10_000).map(|i| format!("line {i}")));
    let start = Instant::now();
    scale.text().unwrap();
    let reading = start.elapsed();
    let pdf = Pdf::from_bytes(short_streams(20_000)).unwrap();
    let start = Instant::now();
    pdf.with_metadata("T", None).unwrap();
    let saving = start.elapsed();
    assert!(
        saving < 8 * reading,
        "{saving:?} to save, {reading:?} to read"
    );
}

/// A document of `pages` ordinary pages with `fonts` fonts (a map, widths,
/// `Differences`) and `forms` forms, each page drawing them all and showing
/// `glyphs` glyphs.
fn ordinary(pages: usize, fonts: usize, forms: usize, glyphs: usize) -> Pdf {
    use navree_pdf::{Dictionary, Objects, Stream, dictionary};
    let mut doc = Objects::new();
    let tree = doc.reserve();
    let map = doc.add(Stream::new(
        Dictionary::new(),
        b"1 begincodespacerange <00> <FF> endcodespacerange\n\
          2 beginbfrange <00> <7F> <0000> <80> <FF> <0080> endbfrange\n"
            .to_vec(),
    ));
    let mut font_names = Dictionary::new();
    let mut content = String::from("BT 12 TL 50 750 Td\n");
    for font in 0..fonts {
        let widths: Vec<Object> = (0..256).map(|_| Object::Integer(500)).collect();
        let mut differences = vec![Object::Integer(128)];
        differences.extend((0..64).map(|i| Object::from(format!("uni{:04X}", 0x400 + i).as_str())));
        let id = doc.add(dictionary! {
            "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "ABCDEF+Body",
            "FirstChar" => 0, "Widths" => widths, "ToUnicode" => map,
            "Encoding" => dictionary! { "Differences" => differences },
        });
        font_names.set(format!("F{font}"), id);
    }
    let line = "x".repeat(70);
    for index in 0..glyphs / 70 {
        content.push_str(&format!("/F{} 10 Tf ({line}) Tj T*\n", index % fonts));
    }
    content.push_str("ET\n");
    let mut form_names = Dictionary::new();
    for form in 0..forms {
        let id = doc.add(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 9.into(), 9.into()],
            },
            b"0 0 1 rg 0 0 9 9 re f".to_vec(),
        ));
        form_names.set(format!("X{form}"), id);
        content.push_str(&format!("q 1 0 0 1 {form} 0 cm /X{form} Do Q\n"));
    }
    let resources = doc.add(dictionary! { "Font" => font_names, "XObject" => form_names });
    let content = doc.add(Stream::new(Dictionary::new(), content.into_bytes()));
    let kids: Vec<Object> = (0..pages)
        .map(|_| {
            doc.add(dictionary! {
                "Type" => "Page", "Parent" => tree, "Contents" => content, "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            })
            .into()
        })
        .collect();
    doc.insert(
        tree,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => pages as i64 },
    );
    let catalog = doc.add(dictionary! { "Type" => "Catalog", "Pages" => tree });
    doc.trailer.set("Root", catalog);
    Pdf::from_bytes(doc.save().unwrap()).unwrap()
}

#[test]
fn large_ordinary_documents_fit_the_default_limits() {
    // 1,000 dense pages; 100 pages of 200 fonts and 500 forms each.
    for pdf in [ordinary(1_000, 8, 2, 3_500), ordinary(100, 200, 500, 2_000)] {
        let text = pdf.page_texts().unwrap();
        assert!(text[0].contains("xxxx"), "{:?}", text[0]);
        let stamped = pdf.stamp(&Stamp::text("PAID")).unwrap();
        assert!(stamped.page_texts().unwrap()[0].contains("PAID"));
        let seal = Pdf::from_lines(["SEAL"]);
        pdf.overlay(&seal, Pages::All).unwrap();
        Pdf::merge([&pdf, &pdf]).unwrap();
    }
}

/// The peak of an edit in a process of its own: the high-water mark of its
/// resident memory. Linux only: the kernel keeps the figure.
#[cfg(target_os = "linux")]
mod peak {
    use super::*;

    fn status(key: &str) -> usize {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let line = status.lines().find(|line| line.starts_with(key)).unwrap();
        let kib: usize = line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .unwrap();
        kib * 1024
    }

    fn in_child(name: &str) -> usize {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pdf_hostile::peak::child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("NAVREE_PDF_EDIT_PEAK_CASE", name)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{out}");
        out.lines()
            .find_map(|line| line.strip_prefix("PEAK "))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_else(|| panic!("{out}"))
    }

    /// What a process holds past what is counted: its allocator's own.
    const SLACK: usize = 8 << 20;
    /// What the documentation says an edit adds for a page, at the most.
    const ADDED_FOR_A_PAGE: usize = 3 * 1024;

    /// The objects `bytes` is counted as holding.
    fn counted(bytes: &[u8]) -> usize {
        let document = Document::load(bytes.to_vec(), ReadLimits::default()).unwrap();
        let _ = document.objects().count();
        document.memory_used()
    }

    /// The peak of `edit` past the documented sum: twice what the objects are
    /// counted as, what it adds for `pages` pages, what it writes as that
    /// grows, the file.
    fn past_the_sum(bytes: Vec<u8>, pages: usize, edit: impl FnOnce(&Pdf) -> Pdf) {
        let held = counted(&bytes);
        let size = bytes.len();
        let pdf = Pdf::from_bytes(bytes).unwrap();
        std::fs::write("/proc/self/clear_refs", "5").unwrap();
        let before = status("VmRSS:");
        let out = edit(&pdf).len();
        let peak = status("VmHWM:").saturating_sub(before);
        println!("held {peak}, objects {held}, file {size}, written {out}");
        let sum = 2 * held + pages * ADDED_FOR_A_PAGE + 2 * out + 2 * size;
        println!("PEAK {}", peak.saturating_sub(sum));
    }

    #[test]
    fn child() {
        let Ok(case) = std::env::var("NAVREE_PDF_EDIT_PEAK_CASE") else {
            return;
        };
        match case.as_str() {
            // 4,000 nodes of 4,000 kids: 70 MB on the stack if each is pushed.
            "kids" => past_the_sum(nodes_sharing_their_kids(4_000), 0, |pdf| {
                pdf.with_metadata("T", None).unwrap()
            }),
            "pages" => past_the_sum(empty_pages(20_000), 20_000, |pdf| {
                pdf.stamp(&Stamp::text("X")).unwrap()
            }),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn nodes_that_share_their_kids_hold_what_they_are_counted_as() {
        let past = in_child("kids");
        assert!(past < SLACK, "{} MiB past the sum", past >> 20);
    }

    #[test]
    fn a_stamp_holds_no_more_than_the_sum_for_each_page() {
        let past = in_child("pages");
        assert!(past < SLACK, "{} MiB past the sum", past >> 20);
    }
}

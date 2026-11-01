//! What the reader promises about bytes in, work done and memory held, proved
//! on the files that try hardest to break it: a scan that restarts, a parse
//! that fails and rewinds, a table looked through for every entry, a filter
//! chain, a stream that inflates and then cannot be used, a map that lies to
//! its caps.
//!
//! Each attack is built at a small scale, and held to small limits where a
//! limit is what stops it. A time is compared with the time for an input four
//! times smaller: work in proportion to the input takes four times as long,
//! work in its square sixteen.

use std::io::Write as _;
use std::time::{Duration, Instant};

use navree_pdf::text::{self, TextLimits};
use navree_pdf::{Document, Objects, PdfError, ReadLimits};

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

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

/// One page: 1 catalog, 2 pages, 3 the page, then `rest` from object 4.
fn doc(page: &str, rest: Vec<Vec<u8>>) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] {page} >>").into_bytes(),
    ];
    objects.extend(rest);
    let size = objects.len() + 1;
    file(&objects, &format!("<< /Root 1 0 R /Size {size} >>"))
}

/// A font with the `ToUnicode` map `map`, and a page that shows `content`.
fn font_doc(map: &[u8], content: &[u8]) -> Vec<u8> {
    doc(
        "/Contents 4 0 R /Resources << /Font << /F 5 0 R >> >>",
        vec![
            stream("", content),
            b"<< /Type /Font /Subtype /Type0 /Encoding /Identity-H /ToUnicode 6 0 R >>".to_vec(),
            stream("/Filter /FlateDecode", &deflate(map)),
        ],
    )
}

/// Whether `run` on `make(4 * n)` takes less than eight times what it takes on
/// `make(n)`: a loop over the input takes four, one in its square sixteen.
/// Times under 10 ms count as 10 ms, and the machine gets three tries.
fn assert_linear(name: &str, n: usize, make: impl Fn(usize) -> Vec<u8>, run: impl Fn(Vec<u8>)) {
    let timed = |n: usize| {
        let input = make(n);
        let start = Instant::now();
        run(input);
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

fn too_large<T: std::fmt::Debug>(result: Result<T, PdfError>) -> String {
    match result {
        Err(PdfError::TooLarge(message)) => message,
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

/// Limits of `work` bytes scanned for a document.
fn work(work: usize) -> ReadLimits {
    ReadLimits {
        work,
        ..ReadLimits::default()
    }
}

/// What a file that holds no object worth reading costs to load: scans of
/// it, a few times over. The hostile files below stay within this or are
/// refused.
fn linear_work(file: usize) -> usize {
    8 * file + 64 * 1024
}

// The scan that rebuilds a cross-reference.

#[test]
fn stream_keywords_without_an_end_are_scanned_once() {
    let make = |n: usize| {
        let mut file = b"%PDF-1.7\n".to_vec();
        file.extend(b"stream\n".repeat(n));
        file
    };
    assert_linear("the scan", 4_000, make, |file| {
        let limits = work(linear_work(file.len()));
        assert!(matches!(
            Document::load(file, limits),
            Err(PdfError::Damaged(_))
        ));
    });
}

#[test]
fn a_comment_after_each_endstream_is_not_read_again() {
    let make = |n: usize| {
        let mut file = b"%PDF-1.7\n1 0 obj\n<<>>\nstream\n".to_vec();
        file.extend(b"endstream%".repeat(n));
        file
    };
    assert_linear("the end of a stream", 3_000, make, |file| {
        let limits = work(linear_work(file.len()));
        assert!(matches!(
            Document::load(file, limits),
            Err(PdfError::Damaged(_))
        ));
    });
}

#[test]
fn objects_without_a_trailer_find_their_end_directly() {
    let make = |n: usize| {
        let mut file = b"%PDF-1.7\n".to_vec();
        for number in 1..=n {
            file.extend(format!("{number} 0 obj\n").bytes());
        }
        file
    };
    assert_linear("the rebuild", 8_000, make, |file| {
        assert!(matches!(
            Document::load(file, ReadLimits::default()),
            Err(PdfError::Damaged(_))
        ));
    });
}

// What a parse looked at, when it failed.

#[test]
fn a_header_that_fails_is_charged_for_the_bytes_it_looked_at() {
    // Entries at a `1` followed by 256 KiB of spaces: each one scans them,
    // 1,000 x 256 KiB in all for a file of about 270 KB.
    let run = 256 * 1024;
    let entries = 1_000;
    let mut file = b"%PDF-1.7\n1".to_vec();
    file.resize(file.len() + run, b' ');
    file.push(b'\n');
    let xref = file.len();
    file.extend(format!("xref\n10 {entries}\n").bytes());
    file.extend(b"9 0 n ".repeat(entries));
    file.extend(
        format!(
            "trailer\n<</Root 1 0 R/Size {}>>\nstartxref\n{xref}\n%%EOF\n",
            entries + 10
        )
        .bytes(),
    );
    let message = too_large(Document::load(file, work(16 * 1024 * 1024)));
    assert!(message.contains("16 MiB"), "{message}");
}

/// A file whose object stream (3) lists `members` objects from
/// `first_number` at `offset` of `body`, with a cross-reference stream that
/// lists `entries` objects from 10 in it.
fn object_stream_file(
    members: usize,
    entries: usize,
    body: &[u8],
    offset: usize,
    first_number: usize,
) -> Vec<u8> {
    let mut header = String::new();
    for index in 0..members {
        header.push_str(&format!("{} {offset} ", first_number + index));
    }
    let mut data = header.clone().into_bytes();
    data.extend_from_slice(body);
    let packed = deflate(&data);
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![out.len()];
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets.push(out.len());
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
    offsets.push(out.len());
    out.extend(
        format!(
            "3 0 obj\n<< /Type /ObjStm /N {members} /First {} /Filter /FlateDecode /Length {} >>\nstream\n",
            header.len(),
            packed.len()
        )
        .bytes(),
    );
    out.extend_from_slice(&packed);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = out.len();
    let row = |kind: u8, second: u32, third: u16| {
        let mut row = vec![kind];
        row.extend(second.to_be_bytes());
        row.extend(third.to_be_bytes());
        row
    };
    let mut rows = row(0, 0, 65535);
    for at in &offsets {
        rows.extend(row(1, *at as u32, 0));
    }
    rows.extend(row(1, xref as u32, 0));
    for index in 0..entries {
        rows.extend(row(2, 3, (index % 65536) as u16));
    }
    let rows = deflate(&rows);
    out.extend(
        format!(
            "4 0 obj\n<< /Type /XRef /Size {} /Root 1 0 R /W [1 4 2] /Index [0 5 10 {entries}] /Filter /FlateDecode /Length {} >>\nstream\n",
            10 + entries,
            rows.len()
        )
        .bytes(),
    );
    out.extend_from_slice(&rows);
    out.extend(format!("\nendstream\nendobj\nstartxref\n{xref}\n%%EOF\n").bytes());
    out
}

#[test]
fn a_string_that_never_ends_is_charged_for_each_object_that_starts_at_it() {
    // 400 members at one `(` and 1 MiB of text: 400 MiB parsed.
    let mut body = vec![b'('];
    body.resize(1 << 20, b'a');
    let file = object_stream_file(400, 400, &body, 0, 10);
    let document = Document::load(file, work(64 * 1024 * 1024)).unwrap();
    let _ = document.objects().count();
    let message = too_large(document.check());
    assert!(message.contains("64 MiB"), "{message}");
}

#[test]
fn a_hex_string_that_never_ends_is_charged_too() {
    let mut body = vec![b'<'];
    body.resize(1 << 20, b'a');
    let file = object_stream_file(400, 400, &body, 0, 10);
    let document = Document::load(file, work(64 * 1024 * 1024)).unwrap();
    let _ = document.objects().count();
    too_large(document.check());
}

#[test]
fn a_string_in_a_trailer_that_never_ends_is_charged() {
    // Each `trailer` parses a string to the end of the file, and fails: the
    // backslash keeps the parentheses from nesting.
    let mut file = b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n".to_vec();
    file.extend(b"trailer<</A\\(".repeat(1_000));
    file.resize(file.len() + (256 << 10), b'a');
    let message = too_large(Document::load(file, work(16 * 1024 * 1024)));
    assert!(message.contains("16 MiB"), "{message}");
}

// A table looked through once for each entry.

#[test]
fn entries_an_object_stream_does_not_list_are_found_directly() {
    let make = |n: usize| object_stream_file(n, n, b"0 ", 0, 10_000_000);
    assert_linear("the members", 6_000, make, |file| {
        let document = Document::load(file, ReadLimits::default()).unwrap();
        assert!(document.objects().count() <= 5);
        document.check().unwrap();
    });
}

// Filters.

#[test]
fn a_filter_chain_longer_than_any_real_one_is_not_run() {
    let packed = deflate(&vec![b' '; 1 << 20]);
    let chain = format!("/Filter [/FlateDecode {}]", "/Crypt ".repeat(2_000));
    let file = doc("/Contents 4 0 R", vec![stream(&chain, &packed)]);
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let stream = document.object((4, 0)).unwrap().as_stream().unwrap();
    assert!(document.decode(stream, 8 << 20).is_err());
    // A work limit smaller than one step of it would have been.
    let limits = TextLimits {
        page_work: 512 << 10,
        ..TextLimits::default()
    };
    text::page_texts_within(&document, limits).unwrap();
}

#[test]
fn eight_filters_are_run() {
    let mut data = b"BT ET".to_vec();
    for _ in 0..8 {
        data = deflate(&data);
    }
    let chain = format!("/Filter [{}]", "/FlateDecode ".repeat(8));
    let file = doc("/Contents 4 0 R", vec![stream(&chain, &data)]);
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let stream = document.object((4, 0)).unwrap().as_stream().unwrap();
    assert_eq!(document.decode(stream, 1024).unwrap(), b"BT ET");
}

#[test]
fn what_inflates_and_then_cannot_be_used_is_charged_on_a_page() {
    // 200 x 1 MiB of content that inflates, then stops at an image codec.
    let packed = deflate(&vec![b' '; 1 << 20]);
    let refs = "4 0 R ".repeat(200);
    let file = doc(
        &format!("/Contents [{refs}]"),
        vec![stream("/Filter [/FlateDecode /DCTDecode]", &packed)],
    );
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let limits = TextLimits {
        page_work: 16 << 20,
        ..TextLimits::default()
    };
    let message = too_large(text::page_texts_within(&document, limits));
    assert!(message.contains("16 MiB"), "{message}");
}

#[test]
fn what_inflates_and_then_cannot_be_used_is_charged_on_a_form() {
    // Flate of Flate: a few hundred bytes stored, 200 draws of 1 MiB.
    let packed = deflate(&deflate(&vec![b' '; 1 << 20]));
    let mut draws = b"/X Do ".repeat(200);
    draws.push(b'\n');
    let file = doc(
        "/Contents 4 0 R /Resources << /XObject << /X 5 0 R >> >>",
        vec![
            stream("", &draws),
            stream(
                "/Subtype /Form /Filter [/FlateDecode /FlateDecode /DCTDecode]",
                &packed,
            ),
        ],
    );
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let limits = TextLimits {
        page_work: 16 << 20,
        ..TextLimits::default()
    };
    let message = too_large(text::page_texts_within(&document, limits));
    assert!(message.contains("16 MiB"), "{message}");
}

#[test]
fn a_map_that_inflates_and_then_cannot_be_used_is_charged() {
    // 40 fonts, each with a map of 1 MiB that stops at an image codec.
    let packed = deflate(&vec![b' '; 1 << 20]);
    let fonts = 40;
    let names: String = (0..fonts)
        .map(|i| format!("/F{i} {} 0 R ", 5 + i))
        .collect();
    let content: String = (0..fonts)
        .map(|i| format!("BT /F{i} 12 Tf <41> Tj ET\n"))
        .collect();
    let mut rest = vec![stream("", content.as_bytes())];
    for i in 0..fonts {
        rest.push(
            format!(
                "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/ToUnicode {} 0 R>>",
                5 + fonts + i
            )
            .into_bytes(),
        );
    }
    for _ in 0..fonts {
        rest.push(stream("/Filter [/FlateDecode /DCTDecode]", &packed));
    }
    let file = doc(
        &format!("/Contents 4 0 R /Resources << /Font << {names} >> >>"),
        rest,
    );
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let limits = TextLimits {
        page_work: 8 << 20,
        ..TextLimits::default()
    };
    too_large(text::page_texts_within(&document, limits));
}

#[test]
fn a_contents_array_many_pages_share_costs_each_page() {
    // 50,000 references to one empty stream, in one array, on 500 pages: 25
    // million lookups for a file of about 350 KB.
    let refs = 50_000;
    let pages = 500;
    let kids: String = (0..pages).map(|i| format!("{} 0 R ", i + 5)).collect();
    let mut objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        format!("<</Type/Pages/Kids[{kids}]/Count {pages}>>").into_bytes(),
        format!("[{}]", "4 0 R ".repeat(refs)).into_bytes(),
        stream("", b""),
    ];
    for _ in 0..pages {
        objects.push(b"<</Type/Page/Parent 2 0 R/Contents 3 0 R>>".to_vec());
    }
    let file = file(
        &objects,
        &format!("<</Root 1 0 R/Size {}>>", objects.len() + 1),
    );
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let limits = TextLimits {
        document_work: 16 << 20,
        ..TextLimits::default()
    };
    too_large(text::page_texts_within(&document, limits));
}

#[test]
fn widths_that_are_not_numbers_are_charged_each_time_they_are_looked_at() {
    // One list of 20,000 names, given 2,000 times by a font's `W`: 40 million
    // looks at a list for a file of about 80 KB.
    let list = format!("[{}]", "/a ".repeat(20_000));
    let w = format!("[{}]", "1 6 0 R ".repeat(2_000));
    let file = doc(
        "/Contents 4 0 R /Resources << /Font << /F 5 0 R >> >>",
        vec![
            stream("", b"BT /F 12 Tf <0041> Tj ET"),
            b"<</Type/Font/Subtype/Type0/Encoding/Identity-H/DescendantFonts[<</W 7 0 R>>]>>"
                .to_vec(),
            list.into_bytes(),
            w.into_bytes(),
        ],
    );
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let limits = TextLimits {
        page_work: 8 << 20,
        ..TextLimits::default()
    };
    too_large(text::page_texts_within(&document, limits));
}

#[test]
fn comparisons_on_many_pages_are_held_to_the_document_s_work() {
    // 40 pages of 14,000 letters that all sit at one place: about 49 million
    // comparisons a page, within the page's limit, from 14 KB of content.
    let pages = 40;
    let kids: String = (0..pages).map(|i| format!("{} 0 R ", i + 5)).collect();
    let mut objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        format!("<</Type/Pages/Kids[{kids}]/Count {pages}/Resources<</Font<</F 4 0 R>>>>>>")
            .into_bytes(),
        stream("", &{
            let mut content = b"BT /F 12 Tf (".to_vec();
            content.extend(b"ab".repeat(7_000));
            content.extend_from_slice(b") Tj ET");
            content
        }),
        b"<</Type/Font/Subtype/TrueType/BaseFont/X/Encoding/WinAnsiEncoding/FirstChar 97/Widths[1000 0]>>"
            .to_vec(),
    ];
    for _ in 0..pages {
        objects.push(b"<</Type/Page/Parent 2 0 R/Contents 3 0 R>>".to_vec());
    }
    let file = file(
        &objects,
        &format!("<</Root 1 0 R/Size {}>>", objects.len() + 1),
    );
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let limits = TextLimits {
        document_work: 8 << 20,
        ..TextLimits::default()
    };
    too_large(text::page_texts_within(&document, limits));
}

// ToUnicode maps: one reader, and the caps it enforces.

#[test]
fn a_map_of_a_string_that_never_ends_is_read_once() {
    let make = |n: usize| {
        let mut map = vec![b'('];
        map.extend(b"\\(".repeat(n));
        font_doc(&map, b"BT /F 12 Tf <0041> Tj ET")
    };
    assert_linear("a map", 4_000, make, |file| {
        let document = Document::load(file, ReadLimits::default()).unwrap();
        let limits = TextLimits {
            page_work: 8 << 20,
            ..TextLimits::default()
        };
        text::page_texts_within(&document, limits).unwrap();
    });
}

fn read_map(map: Vec<u8>) -> Result<Vec<String>, PdfError> {
    let file = font_doc(&map, b"BT /F 12 Tf <0041> Tj ET");
    let document = Document::load(file, ReadLimits::default()).unwrap();
    text::page_texts(&document)
}

#[test]
fn a_map_over_the_codespace_cap_is_refused() {
    let mut map = b"begincodespacerange\n".to_vec();
    map.extend(b"<0000><0000>\n".repeat(65));
    too_large(read_map(map));
    let mut map = b"begincodespacerange\n".to_vec();
    map.extend(b"<0000><FFFF>\n".repeat(64));
    read_map(map).unwrap();
}

#[test]
fn a_text_that_is_not_hex_does_not_hide_a_map_from_its_caps() {
    let mut map = b"begincodespacerange\n<g>\n<00>\n".to_vec();
    map.extend(b"<0000><0000>\n".repeat(1_000));
    too_large(read_map(map));
    let mut map = b"beginbfchar\n<g>\n<00>\n".to_vec();
    for code in 0..140_000u32 {
        map.extend(format!("<{:04X}><0041>\n", code % 65536).bytes());
    }
    too_large(read_map(map));
}

#[test]
fn a_string_that_never_ends_ends_the_map() {
    let mut map = b"beginbfchar\n<0041><0042>\n(\n".to_vec();
    for code in 0..140_000u32 {
        map.extend(format!("<{:04X}><0041>\n", code % 65536).bytes());
    }
    // What is after the string is inside it, and is not read: no cap is
    // passed, and the line before it counts.
    assert_eq!(read_map(map).unwrap(), ["B"]);
}

// Bytes in, bytes out.

/// `k` objects whose explicit `/Length` runs to one shared `endstream`, past
/// the next object: an edit would write each one.
fn overlapping(k: usize, pad: usize, keyword: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![out.len()];
    out.extend_from_slice(b"1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n");
    offsets.push(out.len());
    out.extend_from_slice(b"2 0 obj\n<</Type/Pages/Kids[]/Count 0>>\nendobj\n");
    let mut holes = Vec::new();
    for index in 0..k {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n<</Filter/DCTDecode/Length ", index + 3).bytes());
        holes.push(out.len());
        out.extend_from_slice(format!("0000000000>>\n{keyword}\n").as_bytes());
    }
    out.resize(out.len() + pad, b'x');
    let stop = out.len();
    for hole in &holes {
        let start = hole + "0000000000>>\n".len() + keyword.len() + 1;
        out[*hole..*hole + 10].copy_from_slice(format!("{:010}", stop - start).as_bytes());
    }
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", k + 3);
    for offset in offsets {
        tail.push_str(&format!("{offset:010} 00000 n \n"));
    }
    tail.push_str(&format!(
        "trailer\n<</Root 1 0 R/Size {}>>\nstartxref\n{xref}\n%%EOF\n",
        k + 3
    ));
    out.extend_from_slice(tail.as_bytes());
    out
}

#[test]
fn streams_that_overlap_are_not_written_once_each() {
    let file = overlapping(1_000, 64 * 1024, "stream");
    let len = file.len();
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let objects = Objects::read(document).unwrap();
    let saved = objects.save().unwrap();
    assert!(
        saved.len() < 2 * len,
        "{len} bytes saved as {}",
        saved.len()
    );
}

#[test]
fn the_stream_bytes_a_document_holds_are_at_most_the_file() {
    for (k, pad) in [(100, 16 * 1024), (400, 64 * 1024), (1_600, 64 * 1024)] {
        let file = overlapping(k, pad, "stream");
        let len = file.len();
        let document = Document::load(file, ReadLimits::default()).unwrap();
        let objects = Objects::read(document).unwrap();
        let held: usize = objects
            .objects
            .values()
            .filter_map(|object| object.as_stream())
            .map(|stream| stream.content.len())
            .sum();
        assert!(
            held <= len,
            "{len} bytes of file hold {held} bytes of streams"
        );
    }
}

#[test]
fn streams_that_overlap_in_a_damaged_file_are_not_written_once_each() {
    // A file with no cross-reference, whose objects a scan finds, each a
    // stream that runs to the same `endstream`. The scan does not take
    // `stream ` for a stream, so it sees every header.
    let mut file = overlapping(500, 64 * 1024, "stream ");
    let cut = file
        .windows(4)
        .rposition(|window| window == b"xref")
        .unwrap();
    let cut = file[..cut]
        .windows(4)
        .rposition(|window| window == b"xref")
        .unwrap_or(cut);
    file.truncate(cut);
    file.extend_from_slice(b"trailer\n<</Root 1 0 R/Size 503>>\n");
    let len = file.len();
    let document = Document::load(file, ReadLimits::default()).unwrap();
    assert!(document.rebuilt());
    let objects = Objects::read(document).unwrap();
    let saved = objects.save().unwrap();
    assert!(
        saved.len() < 2 * len,
        "{len} bytes saved as {}",
        saved.len()
    );
}

#[test]
fn stream_objects_a_cross_reference_finds_late_are_held_apart_too() {
    // The cross-reference lists objects where they are not, so the reader
    // finds them with a scan: they still end before the next one starts.
    let mut file = overlapping(400, 64 * 1024, "stream ");
    let xref = file
        .windows(5)
        .rposition(|window| window == b"xref\n")
        .unwrap();
    // Every entry from object 3 on points one byte into the catalog.
    let mut tail = String::from_utf8(file[xref..].to_vec()).unwrap();
    let entries: Vec<String> = tail.lines().map(str::to_owned).collect();
    for (index, line) in entries.iter().enumerate() {
        // `xref`, `0 N`, the free entry, the catalog, the page tree: the
        // rest are the streams.
        if index >= 5 && line.ends_with("00000 n ") {
            tail = tail.replacen(line.as_str(), "0000000010 00000 n ", 1);
        }
    }
    file.truncate(xref);
    file.extend_from_slice(tail.as_bytes());
    let len = file.len();
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let objects = Objects::read(document).unwrap();
    let saved = objects.save().unwrap();
    assert!(
        saved.len() < 2 * len,
        "{len} bytes saved as {}",
        saved.len()
    );
}

#[test]
fn saving_is_held_to_its_limit() {
    let file = doc(
        "/Contents 4 0 R",
        vec![stream("/Filter /DCTDecode", &vec![b'x'; 64 * 1024])],
    );
    let mut objects = Objects::read(Document::load(file, ReadLimits::default()).unwrap()).unwrap();
    let size = objects.save().unwrap().len();
    objects.set_output_limit(size);
    objects.save().unwrap();
    objects.set_output_limit(size - 1);
    too_large(objects.save());
    objects.set_output_limit(32 * 1024);
    too_large(objects.save());
    too_large(objects.save_with(true));
}

#[test]
fn a_document_read_to_be_edited_saves_within_its_bytes_and_its_memory_limit() {
    // Members of an object stream that share their bytes: the memory limit
    // lets each be read, and each is written as a hexadecimal string, twice
    // what it holds.
    let body = format!("<{}>", "41".repeat(16 * 1024));
    let file = object_stream_file(1_000, 1_000, body.as_bytes(), 0, 10);
    let len = file.len();
    let memory = 24 << 20;
    let limits = ReadLimits {
        memory,
        ..ReadLimits::default()
    };
    let objects = Objects::read(Document::load(file, limits).unwrap()).unwrap();
    assert_eq!(objects.output_limit(), len + memory);
    let message = too_large(objects.save());
    assert!(message.contains("write"), "{message}");
}

// Work and memory, scaled.

#[test]
fn the_work_charged_grows_with_the_file_and_no_faster() {
    // Files of nothing but keywords: a fixed multiple of the bytes in.
    for pad in [16 * 1024, 64 * 1024, 256 * 1024] {
        let mut file = b"%PDF-1.7\n".to_vec();
        file.extend(b"stream\n".repeat(pad / 7));
        let limits = work(4 * file.len());
        assert!(matches!(
            Document::load(file, limits),
            Err(PdfError::Damaged(_))
        ));
    }
    // Plain files, read whole: a few times what they weigh, each object costing
    // its lookups and the entry that finds it as well as its bytes.
    let mut last = 0usize;
    for pages in [100, 400, 1_600] {
        let kids: String = (0..pages).map(|i| format!("{} 0 R ", i + 3)).collect();
        let mut objects = vec![
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            format!("<</Type/Pages/Kids[{kids}]/Count {pages}>>").into_bytes(),
        ];
        for _ in 0..pages {
            objects.push(b"<</Type/Page/Parent 2 0 R>>".to_vec());
        }
        let file = file(
            &objects,
            &format!("<</Root 1 0 R/Size {}>>", objects.len() + 1),
        );
        let len = file.len();
        let document = Document::load(file, ReadLimits::default()).unwrap();
        assert_eq!(document.pages().len(), pages);
        let _ = document.objects().count();
        document.check().unwrap();
        let spent = document.work_used();
        assert!(spent <= 3 * len, "{len} bytes in, {spent} of work");
        assert!(spent > last, "{spent} after {last}");
        last = spent;
    }
}

/// The peak of what a case holds, in a process of its own: the high-water mark
/// of its resident memory past what it held when it started. Linux only: the
/// kernel keeps that figure for a process.
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

    /// Runs `run` on `input`, and returns what it took past what was held.
    fn measure(input: Vec<u8>, run: impl FnOnce(Vec<u8>)) -> usize {
        // Writing 5 here sets the high-water mark back to what is held now.
        std::fs::write("/proc/self/clear_refs", "5").unwrap();
        let before = status("VmRSS:");
        run(input);
        status("VmHWM:").saturating_sub(before)
    }

    /// The case `name`, run by the test `child` in a process of its own: its
    /// peak.
    fn in_child(name: &str) -> usize {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "peak::child", "--nocapture", "--test-threads=1"])
            .env("NAVREE_PDF_PEAK_CASE", name)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{out}");
        out.lines()
            .find_map(|line| line.strip_prefix("PEAK "))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_else(|| panic!("{out}"))
    }

    fn name_in_saved_states(tail: &[u8]) -> Vec<u8> {
        let mut content = b"/".to_vec();
        content.resize(1 + (256 << 10), b'A');
        content.extend_from_slice(b" 12 Tf ");
        content.extend(b"q ".repeat(256));
        content.extend_from_slice(tail);
        deflate(&content)
    }

    fn array(item: &[u8], count: usize) -> Vec<u8> {
        let mut body = b"[".to_vec();
        body.extend(item.repeat(count));
        body.push(b']');
        body
    }

    /// What a process holds past the memory counted for it: its allocator's
    /// own, and what small allocations round up to.
    const SLACK: usize = 8 << 20;

    /// The objects of `file` read, within 32 MiB: the peak past twice what
    /// was counted.
    fn charged(file: Vec<u8>) {
        let limits = ReadLimits {
            memory: 32 << 20,
            ..ReadLimits::default()
        };
        let mut used = 0;
        let peak = measure(file, |file| {
            let document = Document::load(file, limits).unwrap();
            let _ = document.objects().count();
            used = document.memory_used();
        });
        println!("USED {used} HELD {peak}");
        println!("PEAK {}", peak.saturating_sub(used * 2));
    }

    fn page_text_peak(file: Vec<u8>) {
        let peak = measure(file, |file| {
            let document = Document::load(file, ReadLimits::default()).unwrap();
            let _ = text::page_texts(&document);
        });
        println!(
            "
PEAK {peak}"
        );
    }

    /// The case the parent asked for, when it asked.
    #[test]
    fn child() {
        let Ok(case) = std::env::var("NAVREE_PDF_PEAK_CASE") else {
            return;
        };
        match case.as_str() {
            "saved-states" => page_text_peak(doc(
                "/Contents 4 0 R",
                vec![stream("/Filter /FlateDecode", &name_in_saved_states(b""))],
            )),
            "saved-states-nested" => page_text_peak(doc(
                "/Contents 4 0 R /Resources << /XObject << /X 5 0 R >> >>",
                vec![
                    stream("/Filter /FlateDecode", &name_in_saved_states(b"/X Do")),
                    stream(
                        "/Subtype /Form /Filter /FlateDecode",
                        &name_in_saved_states(b""),
                    ),
                ],
            )),
            "names" => charged(doc("", vec![array(b"/a ", 150_000)])),
            "integers" => charged(doc("", vec![array(b"0 ", 150_000)])),
            "empty-arrays" => charged(doc("", vec![array(b"[]", 150_000)])),
            "small-dictionaries" => charged(doc("", vec![array(b"<</a 0>>", 60_000)])),
            "one-dictionary" => {
                let mut body = b"<<".to_vec();
                for index in 0..100_000 {
                    body.extend(format!("/{index:x} 0").bytes());
                }
                body.extend_from_slice(b">>");
                charged(doc("", vec![body]));
            }
            "objects" => charged(doc("", (0..60_000).map(|_| b"0".to_vec()).collect())),
            "entries" => {
                let mut file = b"%PDF-1.7\n1 0 obj\n<</Type/Catalog>>\nendobj\n".to_vec();
                let xref = file.len();
                file.extend_from_slice(b"xref\n0 2\n0000000000 65535 f \n0000000009 00000 n \n");
                file.extend_from_slice(b"10 200000\n");
                file.extend(b"0000000009 00000 f \n".repeat(200_000));
                file.extend(
                    format!("trailer\n<</Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").bytes(),
                );
                charged(file);
            }
            other => panic!("{other}"),
        }
    }

    #[test]
    fn saved_states_share_the_font_name() {
        // 256 saved states of a name of 256 KiB: 64 MiB if each had a copy.
        let peak = in_child("saved-states");
        assert!(peak < 16 << 20, "{} MiB held", peak >> 20);
    }

    #[test]
    fn saved_states_across_nested_forms_share_it_too() {
        let peak = in_child("saved-states-nested");
        assert!(peak < 16 << 20, "{} MiB held", peak >> 20);
    }

    #[test]
    fn what_objects_hold_is_at_most_twice_what_is_counted() {
        // The figure the documentation gives, in the shapes where what is
        // held and what is counted are furthest apart.
        for case in [
            "names",
            "integers",
            "empty-arrays",
            "small-dictionaries",
            "one-dictionary",
            "objects",
            "entries",
        ] {
            let past = in_child(case);
            assert!(past < SLACK, "{case}: {} KiB past twice", past >> 10);
        }
    }
}

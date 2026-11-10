//! The reader's budget against files that amplify when they are read: object
//! streams whose objects all point at the same bytes, containers nested
//! deep or wide, and cross-references that make the reader scan the same
//! bytes again and again. Each attack is built at a small scale and read
//! within small limits, so a failure is an assertion, never an exhausted
//! machine.

use std::io::Write as _;

use navree_pdf::{Document, Objects, PdfError, ReadLimits};

/// Zlib-compressed `data`.
fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// A file whose catalog (1) and empty page tree (2) are plain objects,
/// whose object stream (3) holds `body` after a header that lists objects
/// 10 to `10 + members` all at `offset`, and whose cross-reference stream
/// lists those objects in it.
fn object_stream_file(members: u32, offset: usize, body: &[u8]) -> Vec<u8> {
    let mut header = String::new();
    for number in 10..10 + members {
        header.push_str(&format!("{number} {offset} "));
    }
    let mut data = header.clone().into_bytes();
    data.extend_from_slice(body);
    let packed = deflate(&data);

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    offsets.push(out.len());
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets.push(out.len());
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
    offsets.push(out.len());
    out.extend_from_slice(
        format!(
            "3 0 obj\n<< /Type /ObjStm /N {members} /First {} /Filter /FlateDecode /Length {} >>\nstream\n",
            header.len(),
            packed.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&packed);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref_at = out.len();
    // Rows of [type, 4-byte field, 2-byte field] for objects 0 to 4 (the
    // last is the cross-reference stream) and 10 onwards.
    let row = |kind: u8, second: u32, third: u16| {
        let mut row = vec![kind];
        row.extend_from_slice(&second.to_be_bytes());
        row.extend_from_slice(&third.to_be_bytes());
        row
    };
    let mut rows = row(0, 0, 65535);
    for offset in &offsets {
        rows.extend(row(1, *offset as u32, 0));
    }
    rows.extend(row(1, xref_at as u32, 0));
    for index in 0..members {
        rows.extend(row(2, 3, index as u16));
    }
    let rows = deflate(&rows);
    out.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /XRef /Size {} /Root 1 0 R /W [1 4 2] /Index [0 5 10 {members}] \
             /Filter /FlateDecode /Length {} >>\nstream\n",
            10 + members,
            rows.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&rows);
    out.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());
    out
}

fn small() -> ReadLimits {
    ReadLimits {
        memory: 4 * 1024 * 1024,
        ..ReadLimits::default()
    }
}

fn too_large<T: std::fmt::Debug>(result: Result<T, PdfError>) -> String {
    match result {
        Err(PdfError::TooLarge(message)) => message,
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

#[test]
fn object_stream_members_that_share_their_bytes_are_each_counted() {
    // 200 objects, each the same array of 20,000 zeros: 40 KB of text,
    // a few hundred bytes compressed. Read as separate objects, as a reader
    // that parses every member does, they hold 4 million objects.
    let body = format!("[{}]", "0 ".repeat(20_000));
    let file = object_stream_file(200, 0, body.as_bytes());
    assert!(file.len() < 4096, "{} bytes", file.len());
    // Loading reads the cross-reference only.
    let document = Document::load(file, small()).unwrap();
    assert!(
        document.memory_used() < 64 * 1024,
        "{}",
        document.memory_used()
    );
    // One member fits.
    assert_eq!(
        document.object((10, 0)).unwrap().as_array().unwrap().len(),
        20_000
    );
    document.check().unwrap();
    // All of them do not: the budget refuses before they are held.
    let message = too_large(Objects::read(document));
    assert!(message.contains("4 MiB"), "{message}");
    // Ten such objects within a budget that holds them read.
    let roomy = ReadLimits {
        memory: 64 * 1024 * 1024,
        ..ReadLimits::default()
    };
    let few = object_stream_file(10, 0, body.as_bytes());
    let objects = Objects::read(Document::load(few, roomy).unwrap()).unwrap();
    assert_eq!(objects.objects.len(), 14);
}

#[test]
fn wide_containers_are_counted_as_they_are_built() {
    // One object of 200,000 empty arrays: 400 KB of text, each `[]` two
    // bytes building an object of its own.
    let body = format!("[{}]", "[]".repeat(200_000));
    let file = object_stream_file(1, 0, body.as_bytes());
    let document = Document::load(file, small()).unwrap();
    assert!(document.object((10, 0)).is_none());
    too_large(document.check());
}

#[test]
fn containers_nested_past_the_depth_limit_are_not_read() {
    // 100 levels are read; 101 are not, without recursing further.
    let at = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    let deep = Document::load(object_stream_file(1, 0, at(100).as_bytes()), small()).unwrap();
    assert!(deep.object((10, 0)).is_some());
    let deeper = Document::load(object_stream_file(1, 0, at(101).as_bytes()), small()).unwrap();
    assert!(deeper.object((10, 0)).is_none());
    deeper.check().unwrap();
    // A hundred thousand levels: still just an object that is not read.
    let deepest = at(100_000);
    let document = Document::load(object_stream_file(1, 0, deepest.as_bytes()), small()).unwrap();
    assert!(document.object((10, 0)).is_none());
}

#[test]
fn an_object_stream_that_inflates_past_its_limit_is_read_as_damaged() {
    // 64 KiB of spaces before the object, from a few hundred bytes.
    let mut body = vec![b' '; 64 * 1024];
    body.extend_from_slice(b"(found)");
    let file = object_stream_file(1, 64 * 1024, &body);
    let tight = ReadLimits {
        object_stream: 32 * 1024,
        ..small()
    };
    let document = Document::load(file.clone(), tight).unwrap();
    assert!(document.object((10, 0)).is_none());
    document.check().unwrap();
    let document = Document::load(file, small()).unwrap();
    assert_eq!(
        document.object((10, 0)).unwrap().as_str(),
        Some(&b"found"[..])
    );
}

#[test]
fn cross_reference_entries_into_the_same_bytes_are_held_to_the_work_limit() {
    // 2,000 small objects the cross-reference finds, so it is believed,
    // then 1,000 entries all pointing into one run of 100 KB of white
    // space: looking for each one's object scans the run again, 100 MB in
    // all for a file of 200 KB.
    let mut out = b"%PDF-1.7
"
    .to_vec();
    let mut offsets = vec![out.len()];
    out.extend_from_slice(
        b"1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
",
    );
    offsets.push(out.len());
    out.extend_from_slice(
        b"2 0 obj
<< /Type /Pages /Kids [] /Count 0 >>
endobj
",
    );
    for number in 3..2003 {
        offsets.push(out.len());
        out.extend_from_slice(
            format!(
                "{number} 0 obj
null
endobj
"
            )
            .as_bytes(),
        );
    }
    let run = out.len();
    out.extend_from_slice(&vec![b' '; 100_000]);
    out.extend_from_slice(
        b"
",
    );
    offsets.extend((0..1000).map(|i| run + i));
    let xref = out.len();
    out.extend_from_slice(
        format!(
            "xref
0 {}
0000000000 65535 f
",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    for offset in &offsets {
        out.extend_from_slice(
            format!(
                "{offset:010} 00000 n
"
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(
        format!(
            "trailer
<< /Size {} /Root 1 0 R >>
startxref
{xref}
%%EOF
",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    assert!(out.len() < 256 * 1024, "{} bytes", out.len());
    let tight = ReadLimits {
        work: 16 * 1024 * 1024,
        ..ReadLimits::default()
    };
    let message = too_large(Document::load(out.clone(), tight));
    assert!(message.contains("16 MiB"), "{message}");
    // Within the default limit of 1 GiB the file reads, the entries that
    // find nothing missing.
    let document = Document::load(out, ReadLimits::default()).unwrap();
    assert!(document.catalog().is_some());
    let objects = Objects::read(document).unwrap();
    assert_eq!(objects.objects.len(), 2002);
}

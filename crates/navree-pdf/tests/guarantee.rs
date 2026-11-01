//! The guarantee in the crate's documentation, tested: the time a document
//! takes is in proportion to what is charged for it, and what it holds is
//! within the sum the documentation states, on every shape anyone has found
//! that brings them apart.
//!
//! A rate is a ratio against the unit, measured in the same run: how long one
//! byte of the densest plain content takes (one operator every two bytes). A
//! busy machine slows both. Run in a release build with `--nocapture` to print
//! the table that the documentation quotes.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::time::{Duration, Instant};

use navree_pdf::text::{self, TextLimits};
use navree_pdf::{Document, Object, Objects, PdfError, ReadLimits};

const MIB: usize = 1024 * 1024;

/// The most a byte charged may take, in units. A build with no optimizations
/// spreads its time differently, so it gets the larger figure.
const MOST: f64 = if cfg!(debug_assertions) { 8.0 } else { 3.0 };

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    object
}

fn repeated(unit: &[u8], len: usize) -> Vec<u8> {
    unit.repeat(len / unit.len())
}

/// Objects by number; written with a table or a cross-reference stream, with
/// object 1 as the catalog.
#[derive(Default)]
struct Pdf(BTreeMap<u32, Vec<u8>>);

impl Pdf {
    fn add(&mut self, number: u32, body: impl Into<Vec<u8>>) -> &mut Self {
        self.0.insert(number, body.into());
        self
    }

    /// The objects, a table, and a trailer with `extra` in it.
    fn table_with(&self, extra: &str) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut at = BTreeMap::new();
        for (number, body) in &self.0 {
            at.insert(*number, out.len());
            out.extend(format!("{number} 0 obj\n").bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(b"xref\n0 1\n0000000000 65535 f \n");
        for (number, offset) in &at {
            out.extend(format!("{number} 1\n{offset:010} 00000 n \n").bytes());
        }
        let size = at.keys().max().unwrap_or(&0) + 1;
        out.extend(
            format!("trailer\n<< /Root 1 0 R /Size {size} {extra} >>\nstartxref\n{xref}\n%%EOF\n")
                .bytes(),
        );
        out
    }

    fn table(&self) -> Vec<u8> {
        self.table_with("")
    }

    /// With entries for objects in object streams: (number, stream, index). A
    /// stream need not exist until one of them is read.
    fn with_xref_stream(&self, compressed: &[(u32, u32, u32)]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut rows: BTreeMap<u32, (u8, u32, u32)> = BTreeMap::new();
        for (number, body) in &self.0 {
            rows.insert(*number, (1, out.len() as u32, 0));
            out.extend(format!("{number} 0 obj\n").bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        for &(number, container, index) in compressed {
            rows.insert(number, (2, container, index));
        }
        let own = rows.keys().max().unwrap_or(&0) + 1;
        let xref = out.len() as u32;
        rows.insert(own, (1, xref, 0));
        let mut body = Vec::with_capacity(rows.len() * 8);
        let mut index: Vec<(u32, u32)> = Vec::new();
        for (number, (kind, second, third)) in &rows {
            body.push(*kind);
            body.extend_from_slice(&second.to_be_bytes());
            body.extend_from_slice(&third.to_be_bytes()[1..]);
            match index.last_mut() {
                Some((first, count)) if *first + *count == *number => *count += 1,
                _ => index.push((*number, 1)),
            }
        }
        let list: String = index.iter().map(|(f, c)| format!("{f} {c} ")).collect();
        out.extend(format!("{own} 0 obj\n").bytes());
        out.extend_from_slice(&stream(
            &format!(
                "/Type /XRef /Size {} /W [1 4 3] /Index [{list}] /Root 1 0 R /Filter /FlateDecode",
                own + 1
            ),
            &deflate(&body),
        ));
        out.extend_from_slice(b"\nendobj\n");
        out.extend(format!("startxref\n{xref}\n%%EOF\n").bytes());
        out
    }
}

/// An object stream: `members` (number, offset into `body`).
fn object_stream(members: &[(u32, usize)], body: &[u8]) -> Vec<u8> {
    let mut all: String = members.iter().map(|(n, o)| format!("{n} {o} ")).collect();
    let first = all.len();
    all.push_str(&String::from_utf8_lossy(body));
    stream(
        &format!(
            "/Type /ObjStm /N {} /First {first} /Filter /FlateDecode",
            members.len()
        ),
        &deflate(all.as_bytes()),
    )
}

/// 1 the catalog, 2 the tree, 3 a page with `page`.
fn one_page(pdf: &mut Pdf, page: &str) {
    pdf.add(1, "<< /Type /Catalog /Pages 2 0 R >>");
    pdf.add(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    pdf.add(
        3,
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] {page} >>"),
    );
}

/// A page of `content` alone.
fn content_only(content: &[u8]) -> Vec<u8> {
    let mut pdf = Pdf::default();
    one_page(&mut pdf, "/Contents 4 0 R");
    pdf.add(4, stream("", content));
    pdf.table()
}

/// A page with the font `/F` (object 5) showing `content`.
fn font_page(pdf: &mut Pdf, content: &[u8], font: impl Into<Vec<u8>>) {
    one_page(pdf, "/Contents 4 0 R /Resources << /Font << /F 5 0 R >> >>");
    pdf.add(4, stream("", content));
    pdf.add(5, font);
}

/// Objects `first`, `first + gap`, .. each the reference to the next, `hops`
/// references in all, the last leading to `end`. Returns the first.
fn chain(pdf: &mut Pdf, first: u32, gap: u32, hops: u32, end: impl Into<Vec<u8>>) -> u32 {
    let at = |i: u32| first + i * gap;
    for i in 0..hops.saturating_sub(1) {
        pdf.add(at(i), format!("{} 0 R", at(i + 1)));
    }
    pdf.add(at(hops.saturating_sub(1)), end);
    first
}

/// `/X Do` where X is no form: the XObject dictionary, the XObject and its
/// `Subtype` are each `hops` references away. With `slots`, the
/// cross-reference lists that many objects, all compressed in a stream that
/// isn't there.
fn draws_of_no_form(len: usize, hops: u32, slots: u32) -> Vec<u8> {
    let gap = if slots > 0 { slots / (3 * hops + 8) } else { 1 };
    let mut pdf = Pdf::default();
    one_page(
        &mut pdf,
        "/Contents 4 0 R /Resources << /XObject 100 0 R >>",
    );
    pdf.add(4, stream("", &repeated(b"/X Do ", len)));
    let subtype = chain(&mut pdf, 100 + 2 * (hops + 2) * gap, gap, hops, "/Image");
    let image = stream(&format!("/Subtype {subtype} 0 R"), b"");
    let xobject = chain(&mut pdf, 100 + (hops + 2) * gap, gap, hops, image);
    chain(&mut pdf, 100, gap, hops, format!("<< /X {xobject} 0 R >>"));
    if slots == 0 {
        return pdf.table();
    }
    let compressed: Vec<(u32, u32, u32)> = (0..slots)
        .map(|i| 100 + i)
        .filter(|i| !pdf.0.contains_key(i))
        .map(|i| (i, 99_000_000, 0))
        .collect();
    pdf.with_xref_stream(&compressed)
}

/// A page showing `content`, with a font whose map reads `A` and whose encoding
/// has a name no one can read for code 1.
fn with_font(content: &[u8]) -> Vec<u8> {
    let mut pdf = Pdf::default();
    font_page(
        &mut pdf,
        content,
        "<< /Type /Font /Subtype /Type1 /BaseFont /X /ToUnicode 6 0 R \
         /Encoding << /Differences [1 /nosuchglyph] >> >>",
    );
    pdf.add(
        6,
        stream(
            "",
            b"1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <41> <0041> endbfchar",
        ),
    );
    pdf.table()
}

fn prefixed(prefix: &[u8], unit: &[u8], len: usize, suffix: &[u8]) -> Vec<u8> {
    let mut content = prefix.to_vec();
    content.extend(repeated(unit, len));
    content.extend_from_slice(suffix);
    content
}

/// Four letters, different for each `index`.
fn short_name(index: usize) -> [u8; 4] {
    let letters = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    [
        letters[index % 62],
        letters[(index / 62) % 62],
        letters[(index / 3_844) % 62],
        letters[(index / 238_328) % 62],
    ]
}

/// The largest map the caps let through, in a composite font: 64 codespace
/// ranges no code of the text falls in but the last, 131,000 lines.
fn with_the_largest_map(codes: usize) -> Vec<u8> {
    let mut map = String::from("64 begincodespacerange\n");
    for i in 0..63 {
        map.push_str(&format!("<{i:02X}000000> <{i:02X}000001>\n"));
    }
    map.push_str("<0000> <FFFF> endcodespacerange\n");
    for block in 0..1310 {
        map.push_str("100 beginbfchar\n");
        for i in 0..100 {
            let code = (block * 100 + i) % 65_536;
            map.push_str(&format!("<{code:04X}> <0041>\n"));
        }
        map.push_str("endbfchar\n");
    }
    let mut pdf = Pdf::default();
    font_page(
        &mut pdf,
        &prefixed(b"BT /F 1 Tf <", b"0041", codes * 4, b"> Tj ET"),
        "<< /Type /Font /Subtype /Type0 /Encoding /Identity-H /ToUnicode 6 0 R >>",
    );
    pdf.add(6, stream("", map.as_bytes()));
    pdf.table()
}

/// A shape of the walk over a page: its name and its file.
fn page_shapes() -> Vec<(&'static str, Vec<u8>)> {
    let operands = |item: &[u8]| {
        let mut unit = item.repeat(100);
        unit.extend_from_slice(b" n ");
        content_only(&repeated(&unit, MIB / 2))
    };
    let mut shapes: Vec<(&'static str, Vec<u8>)> = vec![
        ("`n `", content_only(&repeated(b"n ", MIB))),
        ("`q Q `", content_only(&repeated(b"q Q ", MIB))),
        (
            "`/X Do`, chains too long to follow",
            draws_of_no_form(MIB / 4, 127, 0),
        ),
        ("`/X Do`, chains of 8", draws_of_no_form(MIB / 4, 8, 0)),
        (
            "`/X Do`, chains of 8, 200,000 slots",
            draws_of_no_form(MIB / 4, 8, 200_000),
        ),
        (
            "`(A)'`, a glyph a line",
            with_font(&prefixed(b"BT /F 1 Tf 1000 TL ", b"(A)'", 400_000, b"")),
        ),
        (
            "a line of glyphs",
            with_font(&prefixed(b"BT /F 1 Tf (", b"A", 200_000, b") Tj")),
        ),
        (
            "codes that read as nothing",
            with_font(&prefixed(b"BT /F 1 Tf (", &[1], 400_000, b") Tj")),
        ),
        (
            "`()'` with a map",
            with_font(&prefixed(b"BT /F 1 Tf ", b"()'", MIB / 2, b"")),
        ),
        ("1-bit TIFF predictor, rows of a byte", {
            let mut pdf = Pdf::default();
            one_page(&mut pdf, "/Contents 4 0 R");
            pdf.add(
                4,
                stream(
                    "/Filter /FlateDecode /DecodeParms << /Predictor 2 /Colors 1 \
                     /Columns 8 /BitsPerComponent 1 >>",
                    &deflate(&vec![0u8; MIB]),
                ),
            );
            pdf.table()
        }),
        ("reals", operands(b".1 ")),
        ("names", operands(b"/a")),
        ("dictionaries", operands(b"<</a 0>>")),
        ("empty arrays", operands(b"[]")),
        ("empty strings", operands(b"()")),
        (
            "inline images",
            content_only(&repeated(b"BI /W 1 ID x EI ", MIB / 2)),
        ),
        (
            "long font names",
            content_only(&repeated(
                format!("/{} 1 Tf ", "a".repeat(127)).as_bytes(),
                MIB / 2,
            )),
        ),
        (
            "properties that lead nowhere",
            content_only(&repeated(b"/P /Q BDC EMC ", MIB / 2)),
        ),
        ("the largest map", with_the_largest_map(100_000)),
    ];

    // A distinct font name for each show, in resources that have none.
    let mut content = b"BT ".to_vec();
    for index in 0..30_000 {
        content.push(b'/');
        content.extend_from_slice(&short_name(index));
        content.extend_from_slice(b" Tf()'");
    }
    let mut pdf = Pdf::default();
    font_page(&mut pdf, &content, "<< /Type /Font >>");
    shapes.push(("a font name a show, no such font", pdf.table()));

    // Many fonts, each read for the first time by one show.
    let mut pdf = Pdf::default();
    let mut names = String::new();
    let mut content = String::from("BT ");
    for font in 0..5_000u32 {
        names.push_str(&format!("/F{font} {} 0 R ", 10 + font));
        content.push_str(&format!("/F{font} 1 Tf()'\n"));
        pdf.add(10 + font, "<< /Type /Font /Subtype /Type1 /BaseFont /X >>");
    }
    one_page(
        &mut pdf,
        &format!("/Contents 4 0 R /Resources << /Font << {names}>> >>"),
    );
    pdf.add(4, stream("", content.as_bytes()));
    shapes.push(("a font a show", pdf.table()));

    // Differences of glyph names at the longest, in parts each looked up.
    let name = format!("/a{}", "_a".repeat(62));
    let mut pdf = Pdf::default();
    font_page(
        &mut pdf,
        b"BT /F 1 Tf ()Tj",
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding << /Differences [{}] >> >>",
            format!("0 {name} ").repeat(20_000)
        ),
    );
    shapes.push(("Differences of the longest names", pdf.table()));

    // Differences whose items are 8 references from a name.
    let mut pdf = Pdf::default();
    let first = chain(&mut pdf, 100, 1, 8, "/A");
    font_page(
        &mut pdf,
        b"BT /F 1 Tf ()Tj",
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding << /Differences [{}] >> >>",
            format!("{first} 0 R ").repeat(100_000)
        ),
    );
    shapes.push(("Differences behind references", pdf.table()));

    // Widths of a composite font behind the same.
    let mut pdf = Pdf::default();
    let first = chain(&mut pdf, 100, 1, 8, "500");
    font_page(
        &mut pdf,
        b"BT /F 1 Tf ()Tj",
        "<< /Type /Font /Subtype /Type0 /Encoding /Identity-H /DescendantFonts [6 0 R] >>",
    );
    pdf.add(
        6,
        format!("<< /W [0 [{}]] >>", format!("{first} 0 R ").repeat(60_000)),
    );
    shapes.push(("widths behind references", pdf.table()));

    // One small Flate stream, many times.
    let mut pdf = Pdf::default();
    one_page(
        &mut pdf,
        &format!("/Contents [{}]", "4 0 R ".repeat(20_000)),
    );
    pdf.add(4, stream("/Filter /FlateDecode", &deflate(b"n")));
    shapes.push(("a Contents array of one tiny Flate stream", pdf.table()));

    let mut pdf = Pdf::default();
    one_page(
        &mut pdf,
        "/Contents 4 0 R /Resources << /XObject << /X 5 0 R >> >>",
    );
    pdf.add(4, stream("", &repeated(b"/X Do ", 120_000)));
    pdf.add(
        5,
        stream("/Subtype /Form /Filter /FlateDecode", &deflate(b"n")),
    );
    shapes.push(("a tiny Flate form, drawn", pdf.table()));

    // A page the tree lists many times, empty.
    let mut pdf = Pdf::default();
    pdf.add(1, "<< /Type /Catalog /Pages 2 0 R >>");
    pdf.add(
        2,
        format!(
            "<< /Type /Pages /Kids [{}] /Count 1 >>",
            "3 0 R ".repeat(60_000)
        ),
    );
    pdf.add(3, "<< /Type /Page /Parent 2 0 R >>");
    for number in 4..60_004 {
        pdf.add(number, "0");
    }
    shapes.push(("empty pages the tree repeats", pdf.table()));

    // Marks that all sit at one place, on letters.
    let mut pdf = Pdf::default();
    font_page(
        &mut pdf,
        &prefixed(b"BT /F 12 Tf (", b"ab", 14_000, b") Tj ET"),
        "<< /Type /Font /Subtype /TrueType /BaseFont /X /Encoding /WinAnsiEncoding \
         /FirstChar 97 /Widths [1000 0] >>",
    );
    shapes.push(("marks on the letters of a line", pdf.table()));

    // The actual text of marked content, read for each.
    let mut pdf = Pdf::default();
    font_page(
        &mut pdf,
        &prefixed(
            b"BT /F 1 Tf ",
            b"/S << /ActualText (x) >> BDC (A) Tj EMC ",
            MIB / 2,
            b"",
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /X >>",
    );
    shapes.push(("actual text", pdf.table()));

    // One glyph many times at one place: the sweep over duplicates.
    shapes.push((
        "glyphs drawn over each other",
        with_font(&prefixed(
            b"BT /F 1 Tf -500 Tc (",
            b"A",
            200_000,
            b") Tj ET",
        )),
    ));
    shapes
}

/// How long `run` takes, the best of `tries`.
fn best(tries: usize, mut run: impl FnMut() -> Duration) -> Duration {
    (0..tries).map(|_| run()).min().unwrap()
}

/// The text of a document whose objects are read already, so that what is
/// timed is the walk: how long it took, and what it charged.
fn walk(document: &Document) -> (Duration, usize) {
    let start = Instant::now();
    let (texts, charged) = text::page_texts_counted(document, TextLimits::default()).unwrap();
    let elapsed = start.elapsed();
    std::hint::black_box(texts);
    (elapsed, charged)
}

/// How long a byte of the densest plain content takes, in nanoseconds.
fn unit() -> f64 {
    let content = repeated(b"n ", 2 * MIB);
    let len = content.len();
    let document = Document::load(content_only(&content), ReadLimits::default()).unwrap();
    walk(&document);
    best(3, || walk(&document).0).as_nanos() as f64 / len as f64
}

/// How many units a byte charged takes for `run`, which gives how long it took
/// for `charged`: a busy machine gets more tries, and the unit is measured
/// again.
fn units(name: &str, charged: usize, base: &mut f64, mut run: impl FnMut() -> Duration) -> f64 {
    let mut multiple = f64::MAX;
    let mut rate = 0.0;
    for _ in 0..4 {
        rate = best(2, &mut run).as_nanos() as f64 / charged as f64;
        multiple = multiple.min(rate / *base);
        if multiple <= MOST {
            break;
        }
        *base = base.min(unit());
    }
    println!("{name}: charged {charged}, {rate:.1} ns a byte, {multiple:.2} units");
    multiple
}

#[test]
fn a_byte_charged_takes_about_the_same_on_every_shape_of_page() {
    let mut base = unit();
    println!("unit: {base:.1} ns a byte of the densest content");
    let mut over = Vec::new();
    let mut slowest = (String::new(), 0.0f64);
    for (name, file) in page_shapes() {
        let document = Document::load(file, ReadLimits::default()).unwrap();
        let (_, charged) = walk(&document);
        document.check().unwrap();
        let multiple = units(name, charged, &mut base, || walk(&document).0);
        if multiple > slowest.1 {
            slowest = (name.to_owned(), multiple);
        }
        if multiple > MOST {
            over.push(format!("{name}: {multiple:.2} units"));
        }
    }
    println!("slowest: {} at {:.2} units", slowest.0, slowest.1);
    assert!(over.is_empty(), "over {MOST} units: {over:?}");
}

/// Reading: the shapes that look at little and do much with it.
fn file_shapes() -> Vec<(&'static str, Vec<u8>)> {
    let mut shapes: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let items: [(&'static str, &[u8], usize); 8] = [
        ("an array of integers", b"0 ", 200_000),
        ("an array of reals", b".1 ", 150_000),
        ("an array of names", b"/a", 200_000),
        ("an array of strings", b"()", 200_000),
        ("an array of arrays", b"[]", 200_000),
        ("an array of dictionaries", b"<<>>", 100_000),
        ("an array of small dictionaries", b"<</a 0>>", 50_000),
        ("an array of references", b"3 0 R ", 80_000),
    ];
    for (name, item, count) in items {
        let mut pdf = Pdf::default();
        one_page(&mut pdf, "");
        pdf.add(4, [b"[", &item.repeat(count)[..], b"]"].concat());
        shapes.push((name, pdf.table()));
    }

    // Many tiny objects, through a table.
    let mut pdf = Pdf::default();
    one_page(&mut pdf, "");
    for number in 4..60_000 {
        pdf.add(number, "0");
    }
    shapes.push(("tiny objects", pdf.table()));

    // A table with the shortest entries there can be.
    let mut file = b"%PDF-1.7\n1 0 obj\n<</Type/Catalog>>\nendobj\n".to_vec();
    let xref = file.len();
    file.extend_from_slice(b"xref\n0 2\n0000000000 65535 f \n0000000009 00000 n \n");
    file.extend_from_slice(b"10 100000\n");
    file.extend(b"0 0 n ".repeat(100_000));
    file.extend(format!("trailer\n<</Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").bytes());
    shapes.push(("a table of the shortest entries", file));

    // A cross-reference stream of the shortest rows.
    let mut file = b"%PDF-1.7\n1 0 obj\n<</Type/Catalog>>\nendobj\n".to_vec();
    let xref = file.len();
    file.extend_from_slice(b"2 0 obj\n");
    file.extend_from_slice(&stream(
        "/Type /XRef /Size 200010 /W [1 0 0] /Index [10 200000] /Root 1 0 R /Filter /FlateDecode",
        &deflate(&vec![2u8; 200_000]),
    ));
    file.extend_from_slice(b"\nendobj\n");
    file.extend(format!("startxref\n{xref}\n%%EOF\n").bytes());
    shapes.push(("a cross-reference stream of the shortest rows", file));

    // An object stream listing many objects at one place.
    let count = 60_000u32;
    let mut pdf = Pdf::default();
    one_page(&mut pdf, "");
    let members: Vec<(u32, usize)> = (0..count).map(|i| (100 + i, 0)).collect();
    pdf.add(90, object_stream(&members, b"0"));
    let compressed: Vec<(u32, u32, u32)> = (0..count).map(|i| (100 + i, 90, i)).collect();
    shapes.push((
        "an object stream of objects at one place",
        pdf.with_xref_stream(&compressed),
    ));

    // A file to scan for its objects.
    let mut file = b"%PDF-1.7\n".to_vec();
    for number in 2..40_000 {
        file.extend(format!("{number} 0 obj\n0\nendobj\n").bytes());
    }
    file.extend_from_slice(b"1 0 obj\n<</Type/Catalog>>\nendobj\ntrailer\n<</Root 1 0 R>>\n");
    shapes.push(("a file scanned for its objects", file));

    // Strings of a file encrypted without a password, each with a key to make.
    shapes.push(("strings of an encrypted file", encrypted(b"()", 100_000)));
    shapes.push(("streams of an encrypted file", encrypted_streams(20_000)));
    shapes
}

/// The `Encrypt` dictionary of the RC4 fixture, and the trailer's `ID`.
fn encryption() -> (Vec<u8>, String) {
    let fixture = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/lines.rc4-128.pdf"
    ))
    .unwrap();
    let find = |needle: &[u8], from: usize| {
        fixture[from..]
            .windows(needle.len())
            .position(|window| window == needle)
            .unwrap()
            + from
    };
    let start = find(b"\n9 0 obj", 0) + 8;
    let encrypt = fixture[start..find(b"endobj", start)].to_vec();
    let id = find(b"/ID [", 0);
    let id = String::from_utf8_lossy(&fixture[id..=find(b"]", id)]).into_owned();
    (encrypt, id)
}

/// A file encrypted as that fixture is, with an array of `count` of `item`.
fn encrypted(item: &[u8], count: usize) -> Vec<u8> {
    let (encrypt, id) = encryption();
    let mut pdf = Pdf::default();
    one_page(&mut pdf, "");
    pdf.add(4, [b"[", &item.repeat(count)[..], b"]"].concat());
    pdf.add(9, encrypt);
    pdf.table_with(&format!("{id} /Encrypt 9 0 R"))
}

/// The same with `count` streams of no bytes, each with a key to make.
fn encrypted_streams(count: u32) -> Vec<u8> {
    let (encrypt, id) = encryption();
    let mut pdf = Pdf::default();
    one_page(&mut pdf, "");
    pdf.add(9, encrypt);
    for number in 10..10 + count {
        pdf.add(number, "<</Length 0>>stream\n\nendstream");
    }
    pdf.table_with(&format!("{id} /Encrypt 9 0 R"))
}

#[test]
fn a_byte_charged_takes_about_the_same_on_every_shape_of_file() {
    let mut base = unit();
    let mut over = Vec::new();
    for (name, file) in file_shapes() {
        let read = || {
            let start = Instant::now();
            let document = Document::load(file.clone(), ReadLimits::default()).unwrap();
            std::hint::black_box(document.objects().count());
            let elapsed = start.elapsed();
            document.check().unwrap();
            (elapsed, document.work_used())
        };
        let (_, charged) = read();
        let multiple = units(name, charged, &mut base, || read().0);
        if multiple > MOST {
            over.push(format!("{name}: {multiple:.2} units"));
        }
    }
    assert!(over.is_empty(), "over {MOST} units: {over:?}");
}

/// The slowest shape found, pages that each show a line of a million glyphs,
/// filling the default 1 GiB of work (with no limit on the document's text,
/// which would stop it a little short). It takes tens of seconds: run it in a
/// release build, with `--ignored --nocapture`, for the figure the
/// documentation gives.
#[test]
#[ignore = "the figure of the documentation, in a release build"]
fn the_whole_budget_of_the_slowest_shape() {
    let content = prefixed(b"BT /F 1 Tf (", b"A", 999_990, b") Tj ET");
    let kids: String = (0..80).map(|page| format!("{} 0 R ", 100 + page)).collect();
    let mut pdf = Pdf::default();
    pdf.add(1, "<< /Type /Catalog /Pages 2 0 R >>");
    pdf.add(
        2,
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count 80 /Resources << /Font << /F 5 0 R >> >> >>"
        ),
    );
    pdf.add(4, stream("/Filter /FlateDecode", &deflate(&content)));
    pdf.add(5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    for page in 0..80 {
        pdf.add(
            100 + page,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        );
    }
    let document = Document::load(pdf.table(), ReadLimits::default()).unwrap();
    let limits = TextLimits {
        document_text: usize::MAX,
        ..TextLimits::default()
    };
    let start = Instant::now();
    let (_, charged) = text::page_texts_counted(&document, limits).unwrap();
    println!(
        "{charged} bytes of work in {:.1} s",
        start.elapsed().as_secs_f64()
    );
    assert!(charged > 1000 * MIB, "{charged}");
}

// What each thing costs, whatever the clock says.

fn charged(file: Vec<u8>) -> usize {
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let (_, charged) = text::page_texts_counted(&document, TextLimits::default()).unwrap();
    charged
}

fn too_large<T: std::fmt::Debug>(result: Result<T, PdfError>) -> String {
    match result {
        Err(PdfError::TooLarge(message)) => message,
        other => panic!("expected TooLarge, got {other:?}"),
    }
}

fn work(page: usize) -> TextLimits {
    TextLimits {
        page_work: page,
        document_work: page,
        ..TextLimits::default()
    }
}

#[test]
fn a_reference_costs_work_each_time_it_is_followed() {
    let mut pdf = Pdf::default();
    one_page(&mut pdf, "");
    let first = chain(&mut pdf, 100, 1, 8, "/End");
    let document = Document::load(pdf.table(), ReadLimits::default()).unwrap();
    let object = Object::Reference((first, 0));
    assert_eq!(
        document.resolve(&object),
        Some(&Object::Name(b"End".to_vec()))
    );
    // Read already: looking again costs all the same.
    let before = document.work_used();
    document.resolve(&object).unwrap();
    let one = document.work_used() - before;
    assert!(one >= 8 * 8, "{one} for 8 references");
    document.resolve(&object).unwrap();
    assert_eq!(document.work_used() - before, 2 * one);
}

#[test]
fn a_chain_of_eight_references_is_followed_and_one_of_nine_is_not() {
    for (hops, found) in [(8, true), (9, false)] {
        let mut pdf = Pdf::default();
        one_page(&mut pdf, "");
        let first = chain(&mut pdf, 100, 1, hops, "/End");
        let document = Document::load(pdf.table(), ReadLimits::default()).unwrap();
        let object = Object::Reference((first, 0));
        assert_eq!(document.resolve(&object).is_some(), found, "{hops}");
        // A chain that is too long is nothing, not a refusal.
        document.check().unwrap();
    }
}

#[test]
fn references_stop_when_the_document_runs_out_of_work() {
    let mut pdf = Pdf::default();
    one_page(&mut pdf, "");
    let first = chain(&mut pdf, 100, 1, 8, "/End");
    let limits = ReadLimits {
        work: 64 * 1024,
        ..ReadLimits::default()
    };
    let document = Document::load(pdf.table(), limits).unwrap();
    let object = Object::Reference((first, 0));
    for _ in 0..1_000 {
        let _ = document.resolve(&object);
    }
    too_large(document.check());
    assert_eq!(document.resolve(&object), None);
}

#[test]
fn a_page_the_tree_repeats_costs_what_its_kind_is_read_through() {
    // Its `Type` is 8 references away: each of the kids follows them.
    let kids = 2_000;
    let mut pdf = Pdf::default();
    pdf.add(1, "<< /Type /Catalog /Pages 2 0 R >>");
    pdf.add(
        2,
        format!(
            "<< /Type /Pages /Kids [{}] /Count 1 >>",
            "3 0 R ".repeat(kids)
        ),
    );
    pdf.add(3, "<< /Type 100 0 R /Parent 2 0 R >>");
    chain(&mut pdf, 100, 1, 8, "/Page");
    for number in 200..200 + kids as u32 {
        pdf.add(number, "0");
    }
    let document = Document::load(pdf.table(), ReadLimits::default()).unwrap();
    let before = document.work_used();
    let pages = document.pages();
    assert_eq!(pages.len(), kids);
    let spent = document.work_used() - before;
    assert!(spent >= kids * 8 * 8, "{spent} for {kids} kids");
}

#[test]
fn an_operator_costs_more_than_its_bytes() {
    let content = repeated(b"n ", 256 * 1024);
    let plain = charged(content_only(&content));
    assert!(plain > 2 * content.len(), "{plain} for {}", content.len());
}

#[test]
fn a_draw_of_what_is_not_a_form_costs_the_page() {
    // 100,000 draws in 600 KB of content.
    let file = draws_of_no_form(600_000, 8, 0);
    let document = Document::load(file, ReadLimits::default()).unwrap();
    too_large(text::page_texts_within(&document, work(4 * MIB)));
}

#[test]
fn what_a_chain_costs_comes_out_of_the_page() {
    let long = charged(draws_of_no_form(60_000, 8, 0));
    let short = charged(draws_of_no_form(60_000, 1, 0));
    assert!(long > short + 10_000 * 7 * 8, "{long} against {short}");
}

#[test]
fn codes_that_read_as_nothing_count_as_glyphs() {
    let content = prefixed(b"BT /F 1 Tf (", &[1], 2_000, b") Tj");
    let document = Document::load(with_font(&content), ReadLimits::default()).unwrap();
    let limits = TextLimits {
        page_glyphs: 1_000,
        ..TextLimits::default()
    };
    let message = too_large(text::page_texts_within(&document, limits));
    assert!(message.contains("1000 glyphs"), "{message}");
    // A code that reads as nothing is charged as one that reads as a letter.
    let nothing = charged(with_font(&prefixed(b"BT /F 1 Tf (", &[1], 10_000, b") Tj")));
    assert!(nothing > 8 * 10_000, "{nothing}");
}

/// A page showing code 1, which `Differences` calls `name`.
fn named_code(name: &str) -> Vec<u8> {
    let mut pdf = Pdf::default();
    font_page(
        &mut pdf,
        b"BT /F 12 Tf (\x01) Tj ET",
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding \
             << /Differences [1 /{name}] >> >>"
        ),
    );
    pdf.table()
}

fn text_of(file: Vec<u8>) -> String {
    let document = Document::load(file, ReadLimits::default()).unwrap();
    text::page_texts(&document).unwrap().join("")
}

#[test]
fn a_glyph_name_over_127_bytes_is_unreadable() {
    // `uni`, then 31 groups: 127 bytes.
    assert_eq!(
        text_of(named_code(&format!("uni{}", "0041".repeat(31)))),
        "A".repeat(31)
    );
    assert_eq!(
        text_of(named_code(&format!("A{}", "_A".repeat(63)))),
        "A".repeat(64)
    );
    // 128 bytes and more.
    assert_eq!(
        text_of(named_code(&format!("A{}", "_A".repeat(63) + "x"))),
        ""
    );
    assert_eq!(
        text_of(named_code(&format!("uni{}", "0041".repeat(32)))),
        ""
    );
}

#[test]
fn the_text_of_a_code_in_a_map_is_capped() {
    let read = |units: usize| {
        let text = "0041".repeat(units);
        let mut pdf = Pdf::default();
        font_page(
            &mut pdf,
            b"BT /F 12 Tf (A) Tj ET",
            "<< /Type /Font /Subtype /Type1 /BaseFont /X /ToUnicode 6 0 R >>",
        );
        let map = format!("1 beginbfchar <41> <{text}> endbfchar");
        pdf.add(6, stream("", map.as_bytes()));
        text_of(pdf.table())
    };
    assert_eq!(read(256), "A".repeat(256));
    assert_eq!(read(257), "");
}

#[test]
fn the_names_of_differences_are_charged_for_their_bytes() {
    let name = format!("/a{}", "_a".repeat(62));
    let mut pdf = Pdf::default();
    font_page(
        &mut pdf,
        b"BT /F 1 Tf ()Tj",
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding << /Differences [{}] >> >>",
            format!("0 {name} ").repeat(1_000)
        ),
    );
    let document = Document::load(pdf.table(), ReadLimits::default()).unwrap();
    too_large(text::page_texts_within(&document, work(512 * 1024)));
}

#[test]
fn a_font_read_for_the_first_time_costs_what_reading_it_again_does() {
    let mut pdf = Pdf::default();
    let mut names = String::new();
    let mut content = String::from("BT ");
    for font in 0..1_000u32 {
        names.push_str(&format!("/F{font} {} 0 R ", 10 + font));
        content.push_str(&format!("/F{font} 1 Tf()'\n"));
        pdf.add(10 + font, "<< /Type /Font /Subtype /Type1 /BaseFont /X >>");
    }
    one_page(
        &mut pdf,
        &format!("/Contents 4 0 R /Resources << /Font << {names}>> >>"),
    );
    pdf.add(4, stream("", content.as_bytes()));
    let spent = charged(pdf.table());
    assert!(spent > 1_000 * 1024, "{spent}");
}

#[test]
fn each_step_of_a_filter_costs_its_setting_up() {
    let mut pdf = Pdf::default();
    one_page(&mut pdf, &format!("/Contents [{}]", "4 0 R ".repeat(1_000)));
    pdf.add(4, stream("/Filter /FlateDecode", &deflate(b"n")));
    let spent = charged(pdf.table());
    assert!(spent > 1_000 * 512, "{spent}");
}

/// `draws` draws of a form of `content`, stored or inflated: both must fit the
/// default limits.
fn draws_fit(content: &[u8], draws: usize) {
    let packed = deflate(content);
    for (dict, data) in [("", content), ("/Filter /FlateDecode", &packed[..])] {
        let mut pdf = Pdf::default();
        one_page(
            &mut pdf,
            "/Contents 4 0 R /Resources << /XObject << /X 5 0 R >> >>",
        );
        pdf.add(4, stream("", &repeated(b"/X Do ", 6 * draws)));
        pdf.add(
            5,
            stream(&format!("/Type /XObject /Subtype /Form {dict}"), data),
        );
        let document = Document::load(pdf.table(), ReadLimits::default()).unwrap();
        text::page_texts(&document).unwrap_or_else(|error| panic!("{dict}: {error}"));
    }
}

#[test]
fn a_form_of_8_kib_of_ordinary_content_fits_ten_thousand_draws() {
    draws_fit(&repeated(b"1 0 0 1 10 10 cm\n", 8 * 1024), 10_000);
}

#[test]
fn a_form_of_3_kib_of_the_densest_content_fits_ten_thousand_draws() {
    draws_fit(&repeated(b"n ", 3 * 1024), 10_000);
}

#[test]
fn saving_stops_at_the_work_an_edit_may_do() {
    // Each lookup an edit makes is charged too: one that runs out refuses to
    // save.
    let mut pdf = Pdf::default();
    one_page(&mut pdf, "");
    let first = chain(&mut pdf, 100, 1, 8, "/End");
    let limits = ReadLimits {
        work: 256 * 1024,
        ..ReadLimits::default()
    };
    let document = Document::load(pdf.table(), limits).unwrap();
    let objects = Objects::read(document).unwrap();
    let object = Object::Reference((first, 0));
    for _ in 0..10_000 {
        let _ = objects.resolve(&object);
    }
    too_large(objects.save());
}

/// The peak of a case in a process of its own, as in `bounds.rs`. Linux only:
/// the kernel keeps the figure.
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

    /// What a process holds past what is counted: its allocator's own.
    const SLACK: usize = 8 * MIB;
    /// What the fonts of these cases may be counted as.
    const FONT_MEMORY: usize = 4 * MIB;

    /// `fonts` fonts sharing an encoding whose `Differences` give each of the
    /// 256 codes one name of `units` groups of `uni0800`, by reference: three
    /// bytes of text for each four of the name.
    fn long_names(fonts: usize, units: usize) -> Vec<u8> {
        let mut name = b"/uni".to_vec();
        name.extend(std::iter::repeat_n(&b"0800"[..], units).flatten());
        let mut pdf = Pdf::default();
        let mut names = String::new();
        let mut content = String::new();
        for font in 0..fonts {
            names.push_str(&format!("/F{font} {} 0 R ", 10 + font));
            content.push_str(&format!("/F{font} 1 Tf ()Tj\n"));
            pdf.add(
                10 + font as u32,
                "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding 5 0 R >>",
            );
        }
        one_page(
            &mut pdf,
            &format!("/Contents 4 0 R /Resources << /Font << {names}>> >>"),
        );
        pdf.add(4, stream("", content.as_bytes()));
        let differences: String = (0..256).map(|code| format!("{code} 6 0 R ")).collect();
        pdf.add(
            5,
            format!("<< /Type /Encoding /Differences [{differences}] >>"),
        );
        pdf.add(6, name);
        pdf.table()
    }

    /// What the glyphs of a page hold, each, as the documentation adds it up.
    const GLYPH_MEMORY: usize = 160;

    /// The peak of reading `file`'s text past the documented sum: twice what
    /// its objects are counted as, the fonts' limit, twice the `content` and
    /// what its glyphs hold.
    fn past_the_sum(file: Vec<u8>, content: usize) {
        std::fs::write("/proc/self/clear_refs", "5").unwrap();
        let before = status("VmRSS:");
        let document = Document::load(file, ReadLimits::default()).unwrap();
        let limits = TextLimits {
            font_memory: FONT_MEMORY,
            ..TextLimits::default()
        };
        let texts = text::page_texts_within(&document, limits);
        let peak = status("VmHWM:").saturating_sub(before);
        let counted = document.memory_used();
        println!("{:?} held {peak} counted {counted}", texts.map(|t| t.len()));
        println!(
            "PEAK {}",
            peak.saturating_sub(2 * counted + FONT_MEMORY + 2 * content)
        );
    }

    #[test]
    fn child() {
        let Ok(case) = std::env::var("NAVREE_PDF_PEAK_CASE") else {
            return;
        };
        match case.as_str() {
            // 8 fonts of 256 texts of 3 x 16,384 bytes: 100 MB if each is built.
            "names" => past_the_sum(long_names(8, 16 * 1024), 0),
            // 200,000 glyphs: 32 MB, counted as the content.
            "a-line" => {
                let content = prefixed(b"BT /F 1 Tf (", b"A", 200_000, b") Tj ET");
                let allowed = content.len() + 200_000 * GLYPH_MEMORY / 2;
                past_the_sum(with_font(&content), allowed);
            }
            "a-glyph-a-line" => {
                let content = prefixed(b"BT /F 1 Tf 1000 TL ", b"(A)'", 800_000, b"");
                let allowed = content.len() + 200_000 * GLYPH_MEMORY / 2;
                past_the_sum(with_font(&content), allowed);
            }
            "font-names" => {
                let mut content = b"BT ".to_vec();
                for index in 0..360_000 {
                    content.push(b'/');
                    content.extend_from_slice(&short_name(index));
                    content.extend_from_slice(b" Tf()'");
                }
                let mut pdf = Pdf::default();
                one_page(&mut pdf, "/Contents 4 0 R /Resources << /Font << >> >>");
                pdf.add(4, stream("/Filter /FlateDecode", &deflate(&content)));
                past_the_sum(pdf.table(), content.len());
            }
            other => panic!("{other}"),
        }
    }

    #[test]
    fn glyph_names_hold_what_they_are_counted_as() {
        let past = in_child("names");
        assert!(past < SLACK, "{} MiB past the sum", past >> 20);
    }

    #[test]
    fn glyphs_hold_what_the_documentation_says() {
        for case in ["a-line", "a-glyph-a-line"] {
            let past = in_child(case);
            assert!(past < SLACK, "{case}: {} MiB past the sum", past >> 20);
        }
    }

    #[test]
    fn the_font_names_of_a_page_are_counted() {
        // 360,000 names: about 137 bytes each if all are kept, 49 MB.
        let past = in_child("font-names");
        assert!(past < SLACK, "{} MiB past the sum", past >> 20);
    }
}

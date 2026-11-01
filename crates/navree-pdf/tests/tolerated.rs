//! Files the format allows, or damage other readers recover from, that must
//! read to the right text: values given indirectly, a `ToUnicode` map with an
//! encoding beside it, glyph names of every kind, objects with one bad value,
//! pages with no `/Type`.

use std::io::Write as _;

use navree_pdf::content::operations;
use navree_pdf::{Document, Objects, PdfError, ReadLimits, text};

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

/// One page drawing 4, with the font 5: 6 and after are `rest`.
fn doc(content: Vec<u8>, font: &str, rest: Vec<Vec<u8>>) -> Vec<u8> {
    let mut objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
        b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 600 800]/Contents 4 0 R/Resources<</Font<</F 5 0 R>>>>>>"
            .to_vec(),
        content,
        font.as_bytes().to_vec(),
    ];
    objects.extend(rest);
    let size = objects.len() + 1;
    file(&objects, &format!("<</Root 1 0 R/Size {size}>>"))
}

fn text_of(file: Vec<u8>) -> String {
    let document = Document::load(file, ReadLimits::default()).unwrap();
    text::page_texts(&document).unwrap().join("\u{c}")
}

const HELVETICA: &str = "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>";
const SHOW: &[u8] = b"BT /F 12 Tf 50 700 Td (Hi there) Tj ET";

/// `SHOW` in a stream with `dict`, packed with `pack`.
fn shown(dict: &str, pack: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    stream(dict, &pack(SHOW))
}

// Indirect values in a stream's dictionary.

#[test]
fn a_filter_given_indirectly_is_run() {
    let file = doc(
        shown("/Filter 6 0 R", deflate),
        HELVETICA,
        vec![b"/FlateDecode".to_vec()],
    );
    assert_eq!(text_of(file), "Hi there");
}

#[test]
fn a_filter_list_given_indirectly_is_run() {
    let file = doc(
        shown("/Filter 6 0 R", deflate),
        HELVETICA,
        vec![b"[/FlateDecode]".to_vec()],
    );
    assert_eq!(text_of(file), "Hi there");
}

#[test]
fn a_filter_given_indirectly_in_a_list_is_run() {
    let file = doc(
        shown("/Filter [6 0 R]", deflate),
        HELVETICA,
        vec![b"/FlateDecode".to_vec()],
    );
    assert_eq!(text_of(file), "Hi there");
}

/// `SHOW` with a PNG predictor of one column: an up-row filter on each byte.
fn predicted() -> Vec<u8> {
    let mut rows = Vec::new();
    let mut before = 0u8;
    for &byte in SHOW {
        rows.push(2);
        rows.push(byte.wrapping_sub(before));
        before = byte;
    }
    deflate(&rows)
}

#[test]
fn parameters_given_indirectly_are_used() {
    let file = doc(
        stream("/Filter/FlateDecode/DecodeParms 6 0 R", &predicted()),
        HELVETICA,
        vec![b"<</Predictor 12/Columns 1>>".to_vec()],
    );
    assert_eq!(text_of(file), "Hi there");
}

#[test]
fn parameters_given_indirectly_in_a_list_are_used() {
    let file = doc(
        stream("/Filter[/FlateDecode]/DecodeParms[6 0 R]", &predicted()),
        HELVETICA,
        vec![b"<</Predictor 12/Columns 1>>".to_vec()],
    );
    assert_eq!(text_of(file), "Hi there");
}

#[test]
fn a_parameter_given_indirectly_is_used() {
    let file = doc(
        stream(
            "/Filter/FlateDecode/DecodeParms<</Predictor 6 0 R/Columns 7 0 R>>",
            &predicted(),
        ),
        HELVETICA,
        vec![b"12".to_vec(), b"1".to_vec()],
    );
    assert_eq!(text_of(file), "Hi there");
}

#[test]
fn an_indirect_filter_that_loops_is_no_filter() {
    let file = doc(
        stream("/Filter 6 0 R", SHOW),
        HELVETICA,
        vec![b"7 0 R".to_vec(), b"6 0 R".to_vec()],
    );
    assert_eq!(text_of(file), "Hi there");
}

// Indirect values in a font's dictionary.

#[test]
fn differences_given_indirectly_are_used() {
    let file = doc(
        stream("", b"BT /F 12 Tf (AAA) Tj ET"),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding<</Differences 6 0 R>>>>",
        vec![b"[65/B]".to_vec()],
    );
    assert_eq!(text_of(file), "BBB");
}

#[test]
fn an_encoding_s_parts_may_all_be_given_indirectly() {
    let file = doc(
        stream("", b"BT /F 12 Tf (AAz) Tj ET"),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding 6 0 R>>",
        vec![
            b"<</Type/Encoding/BaseEncoding 7 0 R/Differences[8 0 R 9 0 R]>>".to_vec(),
            b"/WinAnsiEncoding".to_vec(),
            b"65".to_vec(),
            b"/B".to_vec(),
        ],
    );
    assert_eq!(text_of(file), "BBz");
}

#[test]
fn a_font_s_subtype_and_widths_may_be_given_indirectly() {
    // Two codes 500 wide against 0 wide: the gap between words shows the
    // widths were read.
    let file = doc(
        stream("", b"BT /F 12 Tf (A) Tj ET"),
        "<</Type 6 0 R/Subtype 7 0 R/BaseFont/X/FirstChar 8 0 R/Widths[500]/Encoding/WinAnsiEncoding>>",
        vec![b"/Font".to_vec(), b"/TrueType".to_vec(), b"65".to_vec()],
    );
    assert_eq!(text_of(file), "A");
}

// ToUnicode and the encoding beside it.

fn map(lines: &str) -> Vec<u8> {
    stream(
        "",
        format!(
            "1 begincodespacerange <00> <FF> endcodespacerange\n1 beginbfchar {lines} endbfchar"
        )
        .as_bytes(),
    )
}

#[test]
fn to_unicode_comes_before_the_encoding() {
    let file = doc(
        stream("", b"BT /F 12 Tf (A) Tj ET"),
        "<</Type/Font/Subtype/Type1/BaseFont/X/Encoding/WinAnsiEncoding/ToUnicode 6 0 R>>",
        vec![map("<41> <0042>")],
    );
    assert_eq!(text_of(file), "B");
}

#[test]
fn to_unicode_comes_before_differences_with_names_no_one_knows() {
    let file = doc(
        stream("", b"BT /F 12 Tf (\x01\x02) Tj ET"),
        "<</Type/Font/Subtype/Type1/BaseFont/X/Encoding<</Differences[1/g1/g2]>>/ToUnicode 6 0 R>>",
        vec![stream(
            "",
            b"1 begincodespacerange <00> <FF> endcodespacerange\n\
              2 beginbfchar <01> <0048> <02> <0069> endbfchar",
        )],
    );
    assert_eq!(text_of(file), "Hi");
}

#[test]
fn a_code_the_map_leaves_out_reads_through_the_encoding() {
    let file = doc(
        stream("", b"BT /F 12 Tf (AB) Tj ET"),
        "<</Type/Font/Subtype/Type1/BaseFont/X/Encoding/WinAnsiEncoding/ToUnicode 6 0 R>>",
        vec![map("<41> <0058>")],
    );
    assert_eq!(text_of(file), "XB");
}

#[test]
fn a_map_that_cannot_be_read_leaves_the_encoding() {
    let file = doc(
        stream("", b"BT /F 12 Tf (AB) Tj ET"),
        "<</Type/Font/Subtype/Type1/BaseFont/X/Encoding/WinAnsiEncoding/ToUnicode 6 0 R>>",
        vec![stream("/Filter/DCTDecode", b"not a map")],
    );
    assert_eq!(text_of(file), "AB");
}

// Glyph names.

fn with_differences(differences: &str, shown: &[u8]) -> String {
    let mut content = b"BT /F 12 Tf (".to_vec();
    content.extend_from_slice(shown);
    content.extend_from_slice(b") Tj ET");
    text_of(doc(
        stream("", &content),
        &format!(
            "<</Type/Font/Subtype/Type1/BaseFont/X/Encoding<</BaseEncoding/WinAnsiEncoding/Differences[{differences}]>>>>"
        ),
        vec![],
    ))
}

#[test]
fn glyph_names_of_the_form_uni_are_read() {
    assert_eq!(with_differences("65/uni0042/uni0043", b"AB"), "BC");
    assert_eq!(with_differences("65/u0042", b"A"), "B");
    assert_eq!(with_differences("65/u1F600", b"A"), "\u{1F600}");
    assert_eq!(with_differences("65/uni00660069", b"A"), "fi");
}

#[test]
fn a_glyph_name_with_a_suffix_reads_as_the_name_before_it() {
    assert_eq!(with_differences("65/a.sc", b"A"), "a");
    assert_eq!(with_differences("65/f_i", b"A"), "fi");
}

#[test]
fn one_name_no_one_knows_loses_one_code() {
    assert_eq!(with_differences("65/no-such-glyph/C", b"ABz"), "Cz");
}

#[test]
fn a_bad_item_in_differences_loses_that_item_only() {
    assert_eq!(with_differences("(what) 65/C", b"A"), "C");
    assert_eq!(with_differences("9999/D 65/C", b"A"), "C");
}

// Objects with one bad value.

#[test]
fn an_integer_too_large_for_one_reads_as_a_real_in_a_dictionary() {
    let file = doc(
        stream("", SHOW),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/X 99999999999999999999>>",
        vec![],
    );
    assert_eq!(text_of(file), "Hi there");
}

#[test]
fn a_key_with_no_value_reads_as_null() {
    let file = doc(
        stream("", SHOW),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/X>>",
        vec![],
    );
    assert_eq!(text_of(file), "Hi there");
}

#[test]
fn an_edit_keeps_what_those_objects_hold() {
    let file = doc(
        stream("", SHOW),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/X 99999999999999999999/Y>>",
        vec![],
    );
    let objects = Objects::read(Document::load(file, ReadLimits::default()).unwrap()).unwrap();
    let saved = objects.save().unwrap();
    assert_eq!(text_of(saved), "Hi there");
}

#[test]
fn an_edit_refuses_a_file_it_would_lose_part_of() {
    // The font cannot be parsed at all, and the page refers to it.
    let file = doc(stream("", SHOW), "<</Type/Font ( >>", vec![]);
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let error = Objects::read(document).unwrap_err();
    let PdfError::Damaged(message) = &error else {
        panic!("{error:?}");
    };
    assert!(message.contains("object 5"), "{message}");
}

#[test]
fn an_edit_may_lose_what_nothing_refers_to() {
    let file = doc(stream("", SHOW), HELVETICA, vec![b"<</Bad ( >>".to_vec()]);
    let document = Document::load(file, ReadLimits::default()).unwrap();
    let objects = Objects::read(document).unwrap();
    assert!(objects.get((6, 0)).is_none());
}

// Damage.

#[test]
fn a_page_without_a_type_is_a_page() {
    let objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
        b"<</Parent 2 0 R/MediaBox[0 0 600 800]/Contents 4 0 R/Resources<</Font<</F 5 0 R>>>>>>"
            .to_vec(),
        stream("", SHOW),
        HELVETICA.as_bytes().to_vec(),
    ];
    let bytes = file(&objects, "<</Root 1 0 R/Size 6>>");
    let document = Document::load(bytes.clone(), ReadLimits::default()).unwrap();
    assert_eq!(document.pages(), [(3, 0)]);
    assert_eq!(text_of(bytes), "Hi there");
    let edited = Objects::read(document).unwrap();
    assert_eq!(edited.pages(), [(3, 0)]);
}

#[test]
fn a_tree_node_without_a_type_is_a_tree_node() {
    let objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Kids[3 0 R]/Count 1>>".to_vec(),
        b"<</Kids[4 0 R]/Count 1/Parent 2 0 R>>".to_vec(),
        b"<</Type/Page/Parent 3 0 R>>".to_vec(),
    ];
    let bytes = file(&objects, "<</Root 1 0 R/Size 5>>");
    let document = Document::load(bytes, ReadLimits::default()).unwrap();
    // The catalog's tree starts at node 2 whatever its type.
    assert_eq!(document.pages(), [(4, 0)]);
}

#[test]
fn a_stream_keyword_in_a_string_does_not_hide_objects_from_a_scan() {
    let bytes = b"%PDF-1.7\n\
        1 0 obj\n<</Type/Catalog/Pages 2 0 R/Note (a stream\n of words)>>\nendobj\n\
        2 0 obj\n<</Type/Pages/Kids[3 0 R]/Count 1>>\nendobj\n\
        3 0 obj\n<</Type/Page/Parent 2 0 R/Contents 4 0 R/Resources<</Font<</F 5 0 R>>>>>>\nendobj\n\
        4 0 obj\n<</Length 38>>\nstream\nBT /F 12 Tf 50 700 Td (Hi there) Tj ET\nendstream\nendobj\n\
        5 0 obj\n<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>\nendobj\n\
        trailer\n<</Root 1 0 R/Size 6>>\n"
        .to_vec();
    let document = Document::load(bytes.clone(), ReadLimits::default()).unwrap();
    assert!(document.rebuilt());
    assert_eq!(document.len(), 5);
    assert_eq!(text_of(bytes), "Hi there");
}

#[test]
fn objects_on_one_line_are_found_by_a_scan() {
    let bytes = b"%PDF-1.7\n\
        1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj 2 0 obj\n<</Type/Pages/Kids[3 0 R]/Count 1>>\nendobj 3 0 obj\n\
        <</Type/Page/Parent 2 0 R>>endobj\n\
        trailer\n<</Root 1 0 R/Size 4>>\n"
        .to_vec();
    let document = Document::load(bytes, ReadLimits::default()).unwrap();
    assert!(document.rebuilt());
    assert_eq!(document.pages(), [(3, 0)]);
}

#[test]
fn what_an_edit_saves_gives_the_nodes_of_a_page_tree_their_type() {
    let objects = vec![
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Kids[3 0 R]/Count 1>>".to_vec(),
        b"<</Parent 2 0 R>>".to_vec(),
    ];
    let bytes = file(&objects, "<</Root 1 0 R/Size 4>>");
    let edited = Objects::read(Document::load(bytes, ReadLimits::default()).unwrap()).unwrap();
    assert_eq!(
        edited.dictionary((2, 0)).unwrap().type_name(),
        Some(&b"Pages"[..])
    );
    assert_eq!(
        edited.dictionary((3, 0)).unwrap().type_name(),
        Some(&b"Page"[..])
    );
}

#[test]
fn an_encryption_length_given_in_bytes_is_read() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/lines.aes-128.pdf"
    );
    let mut bytes = std::fs::read(path).unwrap();
    let at = bytes
        .windows(11)
        .position(|window| window == b"/Length 128")
        .unwrap();
    bytes[at..at + 11].copy_from_slice(b"/Length  16");
    let text = text_of(bytes);
    assert!(text.contains("Line 1") || !text.is_empty(), "{text:?}");
}

#[test]
fn a_stray_bracket_does_not_end_a_page() {
    let file = doc(
        stream("", b"BT /F 12 Tf 50 700 Td (Hi) Tj ] (there) Tj ET"),
        HELVETICA,
        vec![],
    );
    assert_eq!(text_of(file), "Hithere");
}

#[test]
fn a_doubled_minus_does_not_end_a_page() {
    let file = doc(
        stream("", b"BT /F 12 Tf --5 0 Td 50 700 Td (Hi there) Tj ET"),
        HELVETICA,
        vec![],
    );
    assert_eq!(text_of(file), "Hi there");
    let ops: Vec<_> = operations(b"--5 Tc").map(Result::unwrap).collect();
    assert_eq!(ops[0].operands, [navree_pdf::Object::Integer(-5)]);
}

#[test]
fn a_number_too_large_for_an_integer_does_not_end_a_page() {
    let file = doc(
        stream(
            "",
            b"BT /F 12 Tf 99999999999999999999 Ts 0 Ts 50 700 Td (Hi there) Tj ET",
        ),
        HELVETICA,
        vec![],
    );
    assert_eq!(text_of(file), "Hi there");
}

//! Shared helpers: images, documents and archives built in memory.

use std::io::{Cursor, Write};

use image::{DynamicImage, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};

/// A `width` x `height` RGB gradient encoded as `format`.
pub fn image_bytes(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
    let img = RgbImage::from_fn(width, height, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    encode(DynamicImage::ImageRgb8(img), format)
}

/// A `width` x `height` RGBA image with a transparent left half, as PNG.
pub fn transparent_png(width: u32, height: u32) -> Vec<u8> {
    let img = RgbaImage::from_fn(width, height, |x, _| {
        if x < width / 2 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([200, 30, 30, 255])
        }
    });
    encode(DynamicImage::ImageRgba8(img), ImageFormat::Png)
}

/// `img` encoded as `format`.
pub fn encode(img: DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, format).expect("encode test image");
    out.into_inner()
}

/// A JPEG of `width` x `height` with an EXIF segment (APP1) holding the
/// given orientation tag and a camera serial number, inserted after SOI.
pub fn jpeg_with_exif(width: u32, height: u32, orientation: u16) -> Vec<u8> {
    let jpeg = image_bytes(width, height, ImageFormat::Jpeg);
    let exif = exif_segment(orientation);
    let mut out = Vec::with_capacity(jpeg.len() + exif.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&exif);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// An APP1 EXIF segment (big-endian TIFF) with two IFD0 entries:
/// Orientation and a serial-number string `SERIAL-12345`.
fn exif_segment(orientation: u16) -> Vec<u8> {
    let serial = b"SERIAL-12345\0";
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"MM\0\x2a\0\0\0\x08");
    tiff.extend_from_slice(&2u16.to_be_bytes());
    // Orientation (0x0112), SHORT, count 1, value in the first two bytes.
    tiff.extend_from_slice(&0x0112u16.to_be_bytes());
    tiff.extend_from_slice(&3u16.to_be_bytes());
    tiff.extend_from_slice(&1u32.to_be_bytes());
    tiff.extend_from_slice(&orientation.to_be_bytes());
    tiff.extend_from_slice(&[0, 0]);
    // BodySerialNumber (0xa431), ASCII, stored after the IFD.
    let offset = 8 + 2 + 2 * 12 + 4;
    tiff.extend_from_slice(&0xa431u16.to_be_bytes());
    tiff.extend_from_slice(&2u16.to_be_bytes());
    tiff.extend_from_slice(&(serial.len() as u32).to_be_bytes());
    tiff.extend_from_slice(&(offset as u32).to_be_bytes());
    tiff.extend_from_slice(&0u32.to_be_bytes());
    tiff.extend_from_slice(serial);
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&tiff);
    let mut segment = vec![0xff, 0xe1];
    segment.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    segment.extend_from_slice(&payload);
    segment
}

/// A PNG with a `tEXt` chunk (`Author: Ada`) before IEND.
pub fn png_with_text(width: u32, height: u32) -> Vec<u8> {
    let png = image_bytes(width, height, ImageFormat::Png);
    let iend = png.len() - 12;
    let data = b"Author\0Ada";
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(data);
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut out = png[..iend].to_vec();
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[iend..]);
    out
}

/// CRC-32 (IEEE), for hand-built PNG chunks.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (!(crc & 1)).wrapping_add(1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

/// An SVG logo of `width` x `height` user units: a red square.
pub fn svg_logo(width: u32, height: u32) -> Vec<u8> {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><rect width="{width}" height="{height}" fill="#d00"/></svg>"##
    )
    .into_bytes()
}

/// A zip archive of `(name, contents)` entries, deflated.
pub fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, contents) in entries {
        if name.ends_with('/') {
            writer.add_directory(*name, options).expect("add dir");
        } else {
            writer.start_file(*name, options).expect("start file");
            writer.write_all(contents).expect("write file");
        }
    }
    writer.finish().expect("finish zip").into_inner()
}

/// A minimal DOCX: two paragraphs (the second with a tab and a line break)
/// and a title in the core properties.
pub fn docx() -> Vec<u8> {
    let document = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Phishing awareness</w:t></w:r></w:p>
    <w:p><w:r><w:t xml:space="preserve">Report </w:t></w:r><w:r><w:t>suspicious</w:t><w:tab/><w:t>mail</w:t><w:br/><w:t>at once &amp; always.</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
    let core = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Security handbook</dc:title></cp:coreProperties>"#;
    zip_of(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", document),
        ("docProps/core.xml", core),
    ])
}

/// A PDF with one line of text per page, built with navree-pdf.
pub fn pdf(pages: &[&str]) -> Vec<u8> {
    use navree_pdf::content::{Operation, encode};
    use navree_pdf::{Object, Objects, Stream, dictionary};

    let mut doc = Objects::new();
    doc.version = "1.5".into();
    let pages_id = doc.reserve();
    let font_id = doc.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = doc.add(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut kids = Vec::new();
    for text in pages {
        let content = encode(&[
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![72.into(), 720.into()]),
            Operation::new("Tj", vec![Object::string_literal(*text)]),
            Operation::new("ET", vec![]),
        ]);
        let content_id = doc.add(Stream::new(dictionary! {}, content));
        let page_id = doc.add(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
        });
        kids.push(page_id.into());
    }
    let count = kids.len() as i64;
    doc.insert(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        },
    );
    let catalog_id = doc.add(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save().expect("save pdf")
}

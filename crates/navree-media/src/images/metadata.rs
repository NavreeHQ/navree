//! Metadata stripping for originals: EXIF, XMP, IPTC and comments are
//! cut out of JPEG, PNG and WebP files without re-encoding them, unless
//! the EXIF orientation turns the picture, in which case it is re-encoded
//! upright (the orientation lives in the metadata being removed).

use image::metadata::Orientation;

use super::{ImageOptions, decode, encode, orientation};
use crate::MediaError;
use crate::conversion::OutputFormat;

/// The JPEG quality of an original re-encoded to turn it upright.
const UPRIGHT_QUALITY: u8 = 92;

/// Removes location, camera and author metadata from an original image:
///
/// - JPEG: the APP1 (EXIF, XMP), APP13 (IPTC) and comment segments;
/// - PNG: the `eXIf`, `tEXt`, `zTXt`, `iTXt` and `tIME` chunks;
/// - WebP: the `EXIF` and `XMP ` chunks.
///
/// Colour profiles stay. A file whose EXIF orientation rotates or flips
/// it is decoded, turned upright and re-encoded in its own format.
/// Returns `None` when there is nothing to remove, and for other types.
pub fn strip_metadata(
    bytes: &[u8],
    mime_type: &str,
    options: &ImageOptions,
) -> Result<Option<Vec<u8>>, MediaError> {
    let format = match mime_type {
        "image/jpeg" | "image/jpg" => OutputFormat::Jpeg,
        "image/png" => OutputFormat::Png,
        "image/webp" => OutputFormat::WebP,
        _ => return Ok(None),
    };
    if orientation(bytes) != Orientation::NoTransforms {
        let img = decode(bytes, options)?;
        return encode(&img, format, UPRIGHT_QUALITY).map(Some);
    }
    match format {
        OutputFormat::Jpeg => strip_jpeg(bytes),
        OutputFormat::Png => strip_png(bytes),
        OutputFormat::WebP => strip_webp(bytes),
    }
}

fn malformed(kind: &str) -> MediaError {
    MediaError::image(format!("malformed {kind} file"))
}

/// Drops APP1, APP13 and COM segments before the scan data.
fn strip_jpeg(bytes: &[u8]) -> Result<Option<Vec<u8>>, MediaError> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return Err(malformed("JPEG"));
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..2]);
    let mut pos = 2;
    let mut changed = false;
    while pos < bytes.len() {
        if bytes[pos] != 0xff || pos + 1 >= bytes.len() {
            return Err(malformed("JPEG"));
        }
        let marker = bytes[pos + 1];
        match marker {
            // Fill bytes before a marker.
            0xff => {
                pos += 1;
                continue;
            }
            // Start of scan, end of image: the rest is copied as it is.
            0xda | 0xd9 => {
                out.extend_from_slice(&bytes[pos..]);
                break;
            }
            // Markers without a length.
            0x01 | 0xd0..=0xd7 => {
                out.extend_from_slice(&bytes[pos..pos + 2]);
                pos += 2;
                continue;
            }
            _ => {}
        }
        if pos + 4 > bytes.len() {
            return Err(malformed("JPEG"));
        }
        let length = usize::from(u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]));
        let end = pos + 2 + length;
        if length < 2 || end > bytes.len() {
            return Err(malformed("JPEG"));
        }
        if matches!(marker, 0xe1 | 0xed | 0xfe) {
            changed = true;
        } else {
            out.extend_from_slice(&bytes[pos..end]);
        }
        pos = end;
    }
    Ok(changed.then_some(out))
}

/// Drops the metadata chunks of a PNG.
fn strip_png(bytes: &[u8]) -> Result<Option<Vec<u8>>, MediaError> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIGNATURE) {
        return Err(malformed("PNG"));
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(SIGNATURE);
    let mut pos = SIGNATURE.len();
    let mut changed = false;
    while pos < bytes.len() {
        if pos + 12 > bytes.len() {
            return Err(malformed("PNG"));
        }
        let length = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
        let end = pos + 12 + length as usize;
        if end > bytes.len() {
            return Err(malformed("PNG"));
        }
        let kind = &bytes[pos + 4..pos + 8];
        if matches!(kind, b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME") {
            changed = true;
        } else {
            out.extend_from_slice(&bytes[pos..end]);
        }
        pos = end;
        if kind == b"IEND" {
            break;
        }
    }
    Ok(changed.then_some(out))
}

/// Drops the `EXIF` and `XMP ` chunks of a WebP and clears their flags in
/// `VP8X`.
fn strip_webp(bytes: &[u8]) -> Result<Option<Vec<u8>>, MediaError> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(malformed("WebP"));
    }
    let mut chunks = Vec::with_capacity(bytes.len());
    let mut pos = 12;
    let mut changed = false;
    while pos + 8 <= bytes.len() {
        let kind = &bytes[pos..pos + 4];
        let length = u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]])
            as usize;
        let end = pos + 8 + length + (length & 1);
        if end > bytes.len() {
            return Err(malformed("WebP"));
        }
        match kind {
            b"EXIF" | b"XMP " => changed = true,
            b"VP8X" if length >= 1 => {
                let start = chunks.len();
                chunks.extend_from_slice(&bytes[pos..end]);
                // Flags: ICC 0x20, alpha 0x10, EXIF 0x08, XMP 0x04, animation 0x02.
                chunks[start + 8] &= !(0x08 | 0x04);
            }
            _ => chunks.extend_from_slice(&bytes[pos..end]),
        }
        pos = end;
    }
    if !changed {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(chunks.len() + 12);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&chunks);
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webp_exif_chunks_and_flags_are_removed() {
        let mut chunks = Vec::new();
        // VP8X with the EXIF flag and a 1x1 canvas.
        chunks.extend_from_slice(b"VP8X");
        chunks.extend_from_slice(&10u32.to_le_bytes());
        chunks.extend_from_slice(&[0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        chunks.extend_from_slice(b"EXIF");
        chunks.extend_from_slice(&3u32.to_le_bytes());
        chunks.extend_from_slice(b"abc\0");
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
        file.extend_from_slice(b"WEBP");
        file.extend_from_slice(&chunks);

        let out = strip_webp(&file).unwrap().expect("changed");
        assert!(!out.windows(4).any(|w| w == b"EXIF"));
        assert_eq!(out[20], 0, "the EXIF flag is cleared");
        let riff = u32::from_le_bytes([out[4], out[5], out[6], out[7]]) as usize;
        assert_eq!(riff + 8, out.len());
    }

    #[test]
    fn truncated_files_are_errors() {
        assert!(strip_jpeg(&[0xff, 0xd8, 0xff, 0xe1, 0x00]).is_err());
        assert!(strip_png(b"\x89PNG\r\n\x1a\n\0\0").is_err());
        assert!(strip_webp(b"RIFF").is_err());
    }
}

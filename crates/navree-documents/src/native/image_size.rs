//! The pixels an image declares, read from its header before any decoder
//! allocates for them.
//!
//! A decoder sizes its buffers from the header, so a file of a few kilobytes
//! can ask for gigabytes. These readers take the largest size the headers
//! claim anywhere a decoder might look (a GIF's frame as well as its
//! screen, a WebP's canvas as well as its picture), so that the count is
//! never smaller than what gets allocated.

/// The most pixels the headers of the PNG, JPEG, GIF or WebP in `bytes`
/// declare, or `None` when it is none of those or its header is cut short
/// (the decoder then refuses it without allocating for it).
pub(super) fn declared_pixels(bytes: &[u8]) -> Option<u64> {
    if bytes.starts_with(b"\x89PNG") {
        png(bytes)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        jpeg(bytes)
    } else if bytes.starts_with(b"GIF8") {
        gif(bytes)
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        webp(bytes)
    } else {
        None
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<u64> {
    let pair = bytes.get(at..at + 2)?;
    Some(u64::from(u16::from_be_bytes([pair[0], pair[1]])))
}

fn le16(bytes: &[u8], at: usize) -> Option<u64> {
    let pair = bytes.get(at..at + 2)?;
    Some(u64::from(u16::from_le_bytes([pair[0], pair[1]])))
}

fn le24(bytes: &[u8], at: usize) -> Option<u64> {
    let triple = bytes.get(at..at + 3)?;
    Some(u64::from(u32::from_le_bytes([
        triple[0], triple[1], triple[2], 0,
    ])))
}

fn le32(bytes: &[u8], at: usize) -> Option<u64> {
    let quad = bytes.get(at..at + 4)?;
    Some(u64::from(u32::from_le_bytes([
        quad[0], quad[1], quad[2], quad[3],
    ])))
}

/// The width and height in the `IHDR` chunk, which comes first.
fn png(bytes: &[u8]) -> Option<u64> {
    if bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u64::from(u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?));
    let height = u64::from(u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?));
    Some(width * height)
}

/// The size in the first frame header (`SOF0` to `SOF15`, except the
/// tables and the arithmetic-coding marker that share their numbers).
fn jpeg(bytes: &[u8]) -> Option<u64> {
    let mut at = 2;
    loop {
        // Markers start with 0xFF, which may be repeated as padding.
        while *bytes.get(at)? != 0xFF {
            at += 1;
        }
        while *bytes.get(at)? == 0xFF {
            at += 1;
        }
        let marker = *bytes.get(at)?;
        at += 1;
        match marker {
            // Stuffed bytes and markers without a length.
            0x00 | 0x01 | 0xD0..=0xD8 => {}
            // The end of the image, or the start of its data: no frame
            // header came before it.
            0xD9 | 0xDA => return None,
            0xC4 | 0xC8 | 0xCC => at += usize::try_from(be16(bytes, at)?).ok()?,
            0xC0..=0xCF => return Some(be16(bytes, at + 3)? * be16(bytes, at + 5)?),
            _ => at += usize::try_from(be16(bytes, at)?).ok()?,
        }
    }
}

/// The larger of the screen and the first frame.
fn gif(bytes: &[u8]) -> Option<u64> {
    let screen = le16(bytes, 6)? * le16(bytes, 8)?;
    let flags = *bytes.get(10)?;
    let mut at = 13;
    if flags & 0x80 != 0 {
        at += 3 * (1usize << ((flags & 7) + 1));
    }
    loop {
        match bytes.get(at) {
            // An image descriptor: left, top, width, height.
            Some(0x2C) => return Some(screen.max(le16(bytes, at + 5)? * le16(bytes, at + 7)?)),
            // An extension: a label, then blocks of data up to an empty one.
            Some(0x21) => {
                at += 2;
                loop {
                    let size = usize::from(*bytes.get(at)?);
                    at += 1 + size;
                    if size == 0 {
                        break;
                    }
                }
            }
            _ => return Some(screen),
        }
    }
}

/// The largest of the canvas (`VP8X`) and the first picture (`VP8 `,
/// `VP8L` or an animation frame).
fn webp(bytes: &[u8]) -> Option<u64> {
    let mut largest = None;
    let mut at = 12;
    while let Some(kind) = bytes.get(at..at + 4) {
        let size = usize::try_from(le32(bytes, at + 4)?).ok()?;
        let data = at + 8;
        let pixels = match kind {
            b"VP8X" => Some((1 + le24(bytes, data + 4)?) * (1 + le24(bytes, data + 7)?)),
            b"VP8 " => Some((le16(bytes, data + 6)? & 0x3FFF) * (le16(bytes, data + 8)? & 0x3FFF)),
            b"VP8L" => {
                let bits = le32(bytes, data + 1)?;
                Some(((bits & 0x3FFF) + 1) * (((bits >> 14) & 0x3FFF) + 1))
            }
            b"ANMF" => Some((1 + le24(bytes, data + 6)?) * (1 + le24(bytes, data + 9)?)),
            _ => None,
        };
        if let Some(pixels) = pixels {
            largest = largest.max(Some(pixels));
            if kind != b"VP8X" {
                break;
            }
        }
        // Chunks are padded to an even size.
        at = data.checked_add(size)?.checked_add(size & 1)?;
    }
    largest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_declares_its_size_in_ihdr() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(6000u32.to_be_bytes());
        png.extend(5000u32.to_be_bytes());
        png.extend([8, 2, 0, 0, 0]);
        assert_eq!(declared_pixels(&png), Some(30_000_000));
        // A huge declaration is counted, not wrapped.
        let mut huge = png.clone();
        huge[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        huge[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(declared_pixels(&huge), Some(u64::from(u32::MAX).pow(2)));
        assert_eq!(declared_pixels(&png[..20]), None);
    }

    #[test]
    fn a_jpeg_declares_its_size_in_the_first_frame_header() {
        // SOI, an APP0 segment of 4 bytes, a DHT table marker that is not a
        // frame, then SOF0: 8 bits, height 3000, width 4000.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46];
        jpeg.extend([0xFF, 0xC4, 0x00, 0x03, 0x00]);
        jpeg.extend([0xFF, 0xFF, 0xC0, 0x00, 0x0B, 0x08]);
        jpeg.extend(3000u16.to_be_bytes());
        jpeg.extend(4000u16.to_be_bytes());
        jpeg.extend([0x01, 0x01, 0x11, 0x00]);
        assert_eq!(declared_pixels(&jpeg), Some(12_000_000));
        // No frame header before the start of the scan.
        assert_eq!(declared_pixels(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02]), None);
        assert_eq!(declared_pixels(&jpeg[..14]), None);
    }

    #[test]
    fn a_gif_counts_the_larger_of_its_screen_and_its_first_frame() {
        let mut gif = b"GIF89a".to_vec();
        // A 10 by 20 screen with a 2-colour table.
        gif.extend([10, 0, 20, 0, 0x80, 0, 0]);
        gif.extend([0, 0, 0, 255, 255, 255]);
        // A graphics control extension, then a 300 by 400 frame.
        gif.extend([0x21, 0xF9, 4, 0, 0, 0, 0, 0]);
        gif.extend([0x2C, 0, 0, 0, 0]);
        gif.extend(300u16.to_le_bytes());
        gif.extend(400u16.to_le_bytes());
        assert_eq!(declared_pixels(&gif), Some(120_000));
        // No frame: the screen.
        assert_eq!(declared_pixels(&gif[..19]), Some(200));
    }

    #[test]
    fn a_webp_counts_its_canvas_and_its_picture() {
        // VP8X with a 1000 by 2000 canvas, then a lossless picture of
        // 3000 by 4000.
        let mut webp = b"RIFF\0\0\0\0WEBP".to_vec();
        webp.extend(b"VP8X");
        webp.extend(10u32.to_le_bytes());
        webp.extend([0, 0, 0, 0]);
        webp.extend(&(1000u32 - 1).to_le_bytes()[..3]);
        webp.extend(&(2000u32 - 1).to_le_bytes()[..3]);
        webp.extend(b"VP8L");
        webp.extend(5u32.to_le_bytes());
        webp.push(0x2F);
        webp.extend((2999u32 | (3999u32 << 14)).to_le_bytes());
        webp.push(0);
        assert_eq!(declared_pixels(&webp), Some(12_000_000));
        // A lossy picture alone.
        let mut lossy = b"RIFF\0\0\0\0WEBP".to_vec();
        lossy.extend(b"VP8 ");
        lossy.extend(10u32.to_le_bytes());
        lossy.extend([0, 0, 0, 0x9D, 0x01, 0x2A]);
        lossy.extend(640u16.to_le_bytes());
        lossy.extend(480u16.to_le_bytes());
        assert_eq!(declared_pixels(&lossy), Some(307_200));
    }

    #[test]
    fn other_bytes_declare_nothing() {
        assert_eq!(declared_pixels(b"not an image"), None);
        assert_eq!(declared_pixels(b""), None);
    }
}

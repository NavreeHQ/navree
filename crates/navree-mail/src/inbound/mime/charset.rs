//! Text in a message's charset, decoded to UTF-8: the WHATWG encodings
//! through encoding_rs, plus UTF-7 and IBM code page 850, which mail still
//! uses. An unknown charset, or none, is read as UTF-8; bytes that don't
//! decode become U+FFFD.

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

/// Decodes `bytes` written in `charset` (a MIME charset name, any case).
pub(super) fn decode(bytes: &[u8], charset: Option<&str>) -> String {
    let Some(label) = charset.map(str::trim).filter(|label| !label.is_empty()) else {
        return utf8(bytes);
    };
    let lower = label.to_ascii_lowercase();
    match lower.as_str() {
        "utf-8" | "utf8" => return utf8(bytes),
        "utf-7" | "utf_7" | "utf7" | "csutf7" | "unicode-1-1-utf-7" => return utf7(bytes),
        "850" | "cp850" | "ibm850" | "ibm-850" | "cspc850multilingual" => return cp850(bytes),
        _ => {}
    }
    let encoding = Encoding::for_label(lower.as_bytes())
        .or_else(|| Encoding::for_label(lower.replace('_', "-").as_bytes()));
    match encoding {
        None => utf8(bytes),
        Some(encoding) if encoding == UTF_8 => utf8(bytes),
        // A byte order mark says which UTF-16 it is.
        // A byte left over at the end is the line break's, not a character.
        Some(encoding) if encoding == UTF_16LE || encoding == UTF_16BE => {
            let whole = bytes.get(..bytes.len() & !1).unwrap_or_default();
            encoding.decode(whole).0.into_owned()
        }
        Some(encoding) => encoding.decode_without_bom_handling(bytes).0.into_owned(),
    }
}

/// UTF-8, with U+FFFD for what isn't.
fn utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// UTF-7 (RFC 2152): `+` starts a run of modified base64 holding UTF-16
/// code units, ended by any other character (a `-` ending it is dropped);
/// `+-` is a plus sign.
fn utf7(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        index += 1;
        if byte != b'+' {
            out.push(char::from(byte));
            continue;
        }
        let start = index;
        while bytes
            .get(index)
            .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        {
            index += 1;
        }
        let run = &bytes[start..index];
        if run.is_empty() {
            if bytes.get(index) == Some(&b'-') {
                index += 1;
            }
            out.push('+');
            continue;
        }
        let mut units = Vec::with_capacity(run.len() * 6 / 16 + 1);
        let mut bits: u32 = 0;
        let mut count = 0u32;
        for &b in run {
            let value = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                _ => 63,
            };
            bits = (bits << 6) | u32::from(value);
            count += 6;
            if count >= 16 {
                count -= 16;
                units.push((bits >> count) as u16);
                bits &= (1 << count) - 1;
            }
        }
        out.extend(char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)));
        if bytes.get(index) == Some(&b'-') {
            index += 1;
        }
    }
    out
}

/// IBM code page 850 (DOS Latin 1): the upper half.
const CP850: [u16; 128] = [
    0x00C7, 0x00FC, 0x00E9, 0x00E2, 0x00E4, 0x00E0, 0x00E5, 0x00E7, //
    0x00EA, 0x00EB, 0x00E8, 0x00EF, 0x00EE, 0x00EC, 0x00C4, 0x00C5, //
    0x00C9, 0x00E6, 0x00C6, 0x00F4, 0x00F6, 0x00F2, 0x00FB, 0x00F9, //
    0x00FF, 0x00D6, 0x00DC, 0x00F8, 0x00A3, 0x00D8, 0x00D7, 0x0192, //
    0x00E1, 0x00ED, 0x00F3, 0x00FA, 0x00F1, 0x00D1, 0x00AA, 0x00BA, //
    0x00BF, 0x00AE, 0x00AC, 0x00BD, 0x00BC, 0x00A1, 0x00AB, 0x00BB, //
    0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x00C1, 0x00C2, 0x00C0, //
    0x00A9, 0x2563, 0x2551, 0x2557, 0x255D, 0x00A2, 0x00A5, 0x2510, //
    0x2514, 0x2534, 0x252C, 0x251C, 0x2500, 0x253C, 0x00E3, 0x00C3, //
    0x255A, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256C, 0x00A4, //
    0x00F0, 0x00D0, 0x00CA, 0x00CB, 0x00C8, 0x0131, 0x00CD, 0x00CE, //
    0x00CF, 0x2518, 0x250C, 0x2588, 0x2584, 0x00A6, 0x00CC, 0x2580, //
    0x00D3, 0x00DF, 0x00D4, 0x00D2, 0x00F5, 0x00D5, 0x00B5, 0x00FE, //
    0x00DE, 0x00DA, 0x00DB, 0x00D9, 0x00FD, 0x00DD, 0x00AF, 0x00B4, //
    0x00AD, 0x00B1, 0x2017, 0x00BE, 0x00B6, 0x00A7, 0x00F7, 0x00B8, //
    0x00B0, 0x00A8, 0x00B7, 0x00B9, 0x00B3, 0x00B2, 0x25A0, 0x00A0, //
];

fn cp850(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| match byte.checked_sub(0x80) {
            None => char::from(byte),
            Some(high) => CP850
                .get(usize::from(high))
                .and_then(|&code| char::from_u32(u32::from(code)))
                .unwrap_or(char::REPLACEMENT_CHARACTER),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charsets_decode_by_any_of_their_names() {
        for (charset, bytes, text) in [
            ("iso-8859-1", &b"\xe1\xe9\xed\xf3\xfa"[..], "áéíóú"),
            ("ISO-8859-1", b"\x805.4bn", "€5.4bn"),
            ("latin1", b"\x805.4bn", "€5.4bn"),
            ("iso88591", b"\x805.4bn", "€5.4bn"),
            ("us-ascii", b"\x805.4bn", "€5.4bn"),
            ("iso8859-5", b"\xbf\xe0\xd8\xd2\xd5\xe2", "Привет"),
            ("iso_8859-1:1987", b"J\xf8rn", "Jørn"),
            ("iso_8859_15", b"\xa4", "€"),
            ("windows-1251", b"\xcf\xf0\xe8\xe2\xe5\xf2", "Привет"),
            ("koi8-r", b"\xf0\xd2\xc9\xd7\xc5\xd4", "Привет"),
            ("macintosh", b"\x87\x8e\x92\x97\x9c", "áéíóú"),
            ("ibm850", b"\x9b\x9c\x9d\x9e", "ø£Ø×"),
            ("shift_jis", b"\x83n\x83\x8D\x81[", "ハロー"),
            ("iso-2022-jp", b"\x1b$B%O%m!<\x1b(B", "ハロー"),
            ("gb2312", b"\xc4\xe3\xba\xc3", "你好"),
            ("big5", b"\xa7A\xa6n", "你好"),
            ("euc-kr", b"\xbe\xc8\xb3\xe7", "안녕"),
            ("utf-16", b"\xff\xfe\xe1\x00\xe9\x00", "áé"),
            ("utf-16", b"\xfe\xff\x00\xe1\x00\xe9", "áé"),
            ("utf-16be", b"0\xcf0\xed", "ハロ"),
            ("utf-8", "Grüße".as_bytes(), "Grüße"),
            ("x-unknown", "Grüße".as_bytes(), "Grüße"),
            ("utf-8", b"bad \xff byte", "bad \u{fffd} byte"),
        ] {
            assert_eq!(decode(bytes, Some(charset)), text, "{charset}");
        }
        assert_eq!(decode(b"plain", None), "plain");
        assert_eq!(decode(b"\xef\xbb\xbfbom", Some("utf-8")), "\u{feff}bom");
    }

    #[test]
    fn utf7_decodes_its_base64_runs() {
        for (input, text) in [
            ("Hello, World+ACE-", "Hello, World!"),
            ("Hi Mom -+Jjo--!", "Hi Mom -☺-!"),
            ("+ZeVnLIqe-", "日本語"),
            ("Item 3 is +AKM-1.", "Item 3 is £1."),
            ("Plus minus +- -+ +--", "Plus minus + -+ +-"),
            (
                "+APw-ber ihre mi+AN8-liche Lage+ADs-",
                "über ihre mißliche Lage;",
            ),
            ("+ZYeB9FH6ckh5Pg-, 1980.", "文致出版社, 1980."),
        ] {
            assert_eq!(decode(input.as_bytes(), Some("utf-7")), text, "{input}");
        }
    }
}

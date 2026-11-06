//! The encoding of an uploaded text file: a byte-order mark, UTF-16
//! without one, UTF-8, or chardetng's guess at a legacy encoding.

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

use crate::SheetsError;

/// How many bytes the UTF-16 check and chardetng look at.
const SAMPLE_BYTES: usize = 1 << 20;

/// A decoded text file and the encoding it was in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decoded {
    pub(crate) text: String,
    /// The encoding's name (`UTF-8`, `UTF-16LE`, `windows-1256`).
    pub(crate) encoding: &'static str,
    /// Some bytes did not decode and became U+FFFD.
    pub(crate) lossy: bool,
}

/// Decodes `bytes`: in `label` when one is given (`windows-1256`,
/// `shift_jis`, `utf-16le`), otherwise by detection:
///
/// 1. a byte-order mark (UTF-8, UTF-16LE, UTF-16BE);
/// 2. UTF-16 without one (every other byte is zero in ASCII text);
/// 3. valid UTF-8;
/// 4. chardetng's guess (Windows-125x, ISO-8859-x, Shift_JIS, EUC-KR, GBK,
///    Big5, KOI8 and the others browsers know).
pub(crate) fn decode(bytes: &[u8], label: Option<&str>) -> Result<Decoded, SheetsError> {
    if let Some(label) = label.map(str::trim).filter(|l| !l.is_empty()) {
        let encoding = Encoding::for_label(label.as_bytes())
            .ok_or_else(|| SheetsError::Invalid(format!("unknown encoding `{label}`")))?;
        let (text, lossy) = encoding.decode_with_bom_removal(bytes);
        return Ok(finish(text.into_owned(), encoding, lossy));
    }
    if let Some((encoding, bom)) = Encoding::for_bom(bytes) {
        let (text, lossy) = encoding.decode_without_bom_handling(&bytes[bom..]);
        return Ok(finish(text.into_owned(), encoding, lossy));
    }
    if let Some(encoding) = utf16_without_bom(bytes) {
        let (text, lossy) = encoding.decode_without_bom_handling(bytes);
        return Ok(finish(text.into_owned(), encoding, lossy));
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(finish(text.to_owned(), UTF_8, false));
    }
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    let sample = &bytes[..bytes.len().min(SAMPLE_BYTES)];
    detector.feed(sample, sample.len() == bytes.len());
    let encoding = detector.guess(None, Utf8Detection::Allow);
    let (text, lossy) = encoding.decode_without_bom_handling(bytes);
    Ok(finish(text.into_owned(), encoding, lossy))
}

fn finish(mut text: String, encoding: &'static Encoding, lossy: bool) -> Decoded {
    if text.starts_with('\u{FEFF}') {
        text.drain(..'\u{FEFF}'.len_utf8());
    }
    Decoded {
        text,
        encoding: encoding.name(),
        lossy,
    }
}

/// UTF-16 text without a byte-order mark: in text that is mostly ASCII
/// (delimiters, digits, line breaks), one byte of most pairs is zero.
fn utf16_without_bom(bytes: &[u8]) -> Option<&'static Encoding> {
    let sample = &bytes[..bytes.len().min(SAMPLE_BYTES)];
    if sample.len() < 4 || !sample.len().is_multiple_of(2) && sample.len() == bytes.len() {
        return None;
    }
    let pairs = sample.len() / 2;
    let (mut even_zero, mut odd_zero) = (0usize, 0usize);
    for [first, second] in sample.as_chunks::<2>().0 {
        if *first == 0 {
            even_zero += 1;
        }
        if *second == 0 {
            odd_zero += 1;
        }
    }
    // Plenty of zeros on one side, almost none on the other.
    let plenty = pairs * 3 / 10;
    let few = pairs / 20;
    if odd_zero > plenty && even_zero <= few {
        Some(UTF_16LE)
    } else if even_zero > plenty && odd_zero <= few {
        Some(UTF_16BE)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{SHIFT_JIS, WINDOWS_1252, WINDOWS_1256};

    const ARABIC: &str = "الاسم,البريد الإلكتروني,المدينة\n\
        محمد عبد الله,mohammed@example.com,الرياض\n\
        فاطمة الزهراء,fatima@example.com,القاهرة\n\
        عبد الرحمن السالم,abdulrahman@example.com,دبي\n\
        نورة القحطاني,noura@example.com,جدة\n";

    fn encode(encoding: &'static Encoding, text: &str) -> Vec<u8> {
        let (bytes, _, unmappable) = encoding.encode(text);
        assert!(!unmappable);
        bytes.into_owned()
    }

    fn utf16(text: &str, little: bool) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|unit| {
                if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                }
            })
            .collect()
    }

    #[test]
    fn utf8_with_and_without_a_bom() {
        let decoded = decode("a,b\n1,2\n".as_bytes(), None).unwrap();
        assert_eq!(decoded.text, "a,b\n1,2\n");
        assert_eq!(decoded.encoding, "UTF-8");
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(ARABIC.as_bytes());
        let decoded = decode(&bom, None).unwrap();
        assert_eq!(decoded.text, ARABIC);
        assert_eq!(decoded.encoding, "UTF-8");
    }

    #[test]
    fn windows_1256_arabic_is_detected() {
        let bytes = encode(WINDOWS_1256, ARABIC);
        assert!(std::str::from_utf8(&bytes).is_err());
        let decoded = decode(&bytes, None).unwrap();
        assert_eq!(decoded.encoding, "windows-1256");
        assert_eq!(decoded.text, ARABIC);
        assert!(!decoded.lossy);
    }

    #[test]
    fn utf16_with_a_bom() {
        for little in [true, false] {
            let mut bytes = if little {
                vec![0xFF, 0xFE]
            } else {
                vec![0xFE, 0xFF]
            };
            bytes.extend(utf16(ARABIC, little));
            let decoded = decode(&bytes, None).unwrap();
            assert_eq!(decoded.text, ARABIC);
            assert_eq!(
                decoded.encoding,
                if little { "UTF-16LE" } else { "UTF-16BE" }
            );
        }
    }

    #[test]
    fn utf16_without_a_bom() {
        let text = "name,email\nAnu,anu@example.com\nArun,arun@example.com\n";
        for little in [true, false] {
            let decoded = decode(&utf16(text, little), None).unwrap();
            assert_eq!(decoded.text, text);
            assert_eq!(
                decoded.encoding,
                if little { "UTF-16LE" } else { "UTF-16BE" }
            );
        }
    }

    #[test]
    fn a_label_wins_over_detection() {
        let bytes = encode(WINDOWS_1252, "caf\u{e9},na\u{ef}ve\n");
        let decoded = decode(&bytes, Some("windows-1252")).unwrap();
        assert_eq!(decoded.text, "café,naïve\n");
        assert_eq!(decoded.encoding, "windows-1252");
        let bytes = encode(SHIFT_JIS, "名前,メール\n山田,yamada@example.jp\n");
        let decoded = decode(&bytes, Some("shift_jis")).unwrap();
        assert_eq!(decoded.text, "名前,メール\n山田,yamada@example.jp\n");
        let error = decode(b"x", Some("klingon")).unwrap_err();
        assert!(error.to_string().contains("unknown encoding `klingon`"));
    }

    #[test]
    fn latin1_text_is_guessed() {
        let bytes = encode(
            WINDOWS_1252,
            "Nom,Ville\nRen\u{e9}e,Montr\u{e9}al\nJos\u{e9},S\u{e3}o Paulo\nFran\u{e7}ois,Gen\u{e8}ve\n",
        );
        let decoded = decode(&bytes, None).unwrap();
        assert!(decoded.text.contains("Renée,Montréal"), "{}", decoded.text);
    }
}

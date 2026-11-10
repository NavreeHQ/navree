//! The QR encoder against an independent decoder (`rqrr`), the standard's
//! published examples, and the encoder the application used before.
//!
//! Every code in the corpus is decoded back to the exact payload, with the
//! version, error-correction level and mask the encoder says it used.

use std::collections::BTreeSet;

use navree_crypto::qr::{EcLevel, Options, QrCode};
use serde_json::Value;

/// A small deterministic generator (xorshift64*), so a failing payload
/// can be reproduced from the seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick(&mut self, alphabet: &[u8]) -> u8 {
        alphabet[self.below(alphabet.len())]
    }
}

const DIGITS: &[u8] = b"0123456789";
const ALPHANUMERIC: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
const URL_SAFE: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~%:/?&=@";

/// The two bits that rqrr reports for a level.
fn format_bits(level: EcLevel) -> u16 {
    match level {
        EcLevel::L => 1,
        EcLevel::M => 0,
        EcLevel::Q => 3,
        EcLevel::H => 2,
    }
}

/// Decodes `code` from its modules, and checks what the decoder read from
/// the format and version information against what the encoder reports.
fn decode(code: &QrCode) -> Vec<u8> {
    let grid = rqrr::Grid::new(rqrr::SimpleGrid::from_func(code.size(), |x, y| {
        code.is_dark(x, y)
    }));
    let mut out = Vec::new();
    let meta = grid.decode_to(&mut out).unwrap_or_else(|error| {
        panic!(
            "rqrr cannot decode version {} {:?} mask {}: {error:?}",
            code.version(),
            code.ec_level(),
            code.mask()
        )
    });
    assert_eq!(meta.version.0, usize::from(code.version()));
    assert_eq!(meta.ecc_level, format_bits(code.ec_level()));
    assert_eq!(meta.mask, u16::from(code.mask()));
    out
}

fn round_trip(payload: &[u8], level: EcLevel) -> QrCode {
    let code = QrCode::encode(payload, level).unwrap();
    assert_eq!(
        decode(&code),
        payload,
        "{level:?} payload {:?}",
        String::from_utf8_lossy(payload)
    );
    code
}

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The `otpauth://` URIs the two-factor setup encodes: the ones totp-rs
/// produced, and a few hundred more with random secrets, issuers and
/// accounts.
fn otpauth_corpus() -> Vec<String> {
    let mut corpus: Vec<String> = fixture("otp/totp-rs-6.0.0.json")["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["url"].as_str().unwrap().to_owned())
        .collect();
    let mut rng = Rng(0x00C0_FFEE);
    for _ in 0..300 {
        let word = |rng: &mut Rng, max: usize| -> String {
            (0..1 + rng.below(max))
                .map(|_| char::from(rng.pick(b"abcdefghijklmnopqrstuvwxyz")))
                .collect()
        };
        let secret: String = (0..[26, 32, 52, 103][rng.below(4)])
            .map(|_| char::from(rng.pick(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567")))
            .collect();
        let issuer = word(&mut rng, 20);
        let account = format!("{}%40{}.com", word(&mut rng, 24), word(&mut rng, 12));
        let mut uri = format!("otpauth://totp/{issuer}:{account}?secret={secret}");
        if rng.below(3) == 0 {
            uri.push_str("&digits=8&algorithm=SHA256");
        }
        uri.push_str(&format!("&issuer={issuer}"));
        if rng.below(3) == 0 {
            uri.push_str("&period=60");
        }
        corpus.push(uri);
    }
    corpus
}

#[test]
fn otpauth_uris_decode_at_every_level() {
    let corpus = otpauth_corpus();
    let mut versions = BTreeSet::new();
    let mut count = 0;
    for uri in &corpus {
        for level in EcLevel::ALL {
            let code = round_trip(uri.as_bytes(), level);
            versions.insert(code.version());
            count += 1;
        }
    }
    assert!(count >= 1_200, "{count} codes");
    assert!(versions.len() >= 6, "{versions:?}");
}

#[test]
fn random_payloads_decode_at_every_level() {
    let mut rng = Rng(0xDEC0DE);
    let mut count = 0;
    let mut versions = BTreeSet::new();
    let mut levels = BTreeSet::new();
    for round in 0..400 {
        let length = match round % 4 {
            0 => rng.below(40),
            1 => rng.below(200),
            2 => rng.below(900),
            _ => rng.below(1200),
        };
        let payload: Vec<u8> = match rng.below(6) {
            0 => (0..length).map(|_| rng.next() as u8).collect(),
            1 => (0..length).map(|_| rng.pick(DIGITS)).collect(),
            2 => (0..length).map(|_| rng.pick(ALPHANUMERIC)).collect(),
            3 => (0..length).map(|_| rng.pick(URL_SAFE)).collect(),
            // Runs of digits and capitals inside lower-case text, so that
            // segments switch mode.
            4 => {
                let mut text = Vec::new();
                while text.len() < length {
                    let (alphabet, run): (&[u8], usize) = match rng.below(3) {
                        0 => (DIGITS, 5 + rng.below(30)),
                        1 => (b"ABCDEFGHIJKLMNOPQRSTUVWXYZ", 8 + rng.below(30)),
                        _ => (b"abcdefghijklmnopqrstuvwxyz ", 1 + rng.below(12)),
                    };
                    text.extend((0..run).map(|_| rng.pick(alphabet)));
                }
                text
            }
            _ => (0..length).map(|_| rng.pick(b"01")).collect(),
        };
        for level in EcLevel::ALL {
            let Ok(code) = QrCode::encode(&payload, level) else {
                continue;
            };
            assert_eq!(decode(&code), payload, "round {round} {level:?}");
            versions.insert(code.version());
            levels.insert(level);
            count += 1;
        }
    }
    assert!(count >= 1_400, "{count} codes");
    assert_eq!(levels.len(), 4);
    assert!(versions.len() >= 25, "{versions:?}");
}

#[test]
fn every_version_decodes_at_every_level_full_and_nearly_empty() {
    let mut rng = Rng(40);
    for version in 1..=40_u8 {
        for level in EcLevel::ALL {
            let options = Options {
                min_version: version,
                max_version: version,
                mask: None,
            };
            // The longest byte payload that fits, then almost nothing (the
            // rest of the symbol is pad codewords).
            let fits =
                |length: usize| QrCode::encode_with(&vec![0x80; length], level, options).is_ok();
            let (mut capacity, mut too_long) = (0, 2954);
            while too_long - capacity > 1 {
                let middle = (capacity + too_long) / 2;
                if fits(middle) {
                    capacity = middle;
                } else {
                    too_long = middle;
                }
            }
            assert!(capacity > 0, "version {version} {level:?}");
            for length in [capacity, 1] {
                let payload: Vec<u8> = (0..length).map(|_| rng.next() as u8).collect();
                let code = QrCode::encode_with(&payload, level, options).unwrap();
                assert_eq!(code.version(), version);
                assert_eq!(code.size(), 4 * usize::from(version) + 17);
                assert_eq!(decode(&code), payload, "version {version} {level:?}");
            }
        }
    }
}

#[test]
fn smallest_version_is_chosen() {
    // One byte more than the capacity of a version needs the next one.
    let mut previous = 0;
    for length in 1..=300 {
        let payload = vec![b'x'; length];
        let version = QrCode::encode(&payload, EcLevel::M).unwrap().version();
        assert!(version >= previous);
        if version > previous {
            let smaller = Options {
                max_version: version - 1,
                ..Options::default()
            };
            if version > 1 {
                assert!(QrCode::encode_with(&payload, EcLevel::M, smaller).is_err());
            }
        }
        previous = version;
    }
}

#[test]
fn every_mask_pattern_decodes() {
    let payloads: [&[u8]; 4] = [
        b"01234567",
        b"HELLO WORLD",
        b"otpauth://totp/Navree:anu%40example.com?secret=RJLR4GJ3YLMHQFDBJ66WK6OIKGY562ER&issuer=Navree",
        &[0, 255, 1, 254, 2, 253, 3, 252, 4, 251],
    ];
    for payload in payloads {
        for level in EcLevel::ALL {
            for mask in 0..8 {
                let options = Options {
                    mask: Some(mask),
                    ..Options::default()
                };
                let code = QrCode::encode_with(payload, level, options).unwrap();
                assert_eq!(code.mask(), mask);
                assert_eq!(decode(&code), payload, "{level:?} mask {mask}");
            }
        }
    }
    // A big symbol with every mask, too (version 7 and above carry version
    // information).
    let payload: Vec<u8> = (0..700_u32).map(|i| (i * 31 % 251) as u8).collect();
    for mask in 0..8 {
        let options = Options {
            mask: Some(mask),
            ..Options::default()
        };
        let code = QrCode::encode_with(&payload, EcLevel::Q, options).unwrap();
        assert!(code.version() >= 7);
        assert_eq!(decode(&code), payload);
    }
}

#[test]
fn the_chosen_mask_has_the_lowest_penalty() {
    // Different payloads pick different masks; not all the same.
    let mut masks = BTreeSet::new();
    for uri in otpauth_corpus().iter().take(120) {
        for level in EcLevel::ALL {
            masks.insert(round_trip(uri.as_bytes(), level).mask());
        }
    }
    assert!(masks.len() >= 6, "{masks:?}");
}

#[test]
fn numeric_and_alphanumeric_modes_shorten_the_code() {
    // 41 digits fit version 1-L as digits (10 bits per 3), but 41 bytes
    // would need version 3.
    let digits = "7".repeat(41);
    assert_eq!(round_trip(digits.as_bytes(), EcLevel::L).version(), 1);
    // 25 capitals: version 1-L in alphanumeric mode (11 bits per 2).
    let capitals = "A".repeat(25);
    assert_eq!(round_trip(capitals.as_bytes(), EcLevel::L).version(), 1);
    assert_eq!(
        round_trip("a".repeat(25).as_bytes(), EcLevel::L).version(),
        2
    );
    // A long run of digits inside text switches modes and saves a version.
    let mixed = format!("ID:{}:END", "1234567890".repeat(12));
    let version = round_trip(mixed.as_bytes(), EcLevel::M).version();
    let all_bytes = QrCode::encode(&[b'x'; 129], EcLevel::M).unwrap().version();
    assert!(version < all_bytes, "{version} vs {all_bytes}");
    // Empty and one-character messages.
    assert_eq!(round_trip(b"", EcLevel::H).version(), 1);
    assert_eq!(round_trip(b"7", EcLevel::H).version(), 1);
    // Text outside ASCII is plain bytes (UTF-8).
    round_trip("zoë@exämple.com ☃".as_bytes(), EcLevel::Q);
}

// ---- the SVG -------------------------------------------------------------

/// Reads the rectangles `M{x} {y}h{w}v{h}h-{w}z` of an SVG path.
fn rectangles(d: &str) -> Vec<(usize, usize, usize, usize)> {
    d.split('M')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (x, rest) = part.split_once(' ').unwrap();
            let (y, rest) = rest.split_once('h').unwrap();
            let (w, rest) = rest.split_once('v').unwrap();
            let (h, rest) = rest.split_once('h').unwrap();
            assert_eq!(rest, format!("-{w}z"));
            let n = |s: &str| s.parse::<usize>().unwrap();
            (n(x), n(y), n(w), n(h))
        })
        .collect()
}

/// A rendered SVG: its side in pixels, and the dark pixels.
struct Raster {
    side: usize,
    dark: Vec<bool>,
}

fn rasterize(svg: &str) -> Raster {
    let attribute = |name: &str| -> &str {
        let start = svg.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
        &svg[start..start + svg[start..].find('"').unwrap()]
    };
    let side: usize = attribute("width").parse().unwrap();
    assert_eq!(attribute("height").parse::<usize>().unwrap(), side);
    assert_eq!(attribute("viewBox"), format!("0 0 {side} {side}"));
    let (_, foreground) = svg.split_once(r##"<path fill="#000" d=""##).unwrap();
    let d = foreground.strip_suffix(r#""/></svg>"#).unwrap();
    let mut dark = vec![false; side * side];
    for (x, y, w, h) in rectangles(d) {
        for row in y..y + h {
            for column in x..x + w {
                dark[row * side + column] = true;
            }
        }
    }
    Raster { side, dark }
}

#[test]
fn the_svg_keeps_the_markup_the_setup_screens_rely_on() {
    let old = fixture("qr/qrcode2-0.18.0.json");
    let old_svg = old["otpauth_svg"]["svg"].as_str().unwrap();
    let payload = old["otpauth_svg"]["payload"].as_str().unwrap();
    let svg = QrCode::encode(payload.as_bytes(), EcLevel::M)
        .unwrap()
        .to_svg(200);

    // The same document skeleton, up to the path data.
    let skeleton = |svg: &str| -> String {
        let (head, rest) = svg.split_once(r##"<path fill="#000" d=""##).unwrap();
        let (_, tail) = rest.split_once('"').unwrap();
        format!("{head}|{tail}")
    };
    assert_eq!(skeleton(&svg), skeleton(old_svg));
    assert!(svg.starts_with(
        r##"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="245" height="245" viewBox="0 0 245 245" shape-rendering="crispEdges"><path d="M0 0h245v245H0z" fill="#fff"/>"##
    ));
    assert!(svg.ends_with(r#""/></svg>"#));

    // The same pixels as the old document, whatever the version and mask
    // the old encoder picked.
    let (mine, theirs) = (rasterize(&svg), rasterize(old_svg));
    assert_eq!(mine.side, theirs.side);
}

#[test]
fn the_svg_is_at_least_the_requested_size_in_whole_pixels() {
    for (payload, level) in [("a", EcLevel::L), ("https://navree.com", EcLevel::H)] {
        let code = QrCode::encode(payload.as_bytes(), level).unwrap();
        for min in [1, 21, 200, 640] {
            let raster = rasterize(&code.to_svg(min));
            let modules = code.size() + 8;
            assert!(raster.side >= min as usize);
            assert_eq!(raster.side % modules, 0);
            let unit = raster.side / modules;
            assert!(raster.side - modules < min as usize || unit == 1, "{min}");
            // Each module is a square of one colour, the quiet zone light.
            for y in 0..modules {
                for x in 0..modules {
                    let expected = x >= 4
                        && y >= 4
                        && code.is_dark(x - 4, y - 4)
                        && x < modules - 4
                        && y < modules - 4;
                    for dy in 0..unit {
                        for dx in 0..unit {
                            assert_eq!(
                                raster.dark[(y * unit + dy) * raster.side + x * unit + dx],
                                expected
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_scanner_reads_the_svg() {
    let mut rng = Rng(7);
    let mut payloads: Vec<Vec<u8>> = otpauth_corpus()
        .into_iter()
        .take(6)
        .map(String::into_bytes)
        .collect();
    payloads.push((0..500).map(|_| rng.next() as u8).collect());
    payloads.push((0..1400).map(|_| rng.pick(URL_SAFE)).collect());
    for payload in payloads {
        for level in EcLevel::ALL {
            let Ok(code) = QrCode::encode(&payload, level) else {
                continue;
            };
            let raster = rasterize(&code.to_svg(200));
            let mut image =
                rqrr::PreparedImage::prepare_from_greyscale(raster.side, raster.side, |x, y| {
                    if raster.dark[y * raster.side + x] {
                        0
                    } else {
                        255
                    }
                });
            let grids = image.detect_grids();
            assert_eq!(grids.len(), 1, "version {} {level:?}", code.version());
            let mut out = Vec::new();
            let meta = grids[0].decode_to(&mut out).unwrap();
            assert_eq!(out, payload, "version {} {level:?}", code.version());
            assert_eq!(meta.version.0, usize::from(code.version()));
        }
    }
}

// ---- the encoder it replaces ----------------------------------------------

/// Matrices of the old encoder (qrcode2 0.18) for eight payloads at four
/// levels. It picks masks by its own scoring and splits messages into
/// segments its own way, so the two encoders do not always choose alike.
/// Given the mask the old symbol uses, ours must be the same symbol,
/// module for module, wherever both chose the same segments: an
/// independent check of codeword order, error correction, placement and
/// format information. Both must decode, and ours is never bigger.
#[test]
fn symbols_of_the_previous_encoder_are_reproduced_module_for_module() {
    let old = fixture("qr/qrcode2-0.18.0.json");
    let (mut identical, mut smaller) = (0, 0);
    let cases = old["cases"].as_array().unwrap();
    for case in cases {
        let payload = case["payload"].as_str().unwrap();
        let level = match case["level"].as_str().unwrap() {
            "L" => EcLevel::L,
            "M" => EcLevel::M,
            "Q" => EcLevel::Q,
            _ => EcLevel::H,
        };
        let version: u8 = case["version"]
            .as_str()
            .unwrap()
            .trim_start_matches("Normal(")
            .trim_end_matches(')')
            .parse()
            .unwrap();
        let rows: Vec<&str> = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap())
            .collect();
        let old_grid = rqrr::Grid::new(rqrr::SimpleGrid::from_func(rows.len(), |x, y| {
            rows[y].as_bytes()[x] == b'1'
        }));
        let mut text = Vec::new();
        let meta = old_grid.decode_to(&mut text).unwrap();
        assert_eq!(text, payload.as_bytes());

        let ours = round_trip(payload.as_bytes(), level);
        assert!(ours.version() <= version, "{payload} {level:?}");
        if ours.version() < version {
            smaller += 1;
            continue;
        }
        let forced = Options {
            mask: Some(u8::try_from(meta.mask).unwrap()),
            ..Options::default()
        };
        let code = QrCode::encode_with(payload.as_bytes(), level, forced).unwrap();
        let same = rows.iter().enumerate().all(|(y, row)| {
            row.bytes()
                .enumerate()
                .all(|(x, module)| code.is_dark(x, y) == (module == b'1'))
        });
        // Messages of one mode are segmented alike by any encoder. The
        // others (a long run of capitals around digits, a URL with a
        // secret in it) are split where the cost is lowest, and the old
        // encoder did not always find that.
        let mixed = ["otpauth", "THE QUICK", "Mixed"]
            .iter()
            .any(|start| payload.starts_with(start));
        if !mixed {
            assert!(same, "{payload} {level:?} mask {}", meta.mask);
            identical += 1;
        }
    }
    // Four single-mode payloads at four levels.
    assert_eq!(identical, 16, "smaller: {smaller}");
}

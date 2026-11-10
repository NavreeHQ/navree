//! `navree_crypto::qr`: encode, then decode with an independent decoder
//! (`rqrr`).
//!
//! Input: level, version range and mask in the first four bytes, the payload
//! after them. Invariant: whatever the encoder accepts, `rqrr` reads back
//! the payload, the version, the level and the mask the encoder reports.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_crypto::qr::{EcLevel, Options, QrCode, QrError};

/// The two bits that rqrr reports for a level.
fn format_bits(level: EcLevel) -> u16 {
    match level {
        EcLevel::L => 1,
        EcLevel::M => 0,
        EcLevel::Q => 3,
        EcLevel::H => 2,
    }
}

fuzz_target!(|data: &[u8]| {
    let [level, min, max, mask, payload @ ..] = data else {
        return;
    };
    let level = EcLevel::ALL[usize::from(*level) % 4];
    let options = Options {
        min_version: *min % 42,
        max_version: *max % 42,
        // Most inputs choose the mask by penalty, as callers do.
        mask: (*mask % 10 < 8).then_some(*mask % 10),
    };
    let code = match QrCode::encode_with(payload, level, options) {
        Ok(code) => code,
        Err(QrError::InvalidOptions) => {
            let bad = !(1..=40).contains(&options.min_version)
                || !(1..=40).contains(&options.max_version)
                || options.min_version > options.max_version
                || options.mask.is_some_and(|mask| mask > 7);
            assert!(bad, "valid options were refused");
            return;
        }
        Err(QrError::TooLong) => return,
        Err(_) => return,
    };
    assert!((options.min_version..=options.max_version).contains(&code.version()));
    assert_eq!(code.size(), 4 * usize::from(code.version()) + 17);
    if let Some(mask) = options.mask {
        assert_eq!(code.mask(), mask);
    }
    let svg = code.to_svg(u32::from(payload.first().copied().unwrap_or(0)));
    assert!(svg.starts_with("<svg") || svg.starts_with("<?xml"));
    let grid = rqrr::Grid::new(rqrr::SimpleGrid::from_func(code.size(), |x, y| {
        code.is_dark(x, y)
    }));
    let mut decoded = Vec::new();
    let meta = grid.decode_to(&mut decoded).unwrap_or_else(|error| {
        panic!(
            "rqrr cannot decode version {} {:?} mask {} ({} bytes): {error:?}",
            code.version(),
            code.ec_level(),
            code.mask(),
            payload.len()
        )
    });
    assert_eq!(decoded, payload, "the payload round-trips");
    assert_eq!(meta.version.0, usize::from(code.version()));
    assert_eq!(meta.ecc_level, format_bits(code.ec_level()));
    assert_eq!(meta.mask, u16::from(code.mask()));
});

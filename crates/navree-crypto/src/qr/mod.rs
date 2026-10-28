//! A QR Code encoder (ISO/IEC 18004, Model 2) with SVG output.
//!
//! [`QrCode::encode`] writes a message as a symbol of the smallest version
//! (1 to 40) that holds it at an error-correction level ([`EcLevel`]: L, M,
//! Q or H, recovering about 7%, 15%, 25% or 30% of the codewords).
//!
//! - **Modes.** The message is split into numeric, alphanumeric and byte
//!   segments, whichever take the fewest bits (`8675309` is far shorter as
//!   digits than as bytes, and an `otpauth://` URL is mostly bytes). There is
//!   no Kanji mode, and no ECI header: bytes are bytes, which readers take
//!   as UTF-8 or Latin-1 as they are set up.
//! - **Mask.** All eight mask patterns are tried and the one with the lowest
//!   penalty by the standard's four rules (runs, 2x2 blocks, finder-like
//!   patterns and the balance of dark modules) wins; a tie goes to the
//!   lowest mask number.
//! - **Output.** [`QrCode::to_svg`] draws the symbol with its four-module
//!   quiet zone as a standalone SVG document; [`QrCode::is_dark`] reads the
//!   modules for any other renderer.
//!
//! This lives in `navree-crypto` because it carries secrets to the user's
//! authenticator app (the two-factor setup of `navree-auth`), so a bug in
//! it is a security matter that is fixed here and not waited for.
//!
//! ```
//! use navree_crypto::qr::{EcLevel, QrCode};
//!
//! let code = QrCode::encode(b"otpauth://totp/Navree:ada?secret=JBSWY3DPEHPK3PXP", EcLevel::M)?;
//! assert_eq!(code.version(), 4);
//! assert!(code.to_svg(200).starts_with("<?xml"));
//! # Ok::<(), navree_crypto::qr::QrError>(())
//! ```

mod matrix;
mod reed_solomon;
mod segment;
mod svg;
mod tables;

use matrix::Matrix;
use segment::{Segment, segment};
use tables::{block_count, data_codewords, ecc_per_block, total_codewords};

/// The pad codewords that fill the data capacity (§7.4.10).
const PAD: [u8; 2] = [0xEC, 0x11];

/// The most characters any symbol holds (digits at version 40-L). A longer
/// message is refused before it is segmented.
const MAX_CHARS: usize = 7089;

/// How much of a symbol may be damaged and still be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EcLevel {
    /// About 7% of the codewords.
    L,
    /// About 15%.
    M,
    /// About 25%.
    Q,
    /// About 30%.
    H,
}

impl EcLevel {
    /// The four levels, weakest first.
    pub const ALL: [EcLevel; 4] = [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H];
}

/// Why a message could not be encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum QrError {
    /// The message does not fit a symbol of the allowed versions at this
    /// level (version 40 holds up to 2953 bytes at L and 1273 at H).
    #[error("the message is too long for a QR code at this error-correction level")]
    TooLong,
    /// The version range or the mask is outside what the standard defines.
    #[error("the version must be 1 to 40 and the mask 0 to 7")]
    InvalidOptions,
}

/// Choices beyond the level, for [`QrCode::encode_with`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The smallest version to use (1 to 40).
    pub min_version: u8,
    /// The largest version to use (1 to 40).
    pub max_version: u8,
    /// A mask pattern (0 to 7) to use instead of the one with the lowest
    /// penalty.
    pub mask: Option<u8>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min_version: 1,
            max_version: 40,
            mask: None,
        }
    }
}

/// An encoded QR code: a square of dark and light modules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrCode {
    version: u8,
    level: EcLevel,
    mask: u8,
    size: usize,
    modules: Vec<bool>,
}

impl QrCode {
    /// Encodes `data` at `level` in the smallest symbol that holds it.
    /// [`QrError::TooLong`] when even version 40 is too small.
    pub fn encode(data: &[u8], level: EcLevel) -> Result<Self, QrError> {
        Self::encode_with(data, level, Options::default())
    }

    /// Like [`encode`](QrCode::encode), with a version range and optionally
    /// a fixed mask. The smallest version in the range that holds `data` is
    /// used; [`QrError::TooLong`] when `max_version` is too small.
    pub fn encode_with(data: &[u8], level: EcLevel, options: Options) -> Result<Self, QrError> {
        let (min, max) = (
            usize::from(options.min_version),
            usize::from(options.max_version),
        );
        if !(1..=40).contains(&min)
            || !(1..=40).contains(&max)
            || min > max
            || options.mask.is_some_and(|mask| mask > 7)
        {
            return Err(QrError::InvalidOptions);
        }
        if data.len() > MAX_CHARS {
            return Err(QrError::TooLong);
        }
        // Segmentations depend on the width of the count indicators, which
        // change at versions 10 and 27.
        let mut by_class: [Option<Vec<Segment>>; 3] = [None, None, None];
        for version in min..=max {
            let class = match version {
                1..=9 => 0,
                10..=26 => 1,
                _ => 2,
            };
            let segments = by_class[class].get_or_insert_with(|| segment(data, version));
            let Some(bits) = segments
                .iter()
                .map(|segment| segment.total_bits(version))
                .sum::<Option<usize>>()
            else {
                continue;
            };
            if bits > data_codewords(version, level) * 8 {
                continue;
            }
            let stream = segment::write(segments, version);
            let codewords = interleave(&pad(stream, version, level), version, level);
            return Ok(Self::build(version, level, &codewords, options.mask));
        }
        Err(QrError::TooLong)
    }

    fn build(version: usize, level: EcLevel, codewords: &[u8], forced: Option<u8>) -> Self {
        let mut matrix = Matrix::new(version, level, codewords);
        let mask = match forced {
            Some(mask) => mask,
            None => {
                let mut best = (usize::MAX, 0_u8);
                for mask in 0..8_u8 {
                    matrix.apply_mask(mask);
                    matrix.draw_format(level, mask);
                    let penalty = matrix.penalty();
                    matrix.apply_mask(mask);
                    if penalty < best.0 {
                        best = (penalty, mask);
                    }
                }
                best.1
            }
        };
        matrix.apply_mask(mask);
        matrix.draw_format(level, mask);
        Self {
            // 1 to 40: checked by the caller.
            version: u8::try_from(version).unwrap_or(40),
            level,
            mask,
            size: matrix.size(),
            modules: matrix.into_modules(),
        }
    }

    /// The version, 1 to 40.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// The error-correction level.
    pub fn ec_level(&self) -> EcLevel {
        self.level
    }

    /// The mask pattern, 0 to 7.
    pub fn mask(&self) -> u8 {
        self.mask
    }

    /// Modules on a side: `4 * version + 17`.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Whether the module in column `x` and row `y` (from the top left) is
    /// dark. `false` outside the symbol, as the quiet zone is light.
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }

    /// The symbol as an SVG document at least `min_size` pixels wide and
    /// high, with a quiet zone of four modules. See the module's output
    /// notes: dark modules are black on a white background, in whole
    /// pixels, with `shape-rendering="crispEdges"`.
    pub fn to_svg(&self, min_size: u32) -> String {
        svg::render(self, min_size)
    }
}

/// Terminates and pads the bit stream to the data capacity: up to four
/// zero bits, zeros to a whole byte, then the pad codewords 0xEC and 0x11
/// in turn.
fn pad(mut bits: Vec<bool>, version: usize, level: EcLevel) -> Vec<u8> {
    let capacity = data_codewords(version, level);
    let terminator = (capacity * 8 - bits.len()).min(4);
    bits.extend(std::iter::repeat_n(false, terminator));
    bits.extend(std::iter::repeat_n(false, bits.len().wrapping_neg() % 8));
    let mut data: Vec<u8> = bits
        .chunks(8)
        .map(|byte| byte.iter().fold(0_u8, |sum, bit| sum << 1 | u8::from(*bit)))
        .collect();
    let mut pads = PAD.iter().cycle();
    while data.len() < capacity {
        data.push(*pads.next().unwrap_or(&PAD[0]));
    }
    data
}

/// Splits `data` into error-correction blocks, appends each block's
/// Reed-Solomon codewords and interleaves them (§7.6): the data codewords
/// of the blocks column by column, then the error-correction codewords.
fn interleave(data: &[u8], version: usize, level: EcLevel) -> Vec<u8> {
    let blocks = block_count(version, level);
    let ecc_len = ecc_per_block(version, level);
    let total = total_codewords(version);
    let short_blocks = blocks - total % blocks;
    let short_len = total / blocks;
    let divisor = reed_solomon::divisor(ecc_len);
    let mut data_blocks: Vec<&[u8]> = Vec::with_capacity(blocks);
    let mut ecc_blocks: Vec<Vec<u8>> = Vec::with_capacity(blocks);
    let mut start = 0;
    for i in 0..blocks {
        let len = short_len - ecc_len + usize::from(i >= short_blocks);
        let block = &data[start..start + len];
        start += len;
        ecc_blocks.push(reed_solomon::remainder(block, &divisor));
        data_blocks.push(block);
    }
    let longest = data_blocks
        .iter()
        .map(|block| block.len())
        .max()
        .unwrap_or(0);
    let mut result = Vec::with_capacity(total);
    for column in 0..longest {
        for block in &data_blocks {
            if let Some(byte) = block.get(column) {
                result.push(*byte);
            }
        }
    }
    for column in 0..ecc_len {
        for block in &ecc_blocks {
            result.push(block[column]);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::tables::{raw_data_modules, total_codewords};
    use super::*;

    #[test]
    fn data_capacities_match_the_standard() {
        // Data bits at L, M, Q, H for versions 1 to 40 (ISO/IEC 18004
        // Table 7, "number of data bits").
        const DATA_BITS: [[usize; 4]; 40] = [
            [152, 128, 104, 72],
            [272, 224, 176, 128],
            [440, 352, 272, 208],
            [640, 512, 384, 288],
            [864, 688, 496, 368],
            [1088, 864, 608, 480],
            [1248, 992, 704, 528],
            [1552, 1232, 880, 688],
            [1856, 1456, 1056, 800],
            [2192, 1728, 1232, 976],
            [2592, 2032, 1440, 1120],
            [2960, 2320, 1648, 1264],
            [3424, 2672, 1952, 1440],
            [3688, 2920, 2088, 1576],
            [4184, 3320, 2360, 1784],
            [4712, 3624, 2600, 2024],
            [5176, 4056, 2936, 2264],
            [5768, 4504, 3176, 2504],
            [6360, 5016, 3560, 2728],
            [6888, 5352, 3880, 3080],
            [7456, 5712, 4096, 3248],
            [8048, 6256, 4544, 3536],
            [8752, 6880, 4912, 3712],
            [9392, 7312, 5312, 4112],
            [10208, 8000, 5744, 4304],
            [10960, 8496, 6032, 4768],
            [11744, 9024, 6464, 5024],
            [12248, 9544, 6968, 5288],
            [13048, 10136, 7288, 5608],
            [13880, 10984, 7880, 5960],
            [14744, 11640, 8264, 6344],
            [15640, 12328, 8920, 6760],
            [16568, 13048, 9368, 7208],
            [17528, 13800, 9848, 7688],
            [18448, 14496, 10288, 7888],
            [19472, 15312, 10832, 8432],
            [20528, 15936, 11408, 8768],
            [21616, 16816, 12016, 9136],
            [22496, 17728, 12656, 9776],
            [23648, 18672, 13328, 10208],
        ];
        for (index, row) in DATA_BITS.iter().enumerate() {
            for (level, bits) in EcLevel::ALL.into_iter().zip(row) {
                assert_eq!(
                    data_codewords(index + 1, level) * 8,
                    *bits,
                    "version {} level {level:?}",
                    index + 1
                );
            }
        }
        // The best-known corners, in codewords.
        assert_eq!(data_codewords(1, EcLevel::L), 19);
        assert_eq!(data_codewords(10, EcLevel::H), 122);
        assert_eq!(data_codewords(40, EcLevel::L), 2956);
        assert_eq!(data_codewords(40, EcLevel::H), 1276);
    }

    #[test]
    fn character_capacities_match_the_standard() {
        // Characters in the largest and smallest symbols (Table 7).
        for (level, numeric, alphanumeric, bytes) in [
            (EcLevel::L, 7089, 4296, 2953),
            (EcLevel::M, 5596, 3391, 2331),
            (EcLevel::Q, 3993, 2420, 1663),
            (EcLevel::H, 3057, 1852, 1273),
        ] {
            for (text, count, fits) in [
                (b'7', numeric, true),
                (b'7', numeric + 1, false),
                (b'A', alphanumeric, true),
                (b'A', alphanumeric + 1, false),
                (b'a', bytes, true),
                (b'a', bytes + 1, false),
            ] {
                let message = vec![text; count];
                assert_eq!(
                    QrCode::encode(&message, level).is_ok(),
                    fits,
                    "{level:?} {} x {count}",
                    char::from(text)
                );
            }
        }
        for (level, numeric, alphanumeric, bytes) in [
            (EcLevel::L, 41, 25, 17),
            (EcLevel::M, 34, 20, 14),
            (EcLevel::Q, 27, 16, 11),
            (EcLevel::H, 17, 10, 7),
        ] {
            for (text, count) in [(b'7', numeric), (b'A', alphanumeric), (b'a', bytes)] {
                let one = QrCode::encode(&vec![text; count], level).unwrap();
                assert_eq!(one.version(), 1, "{level:?}");
                let two = QrCode::encode(&vec![text; count + 1], level).unwrap();
                assert_eq!(two.version(), 2, "{level:?}");
            }
        }
    }

    #[test]
    fn codeword_totals_match_the_standard() {
        for (version, total) in [
            (1, 26),
            (2, 44),
            (7, 196),
            (10, 346),
            (21, 1156),
            (40, 3706),
        ] {
            assert_eq!(total_codewords(version), total, "version {version}");
        }
        // Remainder bits after the codewords (Table 1).
        for (version, remainder) in [
            (1, 0),
            (2, 7),
            (6, 7),
            (7, 0),
            (14, 3),
            (21, 4),
            (28, 3),
            (35, 0),
        ] {
            assert_eq!(
                raw_data_modules(version) % 8,
                remainder,
                "version {version}"
            );
        }
    }

    #[test]
    fn every_block_layout_adds_up() {
        for version in 1..=40 {
            for level in EcLevel::ALL {
                let blocks = block_count(version, level);
                let ecc = ecc_per_block(version, level);
                let data = data_codewords(version, level);
                assert_eq!(data + blocks * ecc, total_codewords(version));
                // Block sizes differ by at most one codeword, and the
                // data of a block is more than its error correction... or
                // at least not empty.
                assert!(data / blocks >= 1, "{version}-{level:?}");
            }
        }
    }

    #[test]
    fn format_and_version_information_match_the_standard() {
        // Annex C: level L mask 0, M mask 0, Q mask 0, H mask 0; and
        // L/M/Q/H with mask 7.
        use matrix::{format_bits, version_bits};
        assert_eq!(format_bits(EcLevel::L, 0), 0b111011111000100);
        assert_eq!(format_bits(EcLevel::M, 0), 0b101010000010010);
        assert_eq!(format_bits(EcLevel::Q, 0), 0b011010101011111);
        assert_eq!(format_bits(EcLevel::H, 0), 0b001011010001001);
        assert_eq!(format_bits(EcLevel::M, 5), 0b100000011001110);
        // Annex D: versions 7 and 40.
        assert_eq!(version_bits(7), 0b000111110010010100);
        assert_eq!(version_bits(40), 0b101000110001101001);
    }

    #[test]
    fn alignment_pattern_positions_match_the_standard() {
        use tables::alignment_positions;
        assert_eq!(alignment_positions(1), Vec::<usize>::new());
        assert_eq!(alignment_positions(2), [6, 18]);
        assert_eq!(alignment_positions(7), [6, 22, 38]);
        assert_eq!(alignment_positions(14), [6, 26, 46, 66]);
        assert_eq!(alignment_positions(32), [6, 34, 60, 86, 112, 138]);
        assert_eq!(alignment_positions(36), [6, 24, 50, 76, 102, 128, 154]);
        assert_eq!(alignment_positions(40), [6, 30, 58, 86, 114, 142, 170]);
    }

    #[test]
    fn the_standards_numeric_example_gets_its_codewords() {
        // ISO/IEC 18004 Annex I: "01234567" in version 1-M. Data codewords
        // 16 32 12 86 97 128 then the pad codewords, and ten
        // error-correction codewords.
        let segments = segment("01234567".as_bytes(), 1);
        assert_eq!(segments.len(), 1);
        let data = pad(segment::write(&segments, 1), 1, EcLevel::M);
        assert_eq!(
            data,
            [
                16, 32, 12, 86, 97, 128, 236, 17, 236, 17, 236, 17, 236, 17, 236, 17
            ]
        );
        let all = interleave(&data, 1, EcLevel::M);
        assert_eq!(all[16..], [165, 36, 212, 193, 237, 54, 199, 135, 44, 85]);
    }

    #[test]
    fn hello_world_gets_the_widely_published_codewords() {
        // The "HELLO WORLD" example of version 1-Q (alphanumeric).
        let segments = segment(b"HELLO WORLD", 1);
        let data = pad(segment::write(&segments, 1), 1, EcLevel::Q);
        assert_eq!(
            data,
            [32, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236]
        );
        let all = interleave(&data, 1, EcLevel::Q);
        assert_eq!(
            all[13..],
            [168, 72, 22, 82, 217, 54, 156, 0, 46, 15, 180, 122, 16]
        );
    }

    #[test]
    fn the_mask_with_the_lowest_penalty_wins() {
        let payload = b"otpauth://totp/Navree:ada%40example.com?secret=RJLR4GJ3YLMHQFDBJ66WK6OIKGY562ER&issuer=Navree";
        for level in EcLevel::ALL {
            let code = QrCode::encode(payload, level).unwrap();
            let version = usize::from(code.version());
            let segments = segment(payload, version);
            let data = pad(segment::write(&segments, version), version, level);
            let codewords = interleave(&data, version, level);
            let penalties: Vec<usize> = (0..8)
                .map(|mask| {
                    let mut matrix = Matrix::new(version, level, &codewords);
                    matrix.apply_mask(mask);
                    matrix.draw_format(level, mask);
                    matrix.penalty()
                })
                .collect();
            let (best, lowest) = penalties
                .iter()
                .enumerate()
                .min_by_key(|(mask, penalty)| (**penalty, *mask))
                .unwrap();
            assert_eq!(usize::from(code.mask()), best, "{level:?} {penalties:?}");
            assert!(penalties.iter().any(|penalty| penalty > lowest));
        }
    }

    #[test]
    fn too_long_and_invalid_requests_fail() {
        let long = vec![b'x'; 2954];
        assert_eq!(QrCode::encode(&long, EcLevel::L), Err(QrError::TooLong));
        for level in EcLevel::ALL {
            assert_eq!(
                QrCode::encode(&vec![b'7'; 7090], level),
                Err(QrError::TooLong)
            );
            assert_eq!(
                QrCode::encode(&vec![0; 1 << 20], level),
                Err(QrError::TooLong)
            );
        }
        assert!(QrCode::encode(&long[..2953], EcLevel::L).is_ok());
        for options in [
            Options {
                min_version: 0,
                ..Options::default()
            },
            Options {
                max_version: 41,
                ..Options::default()
            },
            Options {
                min_version: 5,
                max_version: 4,
                ..Options::default()
            },
            Options {
                mask: Some(8),
                ..Options::default()
            },
        ] {
            assert_eq!(
                QrCode::encode_with(b"x", EcLevel::M, options),
                Err(QrError::InvalidOptions)
            );
        }
    }
}

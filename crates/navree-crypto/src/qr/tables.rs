//! The tables of ISO/IEC 18004 that the encoder needs: error-correction
//! block layouts (Table 9) and the geometry of each version.

use super::EcLevel;

/// Error-correction codewords in each block, by level (`L`, `M`, `Q`, `H`)
/// and version 1 to 40.
const ECC_CODEWORDS_PER_BLOCK: [[u8; 40]; 4] = [
    [
        7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30,
        30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
    [
        10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28,
        28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
    ],
    [
        13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30,
        30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
    [
        17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30,
        30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
    ],
];

/// Error-correction blocks, by level and version 1 to 40.
const NUM_BLOCKS: [[u8; 40]; 4] = [
    [
        1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13,
        14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25,
    ],
    [
        1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23,
        25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49,
    ],
    [
        1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29,
        34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68,
    ],
    [
        1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35,
        37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81,
    ],
];

impl EcLevel {
    /// 0 to 3, the row of the tables above.
    fn row(self) -> usize {
        match self {
            EcLevel::L => 0,
            EcLevel::M => 1,
            EcLevel::Q => 2,
            EcLevel::H => 3,
        }
    }

    /// The two bits that name the level in the format information.
    pub(super) fn format_bits(self) -> u32 {
        match self {
            EcLevel::L => 0b01,
            EcLevel::M => 0b00,
            EcLevel::Q => 0b11,
            EcLevel::H => 0b10,
        }
    }
}

/// Modules on a side of a symbol of `version` (1 to 40).
pub(super) fn symbol_size(version: usize) -> usize {
    version * 4 + 17
}

/// Modules that can carry codewords (everything but the function
/// patterns), including the remainder bits.
pub(super) fn raw_data_modules(version: usize) -> usize {
    let mut modules = (16 * version + 128) * version + 64;
    if version >= 2 {
        let align = version / 7 + 2;
        modules -= (25 * align - 10) * align - 55;
        if version >= 7 {
            modules -= 36;
        }
    }
    modules
}

/// Codewords of a symbol, data and error correction together.
pub(super) fn total_codewords(version: usize) -> usize {
    raw_data_modules(version) / 8
}

/// Error-correction codewords in each block.
pub(super) fn ecc_per_block(version: usize, level: EcLevel) -> usize {
    usize::from(ECC_CODEWORDS_PER_BLOCK[level.row()][version - 1])
}

/// The number of error-correction blocks.
pub(super) fn block_count(version: usize, level: EcLevel) -> usize {
    usize::from(NUM_BLOCKS[level.row()][version - 1])
}

/// Data codewords of a symbol (what is left for the message).
pub(super) fn data_codewords(version: usize, level: EcLevel) -> usize {
    total_codewords(version) - ecc_per_block(version, level) * block_count(version, level)
}

/// The centres (row and column) of the alignment patterns of `version`,
/// ascending. Empty for version 1.
pub(super) fn alignment_positions(version: usize) -> Vec<usize> {
    if version == 1 {
        return Vec::new();
    }
    let count = version / 7 + 2;
    let size = symbol_size(version);
    let step = if version == 32 {
        26
    } else {
        (version * 4 + count * 2 + 1) / (count * 2 - 2) * 2
    };
    let mut positions: Vec<usize> = (0..count - 1).map(|i| size - 7 - i * step).collect();
    positions.push(6);
    positions.reverse();
    positions
}

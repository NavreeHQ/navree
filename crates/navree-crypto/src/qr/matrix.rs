//! The module matrix: function patterns, codeword placement, masking and
//! the choice of mask by the penalty rules of ISO/IEC 18004 §7.8.

use super::EcLevel;
use super::tables::{alignment_positions, symbol_size};

/// Penalty weights (Table 11).
const N1: usize = 3;
const N2: usize = 3;
const N3: usize = 40;
const N4: usize = 10;

/// A square of dark and light modules under construction.
pub(super) struct Matrix {
    size: usize,
    modules: Vec<bool>,
    /// Modules that belong to a function pattern: not data, never masked.
    function: Vec<bool>,
}

impl Matrix {
    /// A symbol of `version` with its function patterns drawn (format
    /// information as zeros) and `codewords` placed.
    pub(super) fn new(version: usize, level: EcLevel, codewords: &[u8]) -> Self {
        let size = symbol_size(version);
        let mut matrix = Self {
            size,
            modules: vec![false; size * size],
            function: vec![false; size * size],
        };
        matrix.draw_function_patterns(version, level);
        matrix.place(codewords);
        matrix
    }

    pub(super) fn size(&self) -> usize {
        self.size
    }

    pub(super) fn into_modules(self) -> Vec<bool> {
        self.modules
    }

    pub(super) fn get(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.size + x]
    }

    fn set_function(&mut self, x: usize, y: usize, dark: bool) {
        self.modules[y * self.size + x] = dark;
        self.function[y * self.size + x] = true;
    }

    fn draw_function_patterns(&mut self, version: usize, level: EcLevel) {
        let size = self.size;
        // Timing patterns.
        for i in 0..size {
            self.set_function(6, i, i % 2 == 0);
            self.set_function(i, 6, i % 2 == 0);
        }
        // Finder patterns with their separators.
        self.draw_finder(3, 3);
        self.draw_finder(size - 4, 3);
        self.draw_finder(3, size - 4);
        // Alignment patterns, except where a finder pattern is.
        let positions = alignment_positions(version);
        let last = positions.len().saturating_sub(1);
        for (i, &cy) in positions.iter().enumerate() {
            for (j, &cx) in positions.iter().enumerate() {
                let on_finder =
                    (i == 0 && j == 0) || (i == 0 && j == last) || (i == last && j == 0);
                if !on_finder {
                    self.draw_alignment(cx, cy);
                }
            }
        }
        self.draw_format(level, 0);
        self.draw_version(version);
    }

    fn draw_finder(&mut self, cx: usize, cy: usize) {
        for dy in -4_i32..=4 {
            for dx in -4_i32..=4 {
                let distance = dx.abs().max(dy.abs());
                let (Some(x), Some(y)) = (
                    cx.checked_add_signed(dx as isize),
                    cy.checked_add_signed(dy as isize),
                ) else {
                    continue;
                };
                if x < self.size && y < self.size {
                    self.set_function(x, y, distance != 2 && distance != 4);
                }
            }
        }
    }

    fn draw_alignment(&mut self, cx: usize, cy: usize) {
        for dy in -2_i32..=2 {
            for dx in -2_i32..=2 {
                let dark = dx.abs().max(dy.abs()) != 1;
                self.set_function(
                    cx.wrapping_add_signed(dx as isize),
                    cy.wrapping_add_signed(dy as isize),
                    dark,
                );
            }
        }
    }

    /// Writes the 15 format bits (level and mask, with BCH(15,5) error
    /// correction, XORed with 101010000010010) in both places, and the dark
    /// module.
    pub(super) fn draw_format(&mut self, level: EcLevel, mask: u8) {
        let size = self.size;
        let bits = format_bits(level, mask);
        let bit = |i: u32| (bits >> i) & 1 == 1;
        for i in 0..6 {
            self.set_function(8, i as usize, bit(i));
        }
        self.set_function(8, 7, bit(6));
        self.set_function(8, 8, bit(7));
        self.set_function(7, 8, bit(8));
        for i in 9..15 {
            self.set_function(14 - i as usize, 8, bit(i));
        }
        for i in 0..8 {
            self.set_function(size - 1 - i as usize, 8, bit(i));
        }
        for i in 8..15 {
            self.set_function(8, size - 15 + i as usize, bit(i));
        }
        // The module that is always dark.
        self.set_function(8, size - 8, true);
    }

    /// Writes the 18 version bits (BCH(18,6)) in both places, from version 7.
    fn draw_version(&mut self, version: usize) {
        if version < 7 {
            return;
        }
        let bits = version_bits(version);
        for i in 0..18 {
            let dark = (bits >> i) & 1 == 1;
            let a = self.size - 11 + i % 3;
            let b = i / 3;
            self.set_function(a, b, dark);
            self.set_function(b, a, dark);
        }
    }

    /// Places `codewords` in the zigzag of §7.7.3: two-module columns from
    /// the right, up and down in turn, skipping the vertical timing column.
    fn place(&mut self, codewords: &[u8]) {
        let size = self.size;
        let mut index = 0_usize;
        let mut right = size as isize - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vertical in 0..size {
                for j in 0..2 {
                    let x = (right - j) as usize;
                    let upward = ((right + 1) & 2) == 0;
                    let y = if upward {
                        size - 1 - vertical
                    } else {
                        vertical
                    };
                    if !self.function[y * size + x] && index < codewords.len() * 8 {
                        let byte = codewords[index >> 3];
                        self.modules[y * size + x] = (byte >> (7 - (index & 7))) & 1 == 1;
                        index += 1;
                    }
                    // Remainder bits (when `index` has run out) stay light.
                }
            }
            right -= 2;
        }
    }

    /// Flips the data modules selected by mask pattern `mask` (§7.8.2).
    /// Applying a mask twice restores the matrix.
    pub(super) fn apply_mask(&mut self, mask: u8) {
        for y in 0..self.size {
            for x in 0..self.size {
                let invert = match mask {
                    0 => (x + y) % 2 == 0,
                    1 => y % 2 == 0,
                    2 => x % 3 == 0,
                    3 => (x + y) % 3 == 0,
                    4 => (x / 3 + y / 2) % 2 == 0,
                    5 => x * y % 2 + x * y % 3 == 0,
                    6 => (x * y % 2 + x * y % 3) % 2 == 0,
                    _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
                };
                if invert && !self.function[y * self.size + x] {
                    self.modules[y * self.size + x] ^= true;
                }
            }
        }
    }

    /// The penalty score of the matrix as it is (§7.8.3): runs of five or
    /// more like modules in a row or column, 2x2 blocks, finder-like
    /// patterns with four light modules beside them (the quiet zone counts
    /// as light), and the balance of dark and light modules.
    pub(super) fn penalty(&self) -> usize {
        let size = self.size;
        let mut score = 0;
        let mut line = vec![false; size];
        for horizontal in [true, false] {
            for a in 0..size {
                for (b, slot) in line.iter_mut().enumerate() {
                    *slot = if horizontal {
                        self.get(b, a)
                    } else {
                        self.get(a, b)
                    };
                }
                score += line_penalty(&line);
            }
        }
        for y in 0..size - 1 {
            for x in 0..size - 1 {
                let color = self.get(x, y);
                if color == self.get(x + 1, y)
                    && color == self.get(x, y + 1)
                    && color == self.get(x + 1, y + 1)
                {
                    score += N2;
                }
            }
        }
        let dark = self.modules.iter().filter(|m| **m).count();
        score + balance_penalty(dark, size * size)
    }
}

/// N4: ten points for each whole step of 5% between the share of dark
/// modules and 50%. A share exactly 5% from 50% is still step 0.
fn balance_penalty(dark: usize, total: usize) -> usize {
    ((dark * 20).abs_diff(total * 10))
        .div_ceil(total)
        .saturating_sub(1)
        * N4
}

/// N1 and N3 of one row or column.
fn line_penalty(line: &[bool]) -> usize {
    let mut score = 0;
    let mut run = 1;
    for i in 1..line.len() {
        if line[i] == line[i - 1] {
            run += 1;
        } else {
            if run >= 5 {
                score += N1 + run - 5;
            }
            run = 1;
        }
    }
    if run >= 5 {
        score += N1 + run - 5;
    }
    // Dark, light, 3 dark, light, dark: 1:1:3:1:1.
    const FINDER: [bool; 7] = [true, false, true, true, true, false, true];
    let light = |range: std::ops::Range<isize>| {
        range.into_iter().all(|i| {
            usize::try_from(i)
                .ok()
                .and_then(|i| line.get(i))
                .is_none_or(|module| !module)
        })
    };
    if line.len() >= FINDER.len() {
        for start in 0..=line.len() - FINDER.len() {
            if line[start..start + FINDER.len()] == FINDER {
                let start = start as isize;
                if light(start - 4..start) {
                    score += N3;
                }
                if light(start + 7..start + 11) {
                    score += N3;
                }
            }
        }
    }
    score
}

/// The 15 format bits of `level` and `mask`.
pub(super) fn format_bits(level: EcLevel, mask: u8) -> u32 {
    let data = level.format_bits() << 3 | u32::from(mask);
    let mut remainder = data;
    for _ in 0..10 {
        remainder = (remainder << 1) ^ ((remainder >> 9) * 0x537);
    }
    (data << 10 | remainder) ^ 0x5412
}

/// The 18 version bits of `version` (7 to 40).
pub(super) fn version_bits(version: usize) -> u32 {
    let version = version as u32;
    let mut remainder = version;
    for _ in 0..12 {
        remainder = (remainder << 1) ^ ((remainder >> 11) * 0x1F25);
    }
    version << 12 | remainder
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules(text: &str) -> Vec<bool> {
        text.chars().map(|c| c == '1').collect()
    }

    #[test]
    fn runs_of_five_or_more_cost_three_plus_the_excess() {
        assert_eq!(line_penalty(&modules("0101010101")), 0);
        assert_eq!(line_penalty(&modules("0111101111")), 0);
        assert_eq!(line_penalty(&modules("0111110000")), 3);
        assert_eq!(line_penalty(&modules("0111111000")), 4);
        assert_eq!(line_penalty(&modules("11111111110")), 3 + 5);
        assert_eq!(line_penalty(&modules("0000011111")), 6);
    }

    #[test]
    fn finder_like_patterns_cost_forty_per_light_side() {
        // 1011101 with four light modules before and after: two sides.
        assert_eq!(line_penalty(&modules("000010111010000")), 80);
        // Light before only (a dark module follows within four).
        assert_eq!(line_penalty(&modules("0000101110101")), 40);
        // The quiet zone counts as light at the ends of a line.
        assert_eq!(line_penalty(&modules("1011101")), 80);
        // Dark within four modules on both sides: no penalty.
        assert_eq!(line_penalty(&modules("1010111010101")), 0);
    }

    #[test]
    fn the_balance_rule_steps_by_five_percent() {
        // 100 modules: 45% to 55% is step 0, then ten points per 5%.
        assert_eq!(balance_penalty(50, 100), 0);
        assert_eq!(balance_penalty(45, 100), 0);
        assert_eq!(balance_penalty(55, 100), 0);
        assert_eq!(balance_penalty(56, 100), 10);
        assert_eq!(balance_penalty(44, 100), 10);
        assert_eq!(balance_penalty(60, 100), 10);
        assert_eq!(balance_penalty(61, 100), 20);
        assert_eq!(balance_penalty(100, 100), 90);
        assert_eq!(balance_penalty(0, 100), 90);
    }
}

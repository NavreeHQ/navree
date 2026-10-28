//! Splitting a message into numeric, alphanumeric and byte segments, and
//! writing those segments as bits (ISO/IEC 18004 §7.3 and §7.4).

/// A mode of the data bit stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    /// Decimal digits, 10 bits for 3.
    Numeric,
    /// `0-9 A-Z space $ % * + - . / :`, 11 bits for 2.
    Alphanumeric,
    /// Any bytes, 8 bits each.
    Byte,
}

const MODES: [Mode; 3] = [Mode::Byte, Mode::Alphanumeric, Mode::Numeric];

/// The alphanumeric character set, in code order.
const ALPHANUMERIC: &[u8; 45] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

impl Mode {
    /// The four-bit mode indicator.
    fn indicator(self) -> u32 {
        match self {
            Mode::Numeric => 0b0001,
            Mode::Alphanumeric => 0b0010,
            Mode::Byte => 0b0100,
        }
    }

    /// Bits of the character count indicator at `version`.
    pub(super) fn count_bits(self, version: usize) -> usize {
        let class = match version {
            1..=9 => 0,
            10..=26 => 1,
            _ => 2,
        };
        match self {
            Mode::Numeric => [10, 12, 14][class],
            Mode::Alphanumeric => [9, 11, 13][class],
            Mode::Byte => [8, 16, 16][class],
        }
    }

    /// Whether `byte` can be written in this mode.
    fn can_encode(self, byte: u8) -> bool {
        match self {
            Mode::Numeric => byte.is_ascii_digit(),
            Mode::Alphanumeric => alphanumeric_value(byte).is_some(),
            Mode::Byte => true,
        }
    }

    /// Sixths of a bit per character, for choosing between modes.
    fn sixths_per_char(self) -> usize {
        match self {
            Mode::Numeric => 20,
            Mode::Alphanumeric => 33,
            Mode::Byte => 48,
        }
    }

    fn index(self) -> usize {
        MODES
            .iter()
            .position(|mode| *mode == self)
            .unwrap_or_default()
    }
}

fn alphanumeric_value(byte: u8) -> Option<u32> {
    ALPHANUMERIC
        .iter()
        .position(|c| *c == byte)
        .and_then(|value| u32::try_from(value).ok())
}

/// A run of the message in one mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Segment {
    pub(super) mode: Mode,
    pub(super) chars: usize,
    pub(super) bits: Vec<bool>,
}

impl Segment {
    fn new(mode: Mode, data: &[u8]) -> Self {
        let mut bits = Vec::new();
        match mode {
            Mode::Byte => {
                for byte in data {
                    push_bits(&mut bits, u32::from(*byte), 8);
                }
            }
            Mode::Numeric => {
                for group in data.chunks(3) {
                    let value = group
                        .iter()
                        .fold(0_u32, |sum, digit| sum * 10 + u32::from(digit - b'0'));
                    push_bits(&mut bits, value, group.len() * 3 + 1);
                }
            }
            Mode::Alphanumeric => {
                for pair in data.chunks(2) {
                    let values: Vec<u32> = pair
                        .iter()
                        .map(|byte| alphanumeric_value(*byte).unwrap_or_default())
                        .collect();
                    if let [first, second] = values[..] {
                        push_bits(&mut bits, first * 45 + second, 11);
                    } else {
                        push_bits(&mut bits, values[0], 6);
                    }
                }
            }
        }
        Self {
            mode,
            chars: data.len(),
            bits,
        }
    }

    /// Bits the segment takes at `version`, or `None` when its character
    /// count does not fit the count indicator.
    pub(super) fn total_bits(&self, version: usize) -> Option<usize> {
        let count_bits = self.mode.count_bits(version);
        if self.chars >> count_bits != 0 {
            return None;
        }
        Some(4 + count_bits + self.bits.len())
    }
}

/// Appends the low `count` bits of `value`, high bit first.
pub(super) fn push_bits(bits: &mut Vec<bool>, value: u32, count: usize) {
    for i in (0..count).rev() {
        bits.push((value >> i) & 1 == 1);
    }
}

/// Splits `data` into the segments that take the fewest bits at `version`.
///
/// A dynamic program over the mode of each byte: a byte may continue the
/// mode of the one before it, or start a new segment (which costs a mode
/// indicator and a character count, and rounds the last segment up to a
/// whole bit). Costs are in sixths of a bit, so that numeric (3 1/3 bits per
/// digit) and alphanumeric (5 1/2 bits) runs are weighed fairly.
pub(super) fn segment(data: &[u8], version: usize) -> Vec<Segment> {
    if data.is_empty() {
        return Vec::new();
    }
    let heads: Vec<usize> = MODES
        .iter()
        .map(|mode| (4 + mode.count_bits(version)) * 6)
        .collect();
    const NONE: usize = usize::MAX;
    // cost[i][m]: the cheapest way to write data[..=i] with data[i] in mode m.
    let mut cost = vec![[NONE; 3]; data.len()];
    // came_from[i][m]: the mode of data[i - 1] on that cheapest way.
    let mut came_from = vec![[0_usize; 3]; data.len()];
    for (i, byte) in data.iter().enumerate() {
        for mode in MODES {
            if !mode.can_encode(*byte) {
                continue;
            }
            let m = mode.index();
            if i == 0 {
                cost[0][m] = heads[m] + mode.sixths_per_char();
                continue;
            }
            for previous in MODES {
                let p = previous.index();
                if cost[i - 1][p] == NONE {
                    continue;
                }
                let total = if p == m {
                    cost[i - 1][p] + mode.sixths_per_char()
                } else {
                    cost[i - 1][p].div_ceil(6) * 6 + heads[m] + mode.sixths_per_char()
                };
                if total < cost[i][m] {
                    cost[i][m] = total;
                    came_from[i][m] = p;
                }
            }
        }
    }
    let last = data.len() - 1;
    let mut mode = (0..3)
        .min_by_key(|m| cost[last][*m])
        .unwrap_or_default();
    let mut modes = vec![0_usize; data.len()];
    for i in (0..data.len()).rev() {
        modes[i] = mode;
        mode = came_from[i][mode];
    }
    let mut segments = Vec::new();
    let mut start = 0;
    for i in 1..=data.len() {
        if i == data.len() || modes[i] != modes[start] {
            segments.push(Segment::new(MODES[modes[start]], &data[start..i]));
            start = i;
        }
    }
    segments
}

/// The bit stream of `segments` at `version`: for each, its mode indicator,
/// character count and data.
pub(super) fn write(segments: &[Segment], version: usize) -> Vec<bool> {
    let mut bits = Vec::new();
    for segment in segments {
        push_bits(&mut bits, segment.mode.indicator(), 4);
        // `chars` fits the indicator: the version was chosen with `total_bits`.
        push_bits(
            &mut bits,
            u32::try_from(segment.chars).unwrap_or_default(),
            segment.mode.count_bits(version),
        );
        bits.extend_from_slice(&segment.bits);
    }
    bits
}

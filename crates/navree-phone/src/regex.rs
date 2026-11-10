//! The matcher for the patterns compiled into `data.rs`.
//!
//! A pattern is a list of 16-bit instructions over the digits `0` to `9`
//! (written by `tools/metadata.mjs`):
//!
//! | Word | Meaning |
//! |---|---|
//! | `0x0000 \| mask` | one digit whose bit is set in the 10-bit mask |
//! | `0x1000 \| slot` | record the position in capture slot `slot` |
//! | `0x2000` | the end of the input |
//! | `0x3000` | a match |
//! | `0x8000 \| to` | continue at `to` |
//! | `0xC000 \| to` | try the next instruction, and if that fails, `to` |
//!
//! Alternatives are tried in order, so the match and its groups are the ones
//! a backtracking engine finds (leftmost first, greedy). Each pair of
//! instruction and input position is explored at most once, so the work is
//! at most the pattern's length times the input's, whatever the pattern.

/// Capturing groups a pattern may have, besides the whole match.
pub(crate) const MAX_GROUPS: usize = 7;

/// The longest input matched; callers never pass more.
pub(crate) const MAX_INPUT: usize = 64;

/// Where each group matched: `(start, end)` by group, 0 being the whole
/// match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Captures {
    slots: [Option<u8>; 2 * (MAX_GROUPS + 1)],
}

impl Captures {
    /// Group `group`'s range, when it took part in the match.
    pub(crate) fn get(&self, group: usize) -> Option<(usize, usize)> {
        let start = (*self.slots.get(2 * group)?)?;
        let end = (*self.slots.get(2 * group + 1)?)?;
        (start <= end).then_some((usize::from(start), usize::from(end)))
    }

    /// The whole match's end (its start is 0).
    pub(crate) fn end(&self) -> usize {
        self.get(0).map_or(0, |(_, end)| end)
    }

    pub(crate) fn set_whole(&mut self, end: usize) {
        self.slots[0] = Some(0);
        self.slots[1] = u8::try_from(end).ok();
    }
}

enum Frame {
    Explore(usize, usize),
    Restore(usize, Option<u8>),
}

/// Runs `code` on `input` (ASCII digits) from its start: the end of the
/// match, which reaches the end of the input when `full`. With `captures`,
/// fills in where each group matched.
pub(crate) fn run(
    code: &[u16],
    input: &[u8],
    full: bool,
    captures: Option<&mut Captures>,
) -> Option<usize> {
    if input.len() > MAX_INPUT || code.is_empty() {
        return None;
    }
    let width = input.len() + 1;
    let mut visited = vec![0_u64; (code.len() * width).div_ceil(64)];
    let mut slots: [Option<u8>; 2 * (MAX_GROUPS + 1)] = [None; 2 * (MAX_GROUPS + 1)];
    let mut stack = vec![Frame::Explore(0, 0)];
    while let Some(frame) = stack.pop() {
        let (mut pc, mut pos) = match frame {
            Frame::Restore(slot, old) => {
                if let Some(entry) = slots.get_mut(slot) {
                    *entry = old;
                }
                continue;
            }
            Frame::Explore(pc, pos) => (pc, pos),
        };
        loop {
            let bit = pc * width + pos;
            let Some(word) = visited.get_mut(bit / 64) else {
                break;
            };
            let flag = 1_u64 << (bit % 64);
            if *word & flag != 0 {
                break;
            }
            *word |= flag;
            let Some(&op) = code.get(pc) else {
                break;
            };
            match op {
                0x0000..=0x03ff => {
                    let digit = input.get(pos).and_then(|byte| byte.checked_sub(b'0'));
                    match digit {
                        Some(digit) if digit < 10 && op & (1 << digit) != 0 => {
                            pc += 1;
                            pos += 1;
                        }
                        _ => break,
                    }
                }
                0x1000..=0x1fff => {
                    let slot = usize::from(op & 0xff);
                    let Some(entry) = slots.get_mut(slot) else {
                        break;
                    };
                    stack.push(Frame::Restore(slot, *entry));
                    *entry = u8::try_from(pos).ok();
                    pc += 1;
                }
                0x2000 => {
                    if pos != input.len() {
                        break;
                    }
                    pc += 1;
                }
                0x3000 => {
                    if full && pos != input.len() {
                        break;
                    }
                    if let Some(captures) = captures {
                        captures.slots = slots;
                    }
                    return Some(pos);
                }
                0x8000..=0xbfff => pc = usize::from(op & 0x3fff),
                0xc000..=0xffff => {
                    stack.push(Frame::Explore(usize::from(op & 0x3fff), pos));
                    pc += 1;
                }
                _ => break,
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-assembled programs, in the generator's encoding.
    const ANY: u16 = 0x3ff;
    const fn digit(d: u16) -> u16 {
        1 << d
    }
    const MATCH: u16 = 0x3000;

    #[test]
    fn classes_and_full_or_prefix_matches() {
        // [2-9]\d
        let code = [0x3fc, ANY, MATCH];
        assert_eq!(run(&code, b"25", true, None), Some(2));
        assert_eq!(run(&code, b"15", true, None), None);
        assert_eq!(run(&code, b"256", true, None), None);
        assert_eq!(run(&code, b"256", false, None), Some(2));
        assert_eq!(run(&code, b"2", false, None), None);
        assert_eq!(run(&code, b"2x", false, None), None);
    }

    #[test]
    fn alternatives_are_tried_in_order() {
        // (1|12)(\d*) : group 1 takes "1" first, as a backtracking engine does.
        let code = [
            0x1002,
            0xc000 | 4,
            digit(1),
            0x8000 | 6,
            digit(1),
            digit(2),
            0x1003,
            0x1004,
            0xc000 | 11,
            ANY,
            0x8000 | 8,
            0x1005,
            MATCH,
        ];
        let mut captures = Captures::default();
        assert_eq!(run(&code, b"123", true, Some(&mut captures)), Some(3));
        assert_eq!(captures.get(1), Some((0, 1)));
        assert_eq!(captures.get(2), Some((1, 3)));
    }

    #[test]
    fn a_group_that_did_not_take_part_is_none() {
        // (1)?2 on "2"
        let code = [0xc000 | 4, 0x1002, digit(1), 0x1003, digit(2), MATCH];
        let mut captures = Captures::default();
        assert_eq!(run(&code, b"2", true, Some(&mut captures)), Some(1));
        assert_eq!(captures.get(1), None);
        assert_eq!(run(&code, b"12", true, Some(&mut captures)), Some(2));
        assert_eq!(captures.get(1), Some((0, 1)));
    }

    #[test]
    fn end_of_input() {
        // 1$ as a prefix: only "1" itself.
        let code = [digit(1), 0x2000, MATCH];
        assert_eq!(run(&code, b"1", false, None), Some(1));
        assert_eq!(run(&code, b"12", false, None), None);
    }

    #[test]
    fn nested_loops_stay_bounded() {
        // (\d*)*9 on 64 ones: exponential for a naive backtracker.
        let code = [
            0xc000 | 6,
            0xc000 | 5,
            ANY,
            0x8000 | 1,
            0x8000,
            0x8000,
            digit(9),
            MATCH,
        ];
        let input = [b'1'; MAX_INPUT];
        assert_eq!(run(&code, &input, true, None), None);
        // Longer input than any caller passes is refused, not slow.
        assert_eq!(run(&code, &[b'1'; MAX_INPUT + 1], true, None), None);
    }

    #[test]
    fn bad_programs_fail_rather_than_panic() {
        for code in [
            &[0x8000 | 50][..],
            &[0xc000 | 99, 0x4000],
            &[0x10ff, MATCH],
            &[0x0fff],
            &[],
        ] {
            let _ = run(code, b"123", true, Some(&mut Captures::default()));
        }
    }
}

//! The shape of the numbering plans in `data.rs`, and how the rest of the
//! crate reads them. The tables are generated; everything that interprets
//! them is here and in the modules that call it.

use crate::data::{CODE, CODE_TERRITORIES, CODES, DESCS, FORMATS, GEOGRAPHIC, TERRITORIES, TEXT};
use crate::regex::{self, Captures};

/// A compiled pattern: `len` instructions of `CODE` from `start`, with
/// `groups` capturing groups. `len == 0` is no pattern.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pat {
    start: u32,
    len: u16,
    groups: u8,
}

/// No pattern.
pub(crate) const NONE: Pat = Pat {
    start: 0,
    len: 0,
    groups: 0,
};

/// A pattern of the tables.
pub(crate) const fn p(start: u32, len: u16, groups: u8) -> Pat {
    Pat { start, len, groups }
}

impl Pat {
    /// Whether there is a pattern.
    pub(crate) fn exists(self) -> bool {
        self.len > 0
    }

    /// Its number of capturing groups.
    pub(crate) fn groups(self) -> usize {
        usize::from(self.groups)
    }

    fn code(self) -> &'static [u16] {
        let start = self.start as usize;
        CODE.get(start..start + usize::from(self.len))
            .unwrap_or(&[])
    }

    /// Whether the pattern matches all of `digits`.
    pub(crate) fn matches(self, digits: &[u8]) -> bool {
        self.exists() && regex::run(self.code(), digits, true, None).is_some()
    }

    /// The end of the pattern's match at the start of `digits`, if any.
    pub(crate) fn looking_at(self, digits: &[u8]) -> Option<usize> {
        if !self.exists() {
            return None;
        }
        regex::run(self.code(), digits, false, None)
    }

    /// The match at the start of `digits` (all of them when `full`), with its
    /// groups.
    pub(crate) fn captures(self, digits: &[u8], full: bool) -> Option<Captures> {
        if !self.exists() {
            return None;
        }
        let mut captures = Captures::default();
        let end = regex::run(self.code(), digits, full, Some(&mut captures))?;
        captures.set_whole(end);
        Some(captures)
    }
}

/// A string of `TEXT`: `len` bytes from `start`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Txt {
    start: u32,
    len: u16,
}

/// The empty string.
pub(crate) const EMPTY: Txt = Txt { start: 0, len: 0 };

/// A string of the tables.
pub(crate) const fn t(start: u32, len: u16) -> Txt {
    Txt { start, len }
}

impl Txt {
    /// The text.
    pub(crate) fn get(self) -> &'static str {
        let start = self.start as usize;
        TEXT.get(start..start + usize::from(self.len)).unwrap_or("")
    }
}

/// The numbers of one type in a territory: the pattern of their national
/// significant numbers, their lengths, the lengths only dialled locally,
/// and an example.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Desc {
    pub(crate) pattern: Pat,
    /// Bit `n` set: `n` digits is a possible length.
    pub(crate) lengths: u32,
    /// Bit `n` set: `n` digits is a length only dialled within an area.
    pub(crate) local: u32,
    pub(crate) example: Txt,
}

/// A description of the tables.
pub(crate) const fn d(pattern: Pat, lengths: u32, local: u32, example: Txt) -> Desc {
    Desc {
        pattern,
        lengths,
        local,
        example,
    }
}

impl Desc {
    /// Whether `nsn` has a possible length and matches the pattern.
    pub(crate) fn matches(&self, nsn: &[u8]) -> bool {
        has_length(self.lengths, nsn.len()) && self.pattern.matches(nsn)
    }
}

/// Whether bit `len` of `mask` is set.
pub(crate) fn has_length(mask: u32, len: usize) -> bool {
    u32::try_from(len)
        .ok()
        .and_then(|len| mask.checked_shr(len))
        .is_some_and(|bits| bits & 1 == 1)
}

/// A number format: the pattern a national significant number must match
/// (its groups are the parts), the leading digits that pick it, the text to
/// write (`$1 $2`) and, when it differs, the text for national use, with the
/// national prefix.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Format {
    pub(crate) pattern: Pat,
    pub(crate) leading: Pat,
    pub(crate) format: Txt,
    pub(crate) national: Txt,
}

/// A format of the tables.
pub(crate) const fn f(pattern: Pat, leading: Pat, format: Txt, national: Txt) -> Format {
    Format {
        pattern,
        leading,
        format,
        national,
    }
}

/// One territory's numbering plan.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Territory {
    /// The ISO 3166-1 code, or `00` for a non-geographic entity.
    pub(crate) id: [u8; 2],
    pub(crate) code: u16,
    pub(crate) name: Txt,
    /// The international (IDD) prefix dialled from here.
    pub(crate) idd: Pat,
    pub(crate) national_prefix: Txt,
    /// What is stripped from a national number before its significant part.
    pub(crate) npp: Pat,
    /// What replaces it, when the pattern captured something (`$1`).
    pub(crate) npt: Txt,
    pub(crate) extension_prefix: Txt,
    /// The leading digits of this territory's numbers, for codes it shares.
    pub(crate) leading: Pat,
    pub(crate) general: Desc,
    /// For each [`NumberType`](crate::NumberType) in `TYPES` order, an index
    /// plus one into `DESCS`, or 0.
    pub(crate) types: [u16; 11],
    pub(crate) formats: (u16, u16),
    pub(crate) intl_formats: (u16, u16),
    pub(crate) same_mobile_and_fixed: bool,
}

/// The descriptions of `Territory::types`, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    FixedLine,
    Mobile,
    TollFree,
    PremiumRate,
    SharedCost,
    PersonalNumber,
    Voip,
    Pager,
    Uan,
    Voicemail,
    NoInternationalDialling,
}

impl Territory {
    /// The description of numbers of `kind`, when the territory has any.
    pub(crate) fn desc(&self, kind: Kind) -> Option<&'static Desc> {
        let index = usize::from(*self.types.get(kind as usize)?);
        DESCS.get(index.checked_sub(1)?)
    }

    /// Whether `nsn` matches the description of `kind`.
    pub(crate) fn is(&self, kind: Kind, nsn: &[u8]) -> bool {
        self.desc(kind).is_some_and(|desc| desc.matches(nsn))
    }

    /// The national formats.
    pub(crate) fn formats(&self) -> &'static [Format] {
        slice(self.formats)
    }

    /// The international formats: their own when they have them.
    pub(crate) fn intl_formats(&self) -> &'static [Format] {
        if self.intl_formats.1 == 0 {
            self.formats()
        } else {
            slice(self.intl_formats)
        }
    }

    /// The id as text: `GB`, or `001` for a non-geographic entity.
    pub(crate) fn id_str(&'static self) -> &'static str {
        if self.is_geographic() {
            std::str::from_utf8(&self.id).unwrap_or("")
        } else {
            "001"
        }
    }

    /// Whether the territory is a country or area, not a global service.
    pub(crate) fn is_geographic(&self) -> bool {
        self.id != *b"00"
    }
}

fn slice((start, len): (u16, u16)) -> &'static [Format] {
    let start = usize::from(start);
    FORMATS.get(start..start + usize::from(len)).unwrap_or(&[])
}

/// The territories of calling code `code`, the main one first.
pub(crate) fn territories_for(code: u16) -> impl Iterator<Item = &'static Territory> {
    let range = CODES
        .binary_search_by(|(cc, _, _)| cc.cmp(&code))
        .ok()
        .and_then(|at| CODES.get(at))
        .map_or(0..0, |(_, start, len)| {
            usize::from(*start)..usize::from(*start) + usize::from(*len)
        });
    CODE_TERRITORIES
        .get(range)
        .unwrap_or(&[])
        .iter()
        .filter_map(|index| TERRITORIES.get(usize::from(*index)))
}

/// The territory whose metadata formats numbers of calling code `code`.
pub(crate) fn main_for(code: u16) -> Option<&'static Territory> {
    territories_for(code).next()
}

/// Whether `code` is a calling code.
pub(crate) fn is_calling_code(code: u16) -> bool {
    CODES.binary_search_by(|(cc, _, _)| cc.cmp(&code)).is_ok()
}

/// Every geographic territory, by id.
pub(crate) fn geographic() -> &'static [Territory] {
    TERRITORIES.get(..GEOGRAPHIC).unwrap_or(&[])
}

/// Every territory.
#[cfg(test)]
pub(crate) fn all() -> &'static [Territory] {
    &TERRITORIES
}

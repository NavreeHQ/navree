//! Phone numbers: parsing what people type, checking it against each
//! country's numbering plan, and writing it in E.164, international,
//! national and `tel:` form.
//!
//! ```
//! use navree_phone::{NumberType, PhoneNumber, Region};
//!
//! let de = Region::parse("DE");
//! let number = PhoneNumber::parse("030 123456", de).unwrap();
//! assert_eq!(number.as_str(), "+4930123456");
//! assert_eq!(number.international(), "+49 30 123456");
//! assert_eq!(number.number_type(), Some(NumberType::FixedLine));
//!
//! // A pasted international number needs no region, and finds its own.
//! let ca = PhoneNumber::parse("+1 416 979 5000", None).unwrap();
//! assert_eq!(ca.region(), Region::parse("CA"));
//! ```
//!
//! The numbering plans are libphonenumber's (Apache-2.0), release
//! [`PLANS_RELEASE`], compiled into tables by `tools/metadata.mjs`: the
//! parser, the matcher of their patterns and the formatter are this
//! crate's own. A plan's patterns run on a small instruction set over the
//! digits, so no regular expression is parsed at run time, and the work on
//! any input is bounded.
//!
//! With the `serde` feature, [`PhoneNumber`] serializes as its E.164 string
//! and deserializes from any international form, and [`Region`] as its
//! code.

#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod data;
mod format;
mod meta;
mod normalize;
mod number;
mod parse;
mod regex;
#[cfg(feature = "serde")]
mod serde_impls;
#[cfg(test)]
mod tests;

pub use format::PhoneFormat;
pub use number::{MAX_E164_DIGITS, NumberType, ParseError, PhoneNumber, Region, UnknownRegion};

/// The release of libphonenumber whose numbering plans are compiled in.
pub const PLANS_RELEASE: &str = data::RELEASE;

/// The country calling code of `region`, as text people dial after `+`.
///
/// ```
/// assert_eq!(navree_phone::calling_code("IN"), Some(91));
/// assert_eq!(navree_phone::calling_code("ZZ"), None);
/// ```
pub fn calling_code(region: &str) -> Option<u16> {
    Region::parse(region).map(Region::calling_code)
}

/// The regions that use calling code `code`, the one whose formats the
/// code follows first: `1` is the United States, then Canada and the rest
/// of the North American plan.
///
/// ```
/// let regions: Vec<_> = navree_phone::regions_for(44).map(|r| r.as_str()).collect();
/// assert_eq!(regions, ["GB", "GG", "IM", "JE"]);
/// ```
pub fn regions_for(code: u16) -> impl Iterator<Item = Region> {
    meta::territories_for(code).filter_map(Region::of)
}

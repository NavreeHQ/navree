//! Navree's router and extractors against references: property tests of
//! the matcher and the decoders, and differential tests against the
//! implementations they replace (the previous router's matcher and
//! extractors, `form_urlencoded`, `serde_urlencoded`, `percent-encoding`).
//!
//! Every difference a differential test accepts is named where it is
//! accepted, with the reason.

mod decoding;
mod extractors;
mod matching;

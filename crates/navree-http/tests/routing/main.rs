//! Navree's router and extractors against references: property tests of
//! the matcher and the decoders, and of the extractors against independent
//! readers of the same input (`mime`, `serde_json`, `serde_path_to_error`,
//! `form_urlencoded`, `serde_urlencoded`, `percent-encoding`).
//!
//! Every difference from reading a reference plainly is named where it is
//! accepted, with the reason.

mod decoding;
mod extractors;
mod matching;

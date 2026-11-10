//! `navree_sso`'s XML reader and Canonical XML (the bytes an XML signature
//! covers). Not a general XML parser: DTDs, external entities and
//! non-UTF-8 encodings are refused, and size, depth and node limits apply.
//!
//! Invariant: the exclusive canonical form of a document's root parses and
//! canonicalizes to itself, and no canonical form is more than a fixed
//! factor larger than its input.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = navree_sso::fuzzing::xml(text);
    }
});

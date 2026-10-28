//! Small parsers of untrusted text, one per mode (byte 0): the logbook's
//! search box (`navree_logbook::Filter`), an OAuth `scope` parameter,
//! the `X-Request-ID` header, an application key and a Web Push
//! subscription key.
//!
//! Invariants: a search that parses prints as something that parses to the
//! same terms; scopes are unique and non-empty and join back to a string
//! that reads the same; a request id is 1 to 64 characters of
//! `[A-Za-z0-9_-]`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_contracts::RequestId;
use navree_crypto::AppKey;
use navree_logbook::Filter;
use navree_oauth::{join_scopes, parse_scope};

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    match mode % 5 {
        0 => {
            if let Ok(filter) = Filter::parse(text) {
                let printed = filter.to_string();
                let again = Filter::parse(&printed)
                    .unwrap_or_else(|error| panic!("{printed:?} does not read back: {error}"));
                assert_eq!(again.to_string(), printed, "parse, print and parse differ");
                let _ = (filter.kinds(), filter.top_level_only(), filter.is_empty());
            }
        }
        1 => {
            let scopes = parse_scope(text);
            let mut unique = scopes.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), scopes.len(), "a scope is listed twice");
            assert!(
                scopes
                    .iter()
                    .all(|scope| !scope.is_empty() && scope == scope.trim())
            );
            assert_eq!(parse_scope(&join_scopes(&scopes)), scopes);
        }
        2 => {
            if let Some(id) = RequestId::parse(text) {
                assert!((1..=RequestId::MAX_LEN).contains(&id.as_str().len()));
                assert!(
                    id.as_str()
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                );
            }
            let _ = RequestId::parse_or_generate(Some(text));
        }
        3 => {
            let _ = AppKey::parse(text);
        }
        _ => {
            let _ = navree_crypto::webpush::decode_key(text);
        }
    }
});

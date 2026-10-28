//! `navree_directory::scim`: SCIM filter expressions and PATCH paths, sent
//! by an identity provider (`GET /Users?filter=...`, `PATCH` operations).
//! The filter parser is recursive, so deep nesting is the interesting
//! input.
//!
//! Invariants: a filter that parses evaluates on any resource without a
//! panic; a parsed filter is the same value when parsed again.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_directory::scim::{Filter, PatchPath};
use serde_json::{Value, json};

fn resources() -> [Value; 3] {
    [
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
            "id": "2819c223", "userName": "ada@acme.test", "active": true,
            "name": { "givenName": "Ada", "familyName": "Lovelace" },
            "emails": [{ "value": "ada@acme.test", "type": "work", "primary": true }],
            "meta": { "lastModified": "2026-10-27T12:00:00Z" },
        }),
        json!({ "userName": null, "emails": [], "active": "yes" }),
        json!([1, "two", null]),
    ]
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(filter) = Filter::parse(text) {
        for resource in resources() {
            let _ = filter.matches(&resource);
        }
        let _ = filter.simple_eq();
        let _ = format!("{filter:?}");
        assert_eq!(Filter::parse(text).as_ref(), Ok(&filter));
    }
    if let Ok(path) = PatchPath::parse(text) {
        let _ = format!("{path:?}");
        assert_eq!(PatchPath::parse(text).as_ref(), Ok(&path));
    }
});

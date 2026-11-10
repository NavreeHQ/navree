//! `navree_openapi::Schema` reading JSON.
//!
//! Invariants: every JSON object is read, and writing it back gives the same
//! JSON; anything else is refused, not a panic; a schema made nullable is read
//! back the same.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_openapi::Schema;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    match Schema::from_value(value.clone()) {
        Ok(schema) => {
            assert!(value.is_object());
            assert_eq!(schema.to_value(), value);
            let nullable = schema.nullable();
            let written = nullable.to_value();
            assert_eq!(Schema::from_value(written.clone()).unwrap().to_value(), written);
        }
        Err(_) => assert!(!value.is_object()),
    }
});

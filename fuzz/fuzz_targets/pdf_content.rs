//! `navree_pdf::content`: the operations of a content stream, read one at a
//! time as the text of a page is.
//!
//! Invariant: operations written back read as the same operations (inline
//! images aside, which are skipped, and numbers too large for a real).
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_pdf::Object;
use navree_pdf::content::{Operation, encode, operations};

fn finite(object: &Object) -> bool {
    match object {
        Object::Real(value) => value.is_finite(),
        Object::Array(items) => items.iter().all(finite),
        Object::Dictionary(dict) => dict.iter().all(|(_, value)| finite(value)),
        _ => true,
    }
}

fuzz_target!(|data: &[u8]| {
    let read: Vec<Operation> = operations(data)
        .take(10_000)
        .map_while(Result::ok)
        .filter(|op| op.operator != "BI" && op.operands.iter().all(finite))
        .collect();
    let again: Vec<Operation> = operations(&encode(&read)).map_while(Result::ok).collect();
    assert_eq!(again, read);
});

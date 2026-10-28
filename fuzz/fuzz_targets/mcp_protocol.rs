//! `navree_mcp`'s wire parsing: JSON-RPC messages and batches (with the
//! nesting limit) and the stdio line framing with its byte limit.
//!
//! Invariants: what parses nests at most `MAX_JSON_DEPTH` levels; a refusal
//! is a well-formed JSON-RPC error; framing is the input split at newlines,
//! a line over the limit is reported and never kept, and the size of the
//! reads changes nothing.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&limit, rest)) = data.split_first() else {
        return;
    };
    navree_mcp::fuzzing::parse(rest);
    // Lines of up to 1 to 256 bytes, read whole, byte by byte and in odd
    // sizes.
    navree_mcp::fuzzing::frame(rest, usize::from(limit) + 1, &[usize::MAX, 1, 3, 64]);
    // Every line of the input is a message of its own, as on stdio.
    for line in rest.split(|byte| *byte == b'\n') {
        navree_mcp::fuzzing::parse(line);
    }
});

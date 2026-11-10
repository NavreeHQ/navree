//! `navree_micro::Envelope`: a message as it arrives from a transport (NATS,
//! Kafka, Redis, ...): a length-prefixed JSON head and the payload.
//!
//! Invariant: an envelope that reads writes bytes that read back to the same
//! pattern, headers and payload.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_micro::Envelope;

fuzz_target!(|data: &[u8]| {
    let Ok(envelope) = Envelope::from_bytes(data.to_vec()) else {
        return;
    };
    // The accessors that read untrusted header values.
    let _ = envelope.message_id();
    let _ = envelope.service();
    let _ = envelope.content_type();
    let _ = envelope.key();
    let _ = envelope.attempt();
    let _ = envelope.status();
    let _ = envelope.deadline();
    let _ = envelope.remaining();
    let _ = envelope.is_expired();
    let _ = envelope.reply(200, "application/json", b"{}".to_vec());

    let again = Envelope::from_bytes(envelope.to_bytes()).expect("a written envelope reads");
    assert_eq!(again.pattern(), envelope.pattern());
    assert_eq!(again.headers(), envelope.headers());
    assert_eq!(again.payload(), envelope.payload());
});

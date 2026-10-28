//! `navree_mail::inbound`: a raw message (RFC 5322 with MIME) received from
//! a provider or an SMTP relay, and the mailbox patterns messages are routed
//! by.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_mail::inbound::{InboundMail, MailboxPattern};

fuzz_target!(|data: &[u8]| {
    if let Ok(mail) = InboundMail::parse(data.to_vec()) {
        let _ = format!("{mail:?}");
    }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = MailboxPattern::parse(text);
    }
});

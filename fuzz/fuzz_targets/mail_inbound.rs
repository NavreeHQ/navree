//! `navree_mail::inbound`: a raw message (RFC 5322 with MIME) received from
//! a provider or an SMTP relay, read by Navree's MIME reader, and the
//! mailbox patterns messages are routed by.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_mail::inbound::{InboundMail, MailboxPattern};

fuzz_target!(|data: &[u8]| {
    if let Ok(mail) = InboundMail::parse(data.to_vec()) {
        let _ = format!("{mail:?}");
        let _ = (mail.recipients(), mail.reply_text(), mail.is_auto_reply());
        // Decoding never grows a part past what its charset can make of
        // the bytes it came from (three bytes of UTF-8 for one byte).
        for attachment in &mail.attachments {
            assert!(attachment.size() <= data.len().saturating_mul(3) + 3);
            if attachment.is_message() {
                let _ = attachment.as_mail();
            }
        }
    }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = MailboxPattern::parse(text);
    }
});

//! `navree_crypto::totp`: key URIs (`otpauth://`, scanned from a QR code or
//! pasted by the user), base32 secrets and code checks.
//!
//! Invariants: a URI that reads writes a URI that reads back to the same
//! settings; a base32 secret survives a write and a read; a code the
//! generator makes is accepted at its own time.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_crypto::totp::{Secret, Totp};

fn same_settings(a: &Totp, b: &Totp) {
    assert_eq!(a.algorithm(), b.algorithm());
    assert_eq!(a.digits(), b.digits());
    assert_eq!(a.period(), b.period());
    assert_eq!(a.issuer(), b.issuer());
    assert_eq!(a.account(), b.account());
    assert_eq!(a.secret(), b.secret());
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    match mode % 3 {
        0 => {
            if let Ok(totp) = Totp::from_url(text) {
                let _ = format!("{totp:?}");
                let code = totp.generate(1_700_000_000);
                assert_eq!(code.len(), usize::from(totp.digits()));
                assert!(totp.check(&code, 1_700_000_000).is_some());
                if let Ok(url) = totp.to_url() {
                    let again = Totp::from_url(&url).expect("a written URI reads");
                    same_settings(&totp, &again);
                }
            }
        }
        1 => {
            if let Ok(secret) = Secret::from_base32(text) {
                let written = secret.to_base32();
                let again = Secret::from_base32(&written).expect("a written secret reads");
                assert_eq!(secret, again);
                assert_eq!(secret.as_bytes(), again.as_bytes());
            }
        }
        _ => {
            // Codes from the user against a fixed generator.
            let secret = Secret::from_bytes([0x42; 20]);
            let totp = Totp::builder(secret)
                .digits(8)
                .skew(2)
                .account("fuzz")
                .build()
                .expect("valid settings");
            let time = 1_700_000_000;
            let _ = totp.check(text, time);
            let _ = totp.check(text, u64::MAX);
            assert!(totp.check(&totp.generate(time), time).is_some());
        }
    }
});

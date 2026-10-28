//! TOTP: the RFC 4226 and RFC 6238 vectors, secrets, URIs and codes that
//! totp-rs produced before this module replaced it, and the edges of the
//! drift window.

use navree_crypto::totp::{Algorithm, MIN_SECRET_LEN, SECRET_LEN, Secret, Totp, TotpError};
use serde_json::Value;

fn fixture() -> Value {
    let path = format!(
        "{}/tests/fixtures/otp/totp-rs-6.0.0.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn totp(secret: &[u8], algorithm: Algorithm, digits: u8, period: u64) -> Totp {
    Totp::builder(Secret::from_bytes(secret))
        .algorithm(algorithm)
        .digits(digits)
        .period(period)
        .build()
        .unwrap()
}

// ---- RFC vectors -----------------------------------------------------------

/// RFC 6238 Appendix B: 8 digits, 30-second steps, the secret of each hash
/// as the RFC gives it (20, 32 and 64 ASCII bytes).
#[test]
fn rfc_6238_appendix_b() {
    let sha1 = b"12345678901234567890".as_slice();
    let sha256 = b"12345678901234567890123456789012".as_slice();
    let sha512 = b"1234567890123456789012345678901234567890123456789012345678901234".as_slice();
    let times: [u64; 6] = [
        59,
        1_111_111_109,
        1_111_111_111,
        1_234_567_890,
        2_000_000_000,
        20_000_000_000,
    ];
    let vectors: [(Algorithm, &[u8], [&str; 6]); 3] = [
        (
            Algorithm::Sha1,
            sha1,
            [
                "94287082", "07081804", "14050471", "89005924", "69279037", "65353130",
            ],
        ),
        (
            Algorithm::Sha256,
            sha256,
            [
                "46119246", "68084774", "67062674", "91819424", "90698825", "77737706",
            ],
        ),
        (
            Algorithm::Sha512,
            sha512,
            [
                "90693936", "25091201", "99943326", "93441116", "38618901", "47863826",
            ],
        ),
    ];
    for (algorithm, secret, codes) in vectors {
        let totp = totp(secret, algorithm, 8, 30);
        for (time, code) in times.iter().zip(codes) {
            assert_eq!(totp.generate(*time), code, "{algorithm} at {time}");
            // Exactly at that time the code is accepted, as its own step.
            assert_eq!(totp.check(code, *time), Some(time / 30));
        }
    }
}

/// RFC 4226 Appendix D: the HOTP values of the first ten counters (6 digits,
/// HMAC-SHA-1). A TOTP with a one-second period at time `n` is the HOTP of
/// counter `n`.
#[test]
fn rfc_4226_appendix_d() {
    let totp = totp(b"12345678901234567890", Algorithm::Sha1, 6, 1);
    let expected = [
        "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583", "399871",
        "520489",
    ];
    for (counter, code) in expected.iter().enumerate() {
        assert_eq!(totp.generate(counter as u64), *code, "counter {counter}");
    }
}

/// RFC 6238's reference implementation reduces a value that is 8 digits
/// long, so 6 and 7 digit codes are the same number's last digits.
#[test]
fn shorter_codes_are_the_tail_of_longer_ones() {
    let eight = totp(b"12345678901234567890", Algorithm::Sha1, 8, 30).generate(1_111_111_109);
    let seven = totp(b"12345678901234567890", Algorithm::Sha1, 7, 30).generate(1_111_111_109);
    let six = totp(b"12345678901234567890", Algorithm::Sha1, 6, 30).generate(1_111_111_109);
    assert_eq!(&eight[1..], seven);
    assert_eq!(&eight[2..], six);
}

#[test]
fn base32_follows_rfc_4648() {
    for (text, encoded) in [
        ("", ""),
        ("f", "MY"),
        ("fo", "MZXQ"),
        ("foo", "MZXW6"),
        ("foob", "MZXW6YQ"),
        ("fooba", "MZXW6YTB"),
        ("foobar", "MZXW6YTBOI"),
    ] {
        let secret = Secret::from_bytes(text);
        assert_eq!(secret.to_base32(), encoded);
        assert_eq!(Secret::from_base32(encoded).unwrap().as_bytes(), text.as_bytes());
        // Lower case and padding are read too.
        let padded = format!("{}{}", encoded.to_lowercase(), "=".repeat((8 - encoded.len() % 8) % 8));
        assert_eq!(Secret::from_base32(&padded).unwrap().as_bytes(), text.as_bytes());
    }
    for bad in ["M", "MZX", "MZXW6Y", "MZ XQ", "MZ1Q", "MZXQ!", "MZXQ-", "M=Y"] {
        assert_eq!(Secret::from_base32(bad), Err(TotpError::InvalidBase32), "{bad}");
    }
}

// ---- compatibility with the previous implementation -------------------------

#[test]
fn secrets_urls_and_codes_from_totp_rs_still_work() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let algorithm: Algorithm = case["algorithm"].as_str().unwrap().parse().unwrap();
        let digits = case["digits"].as_u64().unwrap() as u8;
        let period = case["period"].as_u64().unwrap();
        let secret = Secret::from_base32(case["secret"].as_str().unwrap()).unwrap();
        // The secret re-encodes to the same text.
        assert_eq!(secret.to_base32(), case["secret"].as_str().unwrap(), "{name}");

        let built = Totp::builder(secret)
            .algorithm(algorithm)
            .digits(digits)
            .period(period)
            .issuer(case["issuer"].as_str().unwrap())
            .account(case["account"].as_str().unwrap())
            .build()
            .unwrap();
        // The same URL, byte for byte.
        let url = case["url"].as_str().unwrap();
        assert_eq!(built.to_url().unwrap(), url, "{name}");

        // Reading the URL gives the same generator.
        let read = Totp::from_url(url).unwrap();
        assert_eq!(read.algorithm(), algorithm, "{name}");
        assert_eq!(read.digits(), digits);
        assert_eq!(read.period(), period);
        assert_eq!(read.issuer(), case["issuer"].as_str());
        assert_eq!(read.account(), case["account"].as_str().unwrap());
        assert_eq!(read.secret(), built.secret());

        for code in case["codes"].as_array().unwrap() {
            let time = code["time"].as_u64().unwrap();
            let expected = code["code"].as_str().unwrap();
            assert_eq!(built.generate(time), expected, "{name} at {time}");
            assert_eq!(read.generate(time), expected, "{name} at {time}");
            assert_eq!(built.check(expected, time), Some(time / period), "{name}");
        }
    }
}

// ---- the drift window ---------------------------------------------------------

#[test]
fn the_window_is_one_step_either_side_by_default() {
    let totp = totp(b"12345678901234567890", Algorithm::Sha1, 6, 30);
    assert_eq!(totp.skew(), 1);
    let step = 1_000_000_u64;
    let code = totp.generate(step * 30);
    // The step's own 30 seconds, and the step before and after.
    for time in [(step - 1) * 30, step * 30, step * 30 + 29, (step + 2) * 30 - 1] {
        assert_eq!(totp.check(&code, time), Some(step), "{time}");
    }
    // One second outside either edge.
    for time in [(step - 1) * 30 - 1, (step + 2) * 30, 0, u64::MAX] {
        assert_eq!(totp.check(&code, time), None, "{time}");
    }
}

#[test]
fn the_window_follows_the_skew() {
    let secret = b"12345678901234567890";
    for skew in [0_u16, 1, 2, 5] {
        let totp = Totp::builder(Secret::from_bytes(secret))
            .skew(skew)
            .build()
            .unwrap();
        let step = 5_000_u64;
        let code = totp.generate(step * 30);
        let reach = u64::from(skew);
        assert_eq!(totp.check(&code, (step - reach) * 30), Some(step));
        assert_eq!(totp.check(&code, (step + reach) * 30 + 29), Some(step));
        assert_eq!(totp.check(&code, (step - reach) * 30 - 1), None);
        assert_eq!(totp.check(&code, (step + reach + 1) * 30), None);
    }
}

#[test]
fn the_matched_step_is_reported_for_replay_protection() {
    let totp = totp(b"12345678901234567890", Algorithm::Sha1, 6, 30);
    let now = 1_792_929_600;
    let (before, current, after) = (
        totp.generate(now - 30),
        totp.generate(now),
        totp.generate(now + 30),
    );
    assert_eq!(totp.check(&before, now), Some(now / 30 - 1));
    assert_eq!(totp.check(&current, now), Some(now / 30));
    assert_eq!(totp.check(&after, now), Some(now / 30 + 1));
}

#[test]
fn the_window_does_not_underflow_at_the_start_of_time() {
    let totp = totp(b"12345678901234567890", Algorithm::Sha1, 6, 30);
    assert_eq!(totp.check(&totp.generate(0), 0), Some(0));
    assert_eq!(totp.check(&totp.generate(30), 0), Some(1));
    assert_eq!(totp.check(&totp.generate(60), 0), None);
    // Step numbers near the top of u64 neither panic nor wrap.
    let _ = totp.check("123456", u64::MAX);
    let one_second = self::totp(b"12345678901234567890", Algorithm::Sha1, 6, 1);
    let _ = one_second.check("123456", u64::MAX);
    assert_eq!(one_second.check(&one_second.generate(u64::MAX), u64::MAX), Some(u64::MAX));
}

#[test]
fn codes_that_are_not_the_right_shape_never_match() {
    let totp = totp(b"12345678901234567890", Algorithm::Sha1, 6, 30);
    let time = 1_111_111_109;
    let good = totp.generate(time);
    assert_eq!(totp.check(&good, time), Some(time / 30));
    for bad in [
        "",
        "12345",
        "1234567",
        " 081804",
        "081804 ",
        "08 804",
        "+81804",
        "-81804",
        "0x1804",
        "08180a",
        // Full-width digits are not ASCII digits (and not 6 bytes either).
        "０８１８０４",
        // The right digits with a NUL or newline added.
        "081804\n",
        "\u{0}81804",
    ] {
        assert_eq!(totp.check(bad, time), None, "{bad:?}");
    }
    // A wrong code of the right shape.
    let wrong = if good == "000000" { "000001" } else { "000000" };
    assert_eq!(totp.check(wrong, time), None);
    // The code of another secret.
    let other = self::totp(b"09876543210987654321", Algorithm::Sha1, 6, 30);
    assert_eq!(totp.check(&other.generate(time), time), None);
}

#[test]
fn leading_zeros_are_kept() {
    // Find a time whose code starts with zero: about one in ten does.
    let totp = totp(b"12345678901234567890", Algorithm::Sha1, 6, 30);
    let zero = (0..2_000_u64)
        .map(|step| totp.generate(step * 30))
        .find(|code| code.starts_with('0'))
        .unwrap();
    assert_eq!(zero.len(), 6);
    assert!(zero.bytes().all(|b| b.is_ascii_digit()));
}

// ---- secrets and settings -------------------------------------------------------

#[test]
fn generated_secrets_are_160_random_bits() {
    let a = Secret::generate().unwrap();
    let b = Secret::generate().unwrap();
    assert_eq!(a.as_bytes().len(), SECRET_LEN);
    assert_eq!(a.to_base32().len(), 32);
    assert_ne!(a, b);
    assert_eq!(Secret::from_base32(&a.to_base32()).unwrap(), a);
    // Not all one byte value.
    assert!(a.as_bytes().iter().any(|byte| *byte != a.as_bytes()[0]));
}

#[test]
fn secrets_are_wiped_and_never_printed() {
    fn wiped_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    wiped_on_drop::<Secret>();
    let secret = Secret::from_bytes(b"12345678901234567890");
    let printed = format!("{secret:?} {:?}", Totp::builder(secret.clone()).build().unwrap());
    assert!(!printed.contains("GEZDGNBVGY"), "{printed}");
    assert!(!printed.contains("1234567890"), "{printed}");
}

#[test]
fn settings_are_checked() {
    let secret = || Secret::from_bytes([7_u8; MIN_SECRET_LEN]);
    assert!(Totp::builder(secret()).build().is_ok());
    assert_eq!(
        Totp::builder(Secret::from_bytes([7_u8; MIN_SECRET_LEN - 1])).build().unwrap_err(),
        TotpError::SecretTooShort { bits: 120 }
    );
    for digits in [0, 5, 9, 10] {
        assert_eq!(
            Totp::builder(secret()).digits(digits).build().unwrap_err(),
            TotpError::InvalidDigits { digits }
        );
    }
    for digits in [6, 7, 8] {
        assert!(Totp::builder(secret()).digits(digits).build().is_ok());
    }
    assert_eq!(
        Totp::builder(secret()).period(0).build().unwrap_err(),
        TotpError::InvalidPeriod
    );
    assert_eq!(
        Totp::builder(secret()).issuer("a:b").build().unwrap_err(),
        TotpError::InvalidIssuer
    );
    assert_eq!(
        Totp::builder(secret()).account("a:b").build().unwrap_err(),
        TotpError::InvalidAccount
    );
    // A URL needs an account name; codes do not.
    let nameless = Totp::builder(secret()).build().unwrap();
    assert_eq!(nameless.to_url(), Err(TotpError::AccountNotSet));
    assert_eq!(nameless.generate(0).len(), 6);
}

// ---- key URIs ---------------------------------------------------------------------

#[test]
fn urls_read_what_apps_and_services_write() {
    let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    // %3A and a literal colon both separate issuer and account.
    for label in ["Navree:ada%40example.com", "Navree%3Aada%40example.com", "Navree%3aada@example.com"] {
        let totp = Totp::from_url(&format!("otpauth://totp/{label}?secret={secret}&issuer=Navree")).unwrap();
        assert_eq!(totp.issuer(), Some("Navree"));
        assert_eq!(totp.account(), "ada@example.com");
    }
    // The issuer from the parameter only, spaces as %20 or +.
    let totp = Totp::from_url(&format!("otpauth://totp/ada?secret={secret}&issuer=My+App%20Co")).unwrap();
    assert_eq!(totp.issuer(), Some("My App Co"));
    assert_eq!(totp.account(), "ada");
    // Upper-case scheme and type, lower-case secret, parameters in any
    // order, an unknown parameter and a fragment.
    let totp = Totp::from_url(&format!(
        "OTPAUTH://TOTP/Acme:bob?foo=bar&period=60&algorithm=sha256&digits=8&secret={}#top",
        secret.to_lowercase()
    ))
    .unwrap();
    assert_eq!((totp.digits(), totp.period(), totp.algorithm()), (8, 60, Algorithm::Sha256));
    // A percent-escape in the account is decoded once.
    let totp = Totp::from_url(&format!("otpauth://totp/Acme:100%2525?secret={secret}")).unwrap();
    assert_eq!(totp.account(), "100%25");
}

#[test]
fn bad_urls_are_refused() {
    let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    for url in [
        "".to_owned(),
        "https://totp/Acme:bob?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_owned(),
        format!("otpauth://hotp/Acme:bob?secret={secret}&counter=1"),
        format!("otpauth://totp?secret={secret}"),
        "otpauth://totp/Acme:bob".to_owned(),
        "otpauth://totp/Acme:bob?secret=".to_owned(),
        "otpauth://totp/Acme:bob?secret=!!!!".to_owned(),
        // Too short a secret (80 bits).
        "otpauth://totp/Acme:bob?secret=GEZDGNBVGY3TQOJQ".to_owned(),
        format!("otpauth://totp/Acme:bob?secret={secret}&digits=5"),
        format!("otpauth://totp/Acme:bob?secret={secret}&digits=x"),
        format!("otpauth://totp/Acme:bob?secret={secret}&period=0"),
        format!("otpauth://totp/Acme:bob?secret={secret}&period=-5"),
        format!("otpauth://totp/Acme:bob?secret={secret}&algorithm=MD5"),
        // The label and the parameter name different issuers.
        format!("otpauth://totp/Acme:bob?secret={secret}&issuer=Other"),
        // Invalid UTF-8 and truncated escapes.
        format!("otpauth://totp/Acme:%FF?secret={secret}"),
        format!("otpauth://totp/Acme:bob%?secret={secret}"),
        format!("otpauth://totp/Acme:bob%2?secret={secret}"),
        // A second colon in the account.
        format!("otpauth://totp/Acme:a:b?secret={secret}"),
    ] {
        assert!(Totp::from_url(&url).is_err(), "{url}");
    }
}

#[test]
fn urls_round_trip_awkward_names() {
    for (issuer, account) in [
        ("Navree", "ada@example.com"),
        ("My App", "grace+tag@example.org"),
        ("Acme & Sons", "o'brien@example.com"),
        ("Café 42", "zoë@exämple.com"),
        ("100%", "a b/c?d=e&f#g"),
        ("日本", "ユーザー"),
    ] {
        let totp = Totp::builder(Secret::generate().unwrap())
            .issuer(issuer)
            .account(account)
            .build()
            .unwrap();
        let url = totp.to_url().unwrap();
        let read = Totp::from_url(&url).unwrap();
        assert_eq!(read.issuer(), Some(issuer), "{url}");
        assert_eq!(read.account(), account, "{url}");
        assert_eq!(read.secret(), totp.secret());
        assert_eq!(read.to_url().unwrap(), url);
    }
}

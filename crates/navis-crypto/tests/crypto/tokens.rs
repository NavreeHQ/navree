//! Free token and digest helpers.

use navis_crypto::{constant_time_eq, hmac_sha256, random_token, sha256_hex};

#[test]
fn random_token_is_base62_of_requested_length() {
    let token = random_token(40);
    assert_eq!(token.len(), 40);
    assert!(token.bytes().all(|c| c.is_ascii_alphanumeric()), "{token}");
    assert_ne!(token, random_token(40));
    assert_eq!(random_token(0), "");
}

#[test]
fn random_token_uses_the_whole_base62_alphabet() {
    let sample: String = (0..50).map(|_| random_token(64)).collect();
    assert!(sample.bytes().any(|c| c.is_ascii_lowercase()));
    assert!(sample.bytes().any(|c| c.is_ascii_uppercase()));
    assert!(sample.bytes().any(|c| c.is_ascii_digit()));
}

#[test]
fn sha256_hex_matches_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn hmac_sha256_matches_rfc_4231_vector() {
    let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    assert_eq!(
        hex::encode(mac),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn constant_time_eq_compares_contents_and_length() {
    assert!(constant_time_eq(b"token", b"token"));
    assert!(!constant_time_eq(b"token", b"tokem"));
    assert!(!constant_time_eq(b"token", b"token2"));
    assert!(constant_time_eq(b"", b""));
}

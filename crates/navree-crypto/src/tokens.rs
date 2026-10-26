//! Token and digest helpers: [`random_token`], [`sha256_hex`],
//! [`hmac_sha256`] and [`constant_time_eq`].

use hmac::{Hmac, KeyInit as _, Mac as _};
use rand::distr::{Alphanumeric, SampleString as _};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

/// A random token of `len` base62 characters (`[A-Za-z0-9]`, about 5.95 bits
/// each) from a cryptographically secure generator. 40 characters give
/// about 238 bits: enough for API tokens and password reset links.
pub fn random_token(len: usize) -> String {
    // `ThreadRng` is a CSPRNG (ChaCha12) seeded and periodically reseeded from
    // the operating system; `Alphanumeric` samples without modulo bias.
    Alphanumeric.sample_string(&mut rand::rng(), len)
}

/// The SHA-256 digest of `data` as 64 lower-case hex characters (how tokens
/// are stored at rest).
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// HMAC-SHA-256 of `msg` under `key` (any key length).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256>>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

/// Compares two byte strings in time that depends only on their lengths, for
/// checking tokens and signatures.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

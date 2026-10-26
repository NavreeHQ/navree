//! Temporary URL signatures and path encoding for URLs.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_crypto::{constant_time_eq, hmac_sha256};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// Characters escaped in URL path segments: everything but RFC 3986
/// unreserved characters.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// A normalised path with every segment percent-encoded.
pub(crate) fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| utf8_percent_encode(segment, SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Why a temporary URL was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The signature does not match (tampered or forged).
    Invalid,
    /// The signature is valid but the link has expired.
    Expired,
}

/// Signs `disk/path/expires` with HMAC-SHA-256.
pub(crate) struct Signer {
    key: Vec<u8>,
}

impl Signer {
    /// A signer using `key` as the HMAC key.
    pub(crate) fn new(key: &[u8]) -> Self {
        Self { key: key.to_vec() }
    }

    /// The hex signature of `path` on `disk` until `expires` (unix seconds).
    /// Disk names cannot contain `/` and `expires` is a number, so the
    /// message `disk/path/expires` is unambiguous.
    pub(crate) fn sign(&self, disk: &str, path: &str, expires: i64) -> String {
        hex::encode(self.mac(disk, path, expires))
    }

    /// Checks a signature in constant time, then the expiry.
    pub(crate) fn verify(
        &self,
        disk: &str,
        path: &str,
        expires: i64,
        signature: &str,
        now: DateTime<Utc>,
    ) -> Result<(), Refusal> {
        let given = hex::decode(signature).map_err(|_| Refusal::Invalid)?;
        if !constant_time_eq(&given, &self.mac(disk, path, expires)) {
            return Err(Refusal::Invalid);
        }
        if now.timestamp() >= expires {
            return Err(Refusal::Expired);
        }
        Ok(())
    }

    fn mac(&self, disk: &str, path: &str, expires: i64) -> [u8; 32] {
        hmac_sha256(&self.key, format!("{disk}/{path}/{expires}").as_bytes())
    }
}

impl fmt::Debug for Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Signer([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    #[test]
    fn verify_accepts_own_signature_until_expiry() {
        let signer = Signer::new(b"key");
        let now = Utc::now();
        let expires = (now + Duration::seconds(60)).timestamp();
        let signature = signer.sign("local", "a/b.txt", expires);
        assert_eq!(
            signer.verify("local", "a/b.txt", expires, &signature, now),
            Ok(())
        );
        let at_expiry = DateTime::from_timestamp(expires, 0).unwrap();
        assert_eq!(
            signer.verify("local", "a/b.txt", expires, &signature, at_expiry),
            Err(Refusal::Expired)
        );
    }

    #[test]
    fn verify_rejects_tampering() {
        let signer = Signer::new(b"key");
        let now = Utc::now();
        let expires = (now + Duration::seconds(60)).timestamp();
        let signature = signer.sign("local", "a/b.txt", expires);
        let refused = |disk, path, expires, signature: &str| {
            signer.verify(disk, path, expires, signature, now) == Err(Refusal::Invalid)
        };
        assert!(refused("public", "a/b.txt", expires, &signature));
        assert!(refused("local", "a/c.txt", expires, &signature));
        assert!(refused("local", "a/b.txt", expires + 3600, &signature));
        assert!(refused("local", "a/b.txt", expires, "zz"));
        assert!(refused("local", "a/b.txt", expires, &signature[..62]));
        assert!(refused("local", "a/b.txt", expires, ""));
        // An expired forgery is reported as invalid, not expired.
        assert!(refused("local", "a/b.txt", 0, &signature));
        assert_eq!(
            Signer::new(b"other").verify("local", "a/b.txt", expires, &signature, now),
            Err(Refusal::Invalid)
        );
    }

    #[test]
    fn debug_hides_the_key() {
        assert!(!format!("{:?}", Signer::new(b"hunter2")).contains("hunter2"));
    }

    #[test]
    fn paths_are_encoded_per_segment() {
        assert_eq!(encode_path("a b/c~d_e-f.txt"), "a%20b/c~d_e-f.txt");
        assert_eq!(encode_path("x/%/?/#"), "x/%25/%3F/%23");
        assert_eq!(encode_path("ünï.txt"), "%C3%BCn%C3%AF.txt");
    }
}

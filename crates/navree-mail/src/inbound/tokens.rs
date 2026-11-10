//! [`ReplyTokens`]: signed tokens for reply addresses
//! (`reply+{token}@in.example.com`).

use std::fmt;
use std::sync::Arc;

use navree_contracts::{Clock, SystemClock};
use uuid::Uuid;

use super::error::InboundError;

/// The purpose the reply-token key is derived for from the app key.
pub(crate) const KEY_PURPOSE: &str = "mail-reply-tokens";

/// Bytes of HMAC-SHA-256 kept in a token (80 bits).
const MAC_LEN: usize = 10;
/// A token holding text.
const KIND_TEXT: u8 = 1;
/// A token holding a UUID (16 bytes).
const KIND_UUID: u8 = 2;
/// The base32 alphabet, lower case (RFC 4648 without padding): tokens
/// survive mail servers that change the case of an address.
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
/// The longest local part an address may have.
const MAX_LOCAL_PART: usize = 64;

/// Signs and checks reply tokens: a value (a ticket id, `announcement:42`)
/// sealed into the address people answer, so the reply finds its way back
/// and nobody can forge an address for something else.
///
/// A token is the value, the day it was issued and an 80-bit HMAC-SHA-256
/// tag under a key derived from the application key, written in lower-case
/// base32. A UUID takes 47 characters, so `reply+{token}@...` stays within
/// the 64 characters an address allows before the `@`. Tokens don't expire
/// unless `mail.inbound.reply_token_ttl_days` is set.
///
/// ```
/// use navree_mail::ReplyTokens;
/// use uuid::Uuid;
///
/// let tokens = ReplyTokens::new([7u8; 32]);
/// let ticket = Uuid::now_v7();
///
/// let address = tokens.address_for("reply+{token}@in.acme.test", ticket).unwrap();
/// assert!(address.starts_with("reply+") && address.ends_with("@in.acme.test"));
///
/// let token = address.trim_start_matches("reply+").trim_end_matches("@in.acme.test");
/// assert_eq!(tokens.verify_uuid(token).unwrap(), ticket);
/// assert!(tokens.verify("reply-to-anything").is_err());
/// ```
#[derive(Clone)]
pub struct ReplyTokens {
    key: Option<[u8; 32]>,
    ttl_days: u32,
    clock: Arc<dyn Clock>,
}

impl ReplyTokens {
    /// Tokens signed with `key` (any length; the module derives one from
    /// the app key).
    pub fn new(key: impl AsRef<[u8]>) -> Self {
        Self {
            key: Some(navree_crypto::hmac_sha256(
                key.as_ref(),
                KEY_PURPOSE.as_bytes(),
            )),
            ttl_days: 0,
            clock: Arc::new(SystemClock),
        }
    }

    /// Without a key: every call fails with [`InboundError::MissingKey`].
    pub(crate) fn without_key() -> Self {
        Self {
            key: None,
            ttl_days: 0,
            clock: Arc::new(SystemClock),
        }
    }

    /// Tokens older than `days` fail with [`InboundError::ExpiredToken`]
    /// (0, the default: they never expire).
    #[must_use]
    pub fn with_ttl_days(mut self, days: u32) -> Self {
        self.ttl_days = days;
        self
    }

    /// Reads the date from `clock` (tests use a fixed clock).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Tokens under a key of their own for `purpose`: a token signed for
    /// `tickets` doesn't verify for `announcements`, even with the same
    /// value.
    #[must_use]
    pub fn scoped(&self, purpose: &str) -> Self {
        Self {
            key: self
                .key
                .map(|key| navree_crypto::hmac_sha256(&key, format!("scope:{purpose}").as_bytes())),
            ttl_days: self.ttl_days,
            clock: self.clock.clone(),
        }
    }

    /// Whether the tokens have a key (the app has an application key).
    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    /// A token holding `value`.
    ///
    /// # Errors
    ///
    /// [`InboundError::MissingKey`] without an application key.
    pub fn sign(&self, value: &str) -> Result<String, InboundError> {
        self.seal(KIND_TEXT, value.as_bytes())
    }

    /// A token holding `id`, in 47 characters.
    ///
    /// # Errors
    ///
    /// [`InboundError::MissingKey`] without an application key.
    pub fn sign_uuid(&self, id: Uuid) -> Result<String, InboundError> {
        self.seal(KIND_UUID, id.as_bytes())
    }

    /// The value in `token` (a UUID token gives the UUID's text).
    ///
    /// # Errors
    ///
    /// [`InboundError::InvalidToken`] when the token was not signed with
    /// this key or is damaged, [`InboundError::ExpiredToken`] when it is
    /// older than the TTL, [`InboundError::MissingKey`] without a key.
    pub fn verify(&self, token: &str) -> Result<String, InboundError> {
        let (kind, value) = self.open(token)?;
        match kind {
            KIND_TEXT => String::from_utf8(value).map_err(|_| InboundError::InvalidToken),
            KIND_UUID => Uuid::from_slice(&value)
                .map(|id| id.to_string())
                .map_err(|_| InboundError::InvalidToken),
            _ => Err(InboundError::InvalidToken),
        }
    }

    /// The UUID in `token` (signed with [`sign_uuid`](ReplyTokens::sign_uuid),
    /// or a text token holding a UUID).
    ///
    /// # Errors
    ///
    /// As [`verify`](ReplyTokens::verify), and
    /// [`InboundError::InvalidToken`] when the value is not a UUID.
    pub fn verify_uuid(&self, token: &str) -> Result<Uuid, InboundError> {
        let (kind, value) = self.open(token)?;
        match kind {
            KIND_UUID => Uuid::from_slice(&value).map_err(|_| InboundError::InvalidToken),
            KIND_TEXT => std::str::from_utf8(&value)
                .ok()
                .and_then(|text| Uuid::parse_str(text).ok())
                .ok_or(InboundError::InvalidToken),
            _ => Err(InboundError::InvalidToken),
        }
    }

    /// `pattern` with `{token}` replaced by a token holding `value`:
    /// `address("reply+{token}@in.acme.test", "ticket:42")`.
    ///
    /// # Errors
    ///
    /// [`InboundError::InvalidPattern`] when the pattern has no single
    /// `{token}` or another placeholder, [`InboundError::AddressTooLong`]
    /// when the part before the `@` passes 64 characters,
    /// [`InboundError::MissingKey`] without a key.
    pub fn address(&self, pattern: &str, value: &str) -> Result<String, InboundError> {
        fill(pattern, &self.sign(value)?)
    }

    /// `pattern` with `{token}` replaced by a token holding `id`.
    ///
    /// # Errors
    ///
    /// As [`address`](ReplyTokens::address).
    pub fn address_for(&self, pattern: &str, id: Uuid) -> Result<String, InboundError> {
        fill(pattern, &self.sign_uuid(id)?)
    }

    fn key(&self) -> Result<&[u8; 32], InboundError> {
        self.key.as_ref().ok_or(InboundError::MissingKey)
    }

    fn today(&self) -> u16 {
        let days = self.clock.now().timestamp().div_euclid(86_400);
        u16::try_from(days.max(0)).unwrap_or(u16::MAX)
    }

    fn seal(&self, kind: u8, value: &[u8]) -> Result<String, InboundError> {
        let key = self.key()?;
        let mut bytes = Vec::with_capacity(3 + value.len() + MAC_LEN);
        bytes.push(kind);
        bytes.extend_from_slice(&self.today().to_be_bytes());
        bytes.extend_from_slice(value);
        let tag = navree_crypto::hmac_sha256(key, &bytes);
        bytes.extend_from_slice(&tag[..MAC_LEN]);
        Ok(encode(&bytes))
    }

    fn open(&self, token: &str) -> Result<(u8, Vec<u8>), InboundError> {
        let key = self.key()?;
        let bytes = decode(&token.trim().to_ascii_lowercase()).ok_or(InboundError::InvalidToken)?;
        if bytes.len() < 3 + MAC_LEN {
            return Err(InboundError::InvalidToken);
        }
        let (body, tag) = bytes.split_at(bytes.len() - MAC_LEN);
        let expected = navree_crypto::hmac_sha256(key, body);
        if !navree_crypto::constant_time_eq(&expected[..MAC_LEN], tag) {
            return Err(InboundError::InvalidToken);
        }
        let issued = u16::from_be_bytes([body[1], body[2]]);
        if self.ttl_days > 0 && u32::from(self.today()) > u32::from(issued) + self.ttl_days {
            return Err(InboundError::ExpiredToken);
        }
        Ok((body[0], body[3..].to_vec()))
    }
}

impl fmt::Debug for ReplyTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplyTokens")
            .field("has_key", &self.key.is_some())
            .field("ttl_days", &self.ttl_days)
            .finish_non_exhaustive()
    }
}

/// Replaces the single `{token}` of `pattern`.
fn fill(pattern: &str, token: &str) -> Result<String, InboundError> {
    let invalid = |reason: &str| InboundError::InvalidPattern {
        pattern: pattern.to_owned(),
        reason: reason.to_owned(),
    };
    if pattern.matches("{token}").count() != 1 {
        return Err(invalid("a reply address needs exactly one `{token}`"));
    }
    let address = pattern.replace("{token}", token);
    if address.contains(['{', '}', '*']) {
        return Err(invalid("a reply address can only fill `{token}`"));
    }
    let Some((local, domain)) = address.rsplit_once('@') else {
        return Err(invalid("a reply address needs an `@`"));
    };
    if domain.is_empty() || local.is_empty() {
        return Err(invalid("a reply address needs a local part and a domain"));
    }
    if local.len() > MAX_LOCAL_PART {
        return Err(InboundError::AddressTooLong {
            local: local.to_owned(),
        });
    }
    Ok(address)
}

/// Lower-case base32 without padding.
fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }
    out
}

/// Decodes lower-case base32 without padding; `None` for any other
/// character or a malformed length.
fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.bytes() {
        let value = ALPHABET.iter().position(|&a| a == c)?;
        buffer = (buffer << 5) | u32::try_from(value).ok()?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
    }
    // Leftover bits must be padding zeros, and fewer than five.
    if bits >= 5 || (buffer & ((1 << bits) - 1)) != 0 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_round_trips() {
        for bytes in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"fooba",
            b"foobar",
            &[0xff; 17],
        ] {
            let encoded = encode(bytes);
            assert!(
                encoded
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            );
            assert_eq!(decode(&encoded).as_deref(), Some(bytes), "{encoded}");
        }
        assert_eq!(encode(b"foobar"), "mzxw6ytboi");
        assert_eq!(decode("mzxw6ytboi1"), None, "1 is not in the alphabet");
        assert_eq!(decode("MZXW6"), None, "upper case is lowered by the caller");
    }

    #[test]
    fn a_uuid_token_fits_a_reply_address() {
        let tokens = ReplyTokens::new(b"secret");
        let token = tokens.sign_uuid(Uuid::now_v7()).unwrap();
        assert_eq!(token.len(), 47);
    }

    #[test]
    fn fill_checks_the_pattern() {
        assert!(fill("reply@x.test", "t").is_err());
        assert!(fill("reply+{token}+{id}@x.test", "t").is_err());
        assert!(fill("{token}", "t").is_err());
        assert_eq!(fill("r+{token}@x.test", "t").unwrap(), "r+t@x.test");
        assert!(matches!(
            fill("r+{token}@x.test", &"a".repeat(63)),
            Err(InboundError::AddressTooLong { .. })
        ));
    }
}

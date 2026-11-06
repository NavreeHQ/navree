//! Time-based one-time passwords (TOTP, RFC 6238 over the HOTP of RFC 4226)
//! with HMAC on `ring`, and the `otpauth://` key URIs that authenticator
//! apps scan.
//!
//! - A [`Secret`] is random bytes (160 bits by default, 128 at least), shown
//!   to the user as base32 ([`Secret::to_base32`]) and wiped from memory
//!   when dropped. It never appears in `Debug` output.
//! - A [`Totp`] turns a secret and a time into a code
//!   ([`generate`](Totp::generate)) and checks a presented code
//!   ([`check`](Totp::check)). Defaults are what authenticator apps expect:
//!   HMAC-SHA-1, 6 digits, 30-second steps, and one step of clock drift
//!   either way. HMAC-SHA-256 and HMAC-SHA-512, 6 to 8 digits and other
//!   periods are there for the apps that support them.
//! - [`check`](Totp::check) compares every code of the drift window in
//!   constant time and says which step matched. A code must work once
//!   (RFC 6238 §5.2), which needs state this module does not have: keep the
//!   last accepted step per user and refuse a step that is not newer.
//! - [`Totp::to_url`] writes the key URI
//!   (`otpauth://totp/Issuer:account?secret=..&issuer=..`) and
//!   [`Totp::from_url`] reads one.
//!
//! ```
//! use navree_crypto::totp::{Secret, Totp};
//!
//! let secret = Secret::generate()?;
//! let totp = Totp::builder(secret)
//!     .issuer("Navree")
//!     .account("anu@example.com")
//!     .build()?;
//!
//! // The code the authenticator app shows at this moment, and its check.
//! let now = 1_792_929_600;
//! let code = totp.generate(now);
//! assert_eq!(totp.check(&code, now + 20), Some(now / 30));
//! assert!(totp.to_url()?.starts_with("otpauth://totp/Navree:ada%40example.com?secret="));
//! # Ok::<(), navree_crypto::totp::TotpError>(())
//! ```

use std::fmt;
use std::str::FromStr;

use ring::hmac;
use ring::rand::{SecureRandom as _, SystemRandom};
use zeroize::Zeroizing;

use crate::constant_time_eq;

/// Random bytes in a generated secret: 160 bits, the size RFC 4226
/// recommends.
pub const SECRET_LEN: usize = 20;

/// The shortest secret accepted: 128 bits (RFC 4226 §4, R6).
pub const MIN_SECRET_LEN: usize = 16;

/// Why a secret, a code generator or a key URI was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TotpError {
    /// The system random number generator failed.
    #[error("the system random number generator failed")]
    Random,
    /// The secret is shorter than 128 bits.
    #[error("the secret is {bits} bits long, 128 are the least")]
    SecretTooShort {
        /// Its length.
        bits: usize,
    },
    /// Not base32 (RFC 4648).
    #[error("the secret is not base32")]
    InvalidBase32,
    /// Codes have 6 to 8 digits.
    #[error("{digits} digits: codes have 6 to 8")]
    InvalidDigits {
        /// The digits asked for.
        digits: u8,
    },
    /// A period of zero seconds.
    #[error("the period must be at least one second")]
    InvalidPeriod,
    /// A `:` in the issuer, which the key URI uses as a separator.
    #[error("the issuer must not contain `:`")]
    InvalidIssuer,
    /// A `:` in the account name.
    #[error("the account name must not contain `:`")]
    InvalidAccount,
    /// A key URI needs an account name.
    #[error("the account name is not set")]
    AccountNotSet,
    /// Not an `otpauth://totp/...` key URI with a secret.
    #[error("not a valid otpauth URI: {0}")]
    InvalidUri(&'static str),
}

/// The HMAC hash of a [`Totp`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Algorithm {
    /// HMAC-SHA-1, what every authenticator app supports.
    Sha1,
    /// HMAC-SHA-256.
    Sha256,
    /// HMAC-SHA-512.
    Sha512,
}

impl Algorithm {
    /// The name in key URIs: `SHA1`, `SHA256` or `SHA512`.
    pub fn as_str(self) -> &'static str {
        match self {
            Algorithm::Sha1 => "SHA1",
            Algorithm::Sha256 => "SHA256",
            Algorithm::Sha512 => "SHA512",
        }
    }

    fn ring(self) -> hmac::Algorithm {
        match self {
            Algorithm::Sha1 => hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            Algorithm::Sha256 => hmac::HMAC_SHA256,
            Algorithm::Sha512 => hmac::HMAC_SHA512,
        }
    }
}

impl FromStr for Algorithm {
    type Err = TotpError;

    fn from_str(name: &str) -> Result<Self, TotpError> {
        match name.to_ascii_uppercase().as_str() {
            "SHA1" => Ok(Algorithm::Sha1),
            "SHA256" => Ok(Algorithm::Sha256),
            "SHA512" => Ok(Algorithm::Sha512),
            _ => Err(TotpError::InvalidUri(
                "the algorithm is not SHA1, SHA256 or SHA512",
            )),
        }
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The shared secret of a [`Totp`]: bytes that are wiped when dropped and
/// never shown by `Debug`.
#[derive(Clone)]
pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    /// A new random secret of [`SECRET_LEN`] bytes.
    pub fn generate() -> Result<Self, TotpError> {
        let mut bytes = Zeroizing::new(vec![0_u8; SECRET_LEN]);
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| TotpError::Random)?;
        Ok(Self(bytes))
    }

    /// A secret from bytes you already have. At least 16 are needed to
    /// build a [`Totp`].
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Self {
        Self(Zeroizing::new(bytes.as_ref().to_vec()))
    }

    /// A secret from its base32 text (RFC 4648): upper or lower case,
    /// without padding or with it.
    pub fn from_base32(text: &str) -> Result<Self, TotpError> {
        base32_decode(text)
            .map(Self)
            .ok_or(TotpError::InvalidBase32)
    }

    /// The secret as base32 without padding, for typing in by hand and for
    /// the key URI.
    pub fn to_base32(&self) -> String {
        base32_encode(&self.0)
    }

    /// The secret's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.0, &other.0)
    }
}

impl Eq for Secret {}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl zeroize::ZeroizeOnDrop for Secret {}

/// A time-based one-time password generator and checker.
///
/// Build one with [`Totp::builder`], or read it from a key URI with
/// [`Totp::from_url`].
#[derive(Clone)]
pub struct Totp {
    secret: Secret,
    algorithm: Algorithm,
    digits: u8,
    period: u64,
    skew: u16,
    issuer: Option<String>,
    account: String,
}

/// Settings of a [`Totp`], validated by [`build`](TotpBuilder::build).
#[derive(Clone)]
pub struct TotpBuilder {
    secret: Secret,
    algorithm: Algorithm,
    digits: u8,
    period: u64,
    skew: u16,
    issuer: Option<String>,
    account: String,
}

impl TotpBuilder {
    /// The HMAC hash. Default: SHA-1.
    #[must_use]
    pub fn algorithm(mut self, algorithm: Algorithm) -> Self {
        self.algorithm = algorithm;
        self
    }

    /// Digits in a code, 6 to 8. Default: 6.
    #[must_use]
    pub fn digits(mut self, digits: u8) -> Self {
        self.digits = digits;
        self
    }

    /// Seconds a code is valid. Default: 30.
    #[must_use]
    pub fn period(mut self, seconds: u64) -> Self {
        self.period = seconds;
        self
    }

    /// Steps of clock drift accepted on either side of the current one.
    /// Default: 1.
    #[must_use]
    pub fn skew(mut self, steps: u16) -> Self {
        self.skew = steps;
        self
    }

    /// The service's name, shown by the authenticator app. No `:`.
    #[must_use]
    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// The account's name (an email address), shown by the app. No `:`.
    /// Only the key URI needs one.
    #[must_use]
    pub fn account(mut self, account: impl Into<String>) -> Self {
        self.account = account.into();
        self
    }

    /// Checks the settings and builds the [`Totp`].
    pub fn build(self) -> Result<Totp, TotpError> {
        if !(6..=8).contains(&self.digits) {
            return Err(TotpError::InvalidDigits {
                digits: self.digits,
            });
        }
        if self.period == 0 {
            return Err(TotpError::InvalidPeriod);
        }
        if self
            .issuer
            .as_deref()
            .is_some_and(|issuer| issuer.contains(':'))
        {
            return Err(TotpError::InvalidIssuer);
        }
        if self.account.contains(':') {
            return Err(TotpError::InvalidAccount);
        }
        let bytes = self.secret.as_bytes().len();
        if bytes < MIN_SECRET_LEN {
            return Err(TotpError::SecretTooShort { bits: bytes * 8 });
        }
        Ok(Totp {
            secret: self.secret,
            algorithm: self.algorithm,
            digits: self.digits,
            period: self.period,
            skew: self.skew,
            issuer: self.issuer,
            account: self.account,
        })
    }
}

impl Totp {
    /// Settings for `secret`, with the defaults of authenticator apps.
    pub fn builder(secret: Secret) -> TotpBuilder {
        TotpBuilder {
            secret,
            algorithm: Algorithm::Sha1,
            digits: 6,
            period: 30,
            skew: 1,
            issuer: None,
            account: String::new(),
        }
    }

    /// The shared secret.
    pub fn secret(&self) -> &Secret {
        &self.secret
    }

    /// The HMAC hash.
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// Digits in a code.
    pub fn digits(&self) -> u8 {
        self.digits
    }

    /// Seconds a code is valid.
    pub fn period(&self) -> u64 {
        self.period
    }

    /// Steps of drift accepted either side.
    pub fn skew(&self) -> u16 {
        self.skew
    }

    /// The issuer, if set.
    pub fn issuer(&self) -> Option<&str> {
        self.issuer.as_deref()
    }

    /// The account name (empty if not set).
    pub fn account(&self) -> &str {
        &self.account
    }

    /// The step (counter) that contains Unix time `time`.
    pub fn step_at(&self, time: u64) -> u64 {
        time / self.period
    }

    /// The code of step `counter`: HOTP (RFC 4226 §5.3) with the step as
    /// the counter.
    fn code_at_step(&self, counter: u64) -> Zeroizing<String> {
        let key = hmac::Key::new(self.algorithm.ring(), self.secret.as_bytes());
        let tag = hmac::sign(&key, &counter.to_be_bytes());
        let tag = tag.as_ref();
        // Dynamic truncation: four bytes from the offset in the last
        // nibble, without the top bit.
        let offset = usize::from(tag[tag.len() - 1] & 0x0F);
        let value = u32::from_be_bytes([
            tag[offset] & 0x7F,
            tag[offset + 1],
            tag[offset + 2],
            tag[offset + 3],
        ]);
        let code = value % 10_u32.pow(u32::from(self.digits));
        Zeroizing::new(format!("{code:0width$}", width = usize::from(self.digits)))
    }

    /// The code at Unix time `time` (seconds): `digits` decimal digits,
    /// zero-padded.
    pub fn generate(&self, time: u64) -> String {
        let code = self.code_at_step(self.step_at(time));
        code.as_str().to_owned()
    }

    /// Whether `code` is the code of a step within [`skew`](Totp::skew) steps
    /// of the one at Unix time `time`, and which step: the earliest one that
    /// matches. A code that is not `digits` ASCII digits never matches.
    ///
    /// Every step of the window is computed and compared in constant time,
    /// whether or not an earlier one matched, so the time taken does not say
    /// how close a wrong code was. The caller must refuse a step that is not
    /// newer than the last one accepted for the user (RFC 6238 §5.2).
    pub fn check(&self, code: &str, time: u64) -> Option<u64> {
        let code = code.as_bytes();
        if code.len() != usize::from(self.digits) || !code.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let origin = self.step_at(time);
        let first = origin.saturating_sub(u64::from(self.skew));
        let last = origin.saturating_add(u64::from(self.skew));
        let mut matched: Option<u64> = None;
        for counter in first..=last {
            let expected = self.code_at_step(counter);
            if constant_time_eq(expected.as_bytes(), code) && matched.is_none() {
                matched = Some(counter);
            }
        }
        matched
    }

    /// The key URI for authenticator apps:
    /// `otpauth://totp/{issuer}:{account}?secret=..&issuer=..`, with
    /// `digits`, `algorithm` and `period` only where they are not the
    /// defaults (6, SHA1, 30). [`TotpError::AccountNotSet`] without an
    /// account name.
    pub fn to_url(&self) -> Result<String, TotpError> {
        if self.account.is_empty() {
            return Err(TotpError::AccountNotSet);
        }
        let account = percent_encode(&self.account);
        let mut parameters = vec![format!("secret={}", self.secret.to_base32())];
        if self.digits != 6 {
            parameters.push(format!("digits={}", self.digits));
        }
        if self.algorithm != Algorithm::Sha1 {
            parameters.push(format!("algorithm={}", self.algorithm));
        }
        let label = match &self.issuer {
            Some(issuer) => {
                let issuer = percent_encode(issuer);
                parameters.push(format!("issuer={issuer}"));
                format!("{issuer}:{account}")
            }
            None => account,
        };
        if self.period != 30 {
            parameters.push(format!("period={}", self.period));
        }
        Ok(format!("otpauth://totp/{label}?{}", parameters.join("&")))
    }

    /// Reads a key URI as [`to_url`](Totp::to_url) writes it, and as
    /// authenticator apps and services write them: the issuer in the label,
    /// in the `issuer` parameter or both (they must agree), `%3A` for the
    /// colon, `+` or `%20` for a space, unknown parameters ignored.
    pub fn from_url(url: &str) -> Result<Self, TotpError> {
        const BAD: fn(&'static str) -> TotpError = TotpError::InvalidUri;
        let rest = url
            .get(.."otpauth://".len())
            .filter(|scheme| scheme.eq_ignore_ascii_case("otpauth://"))
            .map(|_| &url["otpauth://".len()..])
            .ok_or(BAD("the scheme is not otpauth"))?;
        let rest = rest.split('#').next().unwrap_or(rest);
        let (kind, rest) = rest.split_once('/').ok_or(BAD("no label"))?;
        if !kind.eq_ignore_ascii_case("totp") {
            return Err(BAD("only totp keys are supported"));
        }
        let (label, query) = rest.split_once('?').unwrap_or((rest, ""));

        // The first `:` (or `%3A`) of the still-encoded label separates
        // issuer and account; each half is decoded once.
        let (issuer_raw, account_raw) = split_label(label);
        let decode = |raw: &str| percent_decode(raw, false).ok_or(BAD("bad percent-encoding"));
        let account = decode(account_raw)?;
        let mut issuer = issuer_raw.map(decode).transpose()?;

        let mut builder = Totp::builder(Secret::from_bytes([]));
        let mut secret = None;
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = percent_decode(name, true).ok_or(BAD("bad percent-encoding"))?;
            let value = percent_decode(value, true).ok_or(BAD("bad percent-encoding"))?;
            match name.as_str() {
                "algorithm" => builder = builder.algorithm(value.parse()?),
                "digits" => {
                    builder =
                        builder.digits(value.parse().map_err(|_| BAD("digits is not a number"))?);
                }
                "period" => {
                    builder =
                        builder.period(value.parse().map_err(|_| BAD("period is not a number"))?);
                }
                "secret" => secret = Some(Secret::from_base32(&value)?),
                "issuer" => {
                    if issuer.as_ref().is_some_and(|label| *label != value) {
                        return Err(BAD("the issuer of the label and the parameter differ"));
                    }
                    issuer = Some(value);
                }
                _ => {}
            }
        }
        builder.secret = secret.ok_or(BAD("no secret"))?;
        builder.account = account;
        builder.issuer = issuer;
        builder.build()
    }
}

impl fmt::Debug for Totp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Totp")
            .field("algorithm", &self.algorithm)
            .field("digits", &self.digits)
            .field("period", &self.period)
            .field("skew", &self.skew)
            .field("issuer", &self.issuer)
            .finish_non_exhaustive()
    }
}

// ---- base32 (RFC 4648) ------------------------------------------------------

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for byte in bytes {
        buffer = buffer << 8 | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            out.push(char::from(BASE32[(buffer >> (bits - 5)) as usize & 31]));
            bits -= 5;
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(BASE32[(buffer << (5 - bits)) as usize & 31]));
    }
    out
}

/// The bytes of base32 `text`, in either case, with or without `=`
/// padding. `None` for other characters and for lengths no byte string
/// encodes to.
fn base32_decode(text: &str) -> Option<Zeroizing<Vec<u8>>> {
    let text = text.trim_end_matches('=');
    if matches!(text.len() % 8, 1 | 3 | 6) {
        return None;
    }
    let mut out = Zeroizing::new(Vec::with_capacity(text.len() * 5 / 8));
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for character in text.bytes() {
        let value = match character {
            b'A'..=b'Z' => character - b'A',
            b'a'..=b'z' => character - b'a',
            b'2'..=b'7' => character - b'2' + 26,
            _ => return None,
        };
        buffer = buffer << 5 | u32::from(value);
        bits += 5;
        if bits >= 8 {
            out.push((buffer >> (bits - 8)) as u8);
            bits -= 8;
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// ---- percent-encoding of labels ---------------------------------------------

/// Everything but ASCII letters and digits and `-_.~` is `%XX`.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `text` with `%XX` decoded as UTF-8 (and `+` as a space in a query).
fn percent_decode(text: &str, plus_is_space: bool) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = text.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
                continue;
            }
            b'+' if plus_is_space => out.push(b' '),
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// Splits an encoded label at its first `:` or `%3A`/`%3a`: neither the
/// issuer nor the account name may contain a colon, so the first is the
/// separator.
fn split_label(label: &str) -> (Option<&str>, &str) {
    let bytes = label.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b':' {
            return (Some(&label[..i]), &label[i + 1..]);
        }
        if bytes[i] == b'%'
            && bytes.get(i + 1) == Some(&b'3')
            && bytes
                .get(i + 2)
                .is_some_and(|c| c.eq_ignore_ascii_case(&b'A'))
        {
            return (Some(&label[..i]), &label[i + 3..]);
        }
    }
    (None, label)
}

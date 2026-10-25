//! [`RequestId`]: the id that ties logs, audit entries and responses together.

use std::fmt;

use uuid::Uuid;

/// A request id: at most 64 characters of `[A-Za-z0-9_-]`. Incoming
/// `X-Request-Id` values are accepted only in that form; anything else is
/// replaced by a generated UUID v7 (32 hex characters).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(String);

impl RequestId {
    /// The longest accepted request id.
    pub const MAX_LEN: usize = 64;

    /// Accepts `candidate` if it is 1..=64 characters of `[A-Za-z0-9_-]`.
    pub fn parse(candidate: &str) -> Option<Self> {
        let valid = !candidate.is_empty()
            && candidate.len() <= Self::MAX_LEN
            && candidate
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        valid.then(|| Self(candidate.to_owned()))
    }

    /// A new id: a UUID v7 in simple (hex) form, so ids sort by time.
    pub fn generate() -> Self {
        Self(Uuid::now_v7().simple().to_string())
    }

    /// The incoming id if it is valid, otherwise a generated one.
    pub fn parse_or_generate(candidate: Option<&str>) -> Self {
        candidate
            .and_then(Self::parse)
            .unwrap_or_else(Self::generate)
    }

    /// The id as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

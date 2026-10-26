//! Opaque refresh tokens: `<id>.<48 random base62 characters>`, stored as
//! the SHA-256 of the whole token.

use std::fmt;

use navis_crypto::{random_token, sha256_hex};
use uuid::Uuid;

/// Random characters after the id (about 285 bits).
pub(crate) const SECRET_LEN: usize = 48;

/// A refresh token as the client holds it, with its row id and digest.
pub(crate) struct RefreshSecret {
    id: Uuid,
    plain: String,
    hash: String,
}

impl RefreshSecret {
    /// A new token with a fresh UUID v7 id.
    pub(crate) fn generate() -> Self {
        let id = Uuid::now_v7();
        Self::from_plain(id, format!("{id}.{}", random_token(SECRET_LEN)))
    }

    /// A token presented by a client, if it is well formed: a hyphenated
    /// UUID, a `.` and exactly [`SECRET_LEN`] base62 characters.
    pub(crate) fn parse(plain: &str) -> Option<Self> {
        let (id, secret) = plain.split_once('.')?;
        if id.len() != 36
            || secret.len() != SECRET_LEN
            || !secret.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return None;
        }
        let id = Uuid::try_parse(id).ok()?;
        Some(Self::from_plain(id, plain.to_owned()))
    }

    fn from_plain(id: Uuid, plain: String) -> Self {
        let hash = sha256_hex(plain.as_bytes());
        Self { id, plain, hash }
    }

    /// The row id.
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    /// The token handed to the client (shown once, never stored).
    pub(crate) fn plain(&self) -> &str {
        &self.plain
    }

    /// The SHA-256 digest stored at rest.
    pub(crate) fn hash(&self) -> &str {
        &self.hash
    }
}

impl fmt::Debug for RefreshSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshSecret")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_id_dot_48_base62_and_hashed_whole() {
        let token = RefreshSecret::generate();
        let plain = token.plain();
        let (id, secret) = plain.split_once('.').unwrap();
        assert_eq!(id, token.id().to_string());
        assert_eq!(secret.len(), SECRET_LEN);
        assert!(secret.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(token.hash(), navis_crypto::sha256_hex(plain.as_bytes()));
        assert_ne!(RefreshSecret::generate().plain(), plain);
    }

    #[test]
    fn parse_accepts_only_well_formed_tokens() {
        let token = RefreshSecret::generate();
        let parsed = RefreshSecret::parse(token.plain()).unwrap();
        assert_eq!(parsed.id(), token.id());
        assert_eq!(parsed.hash(), token.hash());

        let id = token.id();
        for bad in [
            String::new(),
            "nonsense".to_owned(),
            format!("{id}"),
            format!("{id}."),
            format!("{id}.short"),
            format!("{id}.{}", "a".repeat(SECRET_LEN + 1)),
            format!("{id}.{}", "!".repeat(SECRET_LEN)),
            format!("not-a-uuid.{}", "a".repeat(SECRET_LEN)),
            format!("{}.{}", id.simple(), "a".repeat(SECRET_LEN)),
        ] {
            assert!(RefreshSecret::parse(&bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn debug_output_hides_the_token() {
        let token = RefreshSecret::generate();
        let debug = format!("{token:?}");
        let secret = token.plain().split_once('.').unwrap().1.to_owned();
        assert!(!debug.contains(&secret), "{debug}");
        assert!(!debug.contains(token.hash()), "{debug}");
    }
}

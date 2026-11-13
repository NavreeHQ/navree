//! The sealed state: what a flow carries from the redirect to the callback.
//!
//! The state is the JSON [`StatePayload`] encrypted and authenticated with
//! the application key (navree-crypto's AES-256-GCM `Encrypter`), behind
//! the prefix `nvs1.`. It holds the PKCE verifier, the OpenID Connect
//! nonce, the intent, the user a link flow is for, the return path and the
//! expiry, so nothing is stored server-side until the callback, where its
//! id is remembered in the cache until it expires (a state works once).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_crypto::{Encrypter, random_token};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{Intent, SocialError};

/// The prefix of sealed states (format version 1).
pub(crate) const STATE_PREFIX: &str = "nvs1.";

/// What a sealed state holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StatePayload {
    /// A random id: the cache remembers used ids.
    pub(crate) id: String,
    /// The provider key the flow was started for.
    pub(crate) provider: String,
    /// The PKCE code verifier.
    pub(crate) verifier: String,
    /// The OpenID Connect nonce.
    pub(crate) nonce: String,
    /// Sign-in or link.
    pub(crate) intent: Intent,
    /// For a link flow: the user who started it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) user: Option<Uuid>,
    /// The relative path to return to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) return_to: Option<String>,
    /// Expiry, in seconds since the Unix epoch.
    pub(crate) exp: i64,
    /// When a native sign-in attempt started (its nonce was issued), in
    /// seconds since the Unix epoch: the identity token must be newer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) iat: Option<i64>,
}

impl StatePayload {
    /// A new state for `provider` with fresh random values.
    pub(crate) fn new(
        provider: &str,
        intent: Intent,
        user: Option<Uuid>,
        return_to: Option<String>,
        exp: i64,
    ) -> Self {
        Self {
            id: random_token(22),
            provider: provider.to_owned(),
            // RFC 7636: 43 to 128 characters of [A-Za-z0-9-._~].
            verifier: random_token(64),
            nonce: random_token(32),
            intent,
            user,
            return_to,
            exp,
            iat: None,
        }
    }

    /// The sealed form, `nvs1.<encrypted payload>`.
    pub(crate) fn seal(&self, encrypter: &Encrypter) -> Result<String, SocialError> {
        Ok(format!("{STATE_PREFIX}{}", encrypter.encrypt_json(self)?))
    }

    /// Opens a sealed state: [`SocialError::InvalidState`] for anything
    /// that was not sealed with this app's key (or a previous one).
    pub(crate) fn open(encrypter: &Encrypter, sealed: &str) -> Result<Self, SocialError> {
        let payload = sealed
            .strip_prefix(STATE_PREFIX)
            .ok_or(SocialError::InvalidState)?;
        encrypter
            .decrypt_json(payload)
            .map_err(|_| SocialError::InvalidState)
    }

    /// The S256 PKCE challenge of the verifier.
    pub(crate) fn challenge(&self) -> String {
        pkce_challenge(&self.verifier)
    }
}

/// `BASE64URL(SHA256(verifier))` (RFC 7636 §4.2).
pub(crate) fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Checks a `return_to` value: a path on this site (`/deals?tab=won`),
/// never a URL to another one (`https://evil.test`, `//evil.test`,
/// `/\evil.test`).
pub(crate) fn check_return_to(value: &str) -> Result<(), SocialError> {
    let bytes = value.as_bytes();
    let relative = bytes.first() == Some(&b'/')
        && !matches!(bytes.get(1), Some(b'/' | b'\\'))
        && !value.chars().any(|c| c.is_control())
        && value.len() <= 2048;
    if relative {
        Ok(())
    } else {
        Err(SocialError::InvalidReturnTo)
    }
}

#[cfg(test)]
mod tests {
    use navree_crypto::{AppKey, generate_key};

    use super::*;

    fn encrypter() -> Encrypter {
        Encrypter::new(AppKey::parse(&generate_key()).unwrap())
    }

    #[test]
    fn states_round_trip_and_tampering_is_refused() {
        let encrypter = encrypter();
        let state = StatePayload::new("github", Intent::SignIn, None, Some("/deals".into()), 99);
        let sealed = state.seal(&encrypter).unwrap();
        assert!(sealed.starts_with("nvs1."));
        assert_eq!(StatePayload::open(&encrypter, &sealed).unwrap(), state);

        let mut tampered = sealed.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert!(matches!(
            StatePayload::open(&encrypter, &tampered),
            Err(SocialError::InvalidState)
        ));
        assert!(StatePayload::open(&encrypter, "nvs1.garbage").is_err());
        assert!(StatePayload::open(&encrypter, sealed.trim_start_matches("nvs1.")).is_err());
        // Another app's key.
        assert!(StatePayload::open(&self::encrypter(), &sealed).is_err());
    }

    #[test]
    fn each_state_has_fresh_secrets() {
        let a = StatePayload::new("github", Intent::SignIn, None, None, 0);
        let b = StatePayload::new("github", Intent::SignIn, None, None, 0);
        assert_ne!(a.id, b.id);
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.nonce, b.nonce);
        assert!((43..=128).contains(&a.verifier.len()));
    }

    #[test]
    fn the_pkce_challenge_is_s256() {
        // RFC 7636, Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn only_relative_paths_return() {
        for ok in ["/", "/deals", "/deals?tab=won#top"] {
            assert!(check_return_to(ok).is_ok(), "{ok}");
        }
        for bad in [
            "https://evil.test",
            "//evil.test",
            "/\\evil.test",
            "deals",
            "",
            "javascript:alert(1)",
            "/a\nb",
        ] {
            assert!(check_return_to(bad).is_err(), "{bad}");
        }
    }
}

//! The sealed state: what a connect flow carries from the consent screen to
//! the callback.
//!
//! The state is the JSON [`StatePayload`] encrypted and authenticated with
//! the application key (navree-crypto's AES-256-GCM `Encrypter`), behind
//! the prefix `nvi1.`. It holds the provider, the scope set, the owner, the
//! admin who started the flow, the PKCE verifier, the return path and the
//! expiry, so nothing is stored until the callback, where its id is
//! remembered until it expires (a state works once).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_crypto::{Encrypter, random_token};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{IntegrationsError, Owner};

/// The prefix of sealed states (format version 1).
pub(crate) const STATE_PREFIX: &str = "nvi1.";

/// What a sealed state holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StatePayload {
    /// A random id: used ids are remembered.
    pub(crate) id: String,
    /// The provider key.
    pub(crate) provider: String,
    /// The scope set.
    pub(crate) scope_set: String,
    /// Who the connection is for.
    pub(crate) owner: Owner,
    /// The admin who started the flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) user: Option<Uuid>,
    /// The PKCE code verifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) verifier: Option<String>,
    /// The relative path to return to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) return_to: Option<String>,
    /// Expiry, in seconds since the Unix epoch.
    pub(crate) exp: i64,
}

impl StatePayload {
    /// A new state with a fresh id and, with `pkce`, a fresh verifier.
    pub(crate) fn new(
        provider: &str,
        scope_set: &str,
        owner: Owner,
        user: Option<Uuid>,
        return_to: Option<String>,
        pkce: bool,
        exp: i64,
    ) -> Self {
        Self {
            id: random_token(22),
            provider: provider.to_owned(),
            scope_set: scope_set.to_owned(),
            owner,
            user,
            // RFC 7636: 43 to 128 characters of [A-Za-z0-9-._~].
            verifier: pkce.then(|| random_token(64)),
            return_to,
            exp,
        }
    }

    /// The sealed form, `nvi1.<encrypted payload>`.
    pub(crate) fn seal(&self, encrypter: &Encrypter) -> Result<String, IntegrationsError> {
        Ok(format!("{STATE_PREFIX}{}", encrypter.encrypt_json(self)?))
    }

    /// Opens a sealed state: [`IntegrationsError::InvalidState`] for
    /// anything that was not sealed with this app's key.
    pub(crate) fn open(encrypter: &Encrypter, sealed: &str) -> Result<Self, IntegrationsError> {
        let payload = sealed
            .strip_prefix(STATE_PREFIX)
            .ok_or(IntegrationsError::InvalidState)?;
        encrypter
            .decrypt_json(payload)
            .map_err(|_| IntegrationsError::InvalidState)
    }

    /// The S256 PKCE challenge of the verifier.
    pub(crate) fn challenge(&self) -> Option<String> {
        self.verifier.as_deref().map(pkce_challenge)
    }
}

/// `BASE64URL(SHA256(verifier))` (RFC 7636 §4.2).
pub(crate) fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Checks a `return_to` value: a path on this site, never a URL to another
/// one (`https://evil.test`, `//evil.test`, `/\evil.test`).
pub(crate) fn check_return_to(value: &str) -> Result<(), IntegrationsError> {
    let bytes = value.as_bytes();
    let relative = bytes.first() == Some(&b'/')
        && !matches!(bytes.get(1), Some(b'/' | b'\\'))
        && !value.chars().any(char::is_control)
        && value.len() <= 2048;
    if relative {
        Ok(())
    } else {
        Err(IntegrationsError::InvalidReturnTo)
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
        let state = StatePayload::new(
            "microsoft",
            "directory",
            Owner::Workspace(Uuid::from_u128(5)),
            Some(Uuid::from_u128(1)),
            Some("/settings".into()),
            true,
            99,
        );
        let sealed = state.seal(&encrypter).unwrap();
        assert!(sealed.starts_with("nvi1."));
        assert_eq!(StatePayload::open(&encrypter, &sealed).unwrap(), state);

        let mut tampered = sealed.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert!(matches!(
            StatePayload::open(&encrypter, &tampered),
            Err(IntegrationsError::InvalidState)
        ));
        assert!(StatePayload::open(&encrypter, "nvi1.garbage").is_err());
        assert!(StatePayload::open(&encrypter, sealed.trim_start_matches("nvi1.")).is_err());
        assert!(StatePayload::open(&self::encrypter(), &sealed).is_err());
    }

    #[test]
    fn pkce_is_optional_and_s256() {
        let without = StatePayload::new("slack", "default", Owner::App, None, None, false, 0);
        assert_eq!(without.challenge(), None);
        let with = StatePayload::new("google", "default", Owner::App, None, None, true, 0);
        assert!((43..=128).contains(&with.verifier.as_ref().unwrap().len()));
        // RFC 7636, Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn only_relative_paths_return() {
        for ok in ["/", "/settings/integrations", "/s?tab=a#b"] {
            assert!(check_return_to(ok).is_ok(), "{ok}");
        }
        for bad in [
            "https://evil.test",
            "//evil.test",
            "/\\evil.test",
            "x",
            "",
            "/a\nb",
        ] {
            assert!(check_return_to(bad).is_err(), "{bad}");
        }
    }
}

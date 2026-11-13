//! The WebAuthn data passkeys are verified with (W3C Web Authentication
//! Level 3): client data, authenticator data, COSE public keys and
//! assertion signatures, on `ring`.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{ECDSA_P256_SHA256_ASN1, ED25519, UnparsedPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::cbor::Cbor;

/// COSE algorithm ES256 (ECDSA with P-256 and SHA-256).
pub(crate) const ES256: i32 = -7;
/// COSE algorithm EdDSA (Ed25519).
pub(crate) const EDDSA: i32 = -8;

/// Authenticator data flag: the user was present.
pub(crate) const FLAG_UP: u8 = 0x01;
/// Authenticator data flag: the user was verified (PIN, biometrics).
pub(crate) const FLAG_UV: u8 = 0x04;
/// Authenticator data flag: attested credential data follows.
pub(crate) const FLAG_AT: u8 = 0x40;

/// base64url, with or without padding.
pub(crate) fn b64url(value: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(value.trim_end_matches('=')).ok()
}

/// SHA-256 of `bytes`.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// The parts of `clientDataJSON` that are checked.
#[derive(Debug, Deserialize)]
pub(crate) struct ClientData {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) challenge: String,
    pub(crate) origin: String,
    #[serde(default, rename = "crossOrigin")]
    pub(crate) cross_origin: bool,
}

impl ClientData {
    pub(crate) fn parse(json: &[u8]) -> Option<Self> {
        serde_json::from_slice(json).ok()
    }
}

/// A credential's public key (from its COSE_Key).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CredentialKey {
    /// [`ES256`] or [`EDDSA`].
    pub(crate) algorithm: i32,
    /// The uncompressed P-256 point, or the 32 Ed25519 bytes.
    pub(crate) bytes: Vec<u8>,
}

impl CredentialKey {
    /// The key of a COSE_Key map: EC2 P-256 with ES256, or OKP Ed25519 with
    /// EdDSA (RFC 9053); anything else is refused.
    pub(crate) fn from_cose(key: &Cbor) -> Option<Self> {
        let kty = key.get_int(1)?.as_int()?;
        let alg = key.get_int(3)?.as_int()?;
        let crv = key.get_int(-1)?.as_int()?;
        let x = key.get_int(-2)?.as_bytes()?;
        match (kty, alg, crv) {
            (2, -7, 1) => {
                let y = key.get_int(-3)?.as_bytes()?;
                if x.len() != 32 || y.len() != 32 {
                    return None;
                }
                let mut point = Vec::with_capacity(65);
                point.push(0x04);
                point.extend_from_slice(x);
                point.extend_from_slice(y);
                Some(Self {
                    algorithm: ES256,
                    bytes: point,
                })
            }
            (1, -8, 6) if x.len() == 32 => Some(Self {
                algorithm: EDDSA,
                bytes: x.to_vec(),
            }),
            _ => None,
        }
    }

    /// Whether `signature` signs `message` (ES256 signatures are ASN.1 DER,
    /// as authenticators produce them).
    pub(crate) fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        match self.algorithm {
            ES256 => UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, &self.bytes)
                .verify(message, signature)
                .is_ok(),
            EDDSA => UnparsedPublicKey::new(&ED25519, &self.bytes)
                .verify(message, signature)
                .is_ok(),
            _ => false,
        }
    }
}

/// Parsed authenticator data.
#[derive(Debug)]
pub(crate) struct AuthenticatorData {
    pub(crate) rp_id_hash: [u8; 32],
    pub(crate) flags: u8,
    pub(crate) sign_count: u32,
    /// With [`FLAG_AT`]: the credential id and its key.
    pub(crate) credential: Option<(Vec<u8>, CredentialKey)>,
}

impl AuthenticatorData {
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 37 {
            return None;
        }
        let mut rp_id_hash = [0_u8; 32];
        rp_id_hash.copy_from_slice(&bytes[..32]);
        let flags = bytes[32];
        let sign_count = u32::from_be_bytes([bytes[33], bytes[34], bytes[35], bytes[36]]);
        let rest = &bytes[37..];
        let credential = if flags & FLAG_AT != 0 {
            // AAGUID (16), credential id length (2), credential id, COSE key.
            if rest.len() < 18 {
                return None;
            }
            let len = usize::from(u16::from_be_bytes([rest[16], rest[17]]));
            let rest = &rest[18..];
            if rest.len() < len || len == 0 || len > 1023 {
                return None;
            }
            let (id, rest) = rest.split_at(len);
            let (cose, _extensions) = Cbor::decode(rest)?;
            Some((id.to_vec(), CredentialKey::from_cose(&cose)?))
        } else {
            None
        };
        Some(Self {
            rp_id_hash,
            flags,
            sign_count,
            credential,
        })
    }

    pub(crate) fn user_present(&self) -> bool {
        self.flags & FLAG_UP != 0
    }

    pub(crate) fn user_verified(&self) -> bool {
        self.flags & FLAG_UV != 0
    }
}

/// The `authData` of an attestation object (`{fmt, attStmt, authData}`).
/// The attestation statement is not checked: registrations ask for no
/// attestation (`attestation: "none"`).
pub(crate) fn attestation_auth_data(attestation_object: &[u8]) -> Option<Vec<u8>> {
    let (object, _) = Cbor::decode(attestation_object)?;
    object.get_text("fmt")?.as_text()?;
    Some(object.get_text("authData")?.as_bytes()?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cose_es256(x: &[u8], y: &[u8]) -> Cbor {
        Cbor::Map(vec![
            (Cbor::Int(1), Cbor::Int(2)),
            (Cbor::Int(3), Cbor::Int(-7)),
            (Cbor::Int(-1), Cbor::Int(1)),
            (Cbor::Int(-2), Cbor::Bytes(x.to_vec())),
            (Cbor::Int(-3), Cbor::Bytes(y.to_vec())),
        ])
    }

    #[test]
    fn cose_keys_of_the_two_algorithms_are_read() {
        let key = CredentialKey::from_cose(&cose_es256(&[1; 32], &[2; 32])).unwrap();
        assert_eq!(key.algorithm, ES256);
        assert_eq!(key.bytes.len(), 65);
        assert_eq!(key.bytes[0], 0x04);
        let okp = Cbor::Map(vec![
            (Cbor::Int(1), Cbor::Int(1)),
            (Cbor::Int(3), Cbor::Int(-8)),
            (Cbor::Int(-1), Cbor::Int(6)),
            (Cbor::Int(-2), Cbor::Bytes(vec![3; 32])),
        ]);
        assert_eq!(
            CredentialKey::from_cose(&okp),
            Some(CredentialKey {
                algorithm: EDDSA,
                bytes: vec![3; 32]
            })
        );
        // RS256 (-257), P-384, short coordinates: refused.
        for refused in [
            Cbor::Map(vec![
                (Cbor::Int(1), Cbor::Int(3)),
                (Cbor::Int(3), Cbor::Int(-257)),
                (Cbor::Int(-1), Cbor::Bytes(vec![1; 256])),
                (Cbor::Int(-2), Cbor::Bytes(vec![1, 0, 1])),
            ]),
            Cbor::Map(vec![
                (Cbor::Int(1), Cbor::Int(2)),
                (Cbor::Int(3), Cbor::Int(-35)),
                (Cbor::Int(-1), Cbor::Int(2)),
                (Cbor::Int(-2), Cbor::Bytes(vec![1; 48])),
                (Cbor::Int(-3), Cbor::Bytes(vec![1; 48])),
            ]),
            cose_es256(&[1; 31], &[2; 32]),
            Cbor::Int(1),
        ] {
            assert_eq!(CredentialKey::from_cose(&refused), None);
        }
    }

    #[test]
    fn authenticator_data_is_parsed() {
        let mut bytes = vec![9_u8; 32];
        bytes.push(FLAG_UP | FLAG_UV | FLAG_AT);
        bytes.extend_from_slice(&7_u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        bytes.extend_from_slice(&[0xC0, 0xFF, 0xEE]);
        bytes.extend_from_slice(&cose_es256(&[1; 32], &[2; 32]).encode());
        let data = AuthenticatorData::parse(&bytes).unwrap();
        assert_eq!(data.rp_id_hash, [9; 32]);
        assert!(data.user_present() && data.user_verified());
        assert_eq!(data.sign_count, 7);
        let (id, key) = data.credential.unwrap();
        assert_eq!(id, [0xC0, 0xFF, 0xEE]);
        assert_eq!(key.algorithm, ES256);

        // Truncated in every part.
        for len in [0, 36, 40, 55, 57, 60] {
            assert!(AuthenticatorData::parse(&bytes[..len]).is_none(), "{len}");
        }
        // Without attested credential data.
        let mut plain = vec![9_u8; 32];
        plain.push(FLAG_UP);
        plain.extend_from_slice(&0_u32.to_be_bytes());
        let data = AuthenticatorData::parse(&plain).unwrap();
        assert!(data.credential.is_none() && !data.user_verified());
    }

    // The client data of a native app's assertion, in the shapes the
    // platforms document: Android's Credential Manager names the app's
    // signature and package; an Apple app signs for `https://<rp_id>`.
    #[test]
    fn native_client_data_is_read() {
        let android = br#"{"type":"webauthn.get","challenge":"T1xCsnxM2DNL2KdK5CLa6fMhD7OBqho6syzInk_n-Uo","origin":"android:apk-key-hash:FG3pg8VzBlDY7rmVLzT8ZBagg0LmHb6oigSWsj_PROU","androidPackageName":"com.example.orders"}"#;
        let data = ClientData::parse(android).unwrap();
        assert_eq!(data.kind, "webauthn.get");
        assert_eq!(
            data.origin,
            "android:apk-key-hash:FG3pg8VzBlDY7rmVLzT8ZBagg0LmHb6oigSWsj_PROU"
        );
        assert!(!data.cross_origin);
        let apple = br#"{"type":"webauthn.get","challenge":"T1xCsnxM2DNL2KdK5CLa6fMhD7OBqho6syzInk_n-Uo","origin":"https://example.com"}"#;
        assert_eq!(
            ClientData::parse(apple).unwrap().origin,
            "https://example.com"
        );
    }

    #[test]
    fn client_data_and_base64url_are_read() {
        let json = br#"{"type":"webauthn.get","challenge":"abc","origin":"https://app.example.com","crossOrigin":false,"other":1}"#;
        let data = ClientData::parse(json).unwrap();
        assert_eq!(data.kind, "webauthn.get");
        assert_eq!(data.origin, "https://app.example.com");
        assert!(!data.cross_origin);
        assert!(ClientData::parse(b"{}").is_none());
        assert_eq!(b64url("AQID"), Some(vec![1, 2, 3]));
        assert_eq!(b64url("AQI="), Some(vec![1, 2]));
        assert_eq!(b64url("!!"), None);
    }
}

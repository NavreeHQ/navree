//! Verification of compact JWS tokens signed by a trusted issuer, on
//! `ring`: RS256, ES256 and EdDSA (Ed25519), with keys from a JWK, a JWK
//! Set or a PEM `PUBLIC KEY`.
//!
//! This is the auth module's small verification seam. It verifies only
//! (no signing), refuses `alg: none`, HMAC algorithms and critical header
//! extensions, and checks that the key fits the algorithm. `navree_crypto::jose`
//! also verifies, and signs, with more algorithms; this seam is the exchange's
//! own, smaller check.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::signature::{
    ECDSA_P256_SHA256_FIXED, ED25519, RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents,
    UnparsedPublicKey,
};
use serde::Deserialize;
use serde_json::Value;

use super::der;

/// The signature algorithms a trusted issuer may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Alg {
    Rs256,
    Es256,
    EdDsa,
}

impl Alg {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "RS256" => Some(Alg::Rs256),
            "ES256" => Some(Alg::Es256),
            "EdDSA" => Some(Alg::EdDsa),
            _ => None,
        }
    }
}

/// A public key that verifies one algorithm's signatures.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum PublicKey {
    /// RSA modulus and exponent (big-endian).
    Rsa { n: Vec<u8>, e: Vec<u8> },
    /// An RSAPublicKey (PKCS#1) DER document.
    RsaDer(Vec<u8>),
    /// An uncompressed P-256 point (`0x04 || x || y`).
    P256(Vec<u8>),
    /// 32 Ed25519 bytes.
    Ed25519(Vec<u8>),
}

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PublicKey::Rsa { .. } | PublicKey::RsaDer(_) => "PublicKey::Rsa",
            PublicKey::P256(_) => "PublicKey::P256",
            PublicKey::Ed25519(_) => "PublicKey::Ed25519",
        })
    }
}

impl PublicKey {
    fn fits(&self, alg: Alg) -> bool {
        matches!(
            (self, alg),
            (PublicKey::Rsa { .. } | PublicKey::RsaDer(_), Alg::Rs256)
                | (PublicKey::P256(_), Alg::Es256)
                | (PublicKey::Ed25519(_), Alg::EdDsa)
        )
    }

    /// Whether `signature` signs `message` with `alg`.
    pub(crate) fn verify(&self, alg: Alg, message: &[u8], signature: &[u8]) -> bool {
        if !self.fits(alg) {
            return false;
        }
        match self {
            PublicKey::Rsa { n, e } => RsaPublicKeyComponents { n, e }
                .verify(&RSA_PKCS1_2048_8192_SHA256, message, signature)
                .is_ok(),
            PublicKey::RsaDer(der) => UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, der)
                .verify(message, signature)
                .is_ok(),
            PublicKey::P256(point) => UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
                .verify(message, signature)
                .is_ok(),
            PublicKey::Ed25519(key) => UnparsedPublicKey::new(&ED25519, key)
                .verify(message, signature)
                .is_ok(),
        }
    }
}

/// A key with its key id, from a JWK or a PEM document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeyEntry {
    pub(crate) kid: Option<String>,
    pub(crate) key: PublicKey,
}

/// The fields of a JWK this module reads.
#[derive(Deserialize)]
struct RawJwk {
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default, rename = "use")]
    usage: Option<String>,
    #[serde(default)]
    crv: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
    #[serde(default)]
    x: Option<String>,
    #[serde(default)]
    y: Option<String>,
}

/// The public key of a JWK (`RSA`, `EC` P-256 or `OKP` Ed25519); `None`
/// for other keys, encryption keys and malformed ones.
pub(crate) fn key_from_jwk(value: &Value) -> Option<KeyEntry> {
    let jwk: RawJwk = serde_json::from_value(value.clone()).ok()?;
    if jwk.usage.as_deref().is_some_and(|usage| usage != "sig") {
        return None;
    }
    let field = |v: &Option<String>| v.as_deref().and_then(|v| URL_SAFE_NO_PAD.decode(v).ok());
    let key = match (jwk.kty.as_str(), jwk.crv.as_deref()) {
        ("RSA", _) => {
            let n = field(&jwk.n)?;
            let e = field(&jwk.e)?;
            PublicKey::Rsa {
                n: strip_leading_zeros(n),
                e,
            }
        }
        ("EC", Some("P-256")) => {
            let (x, y) = (field(&jwk.x)?, field(&jwk.y)?);
            if x.len() != 32 || y.len() != 32 {
                return None;
            }
            let mut point = Vec::with_capacity(65);
            point.push(0x04);
            point.extend_from_slice(&x);
            point.extend_from_slice(&y);
            PublicKey::P256(point)
        }
        ("OKP", Some("Ed25519")) => {
            let x = field(&jwk.x)?;
            if x.len() != 32 {
                return None;
            }
            PublicKey::Ed25519(x)
        }
        _ => return None,
    };
    Some(KeyEntry { kid: jwk.kid, key })
}

/// The usable keys of a JWK Set (`{"keys": [...]}`).
pub(crate) fn keys_from_jwks(value: &Value) -> Vec<KeyEntry> {
    value["keys"]
        .as_array()
        .map(|keys| keys.iter().filter_map(key_from_jwk).collect())
        .unwrap_or_default()
}

/// The key of `text`: a JWK as JSON, or a PEM `PUBLIC KEY`
/// (SubjectPublicKeyInfo: RSA, P-256 or Ed25519).
pub(crate) fn key_from_text(text: &str) -> Option<KeyEntry> {
    let text = text.trim();
    if text.starts_with('{') {
        let value: Value = serde_json::from_str(text).ok()?;
        return key_from_jwk(&value);
    }
    let body: String = text
        .strip_prefix("-----BEGIN PUBLIC KEY-----")?
        .trim()
        .strip_suffix("-----END PUBLIC KEY-----")?
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let der = STANDARD.decode(body).ok()?;
    let key = der::public_key_from_spki(&der)?;
    Some(KeyEntry { kid: None, key })
}

/// A parsed compact JWS: header, payload and what was signed.
#[derive(Debug)]
pub(crate) struct CompactJws {
    pub(crate) alg: Alg,
    pub(crate) kid: Option<String>,
    pub(crate) payload: Value,
    signing_input: String,
    signature: Vec<u8>,
}

impl CompactJws {
    /// Parses `token`; refuses unsupported algorithms (`none`, HMAC, ...)
    /// and critical header extensions.
    pub(crate) fn parse(token: &str) -> Result<Self, &'static str> {
        let mut parts = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err("malformed");
        };
        let decode = |part: &str| URL_SAFE_NO_PAD.decode(part).map_err(|_| "malformed");
        let header_json: Value =
            serde_json::from_slice(&decode(header)?).map_err(|_| "malformed")?;
        let alg = header_json["alg"]
            .as_str()
            .and_then(Alg::parse)
            .ok_or("unsupported algorithm")?;
        if header_json.get("crit").is_some() {
            return Err("critical header extension");
        }
        let payload_json: Value =
            serde_json::from_slice(&decode(payload)?).map_err(|_| "malformed")?;
        if !payload_json.is_object() {
            return Err("malformed");
        }
        let signature = decode(signature)?;
        if signature.is_empty() {
            return Err("unsigned");
        }
        Ok(Self {
            alg,
            kid: header_json["kid"].as_str().map(str::to_owned),
            payload: payload_json,
            signing_input: format!("{header}.{payload}"),
            signature,
        })
    }

    /// Whether one of `keys` (the one with the token's `kid` when it has
    /// one) signed the token.
    pub(crate) fn verified_by(&self, keys: &[KeyEntry]) -> bool {
        keys.iter()
            .filter(|entry| match (&self.kid, &entry.kid) {
                (Some(kid), Some(key_kid)) => kid == key_kid,
                _ => true,
            })
            .any(|entry| {
                entry
                    .key
                    .verify(self.alg, self.signing_input.as_bytes(), &self.signature)
            })
    }

    /// Whether a key with the token's `kid` is among `keys` (a token
    /// without `kid` matches any).
    pub(crate) fn kid_known(&self, keys: &[KeyEntry]) -> bool {
        match &self.kid {
            Some(kid) => keys.iter().any(|entry| entry.kid.as_deref() == Some(kid)),
            None => !keys.is_empty(),
        }
    }
}

fn strip_leading_zeros(mut bytes: Vec<u8>) -> Vec<u8> {
    let zeros = bytes.iter().take_while(|b| **b == 0).count();
    bytes.drain(..zeros.min(bytes.len().saturating_sub(1)));
    bytes
}

#[cfg(test)]
mod tests {
    use ring::rand::SystemRandom;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};
    use serde_json::json;

    use super::*;

    /// RFC 7515 appendix A.2: an RS256 JWS and its public key.
    const RFC7515_A2_TOKEN: &str = "eyJhbGciOiJSUzI1NiJ9\
        .eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ\
        .cC4hiUPoj9Eetdgtv3hF80EGrhuB__dzERat0XF9g2VtQgr9PJbu3XOiZj5RZmh7AAuHIm4Bh-0Qc_lF5YKt_O8W2Fp5jujGbds9uJdbF9CUAr7t1dnZcAcQjbKBYNX4BAynRFdiuB--f_nZLgrnbyTyWzO75vRK5h6xBArLIARNPvkSjtQBMHlb1L07Qe7K0GarZRmB_eSN9383LcOLn6_dO--xi12jzDwusC-eOkHWEsqtFZESc6BfI7noOPqvhJ1phCnvWh6IeYI2w9QOYEUipUTI8np6LbgGY9Fs98rqVt5AXLIhWkWywlVmtVrBp0igcN_IoypGlUPQGe77Rw";

    const RFC7515_A2_JWK: &str = r#"{"kty":"RSA",
        "n":"ofgWCuLjybRlzo0tZWJjNiuSfb4p4fAkd_wWJcyQoTbji9k0l8W26mPddxHmfHQp-Vaw-4qPCJrcS2mJPMEzP1Pt0Bm4d4QlL-yRT-SFd2lZS-pCgNMsD1W_YpRPEwOWvG6b32690r2jZ47soMZo9wGzjb_7OMg0LOL-bSf63kpaSHSXndS5z5rexMdbBYUsLA9e-KXBdQOS-UTo7WTBEMa2R2CapHg665xsmtdVMTBQY4uDZlxvb3qCo5ZwKh9kG4LT6_I5IhlJH7aGhyxXFvUK-DWNmoudF8NAco9_h9iaGNj8q2ethFkMLs91kzk2PAcDTW9gb54h4FRWyuXpoQ",
        "e":"AQAB"}"#;

    /// RFC 7515 appendix A.3: an ES256 JWS and its public key.
    const RFC7515_A3_TOKEN: &str = "eyJhbGciOiJFUzI1NiJ9\
        .eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ\
        .DtEhU3ljbEg8L38VWAfUAqOyKAM6-Xx-F4GawxaepmXFCgfTjDxw5djxLa8ISlSApmWQxfKTUJqPP3-Kg6NU1Q";

    const RFC7515_A3_JWK: &str = r#"{"kty":"EC","crv":"P-256",
        "x":"f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
        "y":"x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"}"#;

    /// RFC 8037 appendix A.4: an EdDSA JWS and its public key.
    const RFC8037_A4_TOKEN: &str = "eyJhbGciOiJFZERTQSJ9.RXhhbXBsZSBvZiBFZDI1NTE5IHNpZ25pbmc\
        .hgyY0il_MGCjP0JzlnLWG1PPOt7-09PGcvMg3AIbQR6dWbhijcNR4ki4iylGjg5BhVsPt9g7sVvpAr_MuM0KAg";

    const RFC8037_A4_JWK: &str = r#"{"kty":"OKP","crv":"Ed25519",
        "x":"11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"}"#;

    fn jwk(text: &str) -> KeyEntry {
        key_from_text(text).expect("a JWK")
    }

    #[test]
    fn rfc_vectors_verify_and_tampered_tokens_do_not() {
        let rs = CompactJws::parse(RFC7515_A2_TOKEN).unwrap();
        assert_eq!(rs.alg, Alg::Rs256);
        assert!(rs.verified_by(&[jwk(RFC7515_A2_JWK)]));
        assert_eq!(rs.payload["iss"], "joe");

        let es = CompactJws::parse(RFC7515_A3_TOKEN).unwrap();
        assert_eq!(es.alg, Alg::Es256);
        assert!(es.verified_by(&[jwk(RFC7515_A3_JWK)]));

        // The RFC 8037 payload is not JSON: re-sign is impossible, so check
        // the signature directly.
        let parts: Vec<&str> = RFC8037_A4_TOKEN.split('.').collect();
        let key = jwk(RFC8037_A4_JWK).key;
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        let input = format!("{}.{}", parts[0], parts[1]);
        assert!(key.verify(Alg::EdDsa, input.as_bytes(), &signature));
        assert!(!key.verify(Alg::EdDsa, b"another message", &signature));

        // Wrong keys and algorithm confusion.
        assert!(!rs.verified_by(&[jwk(RFC7515_A3_JWK)]));
        assert!(!es.verified_by(&[jwk(RFC7515_A2_JWK), jwk(RFC8037_A4_JWK)]));

        // A tampered payload.
        let mut tampered: Vec<String> = RFC7515_A3_TOKEN.split('.').map(str::to_owned).collect();
        tampered[1] = URL_SAFE_NO_PAD.encode(br#"{"iss":"mallory"}"#);
        let tampered = CompactJws::parse(&tampered.join(".")).unwrap();
        assert!(!tampered.verified_by(&[jwk(RFC7515_A3_JWK)]));
    }

    #[test]
    fn unsupported_and_malformed_tokens_do_not_parse() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"1"}"#);
        for header in [
            r#"{"alg":"none"}"#,
            r#"{"alg":"HS256"}"#,
            r#"{"alg":"RS512"}"#,
            r#"{"alg":"ES256","crit":["exp"]}"#,
        ] {
            let token = format!("{}.{payload}.c2ln", URL_SAFE_NO_PAD.encode(header));
            assert!(CompactJws::parse(&token).is_err(), "{header}");
        }
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256"}"#);
        for token in [
            String::new(),
            "a.b".to_owned(),
            format!("{header}.{payload}."),
            format!("{header}.{payload}.c2ln.extra"),
            format!("{header}.bm90IGpzb24.c2ln"),
            format!("{header}.WzFd.c2ln"),
        ] {
            assert!(CompactJws::parse(&token).is_err(), "{token}");
        }
    }

    #[test]
    fn generated_keys_verify_through_jwks_and_pem() {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        let point = pair.public_key().as_ref();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256","kid":"k1"}"#);
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"7"}"#);
        let input = format!("{header}.{payload}");
        let signature = pair.sign(&rng, input.as_bytes()).unwrap();
        let token = format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()));
        let jws = CompactJws::parse(&token).unwrap();

        let jwks = json!({"keys": [
            {"kty": "EC", "crv": "P-256", "kid": "other",
             "x": URL_SAFE_NO_PAD.encode(&point[1..33]), "y": URL_SAFE_NO_PAD.encode(&point[33..])},
            {"kty": "EC", "crv": "P-256", "kid": "k1", "use": "sig",
             "x": URL_SAFE_NO_PAD.encode(&point[1..33]), "y": URL_SAFE_NO_PAD.encode(&point[33..])},
            {"kty": "EC", "crv": "P-256", "kid": "enc", "use": "enc",
             "x": URL_SAFE_NO_PAD.encode(&point[1..33]), "y": URL_SAFE_NO_PAD.encode(&point[33..])},
            {"kty": "oct", "k": "c2VjcmV0"},
        ]});
        let keys = keys_from_jwks(&jwks);
        assert_eq!(keys.len(), 2, "encryption and HMAC keys are skipped");
        assert!(jws.kid_known(&keys));
        assert!(jws.verified_by(&keys));
        // Only the key with the token's kid is tried.
        let only_other: Vec<KeyEntry> = keys
            .iter()
            .filter(|k| k.kid.as_deref() == Some("other"))
            .cloned()
            .collect();
        assert!(!jws.kid_known(&only_other));
        assert!(!jws.verified_by(&only_other));

        // The same key as a PEM SubjectPublicKeyInfo.
        let spki = der::p256_spki(point);
        let pem = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(spki)
        );
        let entry = key_from_text(&pem).unwrap();
        assert!(jws.verified_by(&[entry]));

        // Ed25519 through PEM.
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA"}"#);
        let input = format!("{header}.{payload}");
        let token = format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(pair.sign(input.as_bytes()).as_ref())
        );
        let pem = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----",
            STANDARD.encode(der::ed25519_spki(pair.public_key().as_ref()))
        );
        let jws = CompactJws::parse(&token).unwrap();
        assert!(jws.verified_by(&[key_from_text(&pem).unwrap()]));
    }

    #[test]
    fn keys_never_show_in_debug_output() {
        let entry = jwk(RFC7515_A3_JWK);
        let debug = format!("{entry:?}");
        assert!(debug.contains("P256"), "{debug}");
        assert!(!debug.contains("127"), "{debug}");
    }

    #[test]
    fn malformed_keys_are_refused() {
        for text in [
            r#"{"kty":"EC","crv":"P-384","x":"AA","y":"AA"}"#,
            r#"{"kty":"EC","crv":"P-256","x":"AA","y":"AA"}"#,
            r#"{"kty":"OKP","crv":"X25519","x":"11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"}"#,
            r#"{"kty":"RSA","n":"AQAB"}"#,
            "not a key",
            "-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----",
        ] {
            assert!(key_from_text(text).is_none(), "{text}");
        }
    }
}

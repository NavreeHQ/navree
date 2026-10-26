//! navree-auth's jsonwebtoken [`CryptoProvider`]: HMAC (HS256, HS384,
//! HS512) on RustCrypto's `hmac` and `sha2`, and nothing else.
//!
//! jsonwebtoken's `rust_crypto` backend compiles in RSA (`rsa`,
//! RUSTSEC-2023-0071) and ECDSA (`p256`/`p384` on `crypto-bigint` 0.5,
//! RUSTSEC-2026-0387) although navree only signs HS256, and its `aws_lc_rs`
//! backend needs CMake and NASM on Windows. With neither feature enabled,
//! jsonwebtoken takes a process-level provider instead: this one, installed
//! by [`JwtService::new`](super::JwtService::new).

use hmac::{Hmac, KeyInit, Mac};
use jsonwebtoken::crypto::{CryptoProvider, JwtSigner, JwtVerifier, KeyUtils};
use jsonwebtoken::errors::{Error, ErrorKind, Result};
use jsonwebtoken::jwk::ThumbprintHash;
use jsonwebtoken::signature::{self, Signer, Verifier};
use jsonwebtoken::{Algorithm, AlgorithmFamily, DecodingKey, EncodingKey};
use sha2::{Digest, Sha256, Sha384, Sha512};

/// The provider. Signers and verifiers exist for HS256, HS384 and HS512
/// only; any other algorithm, and the RSA, EC and EdDSA key helpers of JWK
/// support, fail with [`ErrorKind::UnsupportedAlgorithm`].
pub(super) static HMAC_ONLY: CryptoProvider = CryptoProvider {
    signer_factory: new_signer,
    verifier_factory: new_verifier,
    key_utils: KeyUtils {
        rsa_pub_components_from_private_key: |_| Err(unsupported()),
        rsa_pub_components_from_public_key: |_| Err(unsupported()),
        ec_pub_components_from_private_key: |_, _| Err(unsupported()),
        ed_pub_components_from_private_key: |_, _| Err(unsupported()),
        compute_digest,
    },
};

/// Makes [`HMAC_ONLY`] jsonwebtoken's process-level provider unless one is
/// installed already: the first install in a process wins (as with
/// rustls), so an app that needs other algorithms installs its provider
/// before building the auth module. Installing again is a no-op.
pub(super) fn install() {
    // `Err` only means a provider is installed already (this one, or the
    // app's); HS256 works with any of them, and `JwtService` pins HS256.
    let _ = HMAC_ONLY.install_default();
}

fn new_signer(alg: &Algorithm, key: &EncodingKey) -> Result<Box<dyn JwtSigner>> {
    let hash = HmacHash::for_algorithm(*alg)?;
    require_secret(key.family())?;
    Ok(Box::new(hash.keyed(key.as_bytes())?))
}

fn new_verifier(alg: &Algorithm, key: &DecodingKey) -> Result<Box<dyn JwtVerifier>> {
    let hash = HmacHash::for_algorithm(*alg)?;
    require_secret(key.family())?;
    Ok(Box::new(hash.keyed(key.try_get_as_bytes()?)?))
}

fn require_secret(family: AlgorithmFamily) -> Result<()> {
    match family {
        AlgorithmFamily::Hmac => Ok(()),
        _ => Err(ErrorKind::InvalidKeyFormat.into()),
    }
}

fn compute_digest(data: &[u8], hash: ThumbprintHash) -> Result<Vec<u8>> {
    Ok(match hash {
        ThumbprintHash::SHA256 => Sha256::digest(data).to_vec(),
        ThumbprintHash::SHA384 => Sha384::digest(data).to_vec(),
        ThumbprintHash::SHA512 => Sha512::digest(data).to_vec(),
        _ => return Err(unsupported()),
    })
}

fn unsupported() -> Error {
    ErrorKind::UnsupportedAlgorithm.into()
}

/// The hash of an HMAC algorithm.
#[derive(Clone, Copy)]
enum HmacHash {
    Sha256,
    Sha384,
    Sha512,
}

impl HmacHash {
    /// [`ErrorKind::UnsupportedAlgorithm`] unless `alg` is HS256, HS384 or HS512.
    fn for_algorithm(alg: Algorithm) -> Result<Self> {
        match alg {
            Algorithm::HS256 => Ok(Self::Sha256),
            Algorithm::HS384 => Ok(Self::Sha384),
            Algorithm::HS512 => Ok(Self::Sha512),
            _ => Err(unsupported()),
        }
    }

    fn keyed(self, secret: &[u8]) -> Result<KeyedHmac> {
        Ok(match self {
            Self::Sha256 => KeyedHmac::Hs256(new_mac(secret)?),
            Self::Sha384 => KeyedHmac::Hs384(new_mac(secret)?),
            Self::Sha512 => KeyedHmac::Hs512(new_mac(secret)?),
        })
    }
}

fn new_mac<M: KeyInit>(secret: &[u8]) -> Result<M> {
    M::new_from_slice(secret).map_err(|_| ErrorKind::InvalidKeyFormat.into())
}

/// An HMAC keyed with the secret; signing and verifying work on a clone.
enum KeyedHmac {
    Hs256(Hmac<Sha256>),
    Hs384(Hmac<Sha384>),
    Hs512(Hmac<Sha512>),
}

impl KeyedHmac {
    fn algorithm(&self) -> Algorithm {
        match self {
            Self::Hs256(_) => Algorithm::HS256,
            Self::Hs384(_) => Algorithm::HS384,
            Self::Hs512(_) => Algorithm::HS512,
        }
    }
}

/// The tag of `message`.
fn tag<M: Mac + Clone>(mac: &M, message: &[u8]) -> Vec<u8> {
    let mut mac = mac.clone();
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

/// Whether `tag` is the full-length tag of `message`, compared in constant
/// time.
fn verify_tag<M: Mac + Clone>(mac: &M, message: &[u8], tag: &[u8]) -> bool {
    let mut mac = mac.clone();
    mac.update(message);
    mac.verify_slice(tag).is_ok()
}

impl Signer<Vec<u8>> for KeyedHmac {
    fn try_sign(&self, message: &[u8]) -> std::result::Result<Vec<u8>, signature::Error> {
        Ok(match self {
            Self::Hs256(mac) => tag(mac, message),
            Self::Hs384(mac) => tag(mac, message),
            Self::Hs512(mac) => tag(mac, message),
        })
    }
}

impl Verifier<Vec<u8>> for KeyedHmac {
    fn verify(
        &self,
        message: &[u8],
        signature: &Vec<u8>,
    ) -> std::result::Result<(), signature::Error> {
        let valid = match self {
            Self::Hs256(mac) => verify_tag(mac, message, signature),
            Self::Hs384(mac) => verify_tag(mac, message, signature),
            Self::Hs512(mac) => verify_tag(mac, message, signature),
        };
        if valid {
            Ok(())
        } else {
            Err(signature::Error::new())
        }
    }
}

impl JwtSigner for KeyedHmac {
    fn algorithm(&self) -> Algorithm {
        KeyedHmac::algorithm(self)
    }
}

impl JwtVerifier for KeyedHmac {
    fn algorithm(&self) -> Algorithm {
        KeyedHmac::algorithm(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_algorithms_need_an_hmac_secret() {
        for alg in [Algorithm::HS256, Algorithm::HS384, Algorithm::HS512] {
            let Err(error) = (HMAC_ONLY.signer_factory)(&alg, &EncodingKey::from_rsa_der(b"0"))
            else {
                panic!("{alg:?} signer with an RSA key");
            };
            assert!(matches!(error.kind(), ErrorKind::InvalidKeyFormat));
            let Err(error) = (HMAC_ONLY.verifier_factory)(&alg, &DecodingKey::from_ec_der(b"0"))
            else {
                panic!("{alg:?} verifier with an EC key");
            };
            assert!(matches!(error.kind(), ErrorKind::InvalidKeyFormat));

            let Ok(signer) = (HMAC_ONLY.signer_factory)(&alg, &EncodingKey::from_secret(b"k"))
            else {
                panic!("no {alg:?} signer");
            };
            assert_eq!(signer.algorithm(), alg);
        }
    }

    #[test]
    fn thumbprint_digests_are_sha2() {
        let digest = |hash| (HMAC_ONLY.key_utils.compute_digest)(b"abc", hash).unwrap();
        // FIPS 180-2 "abc" vectors (leading bytes).
        assert_eq!(
            digest(ThumbprintHash::SHA256)[..4],
            [0xba, 0x78, 0x16, 0xbf]
        );
        assert_eq!(
            digest(ThumbprintHash::SHA384)[..4],
            [0xcb, 0x00, 0x75, 0x3f]
        );
        assert_eq!(
            digest(ThumbprintHash::SHA512)[..4],
            [0xdd, 0xaf, 0x35, 0xa1]
        );
    }

    #[test]
    fn jwk_key_helpers_refuse_rsa_ec_and_eddsa() {
        let utils = &HMAC_ONLY.key_utils;
        assert!(matches!(
            (utils.rsa_pub_components_from_private_key)(b"0")
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedAlgorithm
        ));
        assert!(matches!(
            (utils.rsa_pub_components_from_public_key)(b"0")
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedAlgorithm
        ));
        assert!(matches!(
            (utils.ec_pub_components_from_private_key)(b"0", Algorithm::ES256)
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedAlgorithm
        ));
        assert!(matches!(
            (utils.ed_pub_components_from_private_key)(
                b"0",
                &jsonwebtoken::jwk::EllipticCurve::Ed25519
            )
            .unwrap_err()
            .kind(),
            ErrorKind::UnsupportedAlgorithm
        ));
    }
}

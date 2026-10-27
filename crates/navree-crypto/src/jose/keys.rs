//! [`SigningKey`] and [`VerifyingKey`]: private and public keys on `ring`,
//! loaded from PEM, PKCS#8, SubjectPublicKeyInfo, JWK or raw parts.

use std::fmt;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::Hmac;
use ring::rand::{SecureRandom as _, SystemRandom};
use ring::signature::{
    self, EcdsaKeyPair, Ed25519KeyPair, KeyPair as _, RsaKeyPair, RsaPublicKeyComponents,
    UnparsedPublicKey,
};
use serde::Serialize;
use sha2::{Sha256, Sha384, Sha512};

use super::der::{self, Curve, PrivateKeyInfo, PublicKeyInfo};
use super::{Algorithm, Header, JoseError, Jwk};

const NOT_PKCS8: JoseError = JoseError::InvalidKey("not a PKCS#8 private key of a supported type");
const SHORT_SECRET: JoseError =
    JoseError::InvalidKey("an HMAC secret must be at least as long as the hash output");
const BAD_RSA: JoseError = JoseError::InvalidKey("not an RSA private key of 2048 to 8192 bits");
const BAD_EC: JoseError = JoseError::InvalidKey("not a valid EC key pair");
const BAD_ED25519: JoseError = JoseError::InvalidKey("not a valid Ed25519 key pair");
const WRONG_ALG: JoseError = JoseError::InvalidKey("the key does not use that algorithm");

/// The private half.
enum Private {
    Hmac(Vec<u8>),
    Ed25519 {
        pair: Ed25519KeyPair,
        seed: [u8; 32],
    },
    Ecdsa {
        pair: EcdsaKeyPair,
        curve: Curve,
        d: Vec<u8>,
    },
    Rsa {
        pair: RsaKeyPair,
        /// The PKCS#1 document, when the key came from one (for export).
        pkcs1: Option<Vec<u8>>,
    },
}

/// A private key that signs JWS: an HMAC secret (HS256/384/512), an
/// Ed25519 key (EdDSA), a P-256 or P-384 key (ES256, ES384) or an RSA key
/// (RS256/384/512, PS256/384/512).
///
/// Load one with [`from_pem`](SigningKey::from_pem) (PKCS#8, such as an
/// Apple `.p8` file; PKCS#1 and SEC 1 too), [`from_jwk`](SigningKey::from_jwk)
/// or [`hmac`](SigningKey::hmac), or generate one. Cloning is cheap: the
/// key is shared.
///
/// `Debug` shows the algorithm and key id, never the key.
#[derive(Clone)]
pub struct SigningKey {
    private: Arc<Private>,
    alg: Algorithm,
    kid: Option<String>,
}

impl SigningKey {
    fn new(private: Private, alg: Algorithm) -> Self {
        Self {
            private: Arc::new(private),
            alg,
            kid: None,
        }
    }

    /// An HMAC key for `alg` (HS256, HS384 or HS512). The secret must be at
    /// least as long as the hash output: 32, 48 or 64 bytes.
    pub fn hmac(alg: Algorithm, secret: impl AsRef<[u8]>) -> Result<Self, JoseError> {
        let secret = checked_secret(alg, secret.as_ref())?;
        Ok(Self::new(Private::Hmac(secret), alg))
    }

    /// The Ed25519 key with this 32-byte seed (the private key of RFC 8032).
    pub fn ed25519_from_seed(seed: &[u8; 32]) -> Self {
        let pair =
            Ed25519KeyPair::from_seed_unchecked(seed).expect("any 32 bytes are an Ed25519 seed");
        Self::new(Private::Ed25519 { pair, seed: *seed }, Algorithm::EdDSA)
    }

    /// A new random Ed25519 key.
    ///
    /// # Panics
    ///
    /// If the operating system's random number generator fails (it does not
    /// on supported platforms).
    pub fn generate_ed25519() -> Self {
        let mut seed = [0_u8; 32];
        SystemRandom::new()
            .fill(&mut seed)
            .expect("the operating system's random number generator failed");
        Self::ed25519_from_seed(&seed)
    }

    /// A new random P-256 key (ES256).
    ///
    /// # Panics
    ///
    /// If the operating system's random number generator fails.
    pub fn generate_es256() -> Self {
        let document = EcdsaKeyPair::generate_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &SystemRandom::new(),
        )
        .expect("the operating system's random number generator failed");
        Self::from_pkcs8_der(document.as_ref()).expect("a generated key is valid PKCS#8")
    }

    /// Reads a PKCS#8 DER document: Ed25519 (v1 or v2), P-256, P-384 or
    /// RSA. The algorithm follows from the key; RSA keys sign RS256 unless
    /// [`with_algorithm`](SigningKey::with_algorithm) says otherwise.
    pub fn from_pkcs8_der(der: &[u8]) -> Result<Self, JoseError> {
        match der::private_key_info(der).ok_or(NOT_PKCS8)? {
            PrivateKeyInfo::Ed25519 { seed, public } => Self::ed25519_parts(seed, public),
            PrivateKeyInfo::Ec { curve, d, public } => Self::ec_parts(
                curve,
                d,
                public.ok_or(JoseError::InvalidKey(
                    "the EC private key does not include its public key",
                ))?,
            ),
            PrivateKeyInfo::Rsa { pkcs1 } => Self::rsa_pkcs1(pkcs1),
        }
    }

    /// Reads a PEM private key: `PRIVATE KEY` (PKCS#8, such as Apple's `.p8`
    /// files), `RSA PRIVATE KEY` (PKCS#1) or `EC PRIVATE KEY` (SEC 1).
    ///
    /// Literal `\n` sequences (a key pasted into one environment variable)
    /// and missing armor lines are accepted. Encrypted keys are not.
    pub fn from_pem(pem: &str) -> Result<Self, JoseError> {
        let pem = der::decode_pem(pem)?;
        match pem.label.as_deref() {
            Some("PRIVATE KEY") => Self::from_pkcs8_der(&pem.der),
            Some("RSA PRIVATE KEY") => Self::rsa_pkcs1(&pem.der),
            Some("EC PRIVATE KEY") => Self::sec1(&pem.der),
            None => Self::from_pkcs8_der(&pem.der)
                .or_else(|_| Self::rsa_pkcs1(&pem.der))
                .or_else(|_| Self::sec1(&pem.der))
                .map_err(|_| NOT_PKCS8),
            Some(label) if label.ends_with("PUBLIC KEY") => Err(JoseError::InvalidKey(
                "a public key cannot sign: use VerifyingKey::from_pem",
            )),
            Some("ENCRYPTED PRIVATE KEY") => Err(JoseError::InvalidKey(
                "encrypted PEM keys are not supported",
            )),
            Some(_) => Err(JoseError::InvalidKey("not a PEM private key")),
        }
    }

    /// Reads a private JWK: `oct` (`k`; HS256 unless `alg` says otherwise),
    /// `OKP` Ed25519 (`d`, `x`), `EC` P-256 or P-384 (`d`, `x`, `y`) or
    /// `RSA` (`n`, `e`, `d`, `p`, `q`, `dp`, `dq`, `qi`). The JWK's `kid`
    /// and `alg` carry over.
    pub fn from_jwk(jwk: &Jwk) -> Result<Self, JoseError> {
        let alg = match &jwk.alg {
            Some(name) => Some(name.parse::<Algorithm>()?),
            None => None,
        };
        let no_private = "the JWK has no private key";
        let key = match jwk.kty.as_str() {
            "oct" => Self::hmac(
                alg.unwrap_or(Algorithm::HS256),
                Jwk::member(jwk.k.as_deref(), "the oct JWK has no `k`")?,
            )?,
            "OKP" => {
                if jwk.crv.as_deref() != Some("Ed25519") {
                    return Err(JoseError::InvalidKey("unsupported OKP curve"));
                }
                let seed = Jwk::member(jwk.d.as_deref(), no_private)?;
                let public = Jwk::member(jwk.x.as_deref(), "the OKP JWK has no `x`")?;
                Self::ed25519_parts(&seed, Some(&public))?
            }
            "EC" => {
                let curve = jwk
                    .crv
                    .as_deref()
                    .and_then(Curve::from_name)
                    .ok_or(JoseError::InvalidKey("unsupported EC curve"))?;
                let d = Jwk::member(jwk.d.as_deref(), no_private)?;
                let point = ec_point(curve, jwk)?;
                Self::ec_parts(curve, &d, &point)?
            }
            "RSA" => {
                let public = RsaPublicKeyComponents {
                    n: Jwk::member(jwk.n.as_deref(), "the RSA JWK has no `n`")?,
                    e: Jwk::member(jwk.e.as_deref(), "the RSA JWK has no `e`")?,
                };
                let components = ring::rsa::KeyPairComponents {
                    public_key: public,
                    d: Jwk::member(jwk.d.as_deref(), no_private)?,
                    p: jwk.other_member("p")?,
                    q: jwk.other_member("q")?,
                    dP: jwk.other_member("dp")?,
                    dQ: jwk.other_member("dq")?,
                    qInv: jwk.other_member("qi")?,
                };
                let pair = RsaKeyPair::from_components(&components).map_err(|_| BAD_RSA)?;
                Self::new(Private::Rsa { pair, pkcs1: None }, Algorithm::RS256)
            }
            _ => return Err(JoseError::InvalidKey("unsupported key type")),
        };
        let key = match alg {
            Some(alg) => key.with_algorithm(alg)?,
            None => key,
        };
        Ok(match &jwk.kid {
            Some(kid) => key.with_kid(kid.clone()),
            None => key,
        })
    }

    fn ed25519_parts(seed: &[u8], public: Option<&[u8]>) -> Result<Self, JoseError> {
        let seed: [u8; 32] = seed.try_into().map_err(|_| BAD_ED25519)?;
        let pair = match public {
            Some(public) => Ed25519KeyPair::from_seed_and_public_key(&seed, public),
            None => Ed25519KeyPair::from_seed_unchecked(&seed),
        }
        .map_err(|_| BAD_ED25519)?;
        Ok(Self::new(Private::Ed25519 { pair, seed }, Algorithm::EdDSA))
    }

    fn ec_parts(curve: Curve, d: &[u8], point: &[u8]) -> Result<Self, JoseError> {
        let algorithm = match curve {
            Curve::P256 => &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            Curve::P384 => &signature::ECDSA_P384_SHA384_FIXED_SIGNING,
        };
        let pair = EcdsaKeyPair::from_private_key_and_public_key(
            algorithm,
            d,
            point,
            &SystemRandom::new(),
        )
        .map_err(|_| BAD_EC)?;
        Ok(Self::new(
            Private::Ecdsa {
                pair,
                curve,
                d: d.to_vec(),
            },
            curve.algorithm(),
        ))
    }

    fn rsa_pkcs1(pkcs1: &[u8]) -> Result<Self, JoseError> {
        let pair = RsaKeyPair::from_der(pkcs1).map_err(|_| BAD_RSA)?;
        Ok(Self::new(
            Private::Rsa {
                pair,
                pkcs1: Some(pkcs1.to_vec()),
            },
            Algorithm::RS256,
        ))
    }

    fn sec1(der: &[u8]) -> Result<Self, JoseError> {
        let key = der::ec_private_key(der).ok_or(BAD_EC)?;
        let curve = key.curve.ok_or(JoseError::InvalidKey(
            "the EC private key does not name its curve",
        ))?;
        let point = key.public.ok_or(JoseError::InvalidKey(
            "the EC private key does not include its public key",
        ))?;
        Self::ec_parts(curve, key.d, point)
    }

    /// The key with key id `kid` (the `kid` header of what it signs, and
    /// of its public JWK).
    #[must_use]
    pub fn with_kid(mut self, kid: impl Into<String>) -> Self {
        self.kid = Some(kid.into());
        self
    }

    /// The key with its RFC 7638 thumbprint as key id. An HMAC key keeps
    /// its key id: a thumbprint of a secret is not something to publish.
    #[must_use]
    pub fn with_thumbprint_kid(self) -> Self {
        match self
            .verifying_key()
            .to_jwk()
            .and_then(|jwk| jwk.thumbprint().ok())
        {
            Some(thumbprint) => self.with_kid(thumbprint),
            None => self,
        }
    }

    /// The key signing with `alg` instead: another hash for an HMAC secret
    /// long enough for it, or another RSA algorithm (RS256 to PS512).
    /// Ed25519 and ECDSA keys have one algorithm each.
    pub fn with_algorithm(mut self, alg: Algorithm) -> Result<Self, JoseError> {
        let fits = match &*self.private {
            Private::Hmac(secret) => alg.hmac_len().is_some_and(|min| secret.len() >= min),
            Private::Ed25519 { .. } => alg == Algorithm::EdDSA,
            Private::Ecdsa { curve, .. } => alg == curve.algorithm(),
            Private::Rsa { .. } => alg.is_rsa(),
        };
        if !fits {
            return Err(WRONG_ALG);
        }
        self.alg = alg;
        Ok(self)
    }

    /// The algorithm it signs with.
    pub fn algorithm(&self) -> Algorithm {
        self.alg
    }

    /// The key id.
    pub fn kid(&self) -> Option<&str> {
        self.kid.as_deref()
    }

    /// The key that verifies its signatures: the public half, or the same
    /// secret for HMAC. RSA keys verify the whole RSA family.
    pub fn verifying_key(&self) -> VerifyingKey {
        let (public, alg) = match &*self.private {
            Private::Hmac(secret) => (Public::Hmac(secret.clone()), Some(self.alg)),
            Private::Ed25519 { pair, .. } => {
                let mut bytes = [0_u8; 32];
                bytes.copy_from_slice(pair.public_key().as_ref());
                (Public::Ed25519(bytes), None)
            }
            Private::Ecdsa { pair, curve, .. } => (
                Public::Ec {
                    curve: *curve,
                    point: pair.public_key().as_ref().to_vec(),
                },
                None,
            ),
            Private::Rsa { pair, .. } => {
                let components: RsaPublicKeyComponents<Vec<u8>> = pair.public().into();
                (
                    Public::Rsa {
                        n: components.n,
                        e: components.e,
                    },
                    None,
                )
            }
        };
        VerifyingKey {
            public,
            alg,
            kid: self.kid.clone(),
        }
    }

    /// The public key as a JWK, with `kid`, `alg` and `use: sig`: what a
    /// key set publishes. `None` for an HMAC key.
    pub fn public_jwk(&self) -> Option<Jwk> {
        let mut jwk = self.verifying_key().to_jwk()?;
        jwk.alg = Some(self.alg.as_str().to_owned());
        Some(jwk)
    }

    /// The private key as a PKCS#8 DER document (v1; for Ed25519 the
    /// 48-byte RFC 8410 form). `None` for an HMAC key, and for an RSA key
    /// read from a JWK.
    pub fn to_pkcs8_der(&self) -> Option<Vec<u8>> {
        match &*self.private {
            Private::Hmac(_) => None,
            Private::Ed25519 { seed, .. } => Some(der::ed25519_pkcs8(seed)),
            Private::Ecdsa { pair, curve, d } => {
                Some(der::ec_pkcs8(*curve, d, pair.public_key().as_ref()))
            }
            Private::Rsa { pkcs1, .. } => pkcs1.as_deref().map(der::rsa_pkcs8),
        }
    }

    /// The private key as a PEM `PRIVATE KEY` (PKCS#8). See
    /// [`to_pkcs8_der`](SigningKey::to_pkcs8_der).
    pub fn to_pkcs8_pem(&self) -> Option<String> {
        self.to_pkcs8_der()
            .map(|der| der::encode_pem("PRIVATE KEY", &der))
    }

    /// The header this key signs with: `{"alg", "kid"}`.
    pub fn header(&self) -> Header {
        Header {
            kid: self.kid.clone(),
            ..Header::new(self.alg)
        }
    }

    /// The raw JWS signature of `message`: an HMAC, an Ed25519 signature,
    /// ECDSA `R || S`, or an RSA signature.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, JoseError> {
        match &*self.private {
            Private::Hmac(secret) => Ok(hmac(self.alg, secret, message)),
            Private::Ed25519 { pair, .. } => Ok(pair.sign(message).as_ref().to_vec()),
            Private::Ecdsa { pair, .. } => pair
                .sign(&SystemRandom::new(), message)
                .map(|signature| signature.as_ref().to_vec())
                .map_err(|_| JoseError::Signing),
            Private::Rsa { pair, .. } => {
                let padding: &'static dyn signature::RsaEncoding = match self.alg {
                    Algorithm::RS384 => &signature::RSA_PKCS1_SHA384,
                    Algorithm::RS512 => &signature::RSA_PKCS1_SHA512,
                    Algorithm::PS256 => &signature::RSA_PSS_SHA256,
                    Algorithm::PS384 => &signature::RSA_PSS_SHA384,
                    Algorithm::PS512 => &signature::RSA_PSS_SHA512,
                    _ => &signature::RSA_PKCS1_SHA256,
                };
                let mut signature = vec![0; pair.public().modulus_len()];
                pair.sign(padding, &SystemRandom::new(), message, &mut signature)
                    .map_err(|_| JoseError::Signing)?;
                Ok(signature)
            }
        }
    }

    /// A compact JWS of `payload` with `header`, whose `alg` must be the
    /// key's ([`JoseError::NoKey`] otherwise). The header is used as given:
    /// start from [`header`](SigningKey::header) to include the key id.
    pub fn sign_jws(&self, header: &Header, payload: &[u8]) -> Result<String, JoseError> {
        if header.alg != self.alg {
            return Err(JoseError::NoKey);
        }
        let header = serde_json::to_vec(header).map_err(|_| JoseError::Payload)?;
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header),
            URL_SAFE_NO_PAD.encode(payload)
        );
        let signature = self.sign(input.as_bytes())?;
        Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    /// A compact JWS of `claims` as JSON, with `header`.
    pub fn sign_json<T: Serialize + ?Sized>(
        &self,
        header: &Header,
        claims: &T,
    ) -> Result<String, JoseError> {
        let payload = serde_json::to_vec(claims).map_err(|_| JoseError::Payload)?;
        self.sign_jws(header, &payload)
    }

    /// A JWT: `claims` as JSON, with the header `{"alg", "kid", "typ":
    /// "JWT"}`.
    pub fn sign_jwt<T: Serialize + ?Sized>(&self, claims: &T) -> Result<String, JoseError> {
        self.sign_json(&self.header().with_typ("JWT"), claims)
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey")
            .field("alg", &self.alg)
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

/// The public half.
#[derive(Clone)]
enum Public {
    Hmac(Vec<u8>),
    Rsa { n: Vec<u8>, e: Vec<u8> },
    Ec { curve: Curve, point: Vec<u8> },
    Ed25519([u8; 32]),
}

/// A key that verifies JWS: a public key (RSA, P-256, P-384, Ed25519) or
/// an HMAC secret.
///
/// Load one with [`from_jwk`](VerifyingKey::from_jwk),
/// [`from_pem`](VerifyingKey::from_pem) (SubjectPublicKeyInfo), from raw
/// parts ([`rsa`](VerifyingKey::rsa), [`ec`](VerifyingKey::ec),
/// [`ed25519`](VerifyingKey::ed25519), [`hmac`](VerifyingKey::hmac)), or
/// take [`SigningKey::verifying_key`]. A key verifies only the algorithms
/// of its type ([`accepts`](VerifyingKey::accepts)), so a token can never
/// pick an algorithm the key was not meant for.
///
/// `Debug` shows the key type, algorithm and key id, never an HMAC secret.
#[derive(Clone)]
pub struct VerifyingKey {
    public: Public,
    /// The one algorithm the key verifies, when pinned.
    alg: Option<Algorithm>,
    kid: Option<String>,
}

impl VerifyingKey {
    fn new(public: Public) -> Self {
        Self {
            public,
            alg: None,
            kid: None,
        }
    }

    /// An HMAC key for `alg` (HS256, HS384 or HS512), at least as long as
    /// the hash output.
    pub fn hmac(alg: Algorithm, secret: impl AsRef<[u8]>) -> Result<Self, JoseError> {
        let secret = checked_secret(alg, secret.as_ref())?;
        Ok(Self {
            alg: Some(alg),
            ..Self::new(Public::Hmac(secret))
        })
    }

    /// An RSA public key from its modulus and public exponent (big-endian;
    /// leading zeros are ignored). The modulus must have 2048 to 8192 bits.
    pub fn rsa(n: &[u8], e: &[u8]) -> Result<Self, JoseError> {
        let (n, e) = (strip_zeros(n), strip_zeros(e));
        if !(256..=1024).contains(&n.len()) {
            return Err(JoseError::InvalidKey(
                "an RSA key must have 2048 to 8192 bits",
            ));
        }
        if e.is_empty() || e.len() > 8 {
            return Err(JoseError::InvalidKey("invalid RSA public exponent"));
        }
        Ok(Self::new(Public::Rsa {
            n: n.to_vec(),
            e: e.to_vec(),
        }))
    }

    /// An EC public key for `alg` (ES256 or ES384) from its coordinates.
    pub fn ec(alg: Algorithm, x: &[u8], y: &[u8]) -> Result<Self, JoseError> {
        let curve =
            Curve::of(alg).ok_or(JoseError::InvalidKey("an EC key verifies ES256 or ES384"))?;
        Self::ec_point(curve, &[&[0x04], x, y].concat())
    }

    fn ec_point(curve: Curve, point: &[u8]) -> Result<Self, JoseError> {
        if point.len() != 1 + 2 * curve.coordinate_len() || point[0] != 0x04 {
            return Err(JoseError::InvalidKey(
                "the EC public key is not an uncompressed point of its curve",
            ));
        }
        Ok(Self::new(Public::Ec {
            curve,
            point: point.to_vec(),
        }))
    }

    /// An Ed25519 public key (32 bytes).
    pub fn ed25519(public_key: &[u8]) -> Result<Self, JoseError> {
        let bytes: [u8; 32] = public_key
            .try_into()
            .map_err(|_| JoseError::InvalidKey("an Ed25519 public key is 32 bytes"))?;
        Ok(Self::new(Public::Ed25519(bytes)))
    }

    /// Reads a JWK's public members: `RSA` (`n`, `e`), `EC` P-256 or P-384
    /// (`x`, `y`), `OKP` Ed25519 (`x`) or `oct` (`k`). Its `kid` carries
    /// over, and its `alg` pins the algorithm.
    pub fn from_jwk(jwk: &Jwk) -> Result<Self, JoseError> {
        let key = match jwk.kty.as_str() {
            "RSA" => Self::rsa(
                &Jwk::member(jwk.n.as_deref(), "the RSA JWK has no `n`")?,
                &Jwk::member(jwk.e.as_deref(), "the RSA JWK has no `e`")?,
            )?,
            "EC" => {
                let curve = jwk
                    .crv
                    .as_deref()
                    .and_then(Curve::from_name)
                    .ok_or(JoseError::InvalidKey("unsupported EC curve"))?;
                Self::ec_point(curve, &ec_point(curve, jwk)?)?
            }
            "OKP" => {
                if jwk.crv.as_deref() != Some("Ed25519") {
                    return Err(JoseError::InvalidKey("unsupported OKP curve"));
                }
                Self::ed25519(&Jwk::member(jwk.x.as_deref(), "the OKP JWK has no `x`")?)?
            }
            "oct" => {
                let secret = Jwk::member(jwk.k.as_deref(), "the oct JWK has no `k`")?;
                if secret.len() < 32 {
                    return Err(SHORT_SECRET);
                }
                Self::new(Public::Hmac(secret))
            }
            _ => return Err(JoseError::InvalidKey("unsupported key type")),
        };
        let key = match &jwk.alg {
            Some(name) => key.with_algorithm(name.parse()?)?,
            None => key,
        };
        Ok(match &jwk.kid {
            Some(kid) => key.with_kid(kid.clone()),
            None => key,
        })
    }

    /// Reads a PEM public key: `PUBLIC KEY` (SubjectPublicKeyInfo: RSA,
    /// P-256, P-384 or Ed25519), `RSA PUBLIC KEY` (PKCS#1) or the public
    /// key of a `CERTIFICATE` (see
    /// [`from_certificate_der`](VerifyingKey::from_certificate_der)).
    pub fn from_pem(pem: &str) -> Result<Self, JoseError> {
        let pem = der::decode_pem(pem)?;
        match pem.label.as_deref() {
            Some("PUBLIC KEY") | None => Self::from_spki_der(&pem.der),
            Some("RSA PUBLIC KEY") => Self::from_public_key_info(der::rsa_public_key(&pem.der)),
            Some("CERTIFICATE") => Self::from_certificate_der(&pem.der),
            Some(_) => Err(JoseError::InvalidKey(
                "not a PEM public key: read a private key with SigningKey::from_pem",
            )),
        }
    }

    /// Reads a SubjectPublicKeyInfo DER document.
    pub fn from_spki_der(der: &[u8]) -> Result<Self, JoseError> {
        Self::from_public_key_info(der::public_key_info(der))
    }

    /// Reads the public key of an X.509 certificate (DER), such as the
    /// signing certificate in a SAML identity provider's metadata. Only
    /// the key is read: the certificate's own signature, validity dates and
    /// extensions are not checked, so trust the certificate for where it
    /// came from (configured metadata), not for what it says.
    pub fn from_certificate_der(der: &[u8]) -> Result<Self, JoseError> {
        Self::from_public_key_info(der::certificate_public_key_info(der))
    }

    fn from_public_key_info(info: Option<PublicKeyInfo<'_>>) -> Result<Self, JoseError> {
        match info.ok_or(JoseError::InvalidKey(
            "not a public key of a supported type",
        ))? {
            PublicKeyInfo::Ed25519(key) => Self::ed25519(key),
            PublicKeyInfo::Ec { curve, point } => Self::ec_point(curve, point),
            PublicKeyInfo::Rsa { n, e } => Self::rsa(n, e),
        }
    }

    /// The key with key id `kid`.
    #[must_use]
    pub fn with_kid(mut self, kid: impl Into<String>) -> Self {
        self.kid = Some(kid.into());
        self
    }

    /// The key verifying `alg` only. The algorithm must fit the key type.
    pub fn with_algorithm(mut self, alg: Algorithm) -> Result<Self, JoseError> {
        self.alg = None;
        if !self.accepts(alg) {
            return Err(WRONG_ALG);
        }
        self.alg = Some(alg);
        Ok(self)
    }

    /// The key id.
    pub fn kid(&self) -> Option<&str> {
        self.kid.as_deref()
    }

    /// The algorithm the key verifies: pinned, or the only one its type
    /// allows (ES256 for P-256, ES384 for P-384, EdDSA). `None` for an RSA
    /// or HMAC key that verifies several.
    pub fn algorithm(&self) -> Option<Algorithm> {
        self.alg.or(match &self.public {
            Public::Ec { curve, .. } => Some(curve.algorithm()),
            Public::Ed25519(_) => Some(Algorithm::EdDSA),
            Public::Hmac(_) | Public::Rsa { .. } => None,
        })
    }

    /// Whether the key verifies `alg`.
    pub fn accepts(&self, alg: Algorithm) -> bool {
        if self.alg.is_some_and(|pinned| pinned != alg) {
            return false;
        }
        match &self.public {
            Public::Hmac(secret) => alg.hmac_len().is_some_and(|min| secret.len() >= min),
            Public::Rsa { .. } => alg.is_rsa(),
            Public::Ec { curve, .. } => alg == curve.algorithm(),
            Public::Ed25519(_) => alg == Algorithm::EdDSA,
        }
    }

    /// Verifies a raw JWS signature of `message` with `alg`:
    /// [`JoseError::NoKey`] when the key does not verify `alg`,
    /// [`JoseError::BadSignature`] when the signature is wrong. ECDSA
    /// signatures are `R || S`, as in JWS.
    pub fn verify(
        &self,
        alg: Algorithm,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), JoseError> {
        self.check(alg, message, signature, false)
    }

    /// Like [`verify`](VerifyingKey::verify), but ECDSA signatures are DER
    /// (`ECDSA-Sig-Value`), as WebAuthn sends them. Other algorithms have
    /// one form.
    pub fn verify_der(
        &self,
        alg: Algorithm,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), JoseError> {
        self.check(alg, message, signature, true)
    }

    fn check(
        &self,
        alg: Algorithm,
        message: &[u8],
        sig: &[u8],
        der: bool,
    ) -> Result<(), JoseError> {
        if !self.accepts(alg) {
            return Err(JoseError::NoKey);
        }
        let verified = match &self.public {
            Public::Hmac(secret) => {
                return if verify_hmac(alg, secret, message, sig) {
                    Ok(())
                } else {
                    Err(JoseError::BadSignature)
                };
            }
            Public::Rsa { n, e } => {
                let params: &signature::RsaParameters = match alg {
                    Algorithm::RS384 => &signature::RSA_PKCS1_2048_8192_SHA384,
                    Algorithm::RS512 => &signature::RSA_PKCS1_2048_8192_SHA512,
                    Algorithm::PS256 => &signature::RSA_PSS_2048_8192_SHA256,
                    Algorithm::PS384 => &signature::RSA_PSS_2048_8192_SHA384,
                    Algorithm::PS512 => &signature::RSA_PSS_2048_8192_SHA512,
                    _ => &signature::RSA_PKCS1_2048_8192_SHA256,
                };
                RsaPublicKeyComponents { n, e }.verify(params, message, sig)
            }
            Public::Ec { curve, point } => {
                let algorithm: &'static signature::EcdsaVerificationAlgorithm = match (curve, der) {
                    (Curve::P256, false) => &signature::ECDSA_P256_SHA256_FIXED,
                    (Curve::P256, true) => &signature::ECDSA_P256_SHA256_ASN1,
                    (Curve::P384, false) => &signature::ECDSA_P384_SHA384_FIXED,
                    (Curve::P384, true) => &signature::ECDSA_P384_SHA384_ASN1,
                };
                UnparsedPublicKey::new(algorithm, point).verify(message, sig)
            }
            Public::Ed25519(key) => {
                UnparsedPublicKey::new(&signature::ED25519, key).verify(message, sig)
            }
        };
        verified.map_err(|_| JoseError::BadSignature)
    }

    /// The key as a public JWK, with `kid`, `alg` (when
    /// [`algorithm`](VerifyingKey::algorithm) has one) and `use: sig`.
    /// `None` for an HMAC key.
    pub fn to_jwk(&self) -> Option<Jwk> {
        let encode = |bytes: &[u8]| Some(URL_SAFE_NO_PAD.encode(bytes));
        let mut jwk = match &self.public {
            Public::Hmac(_) => return None,
            Public::Rsa { n, e } => Jwk {
                kty: "RSA".into(),
                n: encode(n),
                e: encode(e),
                ..Jwk::default()
            },
            Public::Ec { curve, point } => {
                let (x, y) = point[1..].split_at(curve.coordinate_len());
                Jwk {
                    kty: "EC".into(),
                    crv: Some(curve.name().into()),
                    x: encode(x),
                    y: encode(y),
                    ..Jwk::default()
                }
            }
            Public::Ed25519(key) => Jwk {
                kty: "OKP".into(),
                crv: Some("Ed25519".into()),
                x: encode(key),
                ..Jwk::default()
            },
        };
        jwk.use_ = Some("sig".into());
        jwk.alg = self.algorithm().map(|alg| alg.as_str().to_owned());
        jwk.kid.clone_from(&self.kid);
        Some(jwk)
    }
}

impl fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.public {
            Public::Hmac(_) => "HMAC",
            Public::Rsa { .. } => "RSA",
            Public::Ec {
                curve: Curve::P256, ..
            } => "P-256",
            Public::Ec {
                curve: Curve::P384, ..
            } => "P-384",
            Public::Ed25519(_) => "Ed25519",
        };
        f.debug_struct("VerifyingKey")
            .field("type", &kind)
            .field("alg", &self.alg)
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

/// The uncompressed point `04 || x || y` of an `EC` JWK.
fn ec_point(curve: Curve, jwk: &Jwk) -> Result<Vec<u8>, JoseError> {
    let x = Jwk::member(jwk.x.as_deref(), "the EC JWK has no `x`")?;
    let y = Jwk::member(jwk.y.as_deref(), "the EC JWK has no `y`")?;
    if x.len() != curve.coordinate_len() || y.len() != curve.coordinate_len() {
        return Err(JoseError::InvalidKey(
            "the EC coordinates do not fit the curve",
        ));
    }
    Ok([&[0x04], x.as_slice(), y.as_slice()].concat())
}

fn strip_zeros(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len());
    &bytes[start..]
}

/// `secret` if it is long enough for HMAC `alg`.
fn checked_secret(alg: Algorithm, secret: &[u8]) -> Result<Vec<u8>, JoseError> {
    let min = alg.hmac_len().ok_or(JoseError::InvalidKey(
        "an HMAC key uses HS256, HS384 or HS512",
    ))?;
    if secret.len() < min {
        return Err(SHORT_SECRET);
    }
    Ok(secret.to_vec())
}

fn hmac(alg: Algorithm, secret: &[u8], message: &[u8]) -> Vec<u8> {
    fn mac<M: hmac::Mac + hmac::KeyInit>(secret: &[u8], message: &[u8]) -> Vec<u8> {
        let mut mac =
            <M as hmac::KeyInit>::new_from_slice(secret).expect("HMAC accepts keys of any length");
        mac.update(message);
        mac.finalize().into_bytes().to_vec()
    }
    match alg {
        Algorithm::HS384 => mac::<Hmac<Sha384>>(secret, message),
        Algorithm::HS512 => mac::<Hmac<Sha512>>(secret, message),
        _ => mac::<Hmac<Sha256>>(secret, message),
    }
}

/// Compares the HMAC in constant time.
fn verify_hmac(alg: Algorithm, secret: &[u8], message: &[u8], signature: &[u8]) -> bool {
    fn check<M: hmac::Mac + hmac::KeyInit>(secret: &[u8], message: &[u8], sig: &[u8]) -> bool {
        let Ok(mut mac) = <M as hmac::KeyInit>::new_from_slice(secret) else {
            return false;
        };
        mac.update(message);
        mac.verify_slice(sig).is_ok()
    }
    match alg {
        Algorithm::HS256 => check::<Hmac<Sha256>>(secret, message, signature),
        Algorithm::HS384 => check::<Hmac<Sha384>>(secret, message, signature),
        Algorithm::HS512 => check::<Hmac<Sha512>>(secret, message, signature),
        _ => false,
    }
}

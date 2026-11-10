//! [`Jwk`] and [`JwkSet`]: JSON Web Keys (RFC 7517, RFC 8037) and their
//! thumbprints (RFC 7638).

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use super::{Algorithm, JoseError};

/// The private members kept in [`Jwk::other`] (RSA's CRT parameters).
const PRIVATE_OTHER_MEMBERS: [&str; 6] = ["p", "q", "dp", "dq", "qi", "oth"];

/// A JSON Web Key.
///
/// The members Navree reads have fields; every other member (`x5c`,
/// `x5t`, a provider's `issuer`, RSA's `p` and `q`...) is kept in
/// [`other`](Jwk::other), so a key set survives a round trip unchanged.
/// Binary members are base64url strings, as on the wire.
///
/// `Debug` never shows private members (`d`, `k`, `p`...).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwk {
    /// The key type: `RSA`, `EC`, `OKP` or `oct`.
    pub kty: String,
    /// `sig` or `enc`.
    #[serde(rename = "use", default, skip_serializing_if = "Option::is_none")]
    pub use_: Option<String>,
    /// The operations the key is for (`sign`, `verify`...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_ops: Option<Vec<String>>,
    /// The one algorithm the key is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,
    /// The key id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
    /// The curve of an `EC` or `OKP` key: `P-256`, `P-384` or `Ed25519`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    /// The x coordinate (`EC`) or the public key (`OKP`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    /// The y coordinate (`EC`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    /// The modulus (`RSA`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    /// The public exponent (`RSA`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,
    /// The private key (`EC`, `OKP`) or private exponent (`RSA`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    /// The secret of an `oct` key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k: Option<String>,
    /// Every other member.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Jwk {
    /// Whether the key holds private material (`d`, `k`, or RSA's CRT
    /// parameters).
    pub fn is_private(&self) -> bool {
        self.d.is_some()
            || self.k.is_some()
            || PRIVATE_OTHER_MEMBERS
                .iter()
                .any(|member| self.other.contains_key(*member))
    }

    /// The key without its private members: what a key set publishes.
    pub fn to_public(&self) -> Jwk {
        let mut public = self.clone();
        public.d = None;
        public.k = None;
        for member in PRIVATE_OTHER_MEMBERS {
            public.other.remove(member);
        }
        public
    }

    /// The `alg` member, when it names an [`Algorithm`].
    pub fn algorithm(&self) -> Option<Algorithm> {
        self.alg.as_deref().and_then(|alg| alg.parse().ok())
    }

    /// The RFC 7638 thumbprint: the base64url SHA-256 of the key's required
    /// members, in lexicographic order. It is the same for a private key and
    /// its public half, so it makes a stable key id.
    pub fn thumbprint(&self) -> Result<String, JoseError> {
        let required: &[(&str, &Option<String>)] = match self.kty.as_str() {
            "RSA" => &[("e", &self.e), ("n", &self.n)],
            "EC" => &[("crv", &self.crv), ("x", &self.x), ("y", &self.y)],
            "OKP" => &[("crv", &self.crv), ("x", &self.x)],
            "oct" => &[("k", &self.k)],
            _ => return Err(JoseError::InvalidKey("unsupported key type")),
        };
        let mut members: Vec<(&str, &str)> = vec![("kty", self.kty.as_str())];
        for (name, value) in required {
            let value = value.as_deref().ok_or(JoseError::InvalidKey(
                "the JWK lacks a member its thumbprint needs",
            ))?;
            members.push((name, value));
        }
        members.sort_unstable_by_key(|(name, _)| *name);
        let mut canonical = String::from("{");
        for (i, (name, value)) in members.iter().enumerate() {
            if i > 0 {
                canonical.push(',');
            }
            canonical.push_str(&Value::from(*name).to_string());
            canonical.push(':');
            canonical.push_str(&Value::from(*value).to_string());
        }
        canonical.push('}');
        Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes())))
    }

    /// The bytes of a base64url member.
    pub(crate) fn member(value: Option<&str>, missing: &'static str) -> Result<Vec<u8>, JoseError> {
        let value = value.ok_or(JoseError::InvalidKey(missing))?;
        URL_SAFE_NO_PAD
            .decode(value.trim_end_matches('='))
            .map_err(|_| JoseError::InvalidKey("a JWK member is not base64url"))
    }

    /// The bytes of a base64url member of [`other`](Jwk::other).
    pub(crate) fn other_member(&self, name: &str) -> Result<Vec<u8>, JoseError> {
        Self::member(
            self.other.get(name).and_then(Value::as_str),
            "the RSA JWK lacks a private member",
        )
    }
}

impl fmt::Debug for Jwk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Jwk");
        debug.field("kty", &self.kty);
        let public = [
            ("use", &self.use_),
            ("alg", &self.alg),
            ("kid", &self.kid),
            ("crv", &self.crv),
            ("x", &self.x),
            ("y", &self.y),
            ("n", &self.n),
            ("e", &self.e),
        ];
        for (name, value) in public {
            if let Some(value) = value {
                debug.field(name, value);
            }
        }
        if let Some(ops) = &self.key_ops {
            debug.field("key_ops", ops);
        }
        if self.is_private() {
            debug.field("private", &"[redacted]");
        }
        let other: Vec<&String> = self
            .other
            .keys()
            .filter(|name| !PRIVATE_OTHER_MEMBERS.contains(&name.as_str()))
            .collect();
        if !other.is_empty() {
            debug.field("other", &other);
        }
        debug.finish()
    }
}

/// A JSON Web Key Set: `{"keys": [...]}`.
///
/// Reading a set skips the entries that are not keys (RFC 7517 §5), so one
/// odd key in a provider's set does not make the others unusable. Keys of
/// unknown types are kept; they never verify anything.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct JwkSet {
    /// The keys.
    pub keys: Vec<Jwk>,
}

impl JwkSet {
    /// A set of `keys`.
    pub fn new(keys: impl IntoIterator<Item = Jwk>) -> Self {
        Self {
            keys: keys.into_iter().collect(),
        }
    }

    /// The key with key id `kid`.
    pub fn find(&self, kid: &str) -> Option<&Jwk> {
        self.keys.iter().find(|key| key.kid.as_deref() == Some(kid))
    }

    /// Whether the set has a key with key id `kid`.
    pub fn has_kid(&self, kid: &str) -> bool {
        self.find(kid).is_some()
    }

    /// The set without private members: what a JWKS endpoint serves.
    pub fn to_public(&self) -> JwkSet {
        self.keys.iter().map(Jwk::to_public).collect()
    }
}

impl FromIterator<Jwk> for JwkSet {
    fn from_iter<I: IntoIterator<Item = Jwk>>(keys: I) -> Self {
        Self::new(keys)
    }
}

impl<'de> Deserialize<'de> for JwkSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            keys: Vec<Value>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(raw
            .keys
            .into_iter()
            .filter_map(|key| serde_json::from_value(key).ok())
            .collect())
    }
}

//! [`Header`] and [`Jws`]: the compact serialization of RFC 7515.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::de::DeserializeOwned;
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

use super::{Algorithm, JoseError, Jwk, JwkSet, VerifyingKey};

/// The header parameters with a field of their own.
const RESERVED: [&str; 4] = ["alg", "kid", "typ", "cty"];

/// A JWS protected header.
///
/// Serializes as `alg`, `kid`, `typ`, `cty` (those that are set), then the
/// other parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// The algorithm.
    pub alg: Algorithm,
    /// The key id.
    pub kid: Option<String>,
    /// The media type of the whole token (`JWT`).
    pub typ: Option<String>,
    /// The media type of the payload.
    pub cty: Option<String>,
    /// Every other parameter (`x5t`, `jku`...). Navree never fetches or
    /// trusts keys named here.
    pub extra: Map<String, Value>,
}

impl Header {
    /// A header with `alg` only.
    pub fn new(alg: Algorithm) -> Self {
        Self {
            alg,
            kid: None,
            typ: None,
            cty: None,
            extra: Map::new(),
        }
    }

    /// The header with key id `kid`.
    #[must_use]
    pub fn with_kid(mut self, kid: impl Into<String>) -> Self {
        self.kid = Some(kid.into());
        self
    }

    /// The header with type `typ` (`"JWT"`).
    #[must_use]
    pub fn with_typ(mut self, typ: impl Into<String>) -> Self {
        self.typ = Some(typ.into());
        self
    }

    /// The header with one more parameter. `kid`, `typ` and `cty` set their
    /// fields (when `value` is a string); `alg` is the header's algorithm
    /// and is ignored here.
    #[must_use]
    pub fn with_param(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        let name = name.into();
        let value = value.into();
        match (name.as_str(), value) {
            ("kid", Value::String(kid)) => self.kid = Some(kid),
            ("typ", Value::String(typ)) => self.typ = Some(typ),
            ("cty", Value::String(cty)) => self.cty = Some(cty),
            (reserved, _) if RESERVED.contains(&reserved) => {}
            (_, value) => {
                self.extra.insert(name, value);
            }
        }
        self
    }

    /// Reads a header's JSON, refusing what this implementation does not
    /// understand.
    fn from_json(json: &[u8]) -> Result<Self, JoseError> {
        #[derive(Deserialize)]
        struct Raw {
            alg: Option<String>,
            #[serde(default)]
            kid: Option<String>,
            #[serde(default)]
            typ: Option<String>,
            #[serde(default)]
            cty: Option<String>,
            #[serde(default)]
            crit: Option<Value>,
            #[serde(flatten)]
            extra: Map<String, Value>,
        }
        let raw: Raw = serde_json::from_slice(json)
            .map_err(|_| JoseError::Malformed("the header is not a JSON object"))?;
        // RFC 7515 §4.1.11: no extension is understood, so none may be
        // critical.
        if raw.crit.is_some() {
            return Err(JoseError::Malformed(
                "critical header extensions are not supported",
            ));
        }
        let alg = raw
            .alg
            .ok_or(JoseError::Malformed("the header has no `alg`"))?
            .parse()?;
        Ok(Self {
            alg,
            kid: raw.kid,
            typ: raw.typ,
            cty: raw.cty,
            extra: raw.extra,
        })
    }
}

impl Serialize for Header {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("alg", &self.alg)?;
        for (name, value) in [("kid", &self.kid), ("typ", &self.typ), ("cty", &self.cty)] {
            if let Some(value) = value {
                map.serialize_entry(name, value)?;
            }
        }
        for (name, value) in &self.extra {
            if !RESERVED.contains(&name.as_str()) {
                map.serialize_entry(name, value)?;
            }
        }
        map.end()
    }
}

/// A compact JWS, parsed but not yet verified.
///
/// Until [`verify`](Jws::verify) or [`verify_with`](Jws::verify_with)
/// succeeds, the header and payload are whatever the sender wrote: read the
/// `kid` to find the key, and nothing else.
#[derive(Clone, Debug)]
pub struct Jws {
    header: Header,
    payload: Vec<u8>,
    signing_input: String,
    signature: Vec<u8>,
}

fn base64url(part: &str) -> Result<Vec<u8>, JoseError> {
    URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .map_err(|_| JoseError::Malformed("a part is not base64url"))
}

impl Jws {
    /// Parses `header.payload.signature`. Fails on anything else, on an
    /// unsupported `alg` (`none` included) and on critical extensions.
    pub fn parse(token: &str) -> Result<Self, JoseError> {
        let mut parts = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(JoseError::Malformed("expected three parts"));
        };
        let header_json = base64url(header)?;
        let payload_bytes = base64url(payload)?;
        let signature_bytes = base64url(signature)?;
        Ok(Self {
            header: Header::from_json(&header_json)?,
            payload: payload_bytes,
            signing_input: token[..header.len() + 1 + payload.len()].to_owned(),
            signature: signature_bytes,
        })
    }

    /// The protected header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The payload as JSON of type `T` (the claims of a JWT).
    pub fn claims<T: DeserializeOwned>(&self) -> Result<T, JoseError> {
        serde_json::from_slice(&self.payload).map_err(|_| JoseError::Payload)
    }

    /// What the signature covers: `header.payload` as sent.
    pub fn signing_input(&self) -> &str {
        &self.signing_input
    }

    /// The signature bytes.
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }

    /// Verifies the signature with `key`. [`JoseError::NoKey`] when the key
    /// does not verify the header's algorithm, [`JoseError::BadSignature`]
    /// when the signature is wrong.
    pub fn verify(&self, key: &VerifyingKey) -> Result<(), JoseError> {
        key.verify(
            self.header.alg,
            self.signing_input.as_bytes(),
            &self.signature,
        )
    }

    /// Verifies the signature with a key of a public key set, such as an
    /// identity provider's JWKS, and returns the key that verified it.
    ///
    /// The candidates are the keys of the algorithm's type whose `use` (if
    /// any) is `sig`, whose `key_ops` (if any) include `verify`, whose `alg`
    /// (if any) is the header's, and whose `kid` is the header's `kid` when
    /// the header has one. HMAC algorithms are refused: a key set is public,
    /// so a shared secret never belongs in one.
    pub fn verify_with<'k>(&self, keys: &'k JwkSet) -> Result<&'k Jwk, JoseError> {
        let alg = self.header.alg;
        if alg.is_symmetric() {
            return Err(JoseError::UnsupportedAlgorithm(alg.as_str().to_owned()));
        }
        let candidates = keys.keys.iter().filter(|key| {
            key.kty == alg.key_type()
                && key.use_.as_deref().is_none_or(|u| u == "sig")
                && key
                    .key_ops
                    .as_ref()
                    .is_none_or(|ops| ops.iter().any(|op| op == "verify"))
                && key.alg.as_deref().is_none_or(|a| a == alg.as_str())
                && match &self.header.kid {
                    Some(kid) => key.kid.as_deref() == Some(kid.as_str()),
                    None => true,
                }
        });
        let mut last = JoseError::NoKey;
        for jwk in candidates {
            let checked = VerifyingKey::from_jwk(jwk).and_then(|key| {
                if key.accepts(alg) {
                    self.verify(&key)
                } else {
                    Err(JoseError::InvalidKey(
                        "the key's curve does not match the algorithm",
                    ))
                }
            });
            match checked {
                Ok(()) => return Ok(jwk),
                Err(error) => last = error,
            }
        }
        Err(last)
    }
}

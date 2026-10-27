//! Just enough DER and PEM to read PKCS#8 (RFC 5208, RFC 5958), SEC 1
//! (RFC 5915), PKCS#1 and SubjectPublicKeyInfo (RFC 5280) documents, and to
//! write PKCS#8. `ring` verifies and signs; this only finds the parts.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::{Algorithm, JoseError};

const SEQUENCE: u8 = 0x30;
const INTEGER: u8 = 0x02;
const BIT_STRING: u8 = 0x03;
const OCTET_STRING: u8 = 0x04;
const NULL: u8 = 0x05;
const OID: u8 = 0x06;
/// `[0]`, constructed: PKCS#8 attributes, SEC 1 curve parameters.
const CONTEXT_0: u8 = 0xa0;
/// `[1]`, constructed: an explicitly tagged public key.
const CONTEXT_1: u8 = 0xa1;
/// `[1]`, primitive: an implicitly tagged public key (RFC 5958 v2).
const CONTEXT_1_IMPLICIT: u8 = 0x81;

const OID_ED25519: &[u8] = &[0x2b, 0x65, 0x70];
const OID_EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const OID_P256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const OID_P384: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];
const OID_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];

/// A curve of the ECDSA algorithms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Curve {
    P256,
    P384,
}

impl Curve {
    /// The curve of an `EC` JWK's `crv`.
    pub(crate) fn from_name(name: &str) -> Option<Curve> {
        match name {
            "P-256" => Some(Curve::P256),
            "P-384" => Some(Curve::P384),
            _ => None,
        }
    }

    /// The curve an ECDSA algorithm signs on.
    pub(crate) fn of(alg: Algorithm) -> Option<Curve> {
        match alg {
            Algorithm::ES256 => Some(Curve::P256),
            Algorithm::ES384 => Some(Curve::P384),
            _ => None,
        }
    }

    fn from_oid(oid: &[u8]) -> Option<Curve> {
        match oid {
            OID_P256 => Some(Curve::P256),
            OID_P384 => Some(Curve::P384),
            _ => None,
        }
    }

    fn oid(self) -> &'static [u8] {
        match self {
            Curve::P256 => OID_P256,
            Curve::P384 => OID_P384,
        }
    }

    /// The JWK `crv` name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Curve::P256 => "P-256",
            Curve::P384 => "P-384",
        }
    }

    /// The algorithm that signs on it.
    pub(crate) fn algorithm(self) -> Algorithm {
        match self {
            Curve::P256 => Algorithm::ES256,
            Curve::P384 => Algorithm::ES384,
        }
    }

    /// The length of a coordinate (and of a private scalar), in bytes.
    pub(crate) fn coordinate_len(self) -> usize {
        match self {
            Curve::P256 => 32,
            Curve::P384 => 48,
        }
    }
}

/// Reads DER elements one after the other.
struct Reader<'a> {
    input: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input }
    }

    fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    /// The next element: its tag and contents. Lengths must be minimal.
    fn read(&mut self) -> Option<(u8, &'a [u8])> {
        let (&tag, rest) = self.input.split_first()?;
        let (&first, rest) = rest.split_first()?;
        let (len, rest) = if first < 0x80 {
            (usize::from(first), rest)
        } else {
            let count = usize::from(first & 0x7f);
            if count == 0 || count > 4 || rest.len() < count {
                return None;
            }
            let (bytes, rest) = rest.split_at(count);
            if bytes[0] == 0 {
                return None;
            }
            let len = bytes
                .iter()
                .fold(0_usize, |len, byte| (len << 8) | usize::from(*byte));
            if len < 0x80 {
                return None;
            }
            (len, rest)
        };
        if rest.len() < len {
            return None;
        }
        let (contents, rest) = rest.split_at(len);
        self.input = rest;
        Some((tag, contents))
    }

    /// The next element, which must have `tag`.
    fn expect(&mut self, tag: u8) -> Option<&'a [u8]> {
        match self.read()? {
            (found, contents) if found == tag => Some(contents),
            _ => None,
        }
    }

    /// The only element of `input`, which must have `tag`.
    fn only(input: &'a [u8], tag: u8) -> Option<&'a [u8]> {
        let mut reader = Reader::new(input);
        let contents = reader.expect(tag)?;
        reader.is_empty().then_some(contents)
    }
}

/// The contents of a BIT STRING with no unused bits.
fn bit_string(contents: &[u8]) -> Option<&[u8]> {
    match contents.split_first()? {
        (0, bits) => Some(bits),
        _ => None,
    }
}

/// A positive INTEGER's big-endian bytes without leading zeros.
fn unsigned(contents: &[u8]) -> Option<&[u8]> {
    if contents.first().is_none_or(|byte| byte & 0x80 != 0) {
        return None;
    }
    let start = contents.iter().position(|byte| *byte != 0)?;
    Some(&contents[start..])
}

/// The private key of a PKCS#8 document.
pub(crate) enum PrivateKeyInfo<'a> {
    /// The 32-byte seed, and the public key of a v2 document.
    Ed25519 {
        seed: &'a [u8],
        public: Option<&'a [u8]>,
    },
    /// The private scalar and the public point (uncompressed).
    Ec {
        curve: Curve,
        d: &'a [u8],
        public: Option<&'a [u8]>,
    },
    /// The PKCS#1 `RSAPrivateKey`, for `ring` to read.
    Rsa { pkcs1: &'a [u8] },
}

/// Reads a PKCS#8 `PrivateKeyInfo` (`OneAsymmetricKey`, v1 or v2).
pub(crate) fn private_key_info(der: &[u8]) -> Option<PrivateKeyInfo<'_>> {
    let mut reader = Reader::new(Reader::only(der, SEQUENCE)?);
    let version = reader.expect(INTEGER)?;
    if version != [0] && version != [1] {
        return None;
    }
    let algorithm = reader.expect(SEQUENCE)?;
    let private_key = reader.expect(OCTET_STRING)?;
    let mut public = None;
    while let Some((tag, contents)) = reader.read() {
        match tag {
            CONTEXT_0 => {}
            CONTEXT_1_IMPLICIT => public = Some(bit_string(contents)?),
            CONTEXT_1 => public = Some(bit_string(Reader::only(contents, BIT_STRING)?)?),
            _ => return None,
        }
    }
    let mut algorithm = Reader::new(algorithm);
    match algorithm.expect(OID)? {
        OID_ED25519 => {
            // RFC 8410 §3: the parameters are absent.
            if !algorithm.is_empty() {
                return None;
            }
            let seed = Reader::only(private_key, OCTET_STRING)?;
            (seed.len() == 32).then_some(PrivateKeyInfo::Ed25519 { seed, public })
        }
        OID_EC_PUBLIC_KEY => {
            let curve = Curve::from_oid(algorithm.expect(OID)?)?;
            let key = ec_private_key(private_key)?;
            if key.curve.is_some_and(|inner| inner != curve) {
                return None;
            }
            Some(PrivateKeyInfo::Ec {
                curve,
                d: key.d,
                public: key.public.or(public),
            })
        }
        OID_RSA => {
            if !algorithm.is_empty() && !algorithm.expect(NULL)?.is_empty() {
                return None;
            }
            Some(PrivateKeyInfo::Rsa { pkcs1: private_key })
        }
        _ => None,
    }
}

/// A SEC 1 `ECPrivateKey` (RFC 5915).
pub(crate) struct EcPrivateKey<'a> {
    pub(crate) d: &'a [u8],
    pub(crate) curve: Option<Curve>,
    pub(crate) public: Option<&'a [u8]>,
}

/// Reads a SEC 1 `ECPrivateKey`: inside PKCS#8, or alone (`EC PRIVATE
/// KEY`).
pub(crate) fn ec_private_key(der: &[u8]) -> Option<EcPrivateKey<'_>> {
    let mut reader = Reader::new(Reader::only(der, SEQUENCE)?);
    if reader.expect(INTEGER)? != [1] {
        return None;
    }
    let d = reader.expect(OCTET_STRING)?;
    let mut key = EcPrivateKey {
        d,
        curve: None,
        public: None,
    };
    while let Some((tag, contents)) = reader.read() {
        match tag {
            CONTEXT_0 => key.curve = Some(Curve::from_oid(Reader::only(contents, OID)?)?),
            CONTEXT_1 => key.public = Some(bit_string(Reader::only(contents, BIT_STRING)?)?),
            _ => return None,
        }
    }
    Some(key)
}

/// The public key of a SubjectPublicKeyInfo document.
pub(crate) enum PublicKeyInfo<'a> {
    Ed25519(&'a [u8]),
    Ec { curve: Curve, point: &'a [u8] },
    Rsa { n: &'a [u8], e: &'a [u8] },
}

/// Reads a SubjectPublicKeyInfo (`PUBLIC KEY`).
pub(crate) fn public_key_info(der: &[u8]) -> Option<PublicKeyInfo<'_>> {
    spki_contents(Reader::only(der, SEQUENCE)?)
}

/// Reads the subjectPublicKeyInfo of an X.509 certificate (`CERTIFICATE`,
/// RFC 5280 §4.1). Only the key is read: the certificate's signature,
/// validity and extensions are not checked.
pub(crate) fn certificate_public_key_info(der: &[u8]) -> Option<PublicKeyInfo<'_>> {
    let mut certificate = Reader::new(Reader::only(der, SEQUENCE)?);
    let mut tbs = Reader::new(certificate.expect(SEQUENCE)?);
    // `version [0] EXPLICIT` is optional (absent in v1 certificates).
    let (mut tag, _) = tbs.read()?;
    if tag == CONTEXT_0 {
        (tag, _) = tbs.read()?;
    }
    // serialNumber, then signature, issuer, validity and subject.
    if tag != INTEGER {
        return None;
    }
    for _ in 0..4 {
        tbs.expect(SEQUENCE)?;
    }
    spki_contents(tbs.expect(SEQUENCE)?)
}

/// The contents of a SubjectPublicKeyInfo SEQUENCE.
fn spki_contents(contents: &[u8]) -> Option<PublicKeyInfo<'_>> {
    let mut reader = Reader::new(contents);
    let mut algorithm = Reader::new(reader.expect(SEQUENCE)?);
    let key = bit_string(reader.expect(BIT_STRING)?)?;
    if !reader.is_empty() {
        return None;
    }
    match algorithm.expect(OID)? {
        OID_ED25519 => Some(PublicKeyInfo::Ed25519(key)),
        OID_EC_PUBLIC_KEY => Some(PublicKeyInfo::Ec {
            curve: Curve::from_oid(algorithm.expect(OID)?)?,
            point: key,
        }),
        OID_RSA => rsa_public_key(key),
        _ => None,
    }
}

/// Reads a PKCS#1 `RSAPublicKey` (`RSA PUBLIC KEY`).
pub(crate) fn rsa_public_key(der: &[u8]) -> Option<PublicKeyInfo<'_>> {
    let mut reader = Reader::new(Reader::only(der, SEQUENCE)?);
    let n = unsigned(reader.expect(INTEGER)?)?;
    let e = unsigned(reader.expect(INTEGER)?)?;
    reader.is_empty().then_some(PublicKeyInfo::Rsa { n, e })
}

/// A DER element.
fn element(tag: u8, contents: &[u8]) -> Vec<u8> {
    let len = contents.len();
    let mut out = vec![tag];
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes = len.to_be_bytes();
        let skip = bytes.iter().take_while(|byte| **byte == 0).count();
        out.push(0x80 | (bytes.len() - skip) as u8);
        out.extend_from_slice(&bytes[skip..]);
    }
    out.extend_from_slice(contents);
    out
}

/// An Ed25519 key as PKCS#8 v1 (RFC 8410 §7): 48 bytes.
pub(crate) fn ed25519_pkcs8(seed: &[u8; 32]) -> Vec<u8> {
    private_key_info_v1(&element(OID, OID_ED25519), &element(OCTET_STRING, seed))
}

/// An EC key as PKCS#8 v1, its `ECPrivateKey` carrying the public point.
pub(crate) fn ec_pkcs8(curve: Curve, d: &[u8], point: &[u8]) -> Vec<u8> {
    let algorithm = [element(OID, OID_EC_PUBLIC_KEY), element(OID, curve.oid())].concat();
    let mut public = vec![0];
    public.extend_from_slice(point);
    let ec_private_key = element(
        SEQUENCE,
        &[
            element(INTEGER, &[1]),
            element(OCTET_STRING, d),
            element(CONTEXT_1, &element(BIT_STRING, &public)),
        ]
        .concat(),
    );
    private_key_info_v1(&algorithm, &ec_private_key)
}

/// An RSA key (PKCS#1 `RSAPrivateKey`) as PKCS#8 v1.
pub(crate) fn rsa_pkcs8(pkcs1: &[u8]) -> Vec<u8> {
    let algorithm = [element(OID, OID_RSA), element(NULL, &[])].concat();
    private_key_info_v1(&algorithm, pkcs1)
}

fn private_key_info_v1(algorithm: &[u8], private_key: &[u8]) -> Vec<u8> {
    element(
        SEQUENCE,
        &[
            element(INTEGER, &[0]),
            element(SEQUENCE, algorithm),
            element(OCTET_STRING, private_key),
        ]
        .concat(),
    )
}

/// A decoded PEM document.
pub(crate) struct Pem {
    /// The label of the `-----BEGIN <label>-----` line, if there is one.
    pub(crate) label: Option<String>,
    pub(crate) der: Vec<u8>,
}

/// Reads the first PEM block of `text`. Literal `\n` sequences (a key
/// pasted into one environment variable) and missing armor lines are
/// accepted; encrypted PEM is not.
pub(crate) fn decode_pem(text: &str) -> Result<Pem, JoseError> {
    let not_pem = JoseError::InvalidKey("the key is not PEM");
    let text = text.replace("\\n", "\n");
    let mut label = None;
    let mut body = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(armor) = line.strip_prefix("-----BEGIN ") {
            if label.is_some() || !body.is_empty() {
                return Err(not_pem);
            }
            label = Some(armor.trim_end_matches('-').trim().to_owned());
        } else if line.starts_with("-----END ") {
            break;
        } else if line.contains(':') {
            return Err(JoseError::InvalidKey(
                "encrypted PEM keys are not supported",
            ));
        } else {
            body.extend(line.chars().filter(|c| !c.is_whitespace()));
        }
    }
    if body.is_empty() {
        return Err(not_pem);
    }
    let der = STANDARD.decode(body).map_err(|_| not_pem)?;
    Ok(Pem { label, der })
}

/// `der` as a PEM block with `label`, in lines of 64 characters.
pub(crate) fn encode_pem(label: &str, der: &[u8]) -> String {
    let encoded = STANDARD.encode(der);
    let mut pem = format!("-----BEGIN {label}-----\n");
    for line in encoded.as_bytes().chunks(64) {
        // Base64 is ASCII.
        pem.push_str(std::str::from_utf8(line).unwrap_or_default());
        pem.push('\n');
    }
    pem.push_str(&format!("-----END {label}-----\n"));
    pem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_round_trip_in_short_and_long_form() {
        for len in [0, 1, 127, 128, 255, 256, 1000, 70_000] {
            let contents = vec![7_u8; len];
            let encoded = element(OCTET_STRING, &contents);
            let mut reader = Reader::new(&encoded);
            assert_eq!(reader.read(), Some((OCTET_STRING, contents.as_slice())));
            assert!(reader.is_empty());
        }
    }

    #[test]
    fn non_minimal_and_truncated_lengths_are_refused() {
        for bad in [
            &[0x04, 0x81, 0x05, 1, 2, 3, 4, 5][..],
            &[0x04, 0x82, 0x00, 0x80],
            &[0x04, 0x80],
            &[0x04, 0x85, 1, 1, 1, 1, 1],
            &[0x04, 0x03, 1, 2],
            &[0x04],
        ] {
            assert_eq!(Reader::new(bad).read(), None, "{bad:?}");
        }
    }

    #[test]
    fn unsigned_integers_lose_their_leading_zeros() {
        assert_eq!(unsigned(&[0, 0x80, 1]), Some(&[0x80, 1][..]));
        assert_eq!(unsigned(&[1, 0, 1]), Some(&[1, 0, 1][..]));
        assert_eq!(unsigned(&[0x80]), None, "negative");
        assert_eq!(unsigned(&[0]), None, "zero");
        assert_eq!(unsigned(&[]), None);
    }

    #[test]
    fn pem_reads_the_first_block_only() {
        let one = encode_pem("PUBLIC KEY", &[1, 2, 3]);
        let two = format!("{one}{}", encode_pem("PUBLIC KEY", &[4, 5, 6]));
        let pem = decode_pem(&two).unwrap();
        assert_eq!(pem.label.as_deref(), Some("PUBLIC KEY"));
        assert_eq!(pem.der, [1, 2, 3]);
        let encrypted = "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nAAAA\n-----END RSA PRIVATE KEY-----";
        assert!(decode_pem(encrypted).is_err());
    }
}

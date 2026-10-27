//! DKIM signing (RFC 6376, `relaxed/relaxed`) with RSA-SHA256 or
//! Ed25519-SHA256 (RFC 8463), on `ring` through `navree_crypto::jose`.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use navree_contracts::Secret;
use navree_crypto::jose::{Algorithm, SigningKey};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::MailError;

/// DKIM settings of an SMTP transport (`[mail.transport.dkim]`):
///
/// ```toml
/// [mail.transport]
/// type = "smtp"
/// url = "smtps://user:pass@smtp.acme.test"
///
/// [mail.transport.dkim]
/// domain = "acme.test"
/// selector = "mail"
/// private_key = "-----BEGIN PRIVATE KEY-----\n..."   # APP_MAIL__TRANSPORT__DKIM__PRIVATE_KEY
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DkimConfig {
    /// The signing domain (`d=`): mail from this domain or a subdomain is
    /// signed.
    pub domain: String,
    /// The selector (`s=`): the key is published at
    /// `<selector>._domainkey.<domain>`.
    pub selector: String,
    /// The private key, PEM: RSA (2048 bits or more, PKCS#8 or PKCS#1) or
    /// Ed25519 (PKCS#8). Literal `\n` sequences are accepted.
    pub private_key: Secret<String>,
    /// The headers to sign (default: [`DEFAULT_SIGNED_HEADERS`]); those a
    /// message lacks are skipped.
    pub headers: Vec<String>,
}

/// The headers signed by default.
pub const DEFAULT_SIGNED_HEADERS: &[&str] = &[
    "from",
    "reply-to",
    "subject",
    "date",
    "to",
    "cc",
    "message-id",
    "mime-version",
    "content-type",
    "content-transfer-encoding",
];

/// The DKIM algorithm of a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DkimAlgorithm {
    /// `rsa-sha256`.
    RsaSha256,
    /// `ed25519-sha256` (RFC 8463).
    Ed25519Sha256,
}

impl DkimAlgorithm {
    /// The `a=` value.
    pub fn as_str(self) -> &'static str {
        match self {
            DkimAlgorithm::RsaSha256 => "rsa-sha256",
            DkimAlgorithm::Ed25519Sha256 => "ed25519-sha256",
        }
    }

    /// The `k=` value of the DNS record.
    pub fn key_type(self) -> &'static str {
        match self {
            DkimAlgorithm::RsaSha256 => "rsa",
            DkimAlgorithm::Ed25519Sha256 => "ed25519",
        }
    }
}

/// Signs messages with a DKIM key: adds a `DKIM-Signature` header computed
/// over the `relaxed/relaxed` canonical form of the signed headers and the
/// body.
///
/// ```
/// use navree_crypto::jose::SigningKey;
/// use navree_mail::DkimSigner;
///
/// let key = SigningKey::generate_ed25519().to_pkcs8_pem().unwrap();
/// let signer = DkimSigner::from_pem("acme.test", "mail", &key).unwrap();
/// let message = b"From: hello@acme.test\r\nTo: ada@example.com\r\nSubject: Hi\r\n\r\nHello\r\n";
///
/// let header = signer.sign(message).unwrap();
/// assert!(header.starts_with("DKIM-Signature: v=1;\r\n\ta=ed25519-sha256;\r\n\tc=relaxed/relaxed;"));
/// assert!(signer.dns_record().starts_with("v=DKIM1; k=ed25519; p="));
/// assert!(signer.applies_to("news.acme.test"));
/// ```
pub struct DkimSigner {
    domain: String,
    selector: String,
    key: SigningKey,
    algorithm: DkimAlgorithm,
    headers: Vec<String>,
}

impl DkimSigner {
    /// A signer for `config`.
    pub fn new(config: &DkimConfig) -> Result<Self, MailError> {
        let mut signer = Self::from_pem(
            &config.domain,
            &config.selector,
            config.private_key.expose(),
        )?;
        if !config.headers.is_empty() {
            signer.headers = config
                .headers
                .iter()
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect();
            if !signer.headers.iter().any(|h| h == "from") {
                signer.headers.insert(0, "from".to_owned());
            }
        }
        Ok(signer)
    }

    /// A signer for `domain` and `selector` with the PEM private key `pem`.
    pub fn from_pem(domain: &str, selector: &str, pem: &str) -> Result<Self, MailError> {
        let invalid = |reason: &str| MailError::Config(format!("invalid DKIM settings: {reason}"));
        let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
        if domain.is_empty() || selector.trim().is_empty() {
            return Err(invalid("`domain` and `selector` are required"));
        }
        let key = SigningKey::from_pem(pem).map_err(|error| invalid(&error.to_string()))?;
        let algorithm = match key.algorithm() {
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
                let key = key
                    .with_algorithm(Algorithm::RS256)
                    .map_err(|error| invalid(&error.to_string()))?;
                return Ok(Self {
                    domain,
                    selector: selector.trim().to_owned(),
                    key,
                    algorithm: DkimAlgorithm::RsaSha256,
                    headers: default_headers(),
                });
            }
            Algorithm::EdDSA => DkimAlgorithm::Ed25519Sha256,
            _ => return Err(invalid("the key must be RSA or Ed25519")),
        };
        Ok(Self {
            domain,
            selector: selector.trim().to_owned(),
            key,
            algorithm,
            headers: default_headers(),
        })
    }

    /// The signing domain.
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// The selector.
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// The algorithm the key signs with.
    pub fn algorithm(&self) -> DkimAlgorithm {
        self.algorithm
    }

    /// Whether mail from `sender_domain` is signed: the signing domain or
    /// one of its subdomains (so the signature aligns for DMARC).
    pub fn applies_to(&self, sender_domain: &str) -> bool {
        let sender = sender_domain.trim_end_matches('.').to_ascii_lowercase();
        sender == self.domain || sender.ends_with(&format!(".{}", self.domain))
    }

    /// The TXT record to publish at
    /// [`record_name`](DkimSigner::record_name):
    /// `v=DKIM1; k=rsa; p=<public key>`.
    pub fn dns_record(&self) -> String {
        format!(
            "v=DKIM1; k={}; p={}",
            self.algorithm.key_type(),
            self.public_key()
        )
    }

    /// `<selector>._domainkey.<domain>`.
    pub fn record_name(&self) -> String {
        format!("{}._domainkey.{}", self.selector, self.domain)
    }

    /// The `p=` value: the Ed25519 public key, or the RSA key's
    /// SubjectPublicKeyInfo, in base64.
    fn public_key(&self) -> String {
        let Some(jwk) = self.key.public_jwk() else {
            return String::new();
        };
        let decode = |value: &Option<String>| {
            value
                .as_deref()
                .and_then(|v| URL_SAFE_NO_PAD.decode(v).ok())
                .unwrap_or_default()
        };
        match self.algorithm {
            DkimAlgorithm::Ed25519Sha256 => STANDARD.encode(decode(&jwk.x)),
            DkimAlgorithm::RsaSha256 => {
                STANDARD.encode(rsa_spki(&decode(&jwk.n), &decode(&jwk.e)))
            }
        }
    }

    /// The `DKIM-Signature` header line for `message` (the whole message as
    /// sent: headers, a blank line, the body), signed now; prepend it to the
    /// message. The line ends with CRLF.
    pub fn sign(&self, message: &[u8]) -> Result<String, MailError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        self.sign_at(message, now)
    }

    /// [`sign`](DkimSigner::sign) with the signature time `t=` given.
    pub fn sign_at(&self, message: &[u8], timestamp: u64) -> Result<String, MailError> {
        let (head, body) = split(message);
        let headers = parse_headers(head);
        let signed: Vec<&str> = self
            .headers
            .iter()
            .map(String::as_str)
            .filter(|name| headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
            .collect();
        let body_hash = STANDARD.encode(Sha256::digest(relaxed_body(body)));
        let value = format!(
            "v=1; a={}; c=relaxed/relaxed; d={}; s={}; t={timestamp}; h={}; bh={body_hash}; b=",
            self.algorithm.as_str(),
            self.domain,
            self.selector,
            signed.join(":")
        );
        let signature = self.signature(&headers, &signed, &value)?;
        let folded = value.replace("; ", ";\r\n\t");
        Ok(format!("DKIM-Signature: {folded}{signature}\r\n"))
    }

    /// The base64 signature of the canonical headers and `dkim_value` (the
    /// `DKIM-Signature` value with an empty `b=`).
    fn signature(
        &self,
        headers: &[(String, String)],
        signed: &[&str],
        dkim_value: &str,
    ) -> Result<String, MailError> {
        let input = signing_input(headers, signed, dkim_value);
        let bytes = match self.algorithm {
            DkimAlgorithm::RsaSha256 => self.key.sign(&input),
            DkimAlgorithm::Ed25519Sha256 => self.key.sign(&Sha256::digest(&input)),
        }
        .map_err(|error| MailError::Config(format!("DKIM signing failed: {error}")))?;
        Ok(STANDARD.encode(bytes))
    }
}

impl fmt::Debug for DkimSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DkimSigner")
            .field("domain", &self.domain)
            .field("selector", &self.selector)
            .field("algorithm", &self.algorithm)
            .finish_non_exhaustive()
    }
}

fn default_headers() -> Vec<String> {
    DEFAULT_SIGNED_HEADERS
        .iter()
        .map(|h| (*h).to_owned())
        .collect()
}

/// The message split at the first empty line: (headers, body).
fn split(message: &[u8]) -> (&[u8], &[u8]) {
    if let Some(at) = message.windows(4).position(|w| w == b"\r\n\r\n") {
        (&message[..at + 2], &message[at + 4..])
    } else if let Some(at) = message.windows(2).position(|w| w == b"\n\n") {
        (&message[..at + 1], &message[at + 2..])
    } else {
        (message, &[])
    }
}

/// The header fields, unfolded lines kept as they are: `(name, value)` in
/// order, `value` with its folding.
fn parse_headers(head: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(head).replace("\r\n", "\n");
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = fields.last_mut() {
                value.push_str("\r\n");
                value.push_str(line);
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            fields.push((name.to_owned(), value.to_owned()));
        }
    }
    fields
}

/// `relaxed` header canonicalization: lower-case name, unfolded value with
/// runs of whitespace as one space, no whitespace around the colon or at
/// the end.
fn relaxed_header(name: &str, value: &str) -> String {
    let unfolded = value.replace("\r\n", "");
    let collapsed = unfolded.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{}:{collapsed}", name.trim().to_ascii_lowercase())
}

/// `relaxed` body canonicalization: whitespace runs as one space, none at
/// line ends, no empty lines at the end; a non-empty body ends with CRLF.
fn relaxed_body(body: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(body).replace("\r\n", "\n");
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            let mut out = String::with_capacity(line.len());
            let mut space = false;
            for c in line.chars() {
                if c == ' ' || c == '\t' {
                    space = true;
                } else {
                    if space {
                        out.push(' ');
                        space = false;
                    }
                    out.push(c);
                }
            }
            out
        })
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        return Vec::new();
    }
    let mut out = lines.join("\r\n");
    out.push_str("\r\n");
    out.into_bytes()
}

/// The data signed: each header named in `signed` (a name listed twice
/// takes the next instance from the bottom; a missing one adds nothing),
/// canonical with CRLF, then the `DKIM-Signature` header without CRLF.
fn signing_input(headers: &[(String, String)], signed: &[&str], dkim_value: &str) -> Vec<u8> {
    let mut used = vec![false; headers.len()];
    let mut input = String::new();
    for name in signed {
        let found = headers
            .iter()
            .enumerate()
            .rev()
            .find(|(i, (n, _))| !used[*i] && n.trim().eq_ignore_ascii_case(name));
        if let Some((i, (n, value))) = found {
            used[i] = true;
            input.push_str(&relaxed_header(n, value));
            input.push_str("\r\n");
        }
    }
    input.push_str(&relaxed_header("DKIM-Signature", dkim_value));
    input.into_bytes()
}

/// A DER SubjectPublicKeyInfo for the RSA key (`n`, `e`).
fn rsa_spki(n: &[u8], e: &[u8]) -> Vec<u8> {
    let rsa_public_key = der(0x30, &[der_integer(n), der_integer(e)].concat());
    // AlgorithmIdentifier: rsaEncryption (1.2.840.113549.1.1.1), NULL.
    let algorithm = der(
        0x30,
        &[
            der(0x06, &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01]),
            vec![0x05, 0x00],
        ]
        .concat(),
    );
    let bit_string = der(0x03, &[&[0x00][..], &rsa_public_key].concat());
    der(0x30, &[algorithm, bit_string].concat())
}

/// An unsigned DER INTEGER.
fn der_integer(bytes: &[u8]) -> Vec<u8> {
    let trimmed: &[u8] = {
        let start = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len());
        &bytes[start..]
    };
    let mut content = Vec::with_capacity(trimmed.len() + 1);
    if trimmed.first().is_none_or(|b| b & 0x80 != 0) {
        content.push(0);
    }
    content.extend_from_slice(trimmed);
    der(0x02, &content)
}

/// A DER element with `tag` and `content`.
fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let len = content.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes = len.to_be_bytes();
        let start = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len() - 1);
        out.push(0x80 | (bytes.len() - start) as u8);
        out.extend_from_slice(&bytes[start..]);
    }
    out.extend_from_slice(content);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The message of RFC 8463, appendix A.
    const RFC8463_MESSAGE: &str = "From: Joe SixPack <joe@football.example.com>\r\n\
        To: Suzie Q <suzie@shopping.example.net>\r\n\
        Subject: Is dinner ready?\r\n\
        Date: Fri, 11 Jul 2003 21:00:37 -0700 (PDT)\r\n\
        Message-ID: <20030712040037.46341.5F8J@football.example.com>\r\n\
        \r\n\
        Hi.\r\n\
        \r\n\
        We lost the game.  Are you hungry yet?\r\n\
        \r\n\
        Joe.\r\n";

    /// RFC 8463's Ed25519 private key (the seed, base64).
    const RFC8463_SEED: &str = "nWGxne/9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A=";

    #[test]
    fn relaxed_body_hash_matches_rfc_8463() {
        let (_, body) = split(RFC8463_MESSAGE.as_bytes());
        assert_eq!(
            STANDARD.encode(Sha256::digest(relaxed_body(body))),
            "2jUSOH9NhtVGCQWNr9BrIAPreKQjO6Sn7XIkfJVOzv8="
        );
    }

    #[test]
    fn the_rfc_8463_key_has_its_published_public_key() {
        let seed: [u8; 32] = STANDARD.decode(RFC8463_SEED).unwrap().try_into().unwrap();
        let key = SigningKey::ed25519_from_seed(&seed);
        let x = key.public_jwk().unwrap().x.unwrap();
        assert_eq!(
            STANDARD.encode(URL_SAFE_NO_PAD.decode(x).unwrap()),
            "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo="
        );
    }

    /// Reproduces the Ed25519 signature of RFC 8463, appendix A.3.
    #[test]
    fn ed25519_signature_matches_rfc_8463() {
        let seed: [u8; 32] = STANDARD.decode(RFC8463_SEED).unwrap().try_into().unwrap();
        let signer = DkimSigner {
            domain: "football.example.com".to_owned(),
            selector: "brisbane".to_owned(),
            key: SigningKey::ed25519_from_seed(&seed),
            algorithm: DkimAlgorithm::Ed25519Sha256,
            headers: default_headers(),
        };
        let (head, _) = split(RFC8463_MESSAGE.as_bytes());
        let headers = parse_headers(head);
        let dkim_value = " v=1; a=ed25519-sha256; c=relaxed/relaxed;\r\n \
            d=football.example.com; i=@football.example.com;\r\n \
            q=dns/txt; s=brisbane; t=1528637909; h=from : to :\r\n \
            subject : date : message-id : from : subject : date;\r\n \
            bh=2jUSOH9NhtVGCQWNr9BrIAPreKQjO6Sn7XIkfJVOzv8=;\r\n \
            b=";
        let signed = [
            "from",
            "to",
            "subject",
            "date",
            "message-id",
            "from",
            "subject",
            "date",
        ];
        assert_eq!(
            signer.signature(&headers, &signed, dkim_value).unwrap(),
            "/gCrinpcQOoIfuHNQIbq4pgh9kyIK3AQUdt9OdqQehSwhEIug4D11BusFa3bT3FY5OsU7ZbnKELq+eXdp1Q1Dw=="
        );
    }

    #[test]
    fn header_canonicalization_is_relaxed() {
        assert_eq!(
            relaxed_header("SubJect ", " Is   dinner\r\n\tready?  "),
            "subject:Is dinner ready?"
        );
        assert_eq!(relaxed_body(b""), b"");
        assert_eq!(relaxed_body(b"\r\n\r\n"), b"");
        assert_eq!(relaxed_body(b"a  b \t\r\nc"), b"a b\r\nc\r\n");
    }

    #[test]
    fn rsa_public_keys_are_der_encoded() {
        // n = 0x80 needs a leading zero; e = 65537.
        let spki = rsa_spki(&[0x80], &[0x01, 0x00, 0x01]);
        assert_eq!(
            hex::encode(spki),
            "301d300d06092a864886f70d0101010500030c003009020200800203010001"
        );
    }
}

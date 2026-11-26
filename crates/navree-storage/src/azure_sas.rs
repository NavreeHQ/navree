//! [`SharedKeySas`] and [`UserDelegationKey`]: service SAS and user
//! delegation SAS links for Azure Blob Storage.
//!
//! Azure signs a blob's unencoded name (`reports/q1 final.csv`) under its
//! explicit account and container (`/blob/<account>/<container>/<blob>`),
//! whereas the HTTP address carries the percent-encoded path
//! (`reports/q1%20final.csv`).

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_crypto::hmac_sha256;
use navree_datetime::{Precision, Timestamp};
use url::Url;

/// The SAS version the links name. Every version from 2020-12-06 signs the
/// same sixteen fields for a service SAS and twenty-four fields for a user
/// delegation SAS.
pub(crate) const VERSION: &str = "2023-11-03";

/// Signs service SAS links with a storage account's key.
#[derive(Clone)]
pub(crate) struct SharedKeySas {
    account: String,
    key: Vec<u8>,
}

impl fmt::Debug for SharedKeySas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedKeySas")
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl SharedKeySas {
    /// The signer for `account` with its base64 access key.
    #[allow(dead_code)]
    pub(crate) fn new(account: &str, key: &str) -> Result<Self, base64::DecodeError> {
        let decoded = decode_non_empty_key(key)?;
        Ok(Self {
            account: account.to_owned(),
            key: decoded,
        })
    }

    /// The signer for `account` with its decoded access key bytes.
    pub(crate) fn from_bytes(
        account: impl Into<String>,
        key: Vec<u8>,
    ) -> Result<Self, base64::DecodeError> {
        if key.is_empty() {
            return Err(base64::DecodeError::InvalidLength(0));
        }
        Ok(Self {
            account: account.into(),
            key,
        })
    }

    /// Replaces the query of `url`, the address of `blob` inside `container`,
    /// with a service SAS that allows `permissions` (`r` to read, `w` to
    /// write) from `start` until `expiry`.
    pub(crate) fn sign(
        &self,
        container: &str,
        blob: &str,
        url: &mut Url,
        permissions: &str,
        start: Timestamp,
        expiry: Timestamp,
    ) {
        let start = start.to_rfc3339_with(Precision::Secs, true);
        let expiry = expiry.to_rfc3339_with(Precision::Secs, true);
        let resource = canonical_resource(&self.account, container, blob);
        let protocol = signed_protocol(url);
        let signature = STANDARD.encode(hmac_sha256(
            &self.key,
            Self::string_to_sign(permissions, &start, &expiry, &resource, protocol).as_bytes(),
        ));
        url.set_query(None);
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("sv", VERSION)
                .append_pair("sp", permissions)
                .append_pair("st", &start)
                .append_pair("se", &expiry)
                .append_pair("sr", "b");
            if !protocol.is_empty() {
                pairs.append_pair("spr", protocol);
            }
            pairs.append_pair("sig", &signature);
        }
    }

    /// The fields a blob service SAS signs, in order, for versions
    /// 2020-12-06 and later: permissions, start, expiry, the resource, then
    /// the signed identifier, IP range and protocol (`https` on HTTPS links),
    /// the version, the resource type (`b`, a blob), the snapshot time and
    /// encryption scope (none) and the five response headers a link may
    /// override (none).
    fn string_to_sign(
        permissions: &str,
        start: &str,
        expiry: &str,
        resource: &str,
        protocol: &str,
    ) -> String {
        [
            permissions,
            start,
            expiry,
            resource,
            "",
            "",
            protocol,
            VERSION,
            "b",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
        ]
        .join("\n")
    }
}

/// A user delegation key issued by Azure Blob Storage (`Get User Delegation
/// Key`), used to sign SAS links when the store authenticates with OAuth
/// rather than an account key.
#[derive(Clone, PartialEq, Eq)]
pub struct UserDelegationKey {
    /// The Entra object identifier (`SignedOid` / `skoid`).
    pub signed_oid: String,
    /// The Entra tenant identifier (`SignedTid` / `sktid`).
    pub signed_tid: String,
    /// When the delegation key starts working (`SignedStart` / `skt`).
    pub signed_start: String,
    /// When the delegation key stops working (`SignedExpiry` / `ske`).
    pub signed_expiry: String,
    /// The storage service (`SignedService` / `sks`, `b` for blobs).
    pub signed_service: String,
    /// The REST API version that issued the key (`SignedVersion` / `skv`).
    pub signed_version: String,
    key: Vec<u8>,
}

/// Metadata of the delegation key; never the secret key bytes.
impl fmt::Debug for UserDelegationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserDelegationKey")
            .field("signed_oid", &self.signed_oid)
            .field("signed_tid", &self.signed_tid)
            .field("signed_start", &self.signed_start)
            .field("signed_expiry", &self.signed_expiry)
            .field("signed_service", &self.signed_service)
            .field("signed_version", &self.signed_version)
            .finish_non_exhaustive()
    }
}

impl UserDelegationKey {
    /// Builds a delegation key from its metadata fields and its base64
    /// `value` (`<Value>`).
    pub fn new(
        signed_oid: impl Into<String>,
        signed_tid: impl Into<String>,
        signed_start: impl Into<String>,
        signed_expiry: impl Into<String>,
        signed_service: impl Into<String>,
        signed_version: impl Into<String>,
        value: &str,
    ) -> Result<Self, base64::DecodeError> {
        let key = decode_non_empty_key(value)?;
        Ok(Self {
            signed_oid: signed_oid.into(),
            signed_tid: signed_tid.into(),
            signed_start: signed_start.into(),
            signed_expiry: signed_expiry.into(),
            signed_service: signed_service.into(),
            signed_version: signed_version.into(),
            key,
        })
    }

    /// Replaces the query of `url`, the address of `blob` inside `container`
    /// of `account`, with a user delegation SAS that allows `permissions`
    /// (`r` to read, `w` to write) from `start` until `expiry`.
    #[allow(clippy::too_many_arguments)]
    pub fn sign(
        &self,
        account: &str,
        container: &str,
        blob: &str,
        url: &mut Url,
        permissions: &str,
        start: Timestamp,
        expiry: Timestamp,
    ) {
        let start_text = start.to_rfc3339_with(Precision::Secs, true);
        let expiry_text = expiry.to_rfc3339_with(Precision::Secs, true);
        let protocol = signed_protocol(url);
        let to_sign =
            self.string_to_sign(account, container, blob, url, permissions, start, expiry);
        let signature = STANDARD.encode(hmac_sha256(&self.key, to_sign.as_bytes()));
        url.set_query(None);
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("sv", VERSION)
                .append_pair("sp", permissions)
                .append_pair("st", &start_text)
                .append_pair("se", &expiry_text)
                .append_pair("sr", "b");
            if !protocol.is_empty() {
                pairs.append_pair("spr", protocol);
            }
            pairs
                .append_pair("skoid", &self.signed_oid)
                .append_pair("sktid", &self.signed_tid)
                .append_pair("skt", &self.signed_start)
                .append_pair("ske", &self.signed_expiry)
                .append_pair("sks", &self.signed_service)
                .append_pair("skv", &self.signed_version)
                .append_pair("sig", &signature);
        }
    }

    /// The twenty-four fields a blob user delegation SAS signs, in order,
    /// for versions 2020-12-06 and later.
    #[allow(clippy::too_many_arguments)]
    pub fn string_to_sign(
        &self,
        account: &str,
        container: &str,
        blob: &str,
        url: &Url,
        permissions: &str,
        start: Timestamp,
        expiry: Timestamp,
    ) -> String {
        let start = start.to_rfc3339_with(Precision::Secs, true);
        let expiry = expiry.to_rfc3339_with(Precision::Secs, true);
        let resource = canonical_resource(account, container, blob);
        let protocol = signed_protocol(url);
        [
            permissions,
            &start,
            &expiry,
            &resource,
            &self.signed_oid,
            &self.signed_tid,
            &self.signed_start,
            &self.signed_expiry,
            &self.signed_service,
            &self.signed_version,
            "",
            "",
            "",
            "",
            protocol,
            VERSION,
            "b",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
        ]
        .join("\n")
    }
}

/// Decodes a trimmed base64 key and refuses 0-byte keys.
fn decode_non_empty_key(raw: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let decoded = STANDARD.decode(raw.trim_ascii())?;
    if decoded.is_empty() {
        return Err(base64::DecodeError::InvalidLength(0));
    }
    Ok(decoded)
}

/// Pins HTTPS SAS links to `spr=https` while leaving plain-HTTP emulator
/// links (`http://127.0.0.1:10000/...`) unrestricted so Azurite accepts them.
fn signed_protocol(url: &Url) -> &'static str {
    if url.scheme() == "https" { "https" } else { "" }
}

/// `/blob/<account>/<container>/<blob>` built directly from the store's
/// resolved account, container and unencoded blob path, never inferred from
/// the URL's host or path segments.
pub(crate) fn canonical_resource(account: &str, container: &str, blob: &str) -> String {
    format!("/blob/{account}/{container}/{blob}")
}

#[cfg(test)]
mod tests {
    use navree_datetime::Timestamp;

    use super::*;

    /// The account key Azurite publishes for local development.
    const KEY: &str =
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

    fn signer() -> SharedKeySas {
        SharedKeySas::new("devstoreaccount1", KEY).unwrap()
    }

    #[test]
    fn the_signed_resource_comes_straight_from_account_container_and_blob() {
        assert_eq!(
            canonical_resource("devstoreaccount1", "archive", "reports/q1 final#é.csv"),
            "/blob/devstoreaccount1/archive/reports/q1 final#é.csv"
        );
        // A container named like the account on a custom domain never drops
        // the container segment even if the blob key starts with another name.
        let custom_signer = SharedKeySas::new("acmefiles", KEY).unwrap();
        let start = Timestamp::utc(2026, 11, 5, 6, 18, 21).unwrap();
        let expiry = Timestamp::utc(2026, 11, 5, 6, 23, 21).unwrap();
        let mut custom =
            Url::parse("https://blobs.amritsar.example/acmefiles/other-container/secret.pdf")
                .unwrap();
        custom_signer.sign(
            "acmefiles",
            "other-container/secret.pdf",
            &mut custom,
            "r",
            start,
            expiry,
        );
        let custom_pairs: Vec<(String, String)> = custom.query_pairs().into_owned().collect();
        let custom_sig = &custom_pairs.iter().find(|(k, _)| k == "sig").unwrap().1;
        let expected_custom = format!(
            "r\n2026-11-05T06:18:21Z\n2026-11-05T06:23:21Z\n\
             /blob/acmefiles/acmefiles/other-container/secret.pdf\n\n\nhttps\n{VERSION}\nb\n\n\n\n\n\n\n"
        );
        let key = STANDARD.decode(KEY).unwrap();
        assert_eq!(
            *custom_sig,
            STANDARD.encode(hmac_sha256(&key, expected_custom.as_bytes()))
        );

        // An Azurite container hostname matching the account (`http://azurite:10000/azurite/...`)
        // signs `/blob/azurite/archive/a.txt` without doubling the account.
        let az_signer = SharedKeySas::new("azurite", KEY).unwrap();
        let mut az_url = Url::parse("http://azurite:10000/azurite/archive/a.txt").unwrap();
        az_signer.sign("archive", "a.txt", &mut az_url, "r", start, expiry);
        let az_sig = az_url
            .query_pairs()
            .find(|(k, _)| k == "sig")
            .unwrap()
            .1
            .into_owned();
        let expected_az = format!(
            "r\n2026-11-05T06:18:21Z\n2026-11-05T06:23:21Z\n\
             /blob/azurite/archive/a.txt\n\n\n\n{VERSION}\nb\n\n\n\n\n\n\n"
        );
        assert_eq!(
            az_sig,
            STANDARD.encode(hmac_sha256(&key, expected_az.as_bytes()))
        );
    }

    #[test]
    fn links_carry_the_signature_of_the_sixteen_fields() {
        let start = Timestamp::utc(2026, 11, 5, 6, 18, 21).unwrap();
        let expiry = Timestamp::utc(2026, 11, 5, 6, 23, 21).unwrap();
        let mut url = Url::parse(
            "http://127.0.0.1:10000/devstoreaccount1/navree-test/reports/q1%20final.csv?sp=r&sig=old",
        )
        .unwrap();
        signer().sign(
            "navree-test",
            "reports/q1 final.csv",
            &mut url,
            "r",
            start,
            expiry,
        );
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        let names: Vec<&str> = pairs.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["sv", "sp", "st", "se", "sr", "sig"]);
        let value = |name: &str| {
            pairs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(value("sp"), "r");
        assert_eq!(value("st"), "2026-11-05T06:18:21Z");
        assert_eq!(value("se"), "2026-11-05T06:23:21Z");
        let expected = format!(
            "r\n2026-11-05T06:18:21Z\n2026-11-05T06:23:21Z\n\
             /blob/devstoreaccount1/navree-test/reports/q1 final.csv\n\n\n\n{VERSION}\nb\n\n\n\n\n\n\n"
        );
        let key = STANDARD.decode(KEY).unwrap();
        assert_eq!(
            value("sig"),
            STANDARD.encode(hmac_sha256(&key, expected.as_bytes()))
        );
        // The blob's address is kept as it was.
        assert_eq!(
            url.path(),
            "/devstoreaccount1/navree-test/reports/q1%20final.csv"
        );
    }

    #[test]
    fn a_key_that_is_not_base64_or_is_empty_is_refused() {
        assert!(SharedKeySas::new("devstoreaccount1", "not base64!").is_err());
        assert!(SharedKeySas::new("devstoreaccount1", "").is_err());
        assert!(SharedKeySas::new("devstoreaccount1", "   ").is_err());
        assert!(SharedKeySas::from_bytes("devstoreaccount1", Vec::new()).is_err());
        assert!(UserDelegationKey::new("oid", "tid", "st", "se", "b", VERSION, "   ").is_err());
        assert!(format!("{:?}", signer()).contains("devstoreaccount1"));
        assert!(!format!("{:?}", signer()).contains(KEY));
    }
}

//! [`SharedKeySas`]: service SAS links for Azure Blob Storage, signed with
//! the account key.
//!
//! Azure signs a blob's decoded name. object_store signs the name as it
//! appears in the URL, percent-encoded, so its links are refused (403) for
//! a name with a space, `#`, a non-ASCII letter or anything else a URL
//! encodes. The Azure driver signs its links here instead whenever it has
//! the account key, and keeps object_store's address for the blob.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, SecondsFormat, Utc};
use navree_crypto::hmac_sha256;
use object_store::signer::Url;
use percent_encoding::percent_decode_str;

/// The SAS version the links name. Every version from 2020-12-06 signs the
/// same sixteen fields ([`SharedKeySas::string_to_sign`]).
pub(crate) const VERSION: &str = "2023-11-03";

/// Signs service SAS links with a storage account's key.
#[derive(Clone)]
pub(crate) struct SharedKeySas {
    account: String,
    key: Vec<u8>,
}

impl std::fmt::Debug for SharedKeySas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedKeySas")
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl SharedKeySas {
    /// The signer for `account` with its base64 access key.
    pub(crate) fn new(account: &str, key: &str) -> Result<Self, base64::DecodeError> {
        Ok(Self {
            account: account.to_owned(),
            key: STANDARD.decode(key.trim())?,
        })
    }

    /// Replaces the query of `url`, the address of a blob, with a service
    /// SAS that allows `permissions` (`r` to read, `w` to write) from
    /// `start` until `expiry`.
    pub(crate) fn sign(
        &self,
        url: &mut Url,
        permissions: &str,
        start: DateTime<Utc>,
        expiry: DateTime<Utc>,
    ) {
        let start = start.to_rfc3339_opts(SecondsFormat::Secs, true);
        let expiry = expiry.to_rfc3339_opts(SecondsFormat::Secs, true);
        let resource = self.canonical_resource(url);
        let signature = STANDARD.encode(hmac_sha256(
            &self.key,
            Self::string_to_sign(permissions, &start, &expiry, &resource).as_bytes(),
        ));
        url.set_query(None);
        url.query_pairs_mut()
            .append_pair("sv", VERSION)
            .append_pair("sp", permissions)
            .append_pair("st", &start)
            .append_pair("se", &expiry)
            .append_pair("sr", "b")
            .append_pair("sig", &signature);
    }

    /// The fields a blob SAS signs, in order, for versions 2020-12-06 and
    /// later: permissions, start, expiry, the resource, then the signed
    /// identifier, IP range and protocol (none), the version, the resource
    /// type (`b`, a blob), the snapshot time and encryption scope (none) and
    /// the five response headers a link may override (none).
    fn string_to_sign(permissions: &str, start: &str, expiry: &str, resource: &str) -> String {
        [
            permissions,
            start,
            expiry,
            resource,
            "",
            "",
            "",
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

    /// `/blob/<account>/<container>/<name>` with the name decoded, as Azure
    /// signs it. The address is `https://<account>.blob.core.windows.net/
    /// <container>/<name>`, or names the account first in its path (Azurite
    /// and other emulators: `http://127.0.0.1:10000/<account>/<container>/
    /// <name>`).
    fn canonical_resource(&self, url: &Url) -> String {
        let path = percent_decode_str(url.path()).decode_utf8_lossy();
        let account_in_path = !url.host_str().unwrap_or_default().contains(&self.account)
            && url.path_segments().and_then(|mut s| s.next()) == Some(self.account.as_str());
        if account_in_path {
            format!("/blob{path}")
        } else {
            format!("/blob/{}{path}", self.account)
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    /// The account key Azurite publishes for local development.
    const KEY: &str =
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

    fn signer() -> SharedKeySas {
        SharedKeySas::new("devstoreaccount1", KEY).unwrap()
    }

    #[test]
    fn the_signed_resource_is_the_decoded_name() {
        let emulator = Url::parse(
            "http://127.0.0.1:10000/devstoreaccount1/archive/reports/q1%20final%23%C3%A9.csv",
        )
        .unwrap();
        assert_eq!(
            signer().canonical_resource(&emulator),
            "/blob/devstoreaccount1/archive/reports/q1 final#é.csv"
        );
        let azure = SharedKeySas::new("crmfiles", KEY).unwrap();
        let hosted =
            Url::parse("https://crmfiles.blob.core.windows.net/archive/q1%20final.csv").unwrap();
        assert_eq!(
            azure.canonical_resource(&hosted),
            "/blob/crmfiles/archive/q1 final.csv"
        );
        // A container named like the account, on Azure itself.
        let same = Url::parse("https://crmfiles.blob.core.windows.net/crmfiles/a.csv").unwrap();
        assert_eq!(
            azure.canonical_resource(&same),
            "/blob/crmfiles/crmfiles/a.csv"
        );
    }

    #[test]
    fn links_carry_the_signature_of_the_sixteen_fields() {
        let start = Utc.with_ymd_and_hms(2026, 11, 5, 6, 18, 21).unwrap();
        let expiry = Utc.with_ymd_and_hms(2026, 11, 5, 6, 23, 21).unwrap();
        let mut url = Url::parse(
            "http://127.0.0.1:10000/devstoreaccount1/navree-test/reports/q1%20final.csv?sp=r&sig=old",
        )
        .unwrap();
        signer().sign(&mut url, "r", start, expiry);
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
    fn a_key_that_is_not_base64_is_refused() {
        assert!(SharedKeySas::new("devstoreaccount1", "not base64!").is_err());
        assert!(format!("{:?}", signer()).contains("devstoreaccount1"));
        assert!(!format!("{:?}", signer()).contains(KEY));
    }
}

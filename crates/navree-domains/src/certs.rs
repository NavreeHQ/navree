//! [`DomainCertificates`]: the certificates the app serves itself
//! (`domains.tls = "acme"`), picked by server name for each TLS handshake.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use chrono::{DateTime, TimeZone, Utc};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

use crate::DomainsError;

/// The certificates the app serves, by host: a rustls certificate
/// resolver (`ResolvesServerCert`) that answers each handshake with the
/// certificate of the server name the client asked for, and nothing for a
/// name it has none of.
///
/// With `domains.acme.https_addr` set, `serve` listens for HTTPS with it.
/// Without, hand it to your own rustls server:
///
/// ```no_run
/// # async fn demo(ctx: navree_kernel::AppContext) -> Result<(), navree_kernel::BoxError> {
/// use navree_domains::Domains;
///
/// let domains = ctx.get::<Domains>().await?;
/// let config = domains.certificates().server_config()?; // rustls::ServerConfig
/// let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));
/// # let _ = acceptor;
/// # Ok(())
/// # }
/// ```
#[derive(Default)]
pub struct DomainCertificates {
    keys: RwLock<BTreeMap<String, Served>>,
}

#[derive(Clone)]
struct Served {
    key: Arc<CertifiedKey>,
    not_after: DateTime<Utc>,
}

impl fmt::Debug for DomainCertificates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DomainCertificates")
            .field("hosts", &self.hosts())
            .finish()
    }
}

impl DomainCertificates {
    /// No certificates.
    pub fn new() -> DomainCertificates {
        DomainCertificates::default()
    }

    /// The hosts it has a certificate for, sorted.
    pub fn hosts(&self) -> Vec<String> {
        self.read().keys().cloned().collect()
    }

    /// Whether it has a certificate for `host`.
    pub fn contains(&self, host: &str) -> bool {
        self.read().contains_key(&host.to_ascii_lowercase())
    }

    /// When the certificate of `host` expires.
    pub fn expires_at(&self, host: &str) -> Option<DateTime<Utc>> {
        self.read()
            .get(&host.to_ascii_lowercase())
            .map(|served| served.not_after)
    }

    /// Serves `chain_pem` (leaf first) with `key_pem` for `host`, replacing
    /// its previous certificate.
    pub fn insert_pem(
        &self,
        host: &str,
        chain_pem: &str,
        key_pem: &str,
    ) -> Result<(), DomainsError> {
        let key = certified_key(chain_pem, key_pem)?;
        let validity = validity(chain_pem)?;
        self.keys
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                host.to_ascii_lowercase(),
                Served {
                    key: Arc::new(key),
                    not_after: validity.not_after,
                },
            );
        Ok(())
    }

    /// Stops serving `host`.
    pub fn remove(&self, host: &str) {
        self.keys
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&host.to_ascii_lowercase());
    }

    /// A rustls server configuration (ring, TLS 1.2 and 1.3, ALPN `h2` and
    /// `http/1.1`) that answers with these certificates.
    pub fn server_config(self: &Arc<Self>) -> Result<rustls::ServerConfig, DomainsError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| DomainsError::Certificate(error.to_string()))?
            .with_no_client_auth()
            .with_cert_resolver(self.clone());
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(config)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<String, Served>> {
        self.keys.read().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ResolvesServerCert for DomainCertificates {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let name = client_hello.server_name()?.to_ascii_lowercase();
        self.read().get(&name).map(|served| served.key.clone())
    }
}

/// A leaf certificate's validity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Validity {
    pub(crate) not_before: DateTime<Utc>,
    pub(crate) not_after: DateTime<Utc>,
}

/// The certificates of a PEM chain, leaf first.
fn chain(chain_pem: &str) -> Result<Vec<CertificateDer<'static>>, DomainsError> {
    let certs = CertificateDer::pem_slice_iter(chain_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| DomainsError::Certificate(format!("the certificate chain: {error}")))?;
    if certs.is_empty() {
        return Err(DomainsError::Certificate(
            "the certificate chain holds no certificate".to_owned(),
        ));
    }
    Ok(certs)
}

/// The rustls key of a chain and its private key (ring signing keys).
pub(crate) fn certified_key(chain_pem: &str, key_pem: &str) -> Result<CertifiedKey, DomainsError> {
    let certs = chain(chain_pem)?;
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())
        .map_err(|error| DomainsError::Certificate(format!("the private key: {error}")))?;
    let signing = rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|error| DomainsError::Certificate(format!("the private key: {error}")))?;
    Ok(CertifiedKey::new(certs, signing))
}

/// The validity of the first (leaf) certificate of a PEM chain.
pub(crate) fn validity(chain_pem: &str) -> Result<Validity, DomainsError> {
    let certs = chain(chain_pem)?;
    validity_der(&certs[0])
}

/// The validity of a DER certificate.
pub(crate) fn validity_der(der: &[u8]) -> Result<Validity, DomainsError> {
    let (_, cert) = x509_parser::parse_x509_certificate(der)
        .map_err(|error| DomainsError::Certificate(format!("the certificate: {error}")))?;
    let validity = cert.validity();
    let at = |seconds: i64| {
        Utc.timestamp_opt(seconds, 0).single().ok_or_else(|| {
            DomainsError::Certificate("a certificate date is out of range".to_owned())
        })
    };
    Ok(Validity {
        not_before: at(validity.not_before.timestamp())?,
        not_after: at(validity.not_after.timestamp())?,
    })
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, TimeDelta};

    use super::*;
    use crate::acme::{AcmeFake, AcmeOrder, AcmeStore};
    use crate::{AcmeClient, IssuedCertificate};

    struct NoStore;

    impl AcmeStore for NoStore {
        fn account<'a>(
            &'a self,
            _: &'a str,
        ) -> navree_kernel::BoxFuture<'a, Result<Option<String>, DomainsError>> {
            Box::pin(async { Ok(None) })
        }
        fn save_account<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
        ) -> navree_kernel::BoxFuture<'a, Result<(), DomainsError>> {
            Box::pin(async { Ok(()) })
        }
        fn publish<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: &'a str,
        ) -> navree_kernel::BoxFuture<'a, Result<(), DomainsError>> {
            Box::pin(async { Ok(()) })
        }
        fn withdraw<'a>(
            &'a self,
            _: &'a str,
        ) -> navree_kernel::BoxFuture<'a, Result<(), DomainsError>> {
            Box::pin(async { Ok(()) })
        }
    }

    async fn issued(host: &str, ca: &AcmeFake) -> IssuedCertificate {
        let order = AcmeOrder {
            host: host.to_owned(),
            directory_url: "https://acme.test/directory".to_owned(),
            contact: Vec::new(),
            store: Arc::new(NoStore),
        };
        ca.issue(&order).await.unwrap()
    }

    #[tokio::test]
    async fn certificates_are_served_by_host_with_their_expiry() {
        let ca = AcmeFake::new();
        let expires = Utc::now() + TimeDelta::days(30);
        ca.expire_at(expires);
        let cert = issued("shop.acme.test", &ca).await;
        let validity = validity(&cert.chain_pem).unwrap();
        assert_eq!(validity.not_after.date_naive(), expires.date_naive());
        assert_eq!(
            (validity.not_after - validity.not_before).num_days(),
            90,
            "{validity:?}"
        );
        let certificates = Arc::new(DomainCertificates::new());
        certificates
            .insert_pem("Shop.Acme.test", &cert.chain_pem, &cert.private_key_pem)
            .unwrap();
        assert!(certificates.contains("shop.acme.test"));
        assert_eq!(certificates.hosts(), ["shop.acme.test"]);
        assert_eq!(
            certificates.expires_at("shop.acme.test").map(|at| at.day()),
            Some(expires.day())
        );
        certificates.server_config().unwrap();
        certificates.remove("shop.acme.test");
        assert!(certificates.hosts().is_empty());
    }

    #[test]
    fn unreadable_pem_is_a_certificate_error() {
        assert!(matches!(
            validity("not a certificate"),
            Err(DomainsError::Certificate(_))
        ));
        let certificates = DomainCertificates::new();
        assert!(
            certificates
                .insert_pem("x.test", "garbage", "garbage")
                .is_err()
        );
    }
}

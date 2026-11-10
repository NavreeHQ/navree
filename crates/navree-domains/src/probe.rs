//! Reading a domain's certificate over TLS: [`CertificateProbe`], with
//! [`TlsProbe`] and [`ProbeFake`] for tests. With `domains.tls =
//! "on_demand"` the proxy holds the certificates, so the module learns
//! their expiry this way.

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_kernel::BoxFuture;
use rustls::DigitallySignedStruct;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::net::TcpStream;

use crate::DomainsError;
use crate::certs::validity_der;

/// The certificate a host presents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbedCertificate {
    /// The start of its validity.
    pub not_before: DateTime<Utc>,
    /// The end of its validity.
    pub not_after: DateTime<Utc>,
}

/// Reads the certificate a host presents on HTTPS. The module registers a
/// [`TlsProbe`]; replace it with `override_value::<dyn CertificateProbe>(..)`,
/// for example with a [`ProbeFake`] in tests.
pub trait CertificateProbe: Send + Sync + 'static {
    /// The leaf certificate `host` presents for its own name.
    fn probe<'a>(&'a self, host: &'a str)
    -> BoxFuture<'a, Result<ProbedCertificate, DomainsError>>;
}

/// Connects to `host` on port 443, says its name (SNI), and reads the dates
/// of the certificate it presents. The certificate is read, not trusted:
/// an expired or mismatched one is still reported, and nothing is sent
/// after the handshake.
#[derive(Clone)]
pub struct TlsProbe {
    port: u16,
    timeout: Duration,
    connect_to: BTreeMap<String, SocketAddr>,
}

impl fmt::Debug for TlsProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsProbe")
            .field("port", &self.port)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Default for TlsProbe {
    fn default() -> Self {
        TlsProbe::new()
    }
}

impl TlsProbe {
    /// A probe of port 443 with a 10-second timeout.
    pub fn new() -> TlsProbe {
        TlsProbe {
            port: 443,
            timeout: Duration::from_secs(10),
            connect_to: BTreeMap::new(),
        }
    }

    /// Probes port `port` instead of 443.
    #[must_use]
    pub fn port(mut self, port: u16) -> TlsProbe {
        self.port = port;
        self
    }

    /// Gives up after `timeout`.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> TlsProbe {
        self.timeout = timeout;
        self
    }

    /// Connects to `addr` for `host` instead of resolving it (a server in
    /// a test, a load balancer you want to check behind its DNS).
    #[must_use]
    pub fn connect_to(mut self, host: &str, addr: SocketAddr) -> TlsProbe {
        self.connect_to.insert(host.to_ascii_lowercase(), addr);
        self
    }

    /// Where to connect for `host`: its `connect_to` address, else the
    /// first public address it resolves to. A tenant controls its domain's
    /// DNS, so a host that resolves to loopback, private, link-local or
    /// shared addresses only is refused rather than probed.
    async fn address(&self, host: &str) -> Result<SocketAddr, String> {
        if let Some(addr) = self.connect_to.get(&host.to_ascii_lowercase()) {
            return Ok(*addr);
        }
        let found: Vec<SocketAddr> = tokio::net::lookup_host((host, self.port))
            .await
            .map_err(|error| format!("cannot resolve: {error}"))?
            .collect();
        found
            .iter()
            .copied()
            .find(|addr| is_public(addr.ip()))
            .ok_or_else(|| {
                if found.is_empty() {
                    "cannot resolve: no address".to_owned()
                } else {
                    "resolves to a private address only; it is not probed".to_owned()
                }
            })
    }

    async fn run(&self, host: &str) -> Result<ProbedCertificate, DomainsError> {
        let failed = |reason: String| DomainsError::Certificate(format!("{host}: {reason}"));
        let addr = self.address(host).await.map_err(failed)?;
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|error| failed(format!("cannot connect: {error}")))?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        // Deliberate: this probe only reads the certificate a host presents (to report its expiry)
        // and sends no data. `ReadOnly` below accepts any chain on purpose and still verifies the
        // handshake signatures, so the certificate read is the server's own.
        // nosemgrep: navree-tls-verification-disabled
        let config = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|error| failed(error.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(ReadOnly(provider)))
            .with_no_client_auth();
        let name =
            ServerName::try_from(host.to_owned()).map_err(|error| failed(error.to_string()))?;
        let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, stream)
            .await
            .map_err(|error| failed(format!("the TLS handshake failed: {error}")))?;
        let (_, connection) = tls.get_ref();
        let leaf = connection
            .peer_certificates()
            .and_then(|certs| certs.first())
            .ok_or_else(|| failed("no certificate was presented".to_owned()))?;
        let validity = validity_der(leaf)?;
        Ok(ProbedCertificate {
            not_before: validity.not_before,
            not_after: validity.not_after,
        })
    }
}

impl CertificateProbe for TlsProbe {
    fn probe<'a>(
        &'a self,
        host: &'a str,
    ) -> BoxFuture<'a, Result<ProbedCertificate, DomainsError>> {
        Box::pin(async move {
            tokio::time::timeout(self.timeout, self.run(host))
                .await
                .map_err(|_| DomainsError::Certificate(format!("{host}: timed out")))?
        })
    }
}

/// Whether `ip` is a public unicast address: not unspecified, loopback,
/// private, link-local, shared (100.64.0.0/10), multicast, broadcast or
/// unique local.
pub(crate) fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || (a == 100 && (64..128).contains(&b))
                || a == 0)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// Accepts any certificate (it is only read) but checks the handshake's
/// signatures, so the presented certificate is the server's own.
#[derive(Debug)]
struct ReadOnly(Arc<CryptoProvider>);

impl ServerCertVerifier for ReadOnly {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Certificates in memory for tests: the expiry each host presents, or a
/// failure. Clones share the table.
///
/// ```
/// use chrono::{TimeDelta, Utc};
/// use navree_domains::{CertificateProbe, ProbeFake};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let probe = ProbeFake::new();
/// probe.expires("shop.acme.test", Utc::now() + TimeDelta::days(60));
/// assert!(probe.probe("shop.acme.test").await.is_ok());
/// assert!(probe.probe("unknown.acme.test").await.is_err());
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct ProbeFake {
    state: Arc<Mutex<FakeProbes>>,
}

#[derive(Debug, Default)]
struct FakeProbes {
    answers: BTreeMap<String, Result<DateTime<Utc>, String>>,
    probed: Vec<String>,
}

impl ProbeFake {
    /// No certificates: every probe fails.
    pub fn new() -> ProbeFake {
        ProbeFake::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut FakeProbes) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// `host` presents a certificate that expires at `at`.
    pub fn expires(&self, host: &str, at: DateTime<Utc>) {
        self.with(|probes| {
            probes.answers.insert(host.to_ascii_lowercase(), Ok(at));
        });
    }

    /// Probes of `host` fail with `reason`.
    pub fn fail(&self, host: &str, reason: &str) {
        self.with(|probes| {
            probes
                .answers
                .insert(host.to_ascii_lowercase(), Err(reason.to_owned()));
        });
    }

    /// The hosts probed, in order.
    pub fn probed(&self) -> Vec<String> {
        self.with(|probes| probes.probed.clone())
    }
}

impl CertificateProbe for ProbeFake {
    fn probe<'a>(
        &'a self,
        host: &'a str,
    ) -> BoxFuture<'a, Result<ProbedCertificate, DomainsError>> {
        let host = host.to_ascii_lowercase();
        let answer = self.with(|probes| {
            probes.probed.push(host.clone());
            probes.answers.get(&host).cloned()
        });
        Box::pin(async move {
            match answer {
                Some(Ok(not_after)) => Ok(ProbedCertificate {
                    not_before: not_after - chrono::TimeDelta::days(90),
                    not_after,
                }),
                Some(Err(reason)) => Err(DomainsError::Certificate(format!("{host}: {reason}"))),
                None => Err(DomainsError::Certificate(format!(
                    "{host}: no certificate was presented"
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_are_probed() {
        for ip in [
            "203.0.113.10",
            "8.8.8.8",
            "2606:4700::6810:84e5",
            "::ffff:203.0.113.10",
        ] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn a_host_on_the_apps_own_network_is_not_probed() {
        let error = TlsProbe::new()
            .probe("localhost")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("private address"), "{error}");
    }
}

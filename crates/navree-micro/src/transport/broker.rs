//! What the broker transports share: their URLs, TLS client settings and
//! reconnect pauses.

#![cfg_attr(not(any(feature = "mqtt", feature = "kafka")), allow(dead_code))]

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use percent_encoding::percent_decode_str;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};

use crate::MicroError;
use crate::options::TlsOptions;

/// A broker address from a URL such as `mqtts://user:secret@host:8883`.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(not(feature = "mqtt"), allow(dead_code))]
pub(crate) struct BrokerUrl {
    /// Whether the scheme asks for TLS.
    pub(crate) tls: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

impl fmt::Debug for BrokerUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrokerUrl")
            .field("tls", &self.tls)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

#[cfg_attr(not(feature = "mqtt"), allow(dead_code))]
impl BrokerUrl {
    /// Parses `url`. `schemes` lists the plain and TLS schemes with their
    /// default ports, e.g. `[("mqtt", false, 1883), ("mqtts", true, 8883)]`;
    /// a URL without a scheme is the first one.
    pub(crate) fn parse(url: &str, schemes: &[(&str, bool, u16)]) -> Result<BrokerUrl, MicroError> {
        let url = url.trim();
        let invalid = |why: &str| {
            MicroError::transport(format!(
                "`{}` is not a broker URL: {why}",
                crate::options::redact_url(url)
            ))
        };
        let (scheme, rest) = match url.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
            None => (schemes.first().map(|s| s.0).unwrap_or_default().to_owned(), url),
        };
        let Some(&(_, tls, default_port)) = schemes.iter().find(|(name, _, _)| *name == scheme) else {
            let expected: Vec<String> = schemes.iter().map(|(name, _, _)| format!("{name}://")).collect();
            return Err(invalid(&format!("expected {}", expected.join(" or "))));
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let (credentials, address) = match authority.rsplit_once('@') {
            Some((credentials, address)) => (Some(credentials), address),
            None => (None, authority),
        };
        let (username, password) = match credentials {
            Some(credentials) => {
                let (user, pass) = match credentials.split_once(':') {
                    Some((user, pass)) => (user, Some(pass)),
                    None => (credentials, None),
                };
                let decode = |text: &str| percent_decode_str(text).decode_utf8_lossy().into_owned();
                (
                    Some(decode(user)).filter(|user| !user.is_empty()),
                    pass.map(decode),
                )
            }
            None => (None, None),
        };
        let (host, port) = split_host_port(address).ok_or_else(|| invalid("bad port"))?;
        if host.is_empty() {
            return Err(invalid("no host"));
        }
        Ok(BrokerUrl {
            tls,
            host,
            port: port.unwrap_or(default_port),
            username,
            password,
        })
    }

    /// `host:port`.
    pub(crate) fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// `host`, `host:port`, `[v6]` or `[v6]:port`.
#[cfg_attr(not(feature = "mqtt"), allow(dead_code))]
fn split_host_port(address: &str) -> Option<(String, Option<u16>)> {
    if let Some(rest) = address.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port.parse().ok()?),
            None if after.is_empty() => None,
            None => return None,
        };
        return Some((host.to_owned(), port));
    }
    match address.rsplit_once(':') {
        Some((host, port)) => Some((host.to_owned(), Some(port.parse().ok()?))),
        None => Some((address.to_owned(), None)),
    }
}

/// A TLS client configuration on ring: servers are verified against
/// `tls.ca`, else the public web roots; `tls.cert` and `tls.key` add a
/// client certificate. `key` names the settings in errors (`micro.mqtt`).
pub(crate) fn client_config(tls: &TlsOptions, key: &str) -> Result<Arc<ClientConfig>, MicroError> {
    let read_error = |path: &Path, error: &dyn fmt::Display| {
        MicroError::transport(format!("cannot read {}: {error}", path.display()))
    };
    let tls_error = |error: rustls::Error| MicroError::transport(format!("{key}.tls: {error}"));
    let mut roots = RootCertStore::empty();
    match &tls.ca {
        Some(ca) => {
            for certificate in CertificateDer::pem_file_iter(ca).map_err(|error| read_error(ca, &error))? {
                roots
                    .add(certificate.map_err(|error| read_error(ca, &error))?)
                    .map_err(tls_error)?;
            }
        }
        None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
    }
    let builder = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .with_root_certificates(roots);
    let config = match (&tls.cert, &tls.key) {
        (Some(cert), Some(private)) => {
            let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
                .map_err(|error| read_error(cert, &error))?
                .collect::<Result<_, _>>()
                .map_err(|error| read_error(cert, &error))?;
            let private_key = PrivateKeyDer::from_pem_file(private).map_err(|error| read_error(private, &error))?;
            builder.with_client_auth_cert(chain, private_key).map_err(tls_error)?
        }
        (None, None) => builder.with_no_client_auth(),
        _ => {
            return Err(MicroError::transport(format!(
                "{key}.tls.cert and {key}.tls.key go together"
            )));
        }
    };
    Ok(Arc::new(config))
}

/// Reconnect pauses: 100 ms doubling to 30 seconds.
pub(crate) fn reconnect_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(16);
    Duration::from_millis(100u64.saturating_mul(1u64 << exponent)).min(Duration::from_secs(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MQTT: &[(&str, bool, u16)] = &[("mqtt", false, 1883), ("mqtts", true, 8883)];

    #[test]
    fn urls_give_the_address_credentials_and_tls() {
        let url = BrokerUrl::parse("mqtts://deals:p%40ss@broker.internal/x", MQTT).unwrap();
        assert!(url.tls);
        assert_eq!(url.address(), "broker.internal:8883");
        assert_eq!(url.username.as_deref(), Some("deals"));
        assert_eq!(url.password.as_deref(), Some("p@ss"));
        let plain = BrokerUrl::parse("mqtt://127.0.0.1:1884", MQTT).unwrap();
        assert!(!plain.tls);
        assert_eq!(plain.address(), "127.0.0.1:1884");
        assert_eq!(plain.username, None);
        let bare = BrokerUrl::parse("localhost", MQTT).unwrap();
        assert_eq!(bare.address(), "localhost:1883");
        let v6 = BrokerUrl::parse("mqtt://[::1]:1999", MQTT).unwrap();
        assert_eq!(v6.address(), "[::1]:1999");
    }

    #[test]
    fn bad_urls_are_refused_without_their_password() {
        let error = BrokerUrl::parse("http://u:secret@x", MQTT).unwrap_err().to_string();
        assert!(error.contains("expected mqtt:// or mqtts://"), "{error}");
        assert!(!error.contains("secret"), "{error}");
        assert!(BrokerUrl::parse("mqtt://x:port", MQTT).is_err());
        assert!(BrokerUrl::parse("mqtt://:1883", MQTT).is_err());
        assert!(!format!("{:?}", BrokerUrl::parse("mqtt://u:secret@x", MQTT).unwrap()).contains("secret"));
    }

    #[test]
    fn tls_settings_load_or_name_their_key() {
        assert!(client_config(&TlsOptions::default(), "micro.mqtt").is_ok());
        let error = client_config(
            &TlsOptions {
                cert: Some("cert.pem".into()),
                ..TlsOptions::default()
            },
            "micro.mqtt",
        )
        .unwrap_err();
        assert!(error.to_string().contains("micro.mqtt.tls.cert and micro.mqtt.tls.key"), "{error}");
        let error = client_config(
            &TlsOptions {
                ca: Some("missing-ca.pem".into()),
                ..TlsOptions::default()
            },
            "micro.mqtt",
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing-ca.pem"), "{error}");
    }

    #[test]
    fn reconnects_back_off_from_100_ms_to_30_seconds() {
        assert_eq!(reconnect_delay(1), Duration::from_millis(100));
        assert_eq!(reconnect_delay(4), Duration::from_millis(800));
        assert_eq!(reconnect_delay(50), Duration::from_secs(30));
    }
}

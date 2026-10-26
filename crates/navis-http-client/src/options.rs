//! [`HttpClientOptions`] and [`SsrfPolicy`].

use http::HeaderValue;
use navis_config::ConfigSection;
use serde::{Deserialize, Deserializer};

/// Options for [`HttpClient`](crate::HttpClient) and
/// [`HttpClientModule::for_root`](crate::HttpClientModule::for_root).
///
/// As configuration (section `http_client`, see
/// [`HttpClientModule::from_config`](crate::HttpClientModule::from_config))
/// every key is optional:
///
/// ```toml
/// [http_client]
/// timeout_secs = 30
/// connect_timeout_secs = 5
/// retries = 2
/// retry_base_ms = 200
/// max_response_bytes = 10485760
/// user_agent = "my-app/1.0"
///
/// [http_client.ssrf]
/// block_private = true
/// allow_hosts = ["payments.internal", "*.svc.cluster.local", "10.20.0.0/16"]
/// ```
///
/// From the environment, `APP_HTTP_CLIENT__SSRF__ALLOW_HOSTS` takes a
/// comma-separated list.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct HttpClientOptions {
    /// Time allowed for each attempt, from connecting to the last body byte
    /// (default 30, at least 1).
    pub timeout_secs: u64,
    /// Time allowed to open a connection (default 5, at least 1).
    pub connect_timeout_secs: u64,
    /// Retries after the first attempt, for idempotent requests only
    /// (default 2).
    pub retries: u32,
    /// The first retry delay in milliseconds; each later delay doubles, plus
    /// random jitter, up to 30 s (default 200).
    pub retry_base_ms: u64,
    /// The largest response body read into memory; a bigger one fails with
    /// [`HttpClientError::BodyTooLarge`](crate::HttpClientError::BodyTooLarge)
    /// (default 10 MiB, at least 1).
    pub max_response_bytes: u64,
    /// Which destinations are refused (server-side request forgery guard).
    pub ssrf: SsrfPolicy,
    /// The `User-Agent` header sent with every request (default
    /// `navis-http-client/<version>`).
    pub user_agent: String,
}

impl Default for HttpClientOptions {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            connect_timeout_secs: 5,
            retries: 2,
            retry_base_ms: 200,
            max_response_bytes: 10 * 1024 * 1024,
            ssrf: SsrfPolicy::default(),
            user_agent: default_user_agent(),
        }
    }
}

impl ConfigSection for HttpClientOptions {
    const KEY: &'static str = "http_client";

    fn validate(&self) -> Result<(), String> {
        if self.timeout_secs == 0 {
            return Err("`timeout_secs` must be at least 1".to_owned());
        }
        if self.connect_timeout_secs == 0 {
            return Err("`connect_timeout_secs` must be at least 1".to_owned());
        }
        if self.max_response_bytes == 0 {
            return Err("`max_response_bytes` must be at least 1".to_owned());
        }
        if HeaderValue::try_from(self.user_agent.as_str()).is_err() {
            return Err("`user_agent` is not a valid header value".to_owned());
        }
        Ok(())
    }
}

/// `navis-http-client/<version>`.
fn default_user_agent() -> String {
    concat!("navis-http-client/", env!("CARGO_PKG_VERSION")).to_owned()
}

/// The server-side request forgery (SSRF) policy.
///
/// With `block_private` on, requests to these destinations are refused:
/// loopback, private (RFC 1918), link-local, carrier-grade NAT,
/// unique-local, multicast, documentation, benchmarking and other reserved
/// ranges, plus IPv6 forms that embed such an IPv4 address. The cloud
/// metadata endpoints (`169.254.169.254`, `169.254.170.2`,
/// `100.100.100.200`, `192.0.0.192`, `fd00:ec2::254`) are refused even with
/// `block_private` off. Both literal IP URLs and the addresses a host name
/// resolves to are checked, on every redirect hop too.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SsrfPolicy {
    /// Refuse private, loopback, link-local and other internal addresses
    /// (default `true`).
    pub block_private: bool,
    /// Destinations that bypass the policy: host names (`api.internal`),
    /// host globs (`*.svc.cluster.local`), IP addresses (`127.0.0.1`) or
    /// CIDR networks (`10.20.0.0/16`). Host names match case-insensitively.
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub allow_hosts: Vec<String>,
}

impl Default for SsrfPolicy {
    fn default() -> Self {
        Self {
            block_private: true,
            allow_hosts: Vec::new(),
        }
    }
}

/// A list, or one comma-separated string (what an environment variable holds).
fn list_or_comma_separated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Hosts {
        List(Vec<String>),
        Joined(String),
    }
    let hosts = match Hosts::deserialize(deserializer)? {
        Hosts::List(hosts) => hosts,
        Hosts::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(hosts
        .into_iter()
        .map(|host| host.trim().to_owned())
        .filter(|host| !host.is_empty())
        .collect())
}

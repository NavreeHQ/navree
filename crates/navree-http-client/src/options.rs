//! [`HttpClientOptions`] and [`SsrfPolicy`].

use http::HeaderValue;
use navree_config::{ConfigSection, ConfigTable};
use serde::Deserialize;

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
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(
    key = "http_client",
    docs = "/docs/digging-deeper/http-client/",
    validate = Self::check
)]
#[serde(default)]
pub struct HttpClientOptions {
    /// Time allowed for each attempt, from connecting to the last body byte
    /// (default 30, at least 1).
    pub timeout_secs: u64,
    /// Time allowed to open a connection (default 5, at least 1).
    pub connect_timeout_secs: u64,
    /// Retries after the first attempt, for idempotent requests and
    /// requests with an idempotency key only (default 2).
    pub retries: u32,
    /// The first retry delay in milliseconds; each later delay doubles, plus
    /// random jitter, up to 30 s (default 200).
    pub retry_base_ms: u64,
    /// The largest response body read into memory; a bigger one fails with
    /// [`HttpClientError::BodyTooLarge`](crate::HttpClientError::BodyTooLarge)
    /// (default 10 MiB, at least 1).
    pub max_response_bytes: u64,
    /// Which destinations are refused (server-side request forgery guard).
    #[config(table)]
    pub ssrf: SsrfPolicy,
    /// The `User-Agent` header sent with every request (default
    /// `navree-http-client/<version>`).
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

impl HttpClientOptions {
    /// Checks the values (the boot calls this).
    fn check(&self) -> Result<(), String> {
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

/// `navree-http-client/<version>`.
fn default_user_agent() -> String {
    concat!("navree-http-client/", env!("CARGO_PKG_VERSION")).to_owned()
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
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct SsrfPolicy {
    /// Refuse private, loopback, link-local and other internal addresses
    /// (default `true`).
    pub block_private: bool,
    /// Destinations that bypass the policy: host names (`api.internal`),
    /// host globs (`*.svc.cluster.local`), IP addresses (`127.0.0.1`) or
    /// CIDR networks (`10.20.0.0/16`). Host names match case-insensitively.
    #[serde(deserialize_with = "navree_config::comma_list")]
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

#[cfg(test)]
mod tests {
    use navree_config::FieldType;

    use super::*;

    #[test]
    fn the_section_describes_itself() {
        let schema = HttpClientOptions::schema().expect("schema");
        assert_eq!(schema.key, "http_client");
        assert_eq!(
            schema.docs.as_deref(),
            Some("/docs/digging-deeper/http-client/")
        );
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(field("ssrf").ty, FieldType::Table);
        assert_eq!(field("timeout_secs").ty, FieldType::Integer);
        let leaves: Vec<String> = schema.leaves().into_iter().map(|(path, _)| path).collect();
        assert!(
            leaves.contains(&"http_client.ssrf.allow_hosts".to_owned()),
            "{leaves:?}"
        );
        let defaults = HttpClientOptions::defaults().expect("defaults");
        assert_eq!(defaults["timeout_secs"], 30);
        assert_eq!(defaults["ssrf"]["block_private"], true);
    }

    #[test]
    fn allowed_hosts_come_from_a_list_or_one_comma_separated_string() {
        let list: SsrfPolicy =
            serde_json::from_value(serde_json::json!({ "allow_hosts": ["a.internal", " "] }))
                .expect("list");
        assert_eq!(list.allow_hosts, ["a.internal"]);
        let joined: SsrfPolicy = serde_json::from_value(
            serde_json::json!({ "allow_hosts": "a.internal, *.svc.cluster.local,," }),
        )
        .expect("joined");
        assert_eq!(joined.allow_hosts, ["a.internal", "*.svc.cluster.local"]);
    }
}

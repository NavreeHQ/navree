//! [`GrpcOptions`]: the `grpc` configuration section.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use navree_config::ConfigSection;
use serde::{Deserialize, Deserializer};

/// gRPC settings (configuration section `grpc`; every key optional).
///
/// ```toml
/// [grpc]
/// addr = "0.0.0.0:50051"   # a listener of its own; without it gRPC shares http.addr
/// web = true
/// connect = true
///
/// [grpc.clients.deals]
/// url = "http://deals.internal:8081"
/// timeout_ms = 3000
/// ```
///
/// As environment variables: `APP_GRPC__ADDR=0.0.0.0:50051`,
/// `APP_GRPC__REFLECTION=false`.
#[derive(Clone, Debug, Deserialize, ConfigSection)]
#[config(key = "grpc", docs = "/docs/microservices/grpc/", validate = Self::check_values)]
#[serde(default)]
pub struct GrpcOptions {
    /// A separate listener for gRPC (default none: gRPC shares the HTTP
    /// server's port, `http.addr`). An empty string means none.
    #[serde(deserialize_with = "optional_addr")]
    pub addr: Option<SocketAddr>,
    /// Accept gRPC-Web, binary and text, over HTTP/1.1 and HTTP/2 (default
    /// `true`).
    pub web: bool,
    /// Accept the Connect protocol, with protobuf or JSON (default `true`).
    pub connect: bool,
    /// Serve server reflection, `grpc.reflection.v1` and `v1alpha` (default
    /// `true` in debug builds, `false` in release builds). It describes the
    /// descriptor sets registered with `m.grpc_descriptors(..)`.
    pub reflection: bool,
    /// Serve `grpc.health.v1.Health`, answered from the app's readiness
    /// checks (default `true`).
    pub health: bool,
    /// The deadline of a call whose client sets none, and the longest one a
    /// client's `grpc-timeout` can ask for, in seconds (default:
    /// `http.timeout_secs`).
    pub timeout_secs: Option<u64>,
    /// The largest message received or sent, in bytes (default 4 MiB).
    pub max_message_bytes: usize,
    /// Calls in flight on one HTTP/2 connection of the separate listener
    /// (`grpc.addr`; default 256). On the shared port the HTTP server's
    /// HTTP/2 settings apply.
    pub concurrency_per_connection: u32,
    /// The gRPC services this app calls, by name (`grpc.clients.<name>`),
    /// for [`GrpcChannels`](crate::GrpcChannels).
    pub clients: BTreeMap<String, ClientOptions>,
}

impl Default for GrpcOptions {
    fn default() -> Self {
        Self {
            addr: None,
            web: true,
            connect: true,
            reflection: cfg!(debug_assertions),
            health: true,
            timeout_secs: None,
            max_message_bytes: 4 * 1024 * 1024,
            concurrency_per_connection: 256,
            clients: BTreeMap::new(),
        }
    }
}

impl GrpcOptions {
    /// Checks the values at boot: [`check`](Self::check) with the problems
    /// joined into one message.
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|problems| problems.join("; "))
    }

    /// The deadline of calls without `grpc-timeout`, and the longest one
    /// that header can ask for: `timeout_secs`, else `http_timeout_secs`.
    pub fn default_timeout(&self, http_timeout_secs: u64) -> Duration {
        Duration::from_secs(self.timeout_secs.unwrap_or(http_timeout_secs))
    }

    /// Every problem with the values, each naming its key.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.max_message_bytes == 0 {
            problems.push("grpc.max_message_bytes must be greater than 0".to_owned());
        }
        if self.concurrency_per_connection == 0 {
            problems.push("grpc.concurrency_per_connection must be greater than 0".to_owned());
        }
        if self.timeout_secs == Some(0) {
            problems.push("grpc.timeout_secs must be greater than 0".to_owned());
        }
        for (name, client) in &self.clients {
            problems.extend(client.problems(name));
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// One gRPC service the app calls (`grpc.clients.<name>`).
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ClientOptions {
    /// `http://host:port` (HTTP/2 without TLS) or `https://host:port` (TLS
    /// with the system's root certificates). Required.
    pub url: String,
    /// The deadline of calls on this channel, in milliseconds (default
    /// 5000); a call made while handling a request gets the earlier of this
    /// and the request's own deadline.
    pub timeout_ms: u64,
    /// Forward the current request's `authorization` (default `true`).
    pub forward_auth: bool,
    /// TLS settings for `https://` URLs.
    pub tls: ClientTlsOptions,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            url: String::new(),
            timeout_ms: 5000,
            forward_auth: true,
            tls: ClientTlsOptions::default(),
        }
    }
}

impl ClientOptions {
    /// A client of `url` with the defaults.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    fn problems(&self, name: &str) -> Vec<String> {
        let key = format!("grpc.clients.{name}");
        let mut problems = Vec::new();
        let url = self.url.trim();
        let tls = url.starts_with("https://");
        if url.is_empty() {
            problems.push(format!("{key}.url is required"));
        } else if !(tls || url.starts_with("http://")) {
            problems.push(format!(
                "{key}.url must start with http:// or https://, got `{url}`"
            ));
        } else if url.parse::<http::Uri>().is_err() {
            problems.push(format!("{key}.url is not a valid URL: `{url}`"));
        }
        if self.timeout_ms == 0 {
            problems.push(format!("{key}.timeout_ms must be greater than 0"));
        }
        let configured = self.tls.ca.is_some() || self.tls.cert.is_some() || self.tls.key.is_some();
        if configured && !tls && !url.is_empty() {
            problems.push(format!("{key}.tls is set but {key}.url is not https://"));
        }
        if self.tls.cert.is_some() != self.tls.key.is_some() {
            problems.push(format!(
                "{key}.tls.cert and {key}.tls.key go together (a client certificate and its key)"
            ));
        }
        problems
    }
}

/// TLS for a client channel (`grpc.clients.<name>.tls`). The system's root
/// certificates are always trusted; `ca` adds a private CA, and `cert` with
/// `key` present a client certificate (mutual TLS). Files are PEM.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ClientTlsOptions {
    /// A private CA certificate to trust.
    pub ca: Option<PathBuf>,
    /// The client certificate.
    pub cert: Option<PathBuf>,
    /// The client certificate's private key.
    pub key: Option<PathBuf>,
}

/// An address, or none for a missing or empty value (what an empty
/// environment variable holds).
fn optional_addr<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<SocketAddr>, D::Error> {
    let value: Option<String> = Option::deserialize(deserializer)?;
    match value.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(text) => text.parse().map(Some).map_err(|_| {
            serde::de::Error::custom(format!(
                "`{text}` is not a socket address (expected HOST:PORT, e.g. 0.0.0.0:50051)"
            ))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use navree_config::FieldType;

    #[test]
    fn the_section_describes_itself() {
        let schema = GrpcOptions::schema().expect("schema");
        assert_eq!(schema.key, "grpc");
        assert_eq!(schema.docs.as_deref(), Some("/docs/microservices/grpc/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        assert!(schema.secret_paths().is_empty());
        for name in [
            "addr",
            "web",
            "connect",
            "reflection",
            "health",
            "timeout_secs",
            "max_message_bytes",
            "concurrency_per_connection",
            "clients",
        ] {
            let field = schema.fields.iter().find(|field| field.name == name);
            assert!(field.is_some_and(|field| !field.doc.is_empty()), "{name}");
        }
        let field = |name: &str| schema.fields.iter().find(|field| field.name == name);
        assert_eq!(
            field("max_message_bytes").map(|field| &field.ty),
            Some(&FieldType::Integer)
        );
        assert_eq!(field("web").map(|field| &field.ty), Some(&FieldType::Bool));
        let defaults = GrpcOptions::defaults().expect("defaults");
        assert_eq!(defaults["web"], serde_json::json!(true));
        assert_eq!(defaults["max_message_bytes"], serde_json::json!(4_194_304));
        assert_eq!(defaults["addr"], serde_json::Value::Null);
    }

    #[test]
    fn defaults_match_the_docs() {
        let options = GrpcOptions::default();
        assert_eq!(options.addr, None);
        assert!(options.web && options.connect && options.health);
        assert_eq!(options.reflection, cfg!(debug_assertions));
        assert_eq!(options.max_message_bytes, 4_194_304);
        assert_eq!(options.concurrency_per_connection, 256);
        assert_eq!(options.default_timeout(30), Duration::from_secs(30));
        let client = ClientOptions::default();
        assert_eq!(client.timeout_ms, 5000);
        assert!(client.forward_auth);
    }

    #[test]
    fn deserializes_from_strings_like_the_environment() {
        let options: GrpcOptions = serde_json::from_value(serde_json::json!({
            "addr": "0.0.0.0:50051",
            "timeout_secs": 5,
            "clients": { "deals": { "url": "http://deals.internal:8081", "timeout_ms": 3000 } },
        }))
        .unwrap();
        assert_eq!(options.addr, Some("0.0.0.0:50051".parse().unwrap()));
        assert_eq!(options.default_timeout(30), Duration::from_secs(5));
        assert_eq!(options.clients["deals"].timeout_ms, 3000);
        assert!(options.check().is_ok());

        let empty: GrpcOptions = serde_json::from_value(serde_json::json!({ "addr": "" })).unwrap();
        assert_eq!(empty.addr, None);
        assert!(
            serde_json::from_value::<GrpcOptions>(serde_json::json!({ "addr": "nope" })).is_err()
        );
    }

    #[test]
    fn problems_name_their_keys() {
        let mut options = GrpcOptions {
            max_message_bytes: 0,
            ..GrpcOptions::default()
        };
        options.clients.insert("a".into(), ClientOptions::default());
        options
            .clients
            .insert("b".into(), ClientOptions::new("ftp://x"));
        let mut tls = ClientOptions::new("http://c:1");
        tls.tls.cert = Some("client.pem".into());
        options.clients.insert("c".into(), tls);
        let problems = options.check().unwrap_err().join("\n");
        assert!(problems.contains("grpc.max_message_bytes"), "{problems}");
        assert!(
            problems.contains("grpc.clients.a.url is required"),
            "{problems}"
        );
        assert!(
            problems.contains("grpc.clients.b.url must start with"),
            "{problems}"
        );
        assert!(
            problems.contains("grpc.clients.c.tls is set but"),
            "{problems}"
        );
        assert!(
            problems.contains("grpc.clients.c.tls.cert and grpc.clients.c.tls.key"),
            "{problems}"
        );
    }
}

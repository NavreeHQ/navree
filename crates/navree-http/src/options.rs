//! [`HttpOptions`]: the `http` configuration section.

use std::net::SocketAddr;
use std::path::PathBuf;

use http::HeaderValue;
use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::{Deserialize, Deserializer};

/// HTTP server settings (configuration section `http`; every key optional).
///
/// ```toml
/// [http]
/// addr = "0.0.0.0:8080"
/// api_prefix = "/api"
/// timeout_secs = 30
/// cors = { allowed_origins = ["http://localhost:3000"] }
/// errors = { code = true }
/// server = { max_connections = 10000 }
/// ```
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct HttpOptions {
    /// The address `serve` binds (default `127.0.0.1:8080`).
    pub addr: String,
    /// The prefix of every controller route (default `/api`); routes whose
    /// path starts with `!` ignore it.
    pub api_prefix: String,
    /// Cross-origin settings.
    pub cors: CorsOptions,
    /// The request timeout (default 30 s); routes may override it.
    pub timeout_secs: u64,
    /// The default request body limit (default 2 MiB); routes may override it.
    pub body_limit_bytes: usize,
    /// Requests per minute per client IP (default `Some(600)`; `None` disables).
    pub per_ip_rate_limit: Option<u32>,
    /// How much of an IPv6 address names a client for every per-IP limit
    /// (this one, route throttles for guests, the sign-in throttles, the
    /// OAuth endpoint limits): the first `rate_limit_ipv6_prefix` bits, so
    /// all addresses of one network share one budget (default 64: one /64,
    /// the smallest allocation a provider normally gives a customer; 1 to
    /// 128). IPv4 clients are counted per address, and IPv4-mapped IPv6
    /// addresses (`::ffff:192.0.2.1`) as IPv4. See [`rate_limit_key`](crate::rate_limit_key).
    pub rate_limit_ipv6_prefix: u8,
    /// Send the default security headers (default `true`).
    pub security_headers: bool,
    /// Compress responses with gzip (default `true`). Brotli is not built in.
    pub compression: bool,
    /// While this file exists the app answers 503 (default `storage/framework/down`).
    pub maintenance_file: PathBuf,
    /// Request paths still served during maintenance, matched exactly
    /// (default `/health/live`, `/health/ready` and `/metrics`, so probes and
    /// scrapes keep working and an orchestrator does not restart instances
    /// that are only down for maintenance). From the environment, a
    /// comma-separated list (`APP_HTTP__MAINTENANCE_EXCEPT=/health/live,/up`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub maintenance_except: Vec<String>,
    /// Take the client IP from `X-Forwarded-For` / `X-Real-IP` sent by any
    /// peer (default `false`; enable only when every request comes through
    /// a reverse proxy that sets them). Prefer `trusted_proxies`.
    pub trust_proxy: bool,
    /// The reverse proxies (and backends-for-frontends) whose
    /// `X-Forwarded-For` / `X-Real-IP` name the client: IP addresses or
    /// CIDR blocks (`127.0.0.1`, `::1`, `10.0.0.0/8`). A request from any
    /// other peer is identified by its own address, so clients reaching the
    /// app directly cannot choose theirs. In `X-Forwarded-For`, the client
    /// is the first address from the right that is not a listed proxy.
    /// When set, it replaces `trust_proxy`. From the environment, a
    /// comma-separated list (`APP_HTTP__TRUSTED_PROXIES=127.0.0.1,::1`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub trusted_proxies: Vec<String>,
    /// The key for signed URLs. When unset, a key derived from the app key
    /// (`app.key`, or `APP_KEY`) is used; with no app key either, a random
    /// per-process key, so signed URLs do not survive a restart.
    pub signing_key: Option<Secret<String>>,
    /// How long `serve` waits for in-flight requests after a shutdown signal
    /// (default 30 s).
    pub shutdown_grace_secs: u64,
    /// How error bodies are rendered (`http.errors`).
    pub errors: ErrorOptions,
    /// The limits on connections (`http.server`).
    pub server: ServerOptions,
}

/// The limits on connections, enforced by [`serve`](crate::serve()) on every
/// listener (configuration section `http.server`; every key optional). They
/// bound what a client can hold without finishing anything, and the defaults
/// are meant for production.
///
/// A key of type `Option` is turned off with `None`, which TOML cannot write
/// (set it to `null` through `ConfigSources`, or in code, as for
/// `http.per_ip_rate_limit`); `0` fails the boot.
///
/// ```toml
/// [http.server]
/// header_read_timeout_ms = 10000
/// idle_timeout_ms = 75000
/// body_read_timeout_ms = 20000
/// body_min_bytes_per_sec = 256
/// http2_keep_alive_interval_ms = 30000
/// http2_keep_alive_timeout_ms = 20000
/// http2_max_concurrent_streams = 200
/// max_connections = 10000
/// max_connections_per_ip = 256
/// max_header_bytes = 32768
/// max_headers = 100
/// write_stall_timeout_ms = 60000
/// ```
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ServerOptions {
    /// How long a client has to send a request head, from the moment it
    /// connects (or from its first byte, between two requests of a
    /// connection) to the end of the head (default 10 s). A client that
    /// takes longer, however steadily it sends, is disconnected. On HTTP/2 a
    /// new connection has this long to open its first stream. A head sent in
    /// one read with a request that has no body is timed from the end of
    /// that response; after a request with a body it has `idle_timeout_ms`.
    pub header_read_timeout_ms: Option<u64>,
    /// How long a connection may sit with no request in flight before the
    /// server closes it (default 75 s, above the 60 s of common load
    /// balancers, so they close first and never reuse a closing connection).
    /// Nothing a client sends extends it on HTTP/2; on HTTP/1 its next byte
    /// starts a request head, and with `header_read_timeout_ms` off this
    /// deadline stays. A response still on its way to the client is not
    /// idle: `write_stall_timeout_ms` is the limit then.
    pub idle_timeout_ms: Option<u64>,
    /// The longest the server waits for a request body while a handler reads
    /// it (default 20 s): a stall this long ends the read with a 408, and
    /// the connection closes (on HTTP/2 the stream ends and the connection
    /// stays). Only time spent waiting on the client counts.
    pub body_read_timeout_ms: Option<u64>,
    /// The slowest a request body may arrive, averaged over time (default
    /// 256 bytes per second): each byte earns back `1 / rate` seconds of
    /// `body_read_timeout_ms`, so a client dripping bytes runs out of it even
    /// though it never stalls for long. `None` makes any byte restore the
    /// whole timeout. Does nothing without `body_read_timeout_ms`.
    pub body_min_bytes_per_sec: Option<u64>,
    /// How often an HTTP/2 connection that has gone quiet is pinged (default
    /// 30 s), so dead peers are found while a request is in flight.
    pub http2_keep_alive_interval_ms: Option<u64>,
    /// How long a client has to answer an HTTP/2 ping before the connection
    /// closes (default 20 s).
    pub http2_keep_alive_timeout_ms: u64,
    /// The most requests one HTTP/2 connection may have open at once
    /// (default 200).
    pub http2_max_concurrent_streams: u32,
    /// The most connections open at once, WebSockets and TLS handshakes in
    /// progress included (default 10 000). At the cap new connections wait
    /// until one closes, nearly all of them in the listen backlog, and
    /// nothing is read from or answered on them until then. For a client
    /// that has sent something, the connection idle the longest closes, as at
    /// its idle timeout, one for each such client, and a connection that has
    /// only started its next request head counts as idle. A client that has
    /// sent nothing closes nothing, and has `header_read_timeout_ms` to wait.
    pub max_connections: Option<usize>,
    /// The most connections open at once from one client address (default
    /// 256), counted like `http.per_ip_rate_limit`: per IPv4 address, or per
    /// IPv6 network of `http.rate_limit_ipv6_prefix` bits. A connection over
    /// it is closed at once; it cannot wait, because the address is only
    /// known after the accept.
    ///
    /// This limits what one address can hold. It does not stop a client with
    /// many addresses, which `max_connections` is then the limit for.
    ///
    /// Peers in `http.trusted_proxies`, or every peer with `http.trust_proxy`,
    /// are not counted: each carries many clients. Behind a reverse proxy
    /// list it there (`http.per_ip_rate_limit` needs that too) or raise this,
    /// or a busy proxy runs into it. So can many clients sharing one address,
    /// such as an office or a carrier.
    ///
    /// Counted per listener, like `max_connections`, and by the peer of the
    /// socket, so a proxy's clients are never limited here: limit those at
    /// the proxy.
    pub max_connections_per_ip: Option<usize>,
    /// The largest request head, in bytes (default 32 KiB, at least 8 KiB):
    /// the request line and headers on HTTP/1, the header list on HTTP/2,
    /// where each header also counts 32 bytes. Above it the answer is a 431.
    pub max_header_bytes: usize,
    /// The most headers a request may have (default 100, 431 above it). On
    /// HTTP/2 a header can cost a client a single byte, so without this a
    /// small request could carry a thousand into the app; a browser's cookies
    /// can count one each there, as HTTP/2 allows sending them apart.
    pub max_headers: usize,
    /// How long a response may go without moving (default 60 s). When the
    /// socket takes nothing for this long the connection closes; when an
    /// HTTP/2 client's flow control holds one response back for this long,
    /// that stream is reset and the connection stays. Each byte the socket
    /// takes starts it over, and on HTTP/2 each 16 KiB the client takes from
    /// a response, so only a client that has really stopped runs out. It
    /// also applies to what a handler writes to an upgraded HTTP/1 socket,
    /// but not to an HTTP/2 WebSocket held back by flow control: that is the
    /// work of `upgraded_idle_timeout_ms`, or of a timeout on the handler's
    /// own writes. A client pausing a large download for longer loses it and
    /// has to ask for the rest.
    pub write_stall_timeout_ms: Option<u64>,
    /// How long an upgraded connection (a WebSocket) may go with no bytes
    /// from its client before it closes (default `None`: never). A quiet
    /// WebSocket is a healthy one to the server, so this is for apps that
    /// know how their sockets talk: set it above the heartbeat of the
    /// protocol, whose answers are what count. What the server writes does
    /// not, so a heartbeat of its own cannot keep a silent client's socket
    /// open. The connection closes with an error, so a handler sees its read
    /// fail, and a handler must be reading for this to work. On HTTP/2 it
    /// applies once nothing but upgraded streams is left on the connection,
    /// and only `DATA` counts: the frames that look after the connection,
    /// such as keep-alive pings, are silence.
    pub upgraded_idle_timeout_ms: Option<u64>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            header_read_timeout_ms: Some(10_000),
            idle_timeout_ms: Some(75_000),
            body_read_timeout_ms: Some(20_000),
            body_min_bytes_per_sec: Some(256),
            http2_keep_alive_interval_ms: Some(30_000),
            http2_keep_alive_timeout_ms: 20_000,
            http2_max_concurrent_streams: 200,
            max_connections: Some(10_000),
            max_connections_per_ip: Some(256),
            max_header_bytes: 32 * 1024,
            max_headers: 100,
            write_stall_timeout_ms: Some(60_000),
            upgraded_idle_timeout_ms: None,
        }
    }
}

impl ServerOptions {
    /// Adds every problem with these options to `errors`.
    fn check(&self, errors: &mut Vec<String>) {
        let optional = [
            ("header_read_timeout_ms", self.header_read_timeout_ms),
            ("idle_timeout_ms", self.idle_timeout_ms),
            ("body_read_timeout_ms", self.body_read_timeout_ms),
            ("body_min_bytes_per_sec", self.body_min_bytes_per_sec),
            (
                "http2_keep_alive_interval_ms",
                self.http2_keep_alive_interval_ms,
            ),
            (
                "max_connections",
                self.max_connections.map(|connections| connections as u64),
            ),
            (
                "max_connections_per_ip",
                self.max_connections_per_ip
                    .map(|connections| connections as u64),
            ),
            ("write_stall_timeout_ms", self.write_stall_timeout_ms),
            ("upgraded_idle_timeout_ms", self.upgraded_idle_timeout_ms),
        ];
        for (key, value) in optional {
            if value == Some(0) {
                errors.push(format!("http.server.{key} must be at least 1 (or unset)"));
            }
        }
        if self
            .max_connections
            .is_some_and(|connections| connections > tokio::sync::Semaphore::MAX_PERMITS)
        {
            errors.push("http.server.max_connections is too large".to_owned());
        }
        if self.http2_keep_alive_timeout_ms == 0 {
            errors.push("http.server.http2_keep_alive_timeout_ms must be at least 1".to_owned());
        }
        if self.http2_max_concurrent_streams == 0 {
            errors.push("http.server.http2_max_concurrent_streams must be at least 1".to_owned());
        }
        if self.max_header_bytes < crate::serve::MIN_HEADER_BYTES
            || u32::try_from(self.max_header_bytes).is_err()
        {
            errors.push(format!(
                "http.server.max_header_bytes must be between {} and {} (got {})",
                crate::serve::MIN_HEADER_BYTES,
                u32::MAX,
                self.max_header_bytes
            ));
        }
        if self.max_headers == 0 {
            errors.push("http.server.max_headers must be at least 1".to_owned());
        }
    }
}

/// How error bodies are rendered (configuration section `http.errors`).
///
/// ```toml
/// [http.errors]
/// code = true
/// ```
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ErrorOptions {
    /// Send the machine-readable `code` member in every error body (default
    /// `true`). `false` leaves it out (`APP_HTTP__ERRORS__CODE=false`).
    pub code: bool,
}

impl Default for ErrorOptions {
    fn default() -> Self {
        Self { code: true }
    }
}

impl Default for HttpOptions {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1:8080".to_owned(),
            api_prefix: "/api".to_owned(),
            cors: CorsOptions::default(),
            timeout_secs: 30,
            body_limit_bytes: 2 * 1024 * 1024,
            per_ip_rate_limit: Some(600),
            rate_limit_ipv6_prefix: crate::network::DEFAULT_IPV6_PREFIX,
            security_headers: true,
            compression: true,
            maintenance_file: PathBuf::from("storage/framework/down"),
            maintenance_except: ["/health/live", "/health/ready", "/metrics"]
                .map(str::to_owned)
                .to_vec(),
            trust_proxy: false,
            trusted_proxies: Vec::new(),
            signing_key: None,
            shutdown_grace_secs: 30,
            errors: ErrorOptions::default(),
            server: ServerOptions::default(),
        }
    }
}

/// Cross-origin resource sharing: only the listed origins, with credentials,
/// and explicitly listed request and exposed headers (no wildcards). Applied
/// by [`CorsLayer`](crate::CorsLayer); an origin that is not listed gets no
/// CORS headers.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CorsOptions {
    /// Exact origins allowed to call the API, e.g. `http://localhost:3000`.
    /// Empty (the default) disables CORS. `*` is rejected. From the
    /// environment, a comma-separated list
    /// (`APP_HTTP__CORS__ALLOWED_ORIGINS=http://localhost:3000,https://app.example.com`).
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub allowed_origins: Vec<String>,
    /// Allow cookies on cross-origin requests (default `true`).
    pub allow_credentials: bool,
    /// How long browsers may cache a preflight answer (default 600 s).
    pub max_age_secs: u64,
}

impl Default for CorsOptions {
    fn default() -> Self {
        Self {
            allowed_origins: Vec::new(),
            allow_credentials: true,
            max_age_secs: 600,
        }
    }
}

/// A list, or one comma-separated string (what an environment variable
/// holds); entries are trimmed and empty ones dropped.
fn list_or_comma_separated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Origins {
        List(Vec<String>),
        Joined(String),
    }
    let origins = match Origins::deserialize(deserializer)? {
        Origins::List(origins) => origins,
        Origins::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(origins
        .into_iter()
        .map(|origin| origin.trim().to_owned())
        .filter(|origin| !origin.is_empty())
        .collect())
}

impl ConfigSection for HttpOptions {
    const KEY: &'static str = "http";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

impl HttpOptions {
    /// The key a client's per-IP limits are counted under: its IPv4 address,
    /// or the network of its IPv6 address (`rate_limit_ipv6_prefix`).
    pub fn rate_limit_key(&self, ip: std::net::IpAddr) -> std::net::IpAddr {
        crate::network::rate_limit_key(ip, self.rate_limit_ipv6_prefix)
    }

    /// Every problem with these options (empty when valid).
    pub(crate) fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.addr.parse::<SocketAddr>().is_err() && !self.addr.contains(':') {
            errors.push(format!(
                "http.addr must be HOST:PORT, e.g. 127.0.0.1:8080 (got `{}`)",
                self.addr
            ));
        }
        if !self.api_prefix.is_empty() && !self.api_prefix.starts_with('/') {
            errors.push("http.api_prefix must be empty or start with `/`".to_owned());
        }
        if self.timeout_secs == 0 {
            errors.push("http.timeout_secs must be at least 1".to_owned());
        }
        if self.per_ip_rate_limit == Some(0) {
            errors.push("http.per_ip_rate_limit must be at least 1 (or unset)".to_owned());
        }
        self.server.check(&mut errors);
        if !(1..=128).contains(&self.rate_limit_ipv6_prefix) {
            errors.push(format!(
                "http.rate_limit_ipv6_prefix must be between 1 and 128 (got {})",
                self.rate_limit_ipv6_prefix
            ));
        }
        for proxy in &self.trusted_proxies {
            if crate::extract::IpNetwork::parse(proxy).is_none() {
                errors.push(format!(
                    "http.trusted_proxies: `{proxy}` is not an IP address or CIDR block like 10.0.0.0/8"
                ));
            }
        }
        for origin in &self.cors.allowed_origins {
            if origin.trim() == "*" {
                errors.push(
                    "http.cors.allowed_origins must list exact origins; `*` is not allowed"
                        .to_owned(),
                );
            } else if HeaderValue::from_str(origin).is_err()
                || !(origin.starts_with("http://") || origin.starts_with("https://"))
            {
                errors.push(format!(
                    "http.cors.allowed_origins: `{origin}` is not an origin like https://app.example.com"
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_plan() {
        let options = HttpOptions::default();
        assert_eq!(options.addr, "127.0.0.1:8080");
        assert_eq!(options.api_prefix, "/api");
        assert_eq!(options.timeout_secs, 30);
        assert_eq!(options.body_limit_bytes, 2 * 1024 * 1024);
        assert_eq!(options.per_ip_rate_limit, Some(600));
        assert!(options.security_headers && options.compression && !options.trust_proxy);
        assert!(options.cors.allow_credentials);
        assert!(options.check().is_ok());
    }

    #[test]
    fn error_codes_are_rendered_unless_the_environment_turns_them_off() {
        assert!(HttpOptions::default().errors.code, "on by default");
        let config = navree_config::Config::load(
            &navree_config::ConfigSources::new()
                .env_vars("APP_", [("APP_HTTP__ERRORS__CODE", "false")]),
        )
        .unwrap();
        let options = config.typed::<HttpOptions>().unwrap();
        assert!(!options.errors.code);
        assert_eq!(options.api_prefix, "/api", "other keys keep defaults");
    }

    #[test]
    fn missing_keys_deserialize_to_defaults() {
        let options: HttpOptions = serde_json::from_value(
            serde_json::json!({"cors": {"allowed_origins": ["https://a.example"]}}),
        )
        .unwrap();
        assert_eq!(options.api_prefix, "/api");
        assert!(options.cors.allow_credentials);
        assert_eq!(options.cors.allowed_origins, ["https://a.example"]);
        assert!(options.errors.code);
    }

    #[test]
    fn trusted_proxies_come_from_a_list_or_the_environment_and_are_checked() {
        let config = navree_config::Config::load(&navree_config::ConfigSources::new().env_vars(
            "APP_",
            [("APP_HTTP__TRUSTED_PROXIES", "127.0.0.1, ::1,10.0.0.0/8")],
        ))
        .unwrap();
        let options = config.typed::<HttpOptions>().unwrap();
        assert_eq!(options.trusted_proxies, ["127.0.0.1", "::1", "10.0.0.0/8"]);
        assert!(HttpOptions::default().trusted_proxies.is_empty());

        let options = HttpOptions {
            trusted_proxies: vec![
                "127.0.0.1".into(),
                "proxy.internal".into(),
                "10.0.0.0/40".into(),
            ],
            ..HttpOptions::default()
        };
        let errors = options.check().unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains("proxy.internal"), "{errors:?}");
    }

    #[test]
    fn the_ipv6_rate_limit_prefix_defaults_to_a_slash_64_and_is_checked() {
        assert_eq!(HttpOptions::default().rate_limit_ipv6_prefix, 64);
        let config = navree_config::Config::load(
            &navree_config::ConfigSources::new()
                .env_vars("APP_", [("APP_HTTP__RATE_LIMIT_IPV6_PREFIX", "56")]),
        )
        .unwrap();
        let options = config.typed::<HttpOptions>().unwrap();
        assert_eq!(options.rate_limit_ipv6_prefix, 56);
        assert_eq!(
            options.rate_limit_key("2001:db8:1:ff7f::1".parse().unwrap()),
            "2001:db8:1:ff00::".parse::<std::net::IpAddr>().unwrap()
        );
        for bad in [0, 129] {
            let options = HttpOptions {
                rate_limit_ipv6_prefix: bad,
                ..HttpOptions::default()
            };
            let errors = options.check().unwrap_err();
            assert!(errors[0].contains("rate_limit_ipv6_prefix"), "{errors:?}");
        }
    }

    #[test]
    fn server_limits_default_to_safe_values_and_come_from_the_environment() {
        let server = HttpOptions::default().server;
        assert_eq!(server.header_read_timeout_ms, Some(10_000));
        assert_eq!(server.idle_timeout_ms, Some(75_000));
        assert_eq!(server.body_read_timeout_ms, Some(20_000));
        assert_eq!(server.body_min_bytes_per_sec, Some(256));
        assert_eq!(server.http2_keep_alive_interval_ms, Some(30_000));
        assert_eq!(server.http2_keep_alive_timeout_ms, 20_000);
        assert_eq!(server.http2_max_concurrent_streams, 200);
        assert_eq!(server.max_connections, Some(10_000));
        assert_eq!(server.max_connections_per_ip, Some(256));
        assert_eq!(server.max_header_bytes, 32 * 1024);
        assert_eq!(server.max_headers, 100);
        assert_eq!(server.write_stall_timeout_ms, Some(60_000));
        assert_eq!(server.upgraded_idle_timeout_ms, None);

        let config = navree_config::Config::load(&navree_config::ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_HTTP__SERVER__HEADER_READ_TIMEOUT_MS", "2500"),
                ("APP_HTTP__SERVER__MAX_CONNECTIONS", "64"),
            ],
        ))
        .unwrap();
        let options = config.typed::<HttpOptions>().unwrap();
        assert_eq!(options.server.header_read_timeout_ms, Some(2_500));
        assert_eq!(options.server.max_connections, Some(64));
        assert_eq!(
            options.server.idle_timeout_ms,
            Some(75_000),
            "other keys keep defaults"
        );
    }

    #[test]
    fn server_limits_are_checked() {
        let options = HttpOptions {
            server: ServerOptions {
                header_read_timeout_ms: Some(0),
                idle_timeout_ms: Some(0),
                body_read_timeout_ms: Some(0),
                body_min_bytes_per_sec: Some(0),
                http2_keep_alive_interval_ms: Some(0),
                http2_keep_alive_timeout_ms: 0,
                http2_max_concurrent_streams: 0,
                max_connections: Some(0),
                max_connections_per_ip: Some(0),
                max_header_bytes: 1024,
                max_headers: 0,
                write_stall_timeout_ms: Some(0),
                upgraded_idle_timeout_ms: Some(0),
            },
            ..HttpOptions::default()
        };
        let errors = options.check().unwrap_err();
        assert_eq!(errors.len(), 13, "{errors:?}");
        assert!(errors[0].contains("header_read_timeout_ms"), "{errors:?}");
        for key in [
            "max_connections_per_ip",
            "write_stall_timeout_ms",
            "upgraded_idle_timeout_ms",
        ] {
            assert!(errors.iter().any(|e| e.contains(key)), "{errors:?}");
        }

        // Off is `None`, never `0`.
        let options = HttpOptions {
            server: ServerOptions {
                header_read_timeout_ms: None,
                idle_timeout_ms: None,
                body_read_timeout_ms: None,
                body_min_bytes_per_sec: None,
                http2_keep_alive_interval_ms: None,
                max_connections: None,
                max_connections_per_ip: None,
                write_stall_timeout_ms: None,
                upgraded_idle_timeout_ms: None,
                ..ServerOptions::default()
            },
            ..HttpOptions::default()
        };
        assert!(options.check().is_ok());
    }

    #[test]
    fn allowed_origins_can_come_from_one_environment_variable() {
        let config = navree_config::Config::load(&navree_config::ConfigSources::new().env_vars(
            "APP_",
            [(
                "APP_HTTP__CORS__ALLOWED_ORIGINS",
                "http://localhost:3000, https://app.example.com,",
            )],
        ))
        .unwrap();
        let options = config.typed::<HttpOptions>().unwrap();
        assert_eq!(
            options.cors.allowed_origins,
            ["http://localhost:3000", "https://app.example.com"]
        );
        assert!(options.cors.allow_credentials, "other keys keep defaults");
    }

    #[test]
    fn wildcard_and_malformed_origins_are_rejected() {
        let mut options = HttpOptions::default();
        options.cors.allowed_origins = vec!["*".into(), "localhost:3000".into()];
        let errors = options.check().unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains('*'));
    }
}

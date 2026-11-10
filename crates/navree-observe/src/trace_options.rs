//! [`TracingOptions`]: the `observe.tracing` configuration section.

use std::collections::BTreeMap;
use std::fmt;

use navree_config::{Config, ConfigSection};
use navree_telemetry::{TraceId, TracePolicy, TracePolicySection};
use serde::{Deserialize, Serialize};

/// The OTLP transport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum OtlpProtocol {
    /// OTLP over gRPC (port 4317).
    #[default]
    Grpc,
    /// OTLP over HTTP with protobuf bodies (port 4318), posted to `/v1/traces`.
    Http,
}

/// Tracing settings: configuration section `observe.tracing` (every key
/// optional).
///
/// ```toml
/// [observe.tracing]
/// endpoint = "http://127.0.0.1:4317"   # export spans over OTLP (feature `otel`)
/// sample_ratio = 0.2
/// ```
///
/// navree-http, navree-queue and navree-schedule read `enabled`,
/// `trust_incoming` and `sample_ratio` to start each request's, job's and
/// task's trace; `ObserveModule` reads the rest to export spans. Keys the
/// section leaves out fall back to OpenTelemetry's variables:
/// `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_PROTOCOL` (`grpc` or
/// `http/protobuf`), `OTEL_EXPORTER_OTLP_HEADERS` and
/// `OTEL_RESOURCE_ATTRIBUTES` (`k=v,k=v`), `OTEL_SERVICE_NAME` and
/// `OTEL_TRACES_SAMPLER_ARG`.
#[derive(Clone, PartialEq, Serialize, Deserialize, ConfigSection)]
#[config(
    key = "observe.tracing",
    docs = "/docs/microservices/tracing/",
    validate = Self::check
)]
#[serde(default)]
#[non_exhaustive]
pub struct TracingOptions {
    /// Record spans and propagate trace context (default `true`).
    pub enabled: bool,
    /// The OTLP endpoint; without one, nothing is exported.
    #[config(env_fallback = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    pub endpoint: Option<String>,
    /// `grpc` (default) or `http`.
    #[config(one_of = "grpc, http", env_fallback = "OTEL_EXPORTER_OTLP_PROTOCOL")]
    pub protocol: OtlpProtocol,
    /// Headers sent with every export, such as a vendor's API key.
    #[config(secret, env_fallback = "OTEL_EXPORTER_OTLP_HEADERS")]
    #[serde(skip_serializing)]
    pub headers: BTreeMap<String, String>,
    /// The `service.name` of every span: this key, then `micro.service`,
    /// then `app.name`, then the binary's name.
    #[config(env_fallback = "OTEL_SERVICE_NAME")]
    pub service_name: Option<String>,
    /// Extra resource attributes (`deployment.environment = "production"`).
    #[config(env_fallback = "OTEL_RESOURCE_ATTRIBUTES")]
    pub resource: BTreeMap<String, String>,
    /// The share of new traces recorded, 0 to 1 (default `1.0`).
    #[config(env_fallback = "OTEL_TRACES_SAMPLER_ARG")]
    pub sample_ratio: f64,
    /// Continue traces from incoming `traceparent` headers (default `true`).
    pub trust_incoming: bool,
    /// Also print finished spans to stdout, one JSON object per line.
    pub stdout: bool,
    /// The time limit of one export, in milliseconds (default 10000).
    pub timeout_ms: u64,
    /// Spans waiting for export (default 2048); when full, new spans are
    /// dropped and counted in `navree_tracing_spans_dropped_total`.
    pub max_queue_size: usize,
    /// Your backend's trace page, with `{trace_id}` (for links to traces).
    pub trace_url: Option<String>,
    /// Whether `endpoint` came from `OTEL_EXPORTER_OTLP_ENDPOINT`.
    #[serde(skip)]
    pub(crate) endpoint_from_env: bool,
}

impl Default for TracingOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: None,
            protocol: OtlpProtocol::Grpc,
            headers: BTreeMap::new(),
            service_name: None,
            resource: BTreeMap::new(),
            sample_ratio: 1.0,
            trust_incoming: true,
            stdout: false,
            timeout_ms: 10_000,
            max_queue_size: 2048,
            trace_url: None,
            endpoint_from_env: false,
        }
    }
}

impl fmt::Debug for TracingOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TracingOptions")
            .field("enabled", &self.enabled)
            .field("endpoint", &self.endpoint)
            .field("protocol", &self.protocol)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("service_name", &self.service_name)
            .field("resource", &self.resource)
            .field("sample_ratio", &self.sample_ratio)
            .field("trust_incoming", &self.trust_incoming)
            .field("stdout", &self.stdout)
            .field("timeout_ms", &self.timeout_ms)
            .field("max_queue_size", &self.max_queue_size)
            .field("trace_url", &self.trace_url)
            .finish()
    }
}

impl TracingOptions {
    /// The section from `config`, with the `OTEL_*` environment variables
    /// for the keys it leaves out, and the service name resolved.
    pub fn from_config(config: &Config) -> Result<TracingOptions, String> {
        Self::from_config_with_env(config, |name| std::env::var(name).ok())
    }

    /// [`from_config`](TracingOptions::from_config) with `env` in place of
    /// the process environment.
    pub fn from_config_with_env(
        config: &Config,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<TracingOptions, String> {
        let mut options: TracingOptions = config.typed().map_err(|error| error.to_string())?;
        // The keys that start a trace, read as navree-http, navree-queue,
        // navree-schedule and navree-micro read them, so they always agree.
        let section = config
            .section_or_default::<TracePolicySection>(TracePolicy::SECTION)
            .map_err(|error| error.to_string())?;
        let policy = TracePolicy::from_section(section, &env)?;
        options.enabled = policy.enabled;
        options.trust_incoming = policy.trust_incoming;
        options.sample_ratio = policy.sample_ratio;
        // The other keys the section leaves out come from the variables
        // their fields declare (`config:show --fallbacks` lists them).
        let fallback = |field: &str| config.fallback_with::<TracingOptions>(field, &env);
        if let Some(endpoint) = fallback("endpoint") {
            options.endpoint = Some(endpoint.value);
            options.endpoint_from_env = true;
        }
        if let Some(protocol) = fallback("protocol") {
            options.protocol = match protocol.value.trim() {
                "grpc" => OtlpProtocol::Grpc,
                "http/protobuf" => OtlpProtocol::Http,
                other => {
                    return Err(format!(
                        "{} must be `grpc` or `http/protobuf`, got `{other}`",
                        protocol.variable
                    ));
                }
            };
        }
        if let Some(headers) = fallback("headers") {
            options.headers = pairs(&headers.value);
        }
        if let Some(resource) = fallback("resource") {
            options.resource = pairs(&resource.value);
        }
        if options.service_name.is_none() {
            options.service_name = fallback("service_name")
                .map(|name| name.value)
                .or_else(|| config.get_string("micro.service"))
                .or_else(|| config.app_name())
                .or_else(binary_name);
        }
        options.check()?;
        Ok(options)
    }

    /// Checks the values; the message names the key.
    fn check(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.sample_ratio) {
            return Err(format!(
                "observe.tracing.sample_ratio must be from 0 to 1, got {}",
                self.sample_ratio
            ));
        }
        if self.timeout_ms == 0 {
            return Err("observe.tracing.timeout_ms must be greater than 0".to_owned());
        }
        if self.max_queue_size == 0 {
            return Err("observe.tracing.max_queue_size must be greater than 0".to_owned());
        }
        if let Some(endpoint) = &self.endpoint
            && !(endpoint.starts_with("http://") || endpoint.starts_with("https://"))
        {
            return Err(format!(
                "observe.tracing.endpoint must be an http:// or https:// URL, got `{endpoint}`"
            ));
        }
        Ok(())
    }

    /// Whether spans leave the process: tracing is on and an endpoint or
    /// `stdout` is set.
    pub fn exports(&self) -> bool {
        self.enabled && (self.endpoint.is_some() || self.stdout)
    }

    /// The URL spans are sent to: the endpoint as given for gRPC, with
    /// `/v1/traces` appended for HTTP when its path is empty.
    pub fn traces_endpoint(&self) -> Option<String> {
        let endpoint = self.endpoint.as_deref()?.trim_end_matches('/');
        match self.protocol {
            OtlpProtocol::Grpc => Some(endpoint.to_owned()),
            OtlpProtocol::Http => {
                let after_scheme = endpoint
                    .split_once("://")
                    .map_or(endpoint, |(_, rest)| rest);
                if after_scheme.contains('/') {
                    Some(endpoint.to_owned())
                } else {
                    Some(format!("{endpoint}/v1/traces"))
                }
            }
        }
    }

    /// The link to `trace_id` on your backend (`trace_url` with
    /// `{trace_id}` replaced), when `trace_url` is set.
    pub fn trace_link(&self, trace_id: &TraceId) -> Option<String> {
        self.trace_url
            .as_ref()
            .map(|url| url.replace("{trace_id}", &trace_id.to_string()))
    }
}

/// `k=v,k2=v2` (the `OTEL_*` list format; values may be percent-encoded).
fn pairs(text: &str) -> BTreeMap<String, String> {
    text.split(',')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), percent_decode(value.trim())))
        .filter(|(key, _)| !key.is_empty())
        .collect()
}

/// Decodes `%XX` escapes (invalid ones are kept as they are).
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The running binary's name.
fn binary_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_stem()?
        .to_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;

    use super::*;

    /// The section and the policy navree-http, navree-queue and
    /// navree-schedule start traces with read the same three keys; they must
    /// not drift apart.
    #[test]
    fn the_section_and_the_trace_policy_read_the_policy_keys_alike() {
        use navree_config::{Config, ConfigSources};

        // The keys a case sets, and the error it expects.
        type Case<'a> = (&'a [(&'a str, &'a str)], Option<&'a str>);
        let cases: [Case<'_>; 6] = [
            (&[], None),
            (&[("enabled", "false"), ("sample_ratio", "0.25")], None),
            (&[("trust_incoming", "no")], None),
            (&[("sample_ratio", "2")], None),
            (&[], Some("0.5")),
            (&[], Some("lots")),
        ];
        for (keys, env_ratio) in cases {
            let mut sources = ConfigSources::new();
            for (key, value) in keys {
                sources = sources.set(&format!("observe.tracing.{key}"), *value);
            }
            let config = Config::load(&sources).expect("load");
            let env = |name: &str| {
                (name == "OTEL_TRACES_SAMPLER_ARG")
                    .then(|| env_ratio.map(str::to_owned))
                    .flatten()
            };
            let options = TracingOptions::from_config_with_env(&config, env);
            let section = config
                .section_or_default::<TracePolicySection>(TracePolicy::SECTION)
                .map_err(|error| error.to_string());
            let policy = section.and_then(|section| TracePolicy::from_section(section, env));
            match (options, policy) {
                (Ok(options), Ok(policy)) => {
                    assert_eq!(options.enabled, policy.enabled, "{keys:?} {env_ratio:?}");
                    assert_eq!(
                        options.trust_incoming, policy.trust_incoming,
                        "{keys:?} {env_ratio:?}"
                    );
                    assert_eq!(
                        options.sample_ratio, policy.sample_ratio,
                        "{keys:?} {env_ratio:?}"
                    );
                }
                (Err(_), Err(_)) => {}
                (options, policy) => panic!(
                    "{keys:?} {env_ratio:?}: the section says {:?}, the policy {:?}",
                    options.map(|o| o.sample_ratio),
                    policy.map(|p| p.sample_ratio)
                ),
            }
        }
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = TracingOptions::schema().expect("schema");
        assert_eq!(schema.key, "observe.tracing");
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        // The headers carry a vendor's key, so they are a credential.
        assert_eq!(schema.secret_paths(), ["observe.tracing.headers"]);
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(field("protocol").ty, FieldType::choice(&["grpc", "http"]));
        assert_eq!(field("sample_ratio").ty, FieldType::Float);
        let fallbacks: Vec<(String, String)> = schema
            .env_fallbacks()
            .into_iter()
            .map(|fallback| (fallback.key, fallback.variable))
            .collect();
        for (key, variable) in [
            ("observe.tracing.endpoint", "OTEL_EXPORTER_OTLP_ENDPOINT"),
            ("observe.tracing.protocol", "OTEL_EXPORTER_OTLP_PROTOCOL"),
            ("observe.tracing.headers", "OTEL_EXPORTER_OTLP_HEADERS"),
            ("observe.tracing.service_name", "OTEL_SERVICE_NAME"),
            ("observe.tracing.resource", "OTEL_RESOURCE_ATTRIBUTES"),
            ("observe.tracing.sample_ratio", "OTEL_TRACES_SAMPLER_ARG"),
        ] {
            assert!(
                fallbacks.contains(&(key.to_owned(), variable.to_owned())),
                "{key}: {fallbacks:?}"
            );
        }
        // What the policy reads is what the field declares.
        assert_eq!(
            navree_config::env_fallback::<TracingOptions>("sample_ratio").as_deref(),
            Some(navree_telemetry::SAMPLE_RATIO_ENV)
        );
        let defaults = TracingOptions::defaults().expect("defaults");
        assert_eq!(defaults["enabled"], true);
        assert_eq!(defaults["sample_ratio"], 1.0);
        assert_eq!(defaults["timeout_ms"], 10_000);
    }

    #[test]
    fn the_headers_never_show_in_debug_or_json() {
        let options = TracingOptions {
            headers: BTreeMap::from([("authorization".to_owned(), "Bearer hunter2".to_owned())]),
            ..TracingOptions::default()
        };
        assert!(!format!("{options:?}").contains("hunter2"));
        let json = serde_json::to_string(&options).expect("serializes");
        assert!(!json.contains("hunter2"), "{json}");
    }

    #[test]
    fn pairs_are_split_and_decoded() {
        let parsed = pairs("authorization=Bearer%20abc, x-team = crm,broken,=empty");
        assert_eq!(parsed["authorization"], "Bearer abc");
        assert_eq!(parsed["x-team"], "crm");
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn grpc_endpoints_are_used_as_given() {
        let options = TracingOptions {
            endpoint: Some("http://collector:4317/".to_owned()),
            ..TracingOptions::default()
        };
        assert_eq!(
            options.traces_endpoint().as_deref(),
            Some("http://collector:4317")
        );
        let http = TracingOptions {
            endpoint: Some("https://otlp.example.com/custom/traces".to_owned()),
            protocol: OtlpProtocol::Http,
            ..TracingOptions::default()
        };
        assert_eq!(
            http.traces_endpoint().as_deref(),
            Some("https://otlp.example.com/custom/traces")
        );
    }
}

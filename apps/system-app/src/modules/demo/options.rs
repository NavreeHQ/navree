//! The `demo` configuration section: where the system app's own outgoing
//! calls go, and how long it waits for them.

use navree::config::ConfigSection;
use serde::Deserialize;

/// Settings of the demo routes (configuration section `demo`).
///
/// ```toml
/// [demo]
/// stub_base_url = "http://127.0.0.1:8080"
/// sink_base_url = "http://web:8080"
/// mail_info_url = "http://mailpit:8025/api/v1/info"
/// search_health_url = "http://meilisearch:7700/health"
/// ```
///
/// Every key is optional; the defaults point at services on this machine.
/// From the environment: `APP_DEMO__STUB_BASE_URL=http://127.0.0.1:8080`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct DemoOptions {
    /// The base URL of the app's own `/api/demo/stub/{status}` route, as
    /// the app itself reaches it. The upstream the outgoing calls hit.
    pub stub_base_url: String,
    /// The base URL of the sinks and stubs as every process of the app
    /// reaches them (the web process by its name on the network): where
    /// webhook endpoints seeded for the workspaces post to.
    pub sink_base_url: String,
    /// The mail catcher's information endpoint, called by
    /// `GET /api/demo/outgoing/ok-mailpit`.
    pub mail_info_url: String,
    /// The search engine's health endpoint, called by
    /// `GET /api/demo/outgoing/ok-search`.
    pub search_health_url: String,
    /// An address where nothing listens: the connection is refused.
    pub refused_url: String,
    /// An address whose host name does not resolve.
    pub unresolvable_url: String,
    /// How long an outgoing call may take before the client gives up, in
    /// milliseconds.
    pub call_timeout_ms: u64,
}

impl Default for DemoOptions {
    fn default() -> Self {
        Self {
            stub_base_url: "http://127.0.0.1:8080".to_owned(),
            sink_base_url: "http://127.0.0.1:8080".to_owned(),
            mail_info_url: "http://127.0.0.1:8025/api/v1/info".to_owned(),
            search_health_url: "http://127.0.0.1:7700/health".to_owned(),
            refused_url: "http://127.0.0.1:9/ledger/entries".to_owned(),
            unresolvable_url: "http://ledger.system.invalid/api/entries".to_owned(),
            call_timeout_ms: 3_000,
        }
    }
}

impl ConfigSection for DemoOptions {
    const KEY: &'static str = "demo";

    fn validate(&self) -> Result<(), String> {
        for (key, url) in [
            ("stub_base_url", &self.stub_base_url),
            ("sink_base_url", &self.sink_base_url),
            ("mail_info_url", &self.mail_info_url),
            ("search_health_url", &self.search_health_url),
            ("refused_url", &self.refused_url),
            ("unresolvable_url", &self.unresolvable_url),
        ] {
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(format!("`{key}` must be an http(s) URL"));
            }
        }
        if self.call_timeout_ms == 0 {
            return Err("`call_timeout_ms` must be at least 1".to_owned());
        }
        Ok(())
    }
}

impl DemoOptions {
    /// The URL of the stub route answering `status` after `delay_ms`.
    pub fn stub_url(&self, status: u16, delay_ms: u64) -> String {
        format!(
            "{}/api/demo/stub/{status}?delay_ms={delay_ms}",
            self.stub_base_url.trim_end_matches('/')
        )
    }

    /// The URL of the sink named `name` (`alerts` or `slack`), as every
    /// process reaches it.
    pub fn sink_url(&self, name: &str) -> String {
        format!(
            "{}/api/demo/sink/{name}",
            self.sink_base_url.trim_end_matches('/')
        )
    }

    /// The URL of the stub route answering `status`, as every process
    /// reaches it.
    pub fn shared_stub_url(&self, status: u16) -> String {
        format!(
            "{}/api/demo/stub/{status}",
            self.sink_base_url.trim_end_matches('/')
        )
    }
}

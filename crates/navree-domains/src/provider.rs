//! DNS providers that manage a zone of your own: [`DnsProvider`], with
//! [`CloudflareDns`] and [`DnsProviderFake`] for tests.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree_kernel::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::DomainsError;
use crate::dns::normal_name;
use crate::options::CloudflareOptions;

/// A DNS record type a provider manages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecordKind {
    /// An IPv4 address.
    A,
    /// An IPv6 address.
    Aaaa,
    /// An alias of another name.
    Cname,
    /// Text.
    Txt,
}

impl RecordKind {
    /// `A`, `AAAA`, `CNAME` or `TXT`.
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::A => "A",
            RecordKind::Aaaa => "AAAA",
            RecordKind::Cname => "CNAME",
            RecordKind::Txt => "TXT",
        }
    }
}

impl fmt::Display for RecordKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Changes records in the zone `domains.dns.zone`. A domain added inside
/// that zone gets its records through the provider and is verified at
/// once; removing it deletes them. The module builds [`CloudflareDns`]
/// from `domains.dns.provider = "cloudflare"`; replace it with
/// `override_value::<dyn DnsProvider>(..)`, for example with a
/// [`DnsProviderFake`] in tests.
pub trait DnsProvider: Send + Sync + 'static {
    /// Makes `values` the records of type `kind` at `name`: creates the
    /// missing ones and deletes the others.
    fn set<'a>(
        &'a self,
        kind: RecordKind,
        name: &'a str,
        values: &'a [String],
    ) -> BoxFuture<'a, Result<(), DomainsError>>;

    /// Deletes every record of type `kind` at `name` (none is not an
    /// error).
    fn delete<'a>(&'a self, kind: RecordKind, name: &'a str) -> BoxFuture<'a, Result<(), DomainsError>>;
}

/// The Cloudflare DNS API (`/zones/{zone_id}/dns_records`) with an API
/// token that has `Zone.DNS:Edit` on the zone.
pub struct CloudflareDns {
    http: reqwest::Client,
    api_url: String,
    zone_id: String,
    token: String,
    proxied: bool,
    ttl: u32,
}

impl fmt::Debug for CloudflareDns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudflareDns")
            .field("api_url", &self.api_url)
            .field("zone_id", &self.zone_id)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<ApiMessage>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct ApiMessage {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct ApiRecord {
    id: String,
    content: String,
}

impl CloudflareDns {
    /// A driver from `domains.dns.cloudflare`.
    pub fn new(options: &CloudflareOptions) -> Result<CloudflareDns, DomainsError> {
        let missing = |key: &str| {
            DomainsError::Config(format!(
                "domains.dns.cloudflare.{key} is required with provider = \"cloudflare\""
            ))
        };
        let token = options
            .api_token
            .clone()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| missing("api_token"))?;
        let zone_id = options
            .zone_id
            .clone()
            .filter(|zone| !zone.is_empty())
            .ok_or_else(|| missing("zone_id"))?;
        // reqwest is built without a default crypto provider: use ring.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .no_proxy()
            .build()
            .map_err(|error| DomainsError::Config(format!("cannot build the HTTP client: {error}")))?;
        Ok(CloudflareDns {
            http,
            api_url: options.api_url.trim_end_matches('/').to_owned(),
            zone_id,
            token,
            proxied: options.proxied,
            ttl: options.ttl.max(1),
        })
    }

    fn records_url(&self) -> String {
        format!("{}/zones/{}/dns_records", self.api_url, self.zone_id)
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Option<T>, DomainsError> {
        let response = request
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|error| DomainsError::DnsProvider(format!("Cloudflare: {error}")))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|error| DomainsError::DnsProvider(format!("Cloudflare: {error}")))?;
        let envelope: Envelope<T> = serde_json::from_slice(&body).map_err(|_| {
            DomainsError::DnsProvider(format!("Cloudflare answered {} without JSON", status.as_u16()))
        })?;
        if !status.is_success() || !envelope.success {
            let message = envelope
                .errors
                .first()
                .map(|error| format!("Cloudflare error {}: {}", error.code, error.message))
                .unwrap_or_else(|| format!("Cloudflare answered {}", status.as_u16()));
            return Err(DomainsError::DnsProvider(message));
        }
        Ok(envelope.result)
    }

    async fn list(&self, kind: RecordKind, name: &str) -> Result<Vec<ApiRecord>, DomainsError> {
        let request = self.http.get(self.records_url()).query(&[
            ("type", kind.as_str()),
            ("name", name),
            ("per_page", "100"),
        ]);
        Ok(self.call::<Vec<ApiRecord>>(request).await?.unwrap_or_default())
    }

    async fn remove(&self, id: &str) -> Result<(), DomainsError> {
        let request = self.http.delete(format!("{}/{id}", self.records_url()));
        self.call::<Value>(request).await.map(drop)
    }
}

/// TXT contents are quoted by Cloudflare's API.
fn same_content(kind: RecordKind, stored: &str, value: &str) -> bool {
    match kind {
        RecordKind::Txt => stored.trim_matches('"') == value.trim_matches('"'),
        RecordKind::Cname => normal_name(stored) == normal_name(value),
        _ => stored == value,
    }
}

impl DnsProvider for CloudflareDns {
    fn set<'a>(
        &'a self,
        kind: RecordKind,
        name: &'a str,
        values: &'a [String],
    ) -> BoxFuture<'a, Result<(), DomainsError>> {
        Box::pin(async move {
            let existing = self.list(kind, name).await?;
            for record in &existing {
                if !values
                    .iter()
                    .any(|value| same_content(kind, &record.content, value))
                {
                    self.remove(&record.id).await?;
                }
            }
            for value in values {
                if existing
                    .iter()
                    .any(|record| same_content(kind, &record.content, value))
                {
                    continue;
                }
                let mut body = json!({
                    "type": kind.as_str(),
                    "name": name,
                    "content": value,
                    "ttl": self.ttl,
                });
                if kind != RecordKind::Txt {
                    body["proxied"] = json!(self.proxied);
                }
                let request = self.http.post(self.records_url()).json(&body);
                self.call::<Value>(request).await?;
            }
            Ok(())
        })
    }

    fn delete<'a>(&'a self, kind: RecordKind, name: &'a str) -> BoxFuture<'a, Result<(), DomainsError>> {
        Box::pin(async move {
            for record in self.list(kind, name).await? {
                self.remove(&record.id).await?;
            }
            Ok(())
        })
    }
}

/// A DNS zone in memory for tests: the records the module set, and
/// failures on demand. Clones share the zone.
///
/// ```
/// use navree_domains::{DnsProvider, DnsProviderFake, RecordKind};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let zone = DnsProviderFake::new();
/// zone.set(RecordKind::Cname, "acme.links.example.com", &["domains.example.com".to_owned()])
///     .await
///     .unwrap();
/// assert_eq!(zone.records(RecordKind::Cname, "acme.links.example.com"), ["domains.example.com"]);
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct DnsProviderFake {
    state: Arc<Mutex<FakeZone>>,
}

#[derive(Debug, Default)]
struct FakeZone {
    records: BTreeMap<(RecordKind, String), Vec<String>>,
    fail_next: Option<String>,
}

impl DnsProviderFake {
    /// An empty zone.
    pub fn new() -> DnsProviderFake {
        DnsProviderFake::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut FakeZone) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The records of type `kind` at `name`.
    pub fn records(&self, kind: RecordKind, name: &str) -> Vec<String> {
        self.with(|zone| {
            zone.records
                .get(&(kind, normal_name(name)))
                .cloned()
                .unwrap_or_default()
        })
    }

    /// Every record: `(type, name, value)`.
    pub fn all(&self) -> Vec<(RecordKind, String, String)> {
        self.with(|zone| {
            zone.records
                .iter()
                .flat_map(|((kind, name), values)| {
                    values
                        .iter()
                        .map(|value| (*kind, name.clone(), value.clone()))
                })
                .collect()
        })
    }

    /// The next change fails with `message`.
    pub fn fail_next(&self, message: &str) {
        self.with(|zone| zone.fail_next = Some(message.to_owned()));
    }

    fn take_failure(&self) -> Result<(), DomainsError> {
        match self.with(|zone| zone.fail_next.take()) {
            Some(message) => Err(DomainsError::DnsProvider(message)),
            None => Ok(()),
        }
    }
}

impl DnsProvider for DnsProviderFake {
    fn set<'a>(
        &'a self,
        kind: RecordKind,
        name: &'a str,
        values: &'a [String],
    ) -> BoxFuture<'a, Result<(), DomainsError>> {
        let result = self.take_failure().map(|()| {
            self.with(|zone| {
                zone.records
                    .insert((kind, normal_name(name)), values.to_vec());
            });
        });
        Box::pin(async move { result })
    }

    fn delete<'a>(&'a self, kind: RecordKind, name: &'a str) -> BoxFuture<'a, Result<(), DomainsError>> {
        let result = self.take_failure().map(|()| {
            self.with(|zone| {
                zone.records.remove(&(kind, normal_name(name)));
            });
        });
        Box::pin(async move { result })
    }
}

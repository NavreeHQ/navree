//! TXT lookups for domain verification: [`TxtResolver`], with
//! [`DohResolver`] (DNS over HTTPS, JSON API) and [`StaticTxtResolver`]
//! for tests.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde::Deserialize;

use crate::SsoError;

/// Reads TXT records. The module registers a [`DohResolver`]; replace it
/// with `override_value::<dyn TxtResolver>(..)`, for example with a
/// [`StaticTxtResolver`] in tests.
pub trait TxtResolver: Send + Sync + 'static {
    /// The TXT records of `name` (each record's strings joined); empty when
    /// the name has none or does not exist.
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, SsoError>>;
}

/// TXT lookups over DNS over HTTPS (the JSON API of Cloudflare's and
/// Google's resolvers: `GET {url}?name=..&type=TXT` with
/// `accept: application/dns-json`), through the app's HTTP client.
pub struct DohResolver {
    http: HttpClient,
    url: String,
}

impl DohResolver {
    /// A resolver asking `url` (`https://cloudflare-dns.com/dns-query`).
    pub fn new(http: HttpClient, url: impl Into<String>) -> DohResolver {
        DohResolver {
            http,
            url: url.into(),
        }
    }
}

#[derive(Deserialize)]
struct DohAnswer {
    #[serde(rename = "Status")]
    status: i64,
    #[serde(rename = "Answer", default)]
    answer: Vec<DohRecord>,
}

#[derive(Deserialize)]
struct DohRecord {
    #[serde(rename = "type")]
    kind: u16,
    data: String,
}

/// The type number of TXT records.
const TXT: u16 = 16;
/// NXDOMAIN.
const NAME_ERROR: i64 = 3;

impl TxtResolver for DohResolver {
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, SsoError>> {
        Box::pin(async move {
            let unavailable = |message: String| SsoError::ProviderUnavailable(message);
            let response = self
                .http
                .get(&self.url)
                .query(&[("name", name), ("type", "TXT")])
                .header("accept", "application/dns-json")
                .send()
                .await
                .map_err(|error| unavailable(format!("DNS lookup failed: {error}")))?;
            if !response.is_success() {
                return Err(unavailable(format!(
                    "the DNS resolver answered {}",
                    response.status().as_u16()
                )));
            }
            let answer: DohAnswer = response
                .json()
                .map_err(|_| unavailable("the DNS resolver's answer is not valid".to_owned()))?;
            match answer.status {
                0 => Ok(answer
                    .answer
                    .iter()
                    .filter(|record| record.kind == TXT)
                    .map(|record| txt_strings(&record.data))
                    .collect()),
                NAME_ERROR => Ok(Vec::new()),
                status => Err(unavailable(format!(
                    "DNS lookup failed with status {status}"
                ))),
            }
        })
    }
}

/// A TXT record's presentation form (`"part one" "part two"`, with `\"`
/// and `\\` escapes) as one string; unquoted data is taken as it is.
pub(crate) fn txt_strings(data: &str) -> String {
    let data = data.trim();
    if !data.starts_with('"') {
        return data.to_owned();
    }
    let mut out = String::new();
    let mut chars = data.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', _) => quoted = !quoted,
            ('\\', true) => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            (c, true) => out.push(c),
            (_, false) => {}
        }
    }
    out
}

/// TXT records kept in memory, for tests:
/// `StaticTxtResolver::new().record("acme.test", "navree-sso-verification=..")`.
#[derive(Debug, Default)]
pub struct StaticTxtResolver {
    records: Mutex<BTreeMap<String, Vec<String>>>,
}

impl StaticTxtResolver {
    /// A resolver without records.
    pub fn new() -> StaticTxtResolver {
        StaticTxtResolver::default()
    }

    /// Adds a TXT record to `name`.
    #[must_use]
    pub fn record(self, name: &str, value: &str) -> StaticTxtResolver {
        self.add(name, value);
        self
    }

    /// Adds a TXT record to `name` (on a shared resolver).
    pub fn add(&self, name: &str, value: &str) {
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(name.trim_end_matches('.').to_ascii_lowercase())
            .or_default()
            .push(value.to_owned());
    }

    /// Removes every record of `name`.
    pub fn clear(&self, name: &str) {
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&name.trim_end_matches('.').to_ascii_lowercase());
    }
}

impl TxtResolver for StaticTxtResolver {
    fn txt<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<String>, SsoError>> {
        let records = self
            .records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&name.trim_end_matches('.').to_ascii_lowercase())
            .cloned()
            .unwrap_or_default();
        Box::pin(async move { Ok(records) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_strings_are_joined_and_unescaped() {
        assert_eq!(txt_strings("\"v=1\""), "v=1");
        assert_eq!(
            txt_strings("\"part one \" \"part two\""),
            "part one part two"
        );
        assert_eq!(
            txt_strings(r#""a \"quoted\" \\ word""#),
            r#"a "quoted" \ word"#
        );
        assert_eq!(txt_strings("plain"), "plain");
    }

    #[tokio::test]
    async fn the_static_resolver_answers_by_lower_cased_name() {
        let resolver = StaticTxtResolver::new().record("Acme.test.", "a=1");
        resolver.add("acme.test", "b=2");
        assert_eq!(resolver.txt("acme.test").await.unwrap(), ["a=1", "b=2"]);
        resolver.clear("ACME.test");
        assert!(resolver.txt("acme.test").await.unwrap().is_empty());
    }
}

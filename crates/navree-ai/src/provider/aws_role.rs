//! Temporary AWS credentials from the platform, for Bedrock without
//! configured keys: the container credentials endpoint (ECS task roles,
//! EKS Pod Identity), else the EC2 instance metadata service (IMDSv2).
//! Credentials are cached until five minutes before they expire; a missing
//! metadata service is remembered for a minute, so requests off EC2 fail
//! fast.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use navree_http_client::{HttpClient, HttpClientError, HttpClientOptions, SsrfPolicy};
use serde::Deserialize;
use tokio::sync::Mutex;

use super::sigv4::Credentials;
use crate::error::ProviderError;

/// The ECS container credentials host, for a relative URI.
const CONTAINER_HOST: &str = "http://169.254.170.2";

/// The EC2 instance metadata service.
const IMDS_ENDPOINT: &str = "http://169.254.169.254";

/// How long an IMDS session token lives, in seconds.
const IMDS_TOKEN_TTL: &str = "21600";

/// Credentials are refreshed this long before they expire.
const REFRESH_BEFORE: chrono::TimeDelta = chrono::TimeDelta::minutes(5);

/// How long a missing metadata service is remembered.
const MISSING_FOR: Duration = Duration::from_secs(60);

/// The limit of each call to a credentials endpoint.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Where role credentials come from, read from the environment the way the
/// AWS SDKs read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RoleSource {
    /// `AWS_CONTAINER_CREDENTIALS_RELATIVE_URI` (on `169.254.170.2`) or
    /// `AWS_CONTAINER_CREDENTIALS_FULL_URI`, with an optional
    /// authorization token.
    Container {
        url: String,
        token: Option<ContainerToken>,
    },
    /// The instance metadata service at `endpoint`
    /// (`AWS_EC2_METADATA_SERVICE_ENDPOINT`, else `169.254.169.254`).
    Imds { endpoint: String },
    /// `AWS_EC2_METADATA_DISABLED=true` and no container endpoint.
    None,
}

/// The `Authorization` header of the container endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContainerToken {
    /// `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`, read at each fetch (EKS
    /// rotates it).
    File(PathBuf),
    /// `AWS_CONTAINER_AUTHORIZATION_TOKEN`.
    Value(String),
}

impl RoleSource {
    /// The source `env` (an environment variable reader) describes.
    pub(crate) fn from_env(env: impl Fn(&str) -> Option<String>) -> Self {
        let get = |name: &str| env(name).filter(|value| !value.trim().is_empty());
        let token = get("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE")
            .map(|path| ContainerToken::File(PathBuf::from(path)))
            .or_else(|| get("AWS_CONTAINER_AUTHORIZATION_TOKEN").map(ContainerToken::Value));
        if let Some(relative) = get("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI") {
            return RoleSource::Container {
                url: format!("{CONTAINER_HOST}{relative}"),
                token,
            };
        }
        if let Some(url) = get("AWS_CONTAINER_CREDENTIALS_FULL_URI") {
            return RoleSource::Container { url, token };
        }
        if get("AWS_EC2_METADATA_DISABLED").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")) {
            return RoleSource::None;
        }
        let endpoint = get("AWS_EC2_METADATA_SERVICE_ENDPOINT")
            .unwrap_or_else(|| IMDS_ENDPOINT.to_owned())
            .trim_end_matches('/')
            .to_owned();
        RoleSource::Imds { endpoint }
    }

    /// The host the source calls.
    fn host(&self) -> Option<String> {
        let url = match self {
            RoleSource::Container { url, .. } => url,
            RoleSource::Imds { endpoint } => endpoint,
            RoleSource::None => return None,
        };
        url::Url::parse(url).ok()?.host_str().map(|host| {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned()
        })
    }
}

/// A credentials document of the container endpoint or the metadata
/// service.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Document {
    access_key_id: String,
    secret_access_key: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    expiration: Option<String>,
}

/// What the cache holds.
#[derive(Default)]
struct State {
    credentials: Option<(Credentials, Option<DateTime<Utc>>)>,
    /// The metadata service did not answer; don't ask again before then.
    missing_until: Option<Instant>,
}

/// Role credentials, fetched when needed and cached.
pub(crate) struct RoleCredentials {
    source: RoleSource,
    http: Option<HttpClient>,
    state: Mutex<State>,
}

impl RoleCredentials {
    /// Credentials from `source`. The HTTP client lets through only the
    /// source's own host (the platform's link-local endpoints are private
    /// addresses).
    pub(crate) fn new(source: RoleSource) -> Self {
        let http = source.host().and_then(|host| {
            HttpClient::new(HttpClientOptions {
                timeout_secs: TIMEOUT.as_secs(),
                retries: 0,
                ssrf: SsrfPolicy {
                    block_private: true,
                    allow_hosts: vec![host],
                },
                user_agent: concat!("navree-ai/", env!("CARGO_PKG_VERSION")).to_owned(),
                ..HttpClientOptions::default()
            })
            .ok()
        });
        Self {
            source,
            http,
            state: Mutex::new(State::default()),
        }
    }

    /// Valid credentials at `now`: cached, or fetched. `Ok(None)` when the
    /// platform offers none (no container endpoint, no metadata service or
    /// no role on the instance).
    pub(crate) async fn get(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<Credentials>, ProviderError> {
        let Some(http) = &self.http else {
            return Ok(None);
        };
        let mut state = self.state.lock().await;
        if let Some((credentials, expires)) = &state.credentials
            && expires.is_none_or(|at| at - REFRESH_BEFORE > now)
        {
            return Ok(Some(credentials.clone()));
        }
        if state
            .missing_until
            .is_some_and(|until| Instant::now() < until)
        {
            return Ok(None);
        }
        let fetched = match &self.source {
            RoleSource::Container { url, token } => {
                container(http, url, token.as_ref()).await.map(Some)
            }
            RoleSource::Imds { endpoint } => imds(http, endpoint).await,
            RoleSource::None => Ok(None),
        };
        match fetched {
            Ok(Some(fresh)) => {
                state.missing_until = None;
                state.credentials = Some(fresh.clone());
                Ok(Some(fresh.0))
            }
            Ok(None) => {
                state.missing_until = Some(Instant::now() + MISSING_FOR);
                Ok(None)
            }
            // A failed refresh keeps credentials that have not expired yet.
            Err(error) => match &state.credentials {
                Some((credentials, Some(at))) if *at > now => Ok(Some(credentials.clone())),
                _ => Err(error),
            },
        }
    }
}

type Fetched = (Credentials, Option<DateTime<Utc>>);

/// The container endpoint: configured, so a failure is an error.
async fn container(
    http: &HttpClient,
    url: &str,
    token: Option<&ContainerToken>,
) -> Result<Fetched, ProviderError> {
    let mut request = http.get(url).timeout(TIMEOUT);
    match token {
        Some(ContainerToken::Value(value)) => {
            request = request.header("authorization", value.trim())
        }
        Some(ContainerToken::File(path)) => {
            let value =
                tokio::fs::read_to_string(path)
                    .await
                    .map_err(|error| ProviderError::Auth {
                        message: format!(
                            "cannot read AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE ({}): {error}",
                            path.display()
                        ),
                    })?;
            request = request.header("authorization", value.trim());
        }
        None => {}
    }
    let response = request
        .send()
        .await
        .map_err(|error| unavailable("the container credentials endpoint", &error))?;
    if !response.is_success() {
        return Err(ProviderError::unavailable(format!(
            "the container credentials endpoint answered {}",
            response.status()
        )));
    }
    document(&response.text())
}

/// IMDSv2: a session token, the instance's role, then its credentials. No
/// answer, or no role, is `None`.
async fn imds(http: &HttpClient, endpoint: &str) -> Result<Option<Fetched>, ProviderError> {
    let token = match http
        .put(format!("{endpoint}/latest/api/token"))
        .header("x-aws-ec2-metadata-token-ttl-seconds", IMDS_TOKEN_TTL)
        .timeout(TIMEOUT)
        .send()
        .await
    {
        Ok(response) if response.is_success() => response.text(),
        Ok(_) | Err(_) => return Ok(None),
    };
    let base = format!("{endpoint}/latest/meta-data/iam/security-credentials/");
    let roles = match http
        .get(&base)
        .header("x-aws-ec2-metadata-token", token.trim())
        .timeout(TIMEOUT)
        .send()
        .await
    {
        Ok(response) if response.is_success() => response.text(),
        Ok(_) => return Ok(None),
        Err(error) => return Err(unavailable("the instance metadata service", &error)),
    };
    let Some(role) = roles.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return Ok(None);
    };
    let response = http
        .get(format!("{base}{role}"))
        .header("x-aws-ec2-metadata-token", token.trim())
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|error| unavailable("the instance metadata service", &error))?;
    if !response.is_success() {
        return Err(ProviderError::unavailable(format!(
            "the instance metadata service answered {} for the role's credentials",
            response.status()
        )));
    }
    document(&response.text()).map(Some)
}

fn unavailable(what: &str, error: &HttpClientError) -> ProviderError {
    ProviderError::unavailable(format!("{what} failed: {error}"))
}

/// Credentials and their expiry from a credentials document.
fn document(body: &str) -> Result<Fetched, ProviderError> {
    let document: Document = serde_json::from_str(body).map_err(|error| {
        ProviderError::unavailable(format!("an unreadable AWS credentials document: {error}"))
    })?;
    let expires = document
        .expiration
        .as_deref()
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Utc));
    Ok((
        Credentials {
            access_key_id: document.access_key_id,
            secret_access_key: document.secret_access_key,
            session_token: document.token.filter(|token| !token.is_empty()),
        },
        expires,
    ))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 27, 12, 0, 0).unwrap()
    }

    fn document_json(key: &str, expiration: &str) -> serde_json::Value {
        json!({
            "Code": "Success",
            "Type": "AWS-HMAC",
            "AccessKeyId": key,
            "SecretAccessKey": "secret",
            "Token": "session",
            "Expiration": expiration,
        })
    }

    #[test]
    fn sources_follow_the_sdk_order() {
        assert_eq!(
            RoleSource::from_env(env(&[(
                "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
                "/v2/credentials/abc"
            )])),
            RoleSource::Container {
                url: "http://169.254.170.2/v2/credentials/abc".into(),
                token: None
            }
        );
        assert_eq!(
            RoleSource::from_env(env(&[
                (
                    "AWS_CONTAINER_CREDENTIALS_FULL_URI",
                    "http://169.254.170.23/v1/credentials"
                ),
                ("AWS_CONTAINER_AUTHORIZATION_TOKEN", "t"),
                ("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE", "/var/run/token"),
            ])),
            RoleSource::Container {
                url: "http://169.254.170.23/v1/credentials".into(),
                token: Some(ContainerToken::File("/var/run/token".into()))
            }
        );
        assert_eq!(
            RoleSource::from_env(env(&[])),
            RoleSource::Imds {
                endpoint: "http://169.254.169.254".into()
            }
        );
        assert_eq!(
            RoleSource::from_env(env(&[(
                "AWS_EC2_METADATA_SERVICE_ENDPOINT",
                "http://[fd00:ec2::254]/"
            )])),
            RoleSource::Imds {
                endpoint: "http://[fd00:ec2::254]".into()
            }
        );
        assert_eq!(
            RoleSource::from_env(env(&[("AWS_EC2_METADATA_DISABLED", "TRUE")])),
            RoleSource::None
        );
    }

    #[tokio::test]
    async fn container_credentials_are_fetched_with_the_token_and_cached() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/credentials"))
            .and(header("authorization", "pod-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(document_json("ASIA1", "2026-10-27T18:00:00Z")),
            )
            .expect(1)
            .mount(&server)
            .await;
        let role = RoleCredentials::new(RoleSource::Container {
            url: format!("{}/v1/credentials", server.uri()),
            token: Some(ContainerToken::Value("pod-token".into())),
        });

        let first = role.get(now()).await.unwrap().unwrap();
        let second = role.get(now()).await.unwrap().unwrap();

        assert_eq!(first.access_key_id, "ASIA1");
        assert_eq!(first.session_token.as_deref(), Some("session"));
        assert_eq!(first, second, "the second call is served from the cache");
    }

    #[tokio::test]
    async fn credentials_are_refreshed_before_they_expire() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(document_json("ASIA1", "2026-10-27T12:04:00Z")),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(document_json("ASIA2", "2026-10-27T18:00:00Z")),
            )
            .mount(&server)
            .await;
        let role = RoleCredentials::new(RoleSource::Container {
            url: format!("{}/creds", server.uri()),
            token: None,
        });

        assert_eq!(
            role.get(now()).await.unwrap().unwrap().access_key_id,
            "ASIA1"
        );
        // Four minutes left: inside the five-minute margin, so refreshed.
        assert_eq!(
            role.get(now()).await.unwrap().unwrap().access_key_id,
            "ASIA2"
        );
    }

    #[tokio::test]
    async fn a_failed_refresh_keeps_credentials_that_have_not_expired() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(document_json("ASIA1", "2026-10-27T12:04:00Z")),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let role = RoleCredentials::new(RoleSource::Container {
            url: format!("{}/creds", server.uri()),
            token: None,
        });

        assert_eq!(
            role.get(now()).await.unwrap().unwrap().access_key_id,
            "ASIA1"
        );
        assert_eq!(
            role.get(now()).await.unwrap().unwrap().access_key_id,
            "ASIA1"
        );
        let later = Utc.with_ymd_and_hms(2026, 10, 27, 12, 5, 0).unwrap();
        assert!(matches!(
            role.get(later).await,
            Err(ProviderError::Unavailable { .. })
        ));
    }

    #[tokio::test]
    async fn the_instance_role_is_read_with_an_imdsv2_session() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/latest/api/token"))
            .and(header("x-aws-ec2-metadata-token-ttl-seconds", "21600"))
            .respond_with(ResponseTemplate::new(200).set_body_string("imds-token"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/latest/meta-data/iam/security-credentials/"))
            .and(header("x-aws-ec2-metadata-token", "imds-token"))
            .respond_with(ResponseTemplate::new(200).set_body_string("app-role\n"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/latest/meta-data/iam/security-credentials/app-role"))
            .and(header("x-aws-ec2-metadata-token", "imds-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(document_json("ASIAEC2", "2026-10-27T18:00:00Z")),
            )
            .mount(&server)
            .await;
        let role = RoleCredentials::new(RoleSource::Imds {
            endpoint: server.uri(),
        });

        let credentials = role.get(now()).await.unwrap().unwrap();

        assert_eq!(credentials.access_key_id, "ASIAEC2");
        assert_eq!(credentials.secret_access_key, "secret");
    }

    #[tokio::test]
    async fn no_metadata_service_or_no_role_is_none_and_remembered() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_string("imds-token"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let role = RoleCredentials::new(RoleSource::Imds {
            endpoint: server.uri(),
        });

        assert_eq!(role.get(now()).await.unwrap(), None);
        assert_eq!(
            role.get(now()).await.unwrap(),
            None,
            "not asked again within a minute"
        );

        // Nothing listening: none, not an error.
        let closed = RoleCredentials::new(RoleSource::Imds {
            endpoint: "http://127.0.0.1:9".into(),
        });
        assert_eq!(closed.get(now()).await.unwrap(), None);
        assert_eq!(
            RoleCredentials::new(RoleSource::None)
                .get(now())
                .await
                .unwrap(),
            None
        );
    }
}

//! [`IntegrationsFake`]: provider token endpoints and APIs in memory.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_http_client::{FakeServer, HttpClient};
use navree_kernel::BoxFuture;

use crate::entry::ProviderEntry;
use crate::oauth::{Exchange, Grant, TokenBackend, service_account_assertion};
use crate::IntegrationsError;

/// What a fake token endpoint answers: build one with
/// [`FakeGrant::new`] and the setters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FakeGrant {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    no_expiry: bool,
    scopes: Option<Vec<String>>,
    external_id: Option<String>,
    display_name: Option<String>,
}

impl FakeGrant {
    /// A grant with a generated access token that lasts an hour.
    pub fn new() -> Self {
        Self::default()
    }

    /// The access token (default: `fake-<provider>-<n>`).
    pub fn access_token(mut self, token: impl Into<String>) -> Self {
        self.access_token = Some(token.into());
        self
    }

    /// A refresh token.
    pub fn refresh_token(mut self, token: impl Into<String>) -> Self {
        self.refresh_token = Some(token.into());
        self
    }

    /// The lifetime in seconds (default 3600).
    pub fn expires_in(mut self, secs: i64) -> Self {
        self.expires_in = Some(secs);
        self.no_expiry = false;
        self
    }

    /// A token that does not expire (a Slack bot token without rotation).
    pub fn never_expires(mut self) -> Self {
        self.no_expiry = true;
        self
    }

    /// The scopes the provider reports as granted.
    pub fn scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes = Some(scopes.into_iter().map(Into::into).collect());
        self
    }

    /// The tenant at the provider (a Slack team id).
    pub fn external_id(mut self, id: impl Into<String>) -> Self {
        self.external_id = Some(id.into());
        self
    }

    /// The tenant's name.
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    fn into_grant(self, provider: &str, counter: &AtomicU64) -> Grant {
        let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
        Grant {
            access_token: self
                .access_token
                .unwrap_or_else(|| format!("fake-{provider}-{n}")),
            refresh_token: self.refresh_token,
            expires_in: if self.no_expiry {
                None
            } else {
                Some(self.expires_in.unwrap_or(3600))
            },
            scopes: self.scopes,
            external_id: self.external_id,
            display_name: self.display_name,
        }
    }
}

/// One token request the fake answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Minted {
    /// The provider key.
    pub provider: String,
    /// `authorization_code`, `refresh_token`, `client_credentials` or
    /// `jwt_bearer`.
    pub grant_type: String,
    /// The scope set whose app registration asked.
    pub scope_set: String,
    /// The user a service account acted as, or the Microsoft tenant.
    pub subject: Option<String>,
    /// The client id the grant authenticated with (`None` for service
    /// accounts).
    pub client_id: Option<String>,
}

/// How a provider's token endpoint answers grants other than codes.
#[derive(Clone, Debug)]
enum TokenMode {
    Grant(FakeGrant),
    Refuse(String),
    Fail,
}

/// The fake's shared state (also the fake [`TokenBackend`]).
pub(crate) struct FakeState {
    pub(crate) http: HttpClient,
    server: FakeServer,
    codes: Mutex<HashMap<(String, String), Result<FakeGrant, String>>>,
    tokens: Mutex<HashMap<String, TokenMode>>,
    minted: Mutex<Vec<Minted>>,
    exchanged: Mutex<Vec<(String, String)>>,
    revoked: Mutex<Vec<(String, String)>>,
    counter: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Decides what providers answer in tests, without the network: codes from
/// a consent screen, token grants and refusals, and API responses (through
/// a fake HTTP client, [`api`](IntegrationsFake::api)). Get one from
/// [`Integrations::fake`](crate::Integrations::fake).
///
/// States are still sealed and checked, credentials still encrypted and
/// stored, service-account keys still sign their assertions: only the
/// provider's side is replaced. A code the fake doesn't know is refused
/// with `invalid_grant`, as a provider would; other grants succeed with a
/// generated token that lasts an hour until you say otherwise.
#[derive(Clone)]
pub struct IntegrationsFake {
    state: Arc<FakeState>,
}

impl IntegrationsFake {
    pub(crate) fn new() -> (Self, Arc<FakeState>) {
        let (http, server) = HttpClient::fake();
        let state = Arc::new(FakeState {
            http,
            server,
            codes: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
            minted: Mutex::new(Vec::new()),
            exchanged: Mutex::new(Vec::new()),
            revoked: Mutex::new(Vec::new()),
            counter: AtomicU64::new(0),
        });
        (
            Self {
                state: state.clone(),
            },
            state,
        )
    }

    /// Answers the consent code `code` of `provider` with `grant`.
    pub fn on_code(&self, provider: &str, code: &str, grant: FakeGrant) -> &Self {
        lock(&self.state.codes).insert((provider.to_owned(), code.to_owned()), Ok(grant));
        self
    }

    /// Refuses the consent code `code` of `provider` with an OAuth error
    /// (`invalid_grant`): a 400 `integration_refused`.
    pub fn on_code_error(&self, provider: &str, code: &str, error: &str) -> &Self {
        lock(&self.state.codes).insert(
            (provider.to_owned(), code.to_owned()),
            Err(error.to_owned()),
        );
        self
    }

    /// Answers every other grant of `provider` (refreshes, app-only and
    /// service-account tokens) with `grant`.
    pub fn on_token(&self, provider: &str, grant: FakeGrant) -> &Self {
        lock(&self.state.tokens).insert(provider.to_owned(), TokenMode::Grant(grant));
        self
    }

    /// Refuses every other grant of `provider` with `error`
    /// (`invalid_grant`, `unauthorized_client`): the connection turns
    /// unhealthy and needs connecting again.
    pub fn refuse_tokens(&self, provider: &str, error: &str) -> &Self {
        lock(&self.state.tokens).insert(provider.to_owned(), TokenMode::Refuse(error.to_owned()));
        self
    }

    /// Makes `provider`'s token endpoint unreachable (a transient failure).
    pub fn fail_tokens(&self, provider: &str) -> &Self {
        lock(&self.state.tokens).insert(provider.to_owned(), TokenMode::Fail);
        self
    }

    /// Back to generated tokens for `provider`.
    pub fn restore_tokens(&self, provider: &str) -> &Self {
        lock(&self.state.tokens).remove(provider);
        self
    }

    /// The fake HTTP server behind every typed client and health probe:
    /// stub API answers with `api().on(Method::GET, "https://graph.microsoft.com/v1.0/users*", ..)`
    /// and assert on what was sent.
    pub fn api(&self) -> &FakeServer {
        &self.state.server
    }

    /// Every grant the fake answered (codes excepted), in order.
    pub fn minted(&self) -> Vec<Minted> {
        lock(&self.state.minted).clone()
    }

    /// The `(provider, code)` pairs exchanged so far.
    pub fn exchanged(&self) -> Vec<(String, String)> {
        lock(&self.state.exchanged).clone()
    }

    /// The `(provider, token)` pairs revoked on disconnect.
    pub fn revoked(&self) -> Vec<(String, String)> {
        lock(&self.state.revoked).clone()
    }
}

impl std::fmt::Debug for IntegrationsFake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntegrationsFake").finish_non_exhaustive()
    }
}

impl TokenBackend for FakeState {
    fn grant<'a>(
        &'a self,
        entry: &'a ProviderEntry,
        scope_set: &'a str,
        exchange: Exchange<'a>,
    ) -> BoxFuture<'a, Result<Grant, IntegrationsError>> {
        Box::pin(async move {
            let provider = entry.key.clone();
            let refused = |error: String| IntegrationsError::Refused {
                provider: entry.name.clone(),
                error,
                description: None,
            };
            if let Exchange::Code { code, .. } = &exchange {
                lock(&self.exchanged).push((provider.clone(), (*code).to_owned()));
                let answer = lock(&self.codes)
                    .get(&(provider.clone(), (*code).to_owned()))
                    .cloned();
                return match answer {
                    Some(Ok(grant)) => Ok(grant.into_grant(&provider, &self.counter)),
                    Some(Err(error)) => Err(refused(error)),
                    None => Err(refused("invalid_grant".to_owned())),
                };
            }
            // Service-account keys sign their assertion, as for real.
            let (subject, mut external_id, mut display_name) = match &exchange {
                Exchange::JwtBearer {
                    key,
                    subject,
                    scopes,
                    now,
                } => {
                    service_account_assertion(
                        &entry.token_endpoint(None),
                        key,
                        *subject,
                        scopes,
                        *now,
                    )?;
                    let domain = subject
                        .unwrap_or(key.client_email.as_str())
                        .rsplit_once('@')
                        .map(|(_, domain)| domain.to_owned());
                    (
                        subject.map(str::to_owned),
                        domain,
                        Some(key.client_email.clone()),
                    )
                }
                Exchange::ClientCredentials { tenant } => {
                    (Some((*tenant).to_owned()), Some((*tenant).to_owned()), None)
                }
                _ => (None, None, None),
            };
            // Every other grant authenticates the client, as for real.
            let client_id = if matches!(exchange, Exchange::JwtBearer { .. }) {
                None
            } else {
                Some(entry.client(scope_set)?.0)
            };
            lock(&self.minted).push(Minted {
                provider: provider.clone(),
                grant_type: exchange.grant_type().to_owned(),
                scope_set: scope_set.to_owned(),
                subject,
                client_id,
            });
            let mode = lock(&self.tokens).get(&provider).cloned();
            match mode {
                Some(TokenMode::Refuse(error)) => Err(refused(error)),
                Some(TokenMode::Fail) => Err(IntegrationsError::Unavailable {
                    provider: entry.name.clone(),
                    message: "connection refused (fake)".to_owned(),
                }),
                Some(TokenMode::Grant(grant)) => {
                    let mut grant = grant.into_grant(&provider, &self.counter);
                    grant.external_id = grant.external_id.or(external_id.take());
                    grant.display_name = grant.display_name.or(display_name.take());
                    Ok(grant)
                }
                None => {
                    let mut grant = FakeGrant::new().into_grant(&provider, &self.counter);
                    grant.external_id = external_id;
                    grant.display_name = display_name;
                    Ok(grant)
                }
            }
        })
    }

    fn revoke<'a>(
        &'a self,
        entry: &'a ProviderEntry,
        _scope_set: &'a str,
        token: &'a str,
    ) -> BoxFuture<'a, Result<(), IntegrationsError>> {
        Box::pin(async move {
            if entry.revoke_endpoint().is_some() {
                lock(&self.revoked).push((entry.key.clone(), token.to_owned()));
            }
            Ok(())
        })
    }
}

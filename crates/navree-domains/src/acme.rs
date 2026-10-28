//! Certificates from an ACME CA: [`AcmeClient`], with [`InstantAcme`]
//! (instant-acme over reqwest, HTTP-01 challenges) and [`AcmeFake`] for
//! tests.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use chrono::{DateTime, Datelike, TimeDelta, Utc};
use http_body_util::{BodyExt, Full};
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, BodyWrapper, BytesResponse, ChallengeType,
    HttpClient, Identifier, NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use navree_kernel::BoxFuture;

use crate::DomainsError;

/// What the module gives an [`AcmeClient`] for one certificate: the host,
/// the directory and contacts from `domains.acme`, the stored account,
/// and where to publish HTTP-01 challenge answers (the app serves them at
/// `/.well-known/acme-challenge/{token}` from the database, so any of
/// your servers can answer).
pub struct AcmeOrder {
    pub(crate) host: String,
    pub(crate) directory_url: String,
    pub(crate) contact: Vec<String>,
    pub(crate) store: Arc<dyn AcmeStore>,
}

impl fmt::Debug for AcmeOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcmeOrder")
            .field("host", &self.host)
            .field("directory_url", &self.directory_url)
            .finish_non_exhaustive()
    }
}

impl AcmeOrder {
    /// The host the certificate is for.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The CA's ACME directory (`domains.acme.directory_url`).
    pub fn directory_url(&self) -> &str {
        &self.directory_url
    }

    /// The account's contact addresses, as `mailto:` URIs.
    pub fn contact(&self) -> Vec<String> {
        self.contact
            .iter()
            .map(|contact| {
                let contact = contact.trim();
                if contact.starts_with("mailto:") {
                    contact.to_owned()
                } else {
                    format!("mailto:{contact}")
                }
            })
            .collect()
    }

    /// The stored account credentials for the directory (JSON, as the
    /// client saved them), if any.
    pub async fn account(&self) -> Result<Option<String>, DomainsError> {
        self.store.account(&self.directory_url).await
    }

    /// Stores the account credentials for the directory (sealed with the
    /// application key).
    pub async fn save_account(&self, credentials: &str) -> Result<(), DomainsError> {
        self.store
            .save_account(&self.directory_url, credentials)
            .await
    }

    /// Publishes the answer to an HTTP-01 challenge: the app serves
    /// `key_authorization` at `/.well-known/acme-challenge/{token}`.
    pub async fn publish_challenge(
        &self,
        token: &str,
        key_authorization: &str,
    ) -> Result<(), DomainsError> {
        self.store
            .publish(&self.host, token, key_authorization)
            .await
    }

    /// Stops serving the answer to challenge `token`.
    pub async fn withdraw_challenge(&self, token: &str) -> Result<(), DomainsError> {
        self.store.withdraw(token).await
    }
}

/// Where an [`AcmeOrder`] keeps accounts and challenge answers (the
/// module's tables).
pub(crate) trait AcmeStore: Send + Sync + 'static {
    fn account<'a>(
        &'a self,
        directory: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, DomainsError>>;
    fn save_account<'a>(
        &'a self,
        directory: &'a str,
        credentials: &'a str,
    ) -> BoxFuture<'a, Result<(), DomainsError>>;
    fn publish<'a>(
        &'a self,
        host: &'a str,
        token: &'a str,
        key_authorization: &'a str,
    ) -> BoxFuture<'a, Result<(), DomainsError>>;
    fn withdraw<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<(), DomainsError>>;
}

/// A certificate an [`AcmeClient`] obtained.
#[derive(Clone, PartialEq, Eq)]
pub struct IssuedCertificate {
    /// The certificate chain, PEM, leaf first.
    pub chain_pem: String,
    /// The private key, PEM (PKCS #8).
    pub private_key_pem: String,
}

impl fmt::Debug for IssuedCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedCertificate")
            .field("chain_pem", &self.chain_pem)
            .finish_non_exhaustive()
    }
}

/// Obtains certificates from an ACME CA. The module registers an
/// [`InstantAcme`]; replace it with `override_value::<dyn AcmeClient>(..)`,
/// for example with an [`AcmeFake`] in tests.
pub trait AcmeClient: Send + Sync + 'static {
    /// A certificate for `order.host()`.
    fn issue<'a>(
        &'a self,
        order: &'a AcmeOrder,
    ) -> BoxFuture<'a, Result<IssuedCertificate, DomainsError>>;
}

// -- instant-acme --------------------------------------------------------------

/// [instant-acme](https://docs.rs/instant-acme) on ring, its requests sent
/// through reqwest with rustls: an account per directory (created on the
/// first order and stored), one order per host, HTTP-01 challenges, a key
/// and signing request made by instant-acme with rcgen.
#[derive(Clone)]
pub struct InstantAcme {
    http: AcmeHttp,
    retry: RetryPolicy,
}

impl fmt::Debug for InstantAcme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InstantAcme")
    }
}

impl InstantAcme {
    /// A client trusting the system's root certificates.
    pub fn new() -> Result<InstantAcme, DomainsError> {
        InstantAcme::with_roots(Vec::new())
    }

    /// A client also trusting the root certificates in `pem` (a private
    /// CA, or a test CA such as Pebble's).
    pub fn with_root_pem(pem: &[u8]) -> Result<InstantAcme, DomainsError> {
        let roots = reqwest::Certificate::from_pem_bundle(pem)
            .map_err(|error| DomainsError::Config(format!("the ACME root certificate: {error}")))?;
        if roots.is_empty() {
            return Err(DomainsError::Config(
                "the ACME root certificate file holds no certificate".to_owned(),
            ));
        }
        InstantAcme::with_roots(roots)
    }

    fn with_roots(roots: Vec<reqwest::Certificate>) -> Result<InstantAcme, DomainsError> {
        // reqwest is built without a default crypto provider: use ring.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .no_proxy();
        if !roots.is_empty() {
            builder = builder.tls_certs_merge(roots);
        }
        let client = builder.build().map_err(|error| {
            DomainsError::Config(format!("cannot build the HTTP client: {error}"))
        })?;
        Ok(InstantAcme {
            http: AcmeHttp(client),
            retry: RetryPolicy::new()
                .initial_delay(Duration::from_millis(500))
                .timeout(Duration::from_secs(120)),
        })
    }

    /// How long to wait for the CA to validate and issue (default two
    /// minutes).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> InstantAcme {
        self.retry = self.retry.timeout(timeout);
        self
    }

    async fn account(&self, order: &AcmeOrder) -> Result<Account, DomainsError> {
        let failed = |error: instant_acme::Error| {
            DomainsError::acme(&order.host, format!("the ACME account: {error}"))
        };
        if let Some(stored) = order.account().await? {
            let credentials: AccountCredentials =
                serde_json::from_str(&stored).map_err(|error| {
                    DomainsError::Config(format!(
                        "the stored ACME account is not readable: {error}"
                    ))
                })?;
            return Account::builder_with_http(Box::new(self.http.clone()))
                .from_credentials(credentials)
                .await
                .map_err(failed);
        }
        let contact = order.contact();
        let contact: Vec<&str> = contact.iter().map(String::as_str).collect();
        let (account, credentials) = Account::builder_with_http(Box::new(self.http.clone()))
            .create(
                &NewAccount {
                    contact: &contact,
                    terms_of_service_agreed: true,
                    only_return_existing: false,
                },
                order.directory_url.clone(),
                None,
            )
            .await
            .map_err(failed)?;
        let json = serde_json::to_string(&credentials)
            .map_err(|error| DomainsError::Config(error.to_string()))?;
        order.save_account(&json).await?;
        Ok(account)
    }

    async fn run(
        &self,
        order: &AcmeOrder,
        published: &mut Vec<String>,
    ) -> Result<IssuedCertificate, DomainsError> {
        let host = order.host.clone();
        let failed = |error: instant_acme::Error| DomainsError::acme(&host, error.to_string());
        let account = self.account(order).await?;
        let identifiers = [Identifier::Dns(order.host.clone())];
        let mut acme = account
            .new_order(&NewOrder::new(&identifiers))
            .await
            .map_err(failed)?;
        {
            let mut authorizations = acme.authorizations();
            while let Some(result) = authorizations.next().await {
                let mut authorization = result.map_err(failed)?;
                match authorization.status {
                    AuthorizationStatus::Pending => {}
                    AuthorizationStatus::Valid => continue,
                    status => {
                        return Err(DomainsError::acme(
                            &order.host,
                            format!("the authorization is {status:?}"),
                        ));
                    }
                }
                let mut challenge =
                    authorization
                        .challenge(ChallengeType::Http01)
                        .ok_or_else(|| {
                            DomainsError::acme(&order.host, "the CA offers no HTTP-01 challenge")
                        })?;
                let token = challenge.token.clone();
                let answer = challenge.key_authorization();
                order.publish_challenge(&token, answer.as_str()).await?;
                published.push(token);
                challenge.set_ready().await.map_err(failed)?;
            }
        }
        let status = acme.poll_ready(&self.retry).await.map_err(failed)?;
        if status != OrderStatus::Ready {
            let reason = acme
                .state()
                .error
                .as_ref()
                .and_then(|problem| problem.detail.clone())
                .unwrap_or_else(|| format!("the order is {status:?}"));
            return Err(DomainsError::acme(&order.host, reason));
        }
        let private_key_pem = acme.finalize().await.map_err(failed)?;
        let chain_pem = acme.poll_certificate(&self.retry).await.map_err(failed)?;
        Ok(IssuedCertificate {
            chain_pem,
            private_key_pem,
        })
    }
}

impl AcmeClient for InstantAcme {
    fn issue<'a>(
        &'a self,
        order: &'a AcmeOrder,
    ) -> BoxFuture<'a, Result<IssuedCertificate, DomainsError>> {
        Box::pin(async move {
            let mut published = Vec::new();
            let result = self.run(order, &mut published).await;
            for token in published {
                if let Err(error) = order.withdraw_challenge(&token).await {
                    tracing::warn!(target: "navree::domains", %error, "could not withdraw an ACME challenge");
                }
            }
            result
        })
    }
}

/// instant-acme's requests, sent with reqwest.
#[derive(Clone)]
struct AcmeHttp(reqwest::Client);

impl HttpClient for AcmeHttp {
    fn request(
        &self,
        request: http::Request<BodyWrapper<Bytes>>,
    ) -> Pin<Box<dyn Future<Output = Result<BytesResponse, instant_acme::Error>> + Send>> {
        let client = self.0.clone();
        Box::pin(async move {
            let other =
                |error: Box<dyn std::error::Error + Send + Sync>| instant_acme::Error::Other(error);
            let (parts, body) = request.into_parts();
            let body = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(never) => match never {},
            };
            let mut outgoing = client.request(parts.method, parts.uri.to_string());
            for (name, value) in &parts.headers {
                outgoing = outgoing.header(name, value);
            }
            let response = outgoing
                .body(body)
                .send()
                .await
                .map_err(|e| other(Box::new(e)))?;
            let mut builder = http::Response::builder().status(response.status());
            for (name, value) in response.headers() {
                builder = builder.header(name, value);
            }
            let bytes = response.bytes().await.map_err(|e| other(Box::new(e)))?;
            let response = builder
                .body(Full::new(bytes))
                .map_err(|e| other(Box::new(e)))?;
            Ok(BytesResponse::from(response))
        })
    }
}

// -- the fake ------------------------------------------------------------------

/// A CA in memory for tests: it issues certificates signed by a test root
/// of its own ([`ca_pem`](AcmeFake::ca_pem)) without any challenge being
/// checked, records the hosts it was asked for, and fails on demand.
/// Clones share the CA.
///
/// ```no_run
/// use std::sync::Arc;
/// use navree_domains::{AcmeClient, AcmeFake};
///
/// let ca = AcmeFake::new();
/// // TestApp::builder()...override_value::<dyn AcmeClient>(Arc::new(ca.clone()))
/// ca.fail_next("too many certificates for this domain");
/// assert!(ca.orders().is_empty());
/// ```
#[derive(Clone)]
pub struct AcmeFake {
    state: Arc<Mutex<FakeCa>>,
    root: Arc<FakeRoot>,
}

struct FakeRoot {
    params: rcgen::CertificateParams,
    key: rcgen::KeyPair,
    pem: String,
}

#[derive(Default)]
struct FakeCa {
    orders: Vec<String>,
    failures: Vec<String>,
    valid_until: Option<DateTime<Utc>>,
    keep_challenges: bool,
    published: Vec<(String, String, String)>,
}

impl fmt::Debug for AcmeFake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcmeFake")
            .field("orders", &self.orders())
            .finish_non_exhaustive()
    }
}

impl Default for AcmeFake {
    fn default() -> Self {
        AcmeFake::new()
    }
}

impl AcmeFake {
    /// A CA with a new test root.
    ///
    /// # Panics
    ///
    /// If rcgen cannot make a key (never with ring).
    pub fn new() -> AcmeFake {
        let key = rcgen::KeyPair::generate().expect("a test CA key");
        let mut params =
            rcgen::CertificateParams::new(Vec::<String>::new()).expect("test CA parameters");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Navree test CA");
        let pem = params.self_signed(&key).expect("the test CA").pem();
        AcmeFake {
            state: Arc::new(Mutex::new(FakeCa::default())),
            root: Arc::new(FakeRoot { params, key, pem }),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut FakeCa) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The test root, PEM: trust it to connect to the app's HTTPS listener.
    pub fn ca_pem(&self) -> String {
        self.root.pem.clone()
    }

    /// Certificates issued from now on expire at `at` (default: 90 days
    /// after they are issued).
    pub fn expire_at(&self, at: DateTime<Utc>) {
        self.with(|ca| ca.valid_until = Some(at));
    }

    /// The next order fails with `reason`.
    pub fn fail_next(&self, reason: &str) {
        self.with(|ca| ca.failures.push(reason.to_owned()));
    }

    /// Leave the challenge answer of each order published (so a test can
    /// fetch it from the app).
    pub fn keep_challenges(&self) {
        self.with(|ca| ca.keep_challenges = true);
    }

    /// The hosts ordered, in order.
    pub fn orders(&self) -> Vec<String> {
        self.with(|ca| ca.orders.clone())
    }

    /// The challenges published: `(host, token, key authorization)`.
    pub fn challenges(&self) -> Vec<(String, String, String)> {
        self.with(|ca| ca.published.clone())
    }

    fn sign(&self, host: &str, not_after: DateTime<Utc>) -> Result<IssuedCertificate, String> {
        let key = rcgen::KeyPair::generate().map_err(|error| error.to_string())?;
        let mut params = rcgen::CertificateParams::new(vec![host.to_owned()])
            .map_err(|error| error.to_string())?;
        let not_before = not_after - TimeDelta::days(90);
        let (year, month, day) = ymd(not_before);
        params.not_before = rcgen::date_time_ymd(year, month, day);
        let (year, month, day) = ymd(not_after);
        params.not_after = rcgen::date_time_ymd(year, month, day);
        let issuer = rcgen::Issuer::from_params(&self.root.params, &self.root.key);
        let leaf = params
            .signed_by(&key, &issuer)
            .map_err(|error| error.to_string())?;
        Ok(IssuedCertificate {
            chain_pem: format!("{}{}", leaf.pem(), self.root.pem),
            private_key_pem: key.serialize_pem(),
        })
    }
}

/// The calendar day of `at` (UTC), for rcgen's `date_time_ymd`.
fn ymd(at: DateTime<Utc>) -> (i32, u8, u8) {
    (
        at.year(),
        u8::try_from(at.month()).unwrap_or(1),
        u8::try_from(at.day()).unwrap_or(1),
    )
}

impl AcmeClient for AcmeFake {
    fn issue<'a>(
        &'a self,
        order: &'a AcmeOrder,
    ) -> BoxFuture<'a, Result<IssuedCertificate, DomainsError>> {
        Box::pin(async move {
            let host = order.host().to_owned();
            let (failure, keep, number) = self.with(|ca| {
                ca.orders.push(host.clone());
                let failure = (!ca.failures.is_empty()).then(|| ca.failures.remove(0));
                (failure, ca.keep_challenges, ca.orders.len())
            });
            let token = format!("fake-token-{number}");
            let answer = format!("{token}.fake-thumbprint");
            order.publish_challenge(&token, &answer).await?;
            self.with(|ca| {
                ca.published
                    .push((host.clone(), token.clone(), answer.clone()))
            });
            if !keep {
                order.withdraw_challenge(&token).await?;
            }
            if let Some(reason) = failure {
                return Err(DomainsError::acme(host, reason));
            }
            let not_after = self
                .with(|ca| ca.valid_until)
                .unwrap_or_else(|| Utc::now() + TimeDelta::days(90));
            self.sign(&host, not_after)
                .map_err(|reason| DomainsError::acme(host, reason))
        })
    }
}

//! What the [`Social`](crate::Social) service works with once it is bound
//! to an app: the options, the providers and the app's services.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use navree_auth::{AuthOptions, AuthService};
use navree_cache::Cache;
use navree_contracts::{AuditEntry, AuditSink, Clock, SystemClock};
use navree_crypto::{Encrypter, Hasher};
use navree_db::{Db, Uow};
use navree_events::{Event, EventBus};
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{AppContext, BoxError, WeakAppContext};
use navree_crypto::jose::JwkSet;
use tokio::sync::Mutex;

use crate::provider::ProviderEntry;
use crate::{SocialError, SocialOptions, SocialProvider};

/// How long a fetched JSON Web Key Set is used.
const JWKS_TTL: Duration = Duration::from_secs(3600);

/// The shortest time between two fetches of one key set (an unknown `kid`
/// refetches, but not more often than this).
const JWKS_MIN_REFETCH: Duration = Duration::from_secs(30);

/// The bound service's state.
pub(crate) struct Runtime {
    pub(crate) options: Arc<SocialOptions>,
    pub(crate) providers: BTreeMap<String, Arc<ProviderEntry>>,
    pub(crate) encrypter: Arc<Encrypter>,
    pub(crate) hasher: Arc<Hasher>,
    pub(crate) auth: Arc<AuthService>,
    pub(crate) auth_options: Arc<AuthOptions>,
    pub(crate) db: Arc<Db>,
    pub(crate) cache: Cache,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) http: HttpClient,
    pub(crate) jwks: JwksCache,
    pub(crate) app: WeakAppContext,
}

impl Runtime {
    /// Resolves everything from `app`. Fails the boot when a provider is
    /// misconfigured in a way the options check cannot see (a `custom`
    /// provider with no registered [`SocialProvider`]).
    pub(crate) async fn build(app: &AppContext) -> Result<Runtime, BoxError> {
        let options = app.get::<SocialOptions>().await?;
        let mut custom: HashMap<&'static str, Arc<dyn SocialProvider>> = HashMap::new();
        for provider in app.get_all::<dyn SocialProvider>().await? {
            custom.insert(provider.key(), provider);
        }
        let mut providers = BTreeMap::new();
        for key in options.providers.keys() {
            let config = options.provider(key).expect("a listed provider");
            let entry = ProviderEntry::new(config, custom.get(key.as_str()).cloned())?;
            providers.insert(key.clone(), Arc::new(entry));
        }
        let cache = match app.try_get::<Cache>().await {
            Some(cache) => (*cache).clone(),
            None => Cache::memory(),
        };
        let clock = match app.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        let http = match app.try_get::<HttpClient>().await {
            Some(http) => (*http).clone(),
            None => HttpClient::new(HttpClientOptions::default())?,
        };
        Ok(Runtime {
            providers,
            encrypter: app.get::<Encrypter>().await?,
            hasher: app.get::<Hasher>().await?,
            auth: app.get::<AuthService>().await?,
            auth_options: app.get::<AuthOptions>().await?,
            db: app.get::<Db>().await?,
            cache,
            clock,
            http,
            jwks: JwksCache::default(),
            app: app.downgrade(),
            options,
        })
    }

    /// The provider under `key`.
    pub(crate) fn provider(&self, key: &str) -> Result<&Arc<ProviderEntry>, SocialError> {
        self.providers
            .get(key)
            .ok_or_else(|| SocialError::UnknownProvider(key.to_owned()))
    }

    /// The display name of `key` (the key itself for a provider that is no
    /// longer configured).
    pub(crate) fn provider_name(&self, key: &str) -> String {
        self.providers
            .get(key)
            .map_or_else(|| key.to_owned(), |entry| entry.name.clone())
    }

    /// The redirect URI of `key`: `{redirect_base}/{key}`.
    pub(crate) fn redirect_uri(&self, key: &str) -> String {
        format!("{}/{key}", self.options.redirect_base.trim_end_matches('/'))
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// Whether a first sign-in may create a user: `social.registration`,
    /// else `auth.registration`.
    pub(crate) fn registration(&self) -> bool {
        self.options
            .registration
            .unwrap_or(self.auth_options.registration)
    }

    /// The app's audit sink, when one is registered.
    pub(crate) async fn audit_sink(&self) -> Option<Arc<dyn AuditSink>> {
        self.app.upgrade()?.try_get::<dyn AuditSink>().await
    }

    /// Records `entry` now.
    pub(crate) async fn audit(&self, entry: AuditEntry) {
        if let Some(sink) = self.audit_sink().await {
            record(sink, entry).await;
        }
    }

    /// Records `entry` once `uow` commits.
    pub(crate) async fn audit_after_commit(&self, uow: &Uow, entry: AuditEntry) {
        if let Some(sink) = self.audit_sink().await {
            uow.after_commit(move || Box::pin(record(sink, entry)));
        }
    }

    /// Emits `event` once `uow` commits, when the app has an `EventBus`.
    pub(crate) async fn emit_after_commit<E: Event>(&self, uow: &Uow, event: E) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        uow.after_commit(move || {
            Box::pin(async move {
                if let Err(error) = bus.emit(event).await {
                    tracing::warn!(target: "navree::social", event = E::NAME, %error, "a social event listener failed");
                }
            })
        });
    }
}

async fn record(sink: Arc<dyn AuditSink>, entry: AuditEntry) {
    let action = entry.action.clone();
    if let Err(error) = sink.record(entry).await {
        tracing::warn!(target: "navree::audit", %error, action, "could not write a social audit entry");
    }
}

/// Fetched JSON Web Key Sets, by URL.
#[derive(Default)]
pub(crate) struct JwksCache {
    sets: Mutex<HashMap<String, (JwkSet, Instant)>>,
}

impl JwksCache {
    /// The key set at `url`, fetched when it is not cached, older than an
    /// hour, or lacks `kid` (key rotation).
    pub(crate) async fn get<F, Fut>(
        &self,
        url: &str,
        kid: Option<&str>,
        fetch: F,
    ) -> Result<JwkSet, SocialError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<JwkSet, SocialError>>,
    {
        let mut sets = self.sets.lock().await;
        if let Some((set, fetched)) = sets.get(url) {
            let fresh = fetched.elapsed() < JWKS_TTL;
            let knows_kid = kid.is_none_or(|kid| set.has_kid(kid));
            if fresh && (knows_kid || fetched.elapsed() < JWKS_MIN_REFETCH) {
                return Ok(set.clone());
            }
        }
        let set = fetch().await?;
        sets.insert(url.to_owned(), (set.clone(), Instant::now()));
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use navree_crypto::jose::Jwk;

    use super::*;

    fn set(kid: &str) -> JwkSet {
        JwkSet {
            keys: vec![Jwk {
                kty: "RSA".to_owned(),
                kid: Some(kid.to_owned()),
                ..Jwk::default()
            }],
        }
    }

    #[tokio::test]
    async fn key_sets_are_cached_until_an_unknown_kid_after_the_refetch_delay() {
        let cache = JwksCache::default();
        let fetches = AtomicUsize::new(0);
        let fetch = |kid: &'static str| {
            let fetches = &fetches;
            move || async move {
                fetches.fetch_add(1, Ordering::SeqCst);
                Ok(set(kid))
            }
        };
        cache
            .get("https://idp/keys", Some("a"), fetch("a"))
            .await
            .unwrap();
        cache
            .get("https://idp/keys", Some("a"), fetch("a"))
            .await
            .unwrap();
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        // An unknown kid right after a fetch does not refetch (no hammering).
        let keys = cache
            .get("https://idp/keys", Some("b"), fetch("b"))
            .await
            .unwrap();
        assert!(!keys.has_kid("b"));
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        // Another URL is its own entry.
        cache
            .get("https://other/keys", None, fetch("c"))
            .await
            .unwrap();
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
    }
}

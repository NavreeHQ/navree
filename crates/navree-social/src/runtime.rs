//! What the [`Social`](crate::Social) service works with once it is bound
//! to an app: the options, the providers and the app's services.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree_auth::{AppsOptions, AuthOptions, AuthService};
use navree_cache::Cache;
use navree_contracts::{AuditEntry, AuditSink, Clock, SystemClock};
use navree_crypto::jose::JwkSet;
use navree_crypto::{Encrypter, Hasher};
use navree_db::{Db, Uow};
use navree_events::{Event, EventBus};
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{AppContext, BoxError, WeakAppContext};
use tokio::sync::Mutex;

use crate::provider::ProviderEntry;
use crate::{SocialError, SocialOptions, SocialProvider};

/// How long a fetched JSON Web Key Set is used when the provider does not
/// say.
const JWKS_TTL: Duration = Duration::from_secs(3600);

/// The shortest and longest time a key set is used, whatever the provider
/// says.
const JWKS_MIN_TTL: Duration = Duration::from_secs(300);
const JWKS_MAX_TTL: Duration = Duration::from_secs(86_400);

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
    /// `http.rate_limit_ipv6_prefix`: callbacks are counted per client network.
    pub(crate) ipv6_prefix: u8,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) http: HttpClient,
    pub(crate) jwks: JwksCache,
    /// The bundle ids of the site's Apple apps (`auth.apps.apple`): the
    /// audiences of Sign in with Apple's identity tokens from the apps.
    pub(crate) apple_apps: Vec<String>,
    /// The package names of the site's Android apps (`auth.apps.android`):
    /// native Sign in with Google is offered only when there is one.
    pub(crate) android_apps: Vec<String>,
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
        let apps = app.try_get::<AppsOptions>().await;
        Ok(Runtime {
            providers,
            encrypter: app.get::<Encrypter>().await?,
            hasher: app.get::<Hasher>().await?,
            auth: app.get::<AuthService>().await?,
            auth_options: app.get::<AuthOptions>().await?,
            db: app.get::<Db>().await?,
            cache,
            ipv6_prefix: match app.try_get::<navree_http::HttpOptions>().await {
                Some(options) => options.rate_limit_ipv6_prefix,
                None => navree_http::DEFAULT_IPV6_PREFIX,
            },
            clock,
            http,
            jwks: JwksCache::default(),
            apple_apps: match &apps {
                Some(apps) => apps
                    .apple
                    .values()
                    .map(|app| app.bundle_id.trim().to_owned())
                    .filter(|bundle| !bundle.is_empty())
                    .collect(),
                None => Vec::new(),
            },
            android_apps: match &apps {
                Some(apps) => apps
                    .android
                    .values()
                    .map(|app| app.package.trim().to_owned())
                    .filter(|package| !package.is_empty())
                    .collect(),
                None => Vec::new(),
            },
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

    /// Emits `event` now, when the app has an `EventBus`.
    pub(crate) async fn emit<E: Event>(&self, event: E) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        if let Err(error) = bus.emit(event).await {
            tracing::warn!(target: "navree::social", event = E::NAME, %error, "a social event listener failed");
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

/// A fetched key set: when it was fetched and until when it is used.
struct CachedSet {
    set: JwkSet,
    fetched: DateTime<Utc>,
    expires: DateTime<Utc>,
}

/// Fetched JSON Web Key Sets, by URL.
#[derive(Default)]
pub(crate) struct JwksCache {
    sets: Mutex<HashMap<String, CachedSet>>,
}

impl JwksCache {
    /// The key set at `url` at time `now`. It is fetched when it is not
    /// cached, when its lifetime is over (the `max-age` the provider sent
    /// with it, see [`jwks_lifetime`]), or when it lacks `kid` (a key
    /// rotation): then once, and not again within 30 seconds, so a token
    /// with a made-up key id cannot make every request fetch the set.
    pub(crate) async fn get<F, Fut>(
        &self,
        url: &str,
        kid: Option<&str>,
        now: DateTime<Utc>,
        fetch: F,
    ) -> Result<JwkSet, SocialError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(JwkSet, Duration), SocialError>>,
    {
        let mut sets = self.sets.lock().await;
        if let Some(cached) = sets.get(url) {
            let fresh = now < cached.expires;
            let knows_kid = kid.is_none_or(|kid| cached.set.has_kid(kid));
            let refetched_lately =
                now.signed_duration_since(cached.fetched) < delta(JWKS_MIN_REFETCH);
            if fresh && (knows_kid || refetched_lately) {
                return Ok(cached.set.clone());
            }
        }
        let (set, lifetime) = fetch().await?;
        sets.insert(
            url.to_owned(),
            CachedSet {
                set: set.clone(),
                fetched: now,
                expires: now + delta(lifetime),
            },
        );
        Ok(set)
    }
}

fn delta(duration: Duration) -> chrono::TimeDelta {
    chrono::TimeDelta::from_std(duration).unwrap_or(chrono::TimeDelta::MAX)
}

/// How long a key set is used: the `max-age` of its `Cache-Control` header
/// (Google sends several hours), kept between five minutes and a day; an
/// hour without one.
pub(crate) fn jwks_lifetime(cache_control: Option<&str>) -> Duration {
    let max_age = cache_control.and_then(|value| {
        value.split(',').find_map(|directive| {
            let (name, seconds) = directive.trim().split_once('=')?;
            name.trim()
                .eq_ignore_ascii_case("max-age")
                .then(|| seconds.trim().trim_matches('"').parse::<u64>().ok())
                .flatten()
        })
    });
    match max_age {
        Some(seconds) => Duration::from_secs(seconds).clamp(JWKS_MIN_TTL, JWKS_MAX_TTL),
        None => JWKS_TTL,
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

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_800_000_000 + seconds, 0).unwrap()
    }

    #[tokio::test]
    async fn key_sets_are_cached_until_an_unknown_kid_after_the_refetch_delay() {
        let cache = JwksCache::default();
        let fetches = AtomicUsize::new(0);
        let fetch = |kid: &'static str| {
            let fetches = &fetches;
            move || async move {
                fetches.fetch_add(1, Ordering::SeqCst);
                Ok((set(kid), JWKS_TTL))
            }
        };
        let url = "https://idp/keys";
        cache.get(url, Some("a"), at(0), fetch("a")).await.unwrap();
        cache.get(url, Some("a"), at(1), fetch("a")).await.unwrap();
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        // An unknown kid right after a fetch does not refetch (no hammering).
        let keys = cache.get(url, Some("b"), at(2), fetch("b")).await.unwrap();
        assert!(!keys.has_kid("b"));
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        // Later, an unknown kid refetches once (a rotation)...
        let keys = cache.get(url, Some("b"), at(40), fetch("b")).await.unwrap();
        assert!(keys.has_kid("b"));
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
        // ...and a kid that stays unknown does not refetch again at once.
        let keys = cache.get(url, Some("z"), at(41), fetch("b")).await.unwrap();
        assert!(!keys.has_kid("z"));
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
        // Another URL is its own entry.
        cache
            .get("https://other/keys", None, at(42), fetch("c"))
            .await
            .unwrap();
        assert_eq!(fetches.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn key_sets_live_for_their_max_age() {
        let cache = JwksCache::default();
        let fetches = AtomicUsize::new(0);
        let counter = &fetches;
        let fetch = move || async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok((
                set("a"),
                jwks_lifetime(Some("public, max-age=21600, must-revalidate")),
            ))
        };
        let url = "https://idp/keys";
        cache.get(url, Some("a"), at(0), fetch).await.unwrap();
        cache.get(url, Some("a"), at(21_599), fetch).await.unwrap();
        assert_eq!(fetches.load(Ordering::SeqCst), 1, "fresh for six hours");
        cache.get(url, Some("a"), at(21_600), fetch).await.unwrap();
        assert_eq!(fetches.load(Ordering::SeqCst), 2, "stale after six hours");
    }

    #[test]
    fn the_lifetime_is_read_from_cache_control() {
        assert_eq!(
            jwks_lifetime(Some("public, max-age=19800, must-revalidate, no-transform")),
            Duration::from_secs(19_800)
        );
        assert_eq!(jwks_lifetime(Some("max-age=5")), JWKS_MIN_TTL);
        assert_eq!(jwks_lifetime(Some("max-age=9999999")), JWKS_MAX_TTL);
        assert_eq!(jwks_lifetime(Some("no-store")), JWKS_TTL);
        assert_eq!(jwks_lifetime(None), JWKS_TTL);
        assert_eq!(jwks_lifetime(Some("max-age=soon")), JWKS_TTL);
    }
}

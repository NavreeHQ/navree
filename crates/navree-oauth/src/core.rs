//! [`Core`]: what every OAuth service shares (database, options, scopes,
//! signing keys, clock, the token cache and the audit sink).

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use moka::future::Cache;
use navree_contracts::{AuditEntry, AuditSink, Clock, SystemClock};
use navree_db::{Db, Uow};
use navree_kernel::{AppContext, WeakAppContext};
use uuid::Uuid;

use crate::config::OAuthOptions;
use crate::keys::SigningKeys;
use crate::scope::ScopeRegistry;

/// How many token lookups each instance caches.
const CACHE_CAPACITY: u64 = 100_000;

/// A token found by the authentication strategy, as cached.
#[derive(Clone, Debug)]
pub(crate) struct CachedToken {
    pub(crate) token_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) client_id: Option<Uuid>,
    pub(crate) client_name: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) grant: crate::Grant,
    pub(crate) workspace_id: Option<Uuid>,
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) last_used_at: Option<DateTime<Utc>>,
}

/// The shared state of the OAuth services.
pub(crate) struct Core {
    pub(crate) db: Arc<Db>,
    pub(crate) options: Arc<OAuthOptions>,
    pub(crate) issuer: String,
    pub(crate) scopes: ScopeRegistry,
    pub(crate) keys: SigningKeys,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) cache: Option<Cache<String, CachedToken>>,
    pub(crate) app: WeakAppContext,
}

impl Core {
    pub(crate) fn new(
        db: Arc<Db>,
        options: Arc<OAuthOptions>,
        issuer: String,
        keys: SigningKeys,
        clock: Arc<dyn Clock>,
        app: WeakAppContext,
    ) -> Self {
        let cache = (options.token_cache_secs > 0).then(|| {
            Cache::builder()
                .max_capacity(CACHE_CAPACITY)
                .time_to_live(Duration::from_secs(options.token_cache_secs))
                .build()
        });
        Self {
            scopes: ScopeRegistry::new(&options),
            db,
            options,
            issuer: issuer.trim_end_matches('/').to_owned(),
            keys,
            clock,
            cache,
            app,
        }
    }

    /// The current time.
    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// `now` plus `secs` seconds.
    pub(crate) fn after(&self, secs: u64) -> DateTime<Utc> {
        let delta = i64::try_from(secs)
            .ok()
            .and_then(TimeDelta::try_seconds)
            .unwrap_or(TimeDelta::MAX);
        self.now()
            .checked_add_signed(delta)
            .unwrap_or(DateTime::<Utc>::MAX_UTC)
    }

    /// The absolute URL of protocol endpoint `name` (`token` →
    /// `https://api.acme.test/oauth/token`).
    pub(crate) fn endpoint(&self, name: &str) -> String {
        format!("{}{}/{name}", self.issuer, self.options.base_path())
    }

    /// The app, while it runs.
    pub(crate) fn app(&self) -> Option<AppContext> {
        self.app.upgrade()
    }

    /// Records `entry` now (when a `dyn AuditSink` is registered).
    pub(crate) async fn audit(&self, entry: AuditEntry) {
        if let Some(sink) = self.audit_sink().await {
            record(sink, entry).await;
        }
    }

    /// Records `entry` once `uow` commits (dropped on rollback).
    pub(crate) async fn audit_after_commit(&self, uow: &Uow, entry: AuditEntry) {
        if let Some(sink) = self.audit_sink().await {
            uow.after_commit(move || Box::pin(record(sink, entry)));
        }
    }

    async fn audit_sink(&self) -> Option<Arc<dyn AuditSink>> {
        self.app()?.try_get::<dyn AuditSink>().await
    }

    /// Forgets the cached lookups of `hashes` (token digests).
    pub(crate) async fn forget(&self, hashes: impl IntoIterator<Item = String>) {
        if let Some(cache) = &self.cache {
            for hash in hashes {
                cache.invalidate(&hash).await;
            }
        }
    }

    /// Forgets every cached lookup (after bulk revocations).
    pub(crate) fn forget_all(&self) {
        if let Some(cache) = &self.cache {
            cache.invalidate_all();
        }
    }
}

/// The app's `dyn Clock`, or the system clock.
pub(crate) async fn clock_of(ctx: &AppContext) -> Arc<dyn Clock> {
    ctx.try_get::<dyn Clock>()
        .await
        .unwrap_or_else(|| Arc::new(SystemClock))
}

async fn record(sink: Arc<dyn AuditSink>, entry: AuditEntry) {
    let action = entry.action.clone();
    if let Err(error) = sink.record(entry).await {
        tracing::warn!(target: "navree::audit", %error, action, "could not write an OAuth audit entry");
    }
}

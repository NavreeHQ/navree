//! [`Provider`]: the credential a cloud store signs its requests with, kept
//! until shortly before it expires and then fetched again, once, whoever
//! asks.
//!
//! A way of signing in is a [`Source`]: static keys, a token file exchanged
//! for a session, the metadata service of an instance, a managed identity.
//! A driver writes its sources (and the order they are tried in) and hands
//! one to a [`Provider`], which every request then asks:
//!
//! ```
//! use std::time::{Duration, SystemTime};
//!
//! use navree_storage::cloud::credentials::{Credential, Provider};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> navree_storage::cloud::Result<()> {
//! // A token that lasts an hour, as a metadata service would give it.
//! let tokens = Provider::new(|| async {
//!     let token = "ya29.token-from-the-metadata-service".to_owned();
//!     Ok(Credential::until(token, SystemTime::now() + Duration::from_secs(3600)))
//! });
//! let first = tokens.credential().await?;
//! let again = tokens.credential().await?;
//! assert_eq!(first.secret, again.secret);
//! assert_eq!(first.expires_at, again.expires_at, "kept, not fetched again");
//!
//! // Keys from the disk's settings never expire and are never fetched.
//! let keys = Provider::fixed(("AKIAAGRA".to_owned(), "s3cret".to_owned()));
//! assert_eq!(keys.credential().await?.secret.0, "AKIAAGRA");
//! assert!(!format!("{keys:?} {first:?}").contains("s3cret"));
//! # Ok(())
//! # }
//! ```
//!
//! A request that is tried again asks again
//! ([`with_backoff`](super::retry::with_backoff)), and so does every part of
//! a long upload: a credential that runs out half-way is replaced before
//! the next part is signed.

use std::fmt;
use std::future::Future;
use std::time::{Duration, SystemTime};

use navree_kernel::BoxFuture;
use tokio::sync::RwLock;

use super::{Error, ErrorKind, Result};

/// How long before a credential expires it is fetched again.
pub const REFRESH_BEFORE: Duration = Duration::from_secs(5 * 60);

/// How long a credential that was just fetched is used although it is
/// inside [`REFRESH_BEFORE`] already (but not expired): a service that
/// hands out the same short-lived token until it rotates it is not asked
/// for it by every request.
const REFETCH_PAUSE: Duration = Duration::from_millis(100);

/// A credential and when it stops working.
///
/// `T` is what the provider's requests are signed with: a bearer token, a
/// set of keys with a session token, an account key. `Debug` shows when it
/// expires and never the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential<T> {
    /// What requests are signed with.
    pub secret: T,
    /// When it stops working; `None` for one that never does.
    pub expires_at: Option<SystemTime>,
}

impl<T> Credential<T> {
    /// A credential that never expires: keys from the disk's settings.
    pub fn fixed(secret: T) -> Self {
        Self {
            secret,
            expires_at: None,
        }
    }

    /// A credential that works until `expires_at`.
    pub fn until(secret: T, expires_at: SystemTime) -> Self {
        Self {
            secret,
            expires_at: Some(expires_at),
        }
    }
}

/// When it expires; never the secret.
impl<T> fmt::Debug for Credential<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// One way of signing in: where a credential comes from.
///
/// [`fetch`](Source::fetch) gets a new credential each time it is called;
/// the [`Provider`] in front decides when. Its failure says which way of
/// signing in failed and why, and never holds a key, a token or the body
/// of an answer. A source does its own trying again: the provider asks it
/// once.
///
/// A closure that returns the future is a source, and so is a boxed
/// source, for a driver that picks its way of signing in from the disk's
/// settings. Several ways tried in order are a source that asks each of
/// its own in turn.
pub trait Source<T>: Send + Sync + 'static {
    /// A new credential.
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<T>>>;
}

impl<T, F, Fut> Source<T> for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Credential<T>>> + Send + 'static,
{
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<T>>> {
        Box::pin(self())
    }
}

impl<T: 'static> Source<T> for Box<dyn Source<T>> {
    fn fetch(&self) -> BoxFuture<'_, Result<Credential<T>>> {
        (**self).fetch()
    }
}

/// The time a [`Provider`] reads.
type Clock = Box<dyn Fn() -> SystemTime + Send + Sync>;

/// The credential of one cloud store: asked for before every request, and
/// fetched from its [`Source`] only when there is none yet or the one it
/// has expires within [`REFRESH_BEFORE`].
///
/// A fetch happens under a lock: callers that arrive while one is under
/// way wait for it and get its credential, so the source is asked once
/// however many requests are waiting. A fetch that fails changes nothing,
/// and the next caller tries again.
pub struct Provider<T> {
    /// `None` for a credential that is fixed: it is never fetched.
    source: Option<Box<dyn Source<T>>>,
    kept: RwLock<Option<Kept<T>>>,
    clock: Clock,
}

/// A credential and when it was fetched.
struct Kept<T> {
    credential: Credential<T>,
    fetched_at: SystemTime,
}

impl<T: Clone + Send + Sync + 'static> Provider<T> {
    /// A provider of the credentials `source` gives.
    pub fn new(source: impl Source<T>) -> Self {
        Self::with_clock(source, SystemTime::now)
    }

    /// Like [`new`](Self::new), reading the time from `clock` and not from
    /// the system: for tests of what happens as a credential nears its end.
    pub fn with_clock(
        source: impl Source<T>,
        clock: impl Fn() -> SystemTime + Send + Sync + 'static,
    ) -> Self {
        Self {
            source: Some(Box::new(source)),
            kept: RwLock::new(None),
            clock: Box::new(clock),
        }
    }

    /// A provider of one credential that never expires and is never
    /// fetched: keys from the disk's settings.
    pub fn fixed(secret: T) -> Self {
        Self {
            source: None,
            kept: RwLock::new(Some(Kept {
                credential: Credential::fixed(secret),
                fetched_at: SystemTime::UNIX_EPOCH,
            })),
            clock: Box::new(SystemTime::now),
        }
    }

    /// The credential to sign the next request with: the one kept while it
    /// has more than [`REFRESH_BEFORE`] left, else a new one from the
    /// source.
    ///
    /// A failure is the source's, as [`ErrorKind::Credential`] with the
    /// source's failure under it: whatever it was, it is not worth sending
    /// the request again for, since the source has done its own trying.
    pub async fn credential(&self) -> Result<Credential<T>> {
        self.credential_for(REFRESH_BEFORE).await
    }

    /// Like [`credential`](Self::credential), refreshing early when the kept
    /// temporary credential has at most `min_remaining` left (for instance
    /// when signing a link whose `expires_in` exceeds the remaining session
    /// lifetime), subject to a short pause after a fresh fetch.
    pub async fn credential_for(&self, min_remaining: Duration) -> Result<Credential<T>> {
        let needed = min_remaining.max(REFRESH_BEFORE);
        if let Some(credential) = self.usable(self.kept.read().await.as_ref(), needed) {
            return Ok(credential);
        }
        let mut kept = self.kept.write().await;
        // Another caller may have fetched it while this one waited.
        if let Some(credential) = self.usable(kept.as_ref(), needed) {
            return Ok(credential);
        }
        let Some(source) = &self.source else {
            return Err(Error::new(
                ErrorKind::Credential,
                "the store has no credential",
            ));
        };
        let credential = source.fetch().await.map_err(|error| match error.kind() {
            ErrorKind::Credential => error,
            _ => Error::new(ErrorKind::Credential, error.to_string()).with_source(error),
        })?;
        *kept = Some(Kept {
            credential: credential.clone(),
            fetched_at: (self.clock)(),
        });
        Ok(credential)
    }

    /// The kept credential, if it has more than `needed` left (or was just
    /// fetched and has not expired).
    fn usable(&self, kept: Option<&Kept<T>>, needed: Duration) -> Option<Credential<T>> {
        let kept = kept?;
        let Some(expires_at) = kept.credential.expires_at else {
            return Some(kept.credential.clone());
        };
        let now = (self.clock)();
        // Nothing is left of one that has expired, or when the clock was
        // set back past its fetch.
        let left = expires_at.duration_since(now).unwrap_or_default();
        let just_fetched = now
            .duration_since(kept.fetched_at)
            .is_ok_and(|age| age < REFETCH_PAUSE);
        (left > needed || (just_fetched && !left.is_zero())).then(|| kept.credential.clone())
    }
}

/// Whether it fetches; never a credential.
impl<T> fmt::Debug for Provider<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Provider")
            .field("fixed", &self.source.is_none())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// A clock tests move: seconds after a fixed start.
    #[derive(Clone, Default)]
    struct Hands(Arc<AtomicU64>);

    impl Hands {
        fn now(&self) -> SystemTime {
            SystemTime::UNIX_EPOCH
                + Duration::from_secs(1_795_000_000)
                + Duration::from_millis(self.0.load(Ordering::SeqCst))
        }

        fn advance(&self, by: Duration) {
            self.0
                .fetch_add(u64::try_from(by.as_millis()).unwrap(), Ordering::SeqCst);
        }
    }

    /// A provider whose source counts its fetches and gives the token
    /// `token-<n>`, good for `lifetime` from the moment it is fetched.
    fn counting(hands: &Hands, lifetime: Duration) -> (Provider<String>, Arc<AtomicU64>) {
        let fetches = Arc::new(AtomicU64::new(0));
        let (counted, source_hands) = (fetches.clone(), hands.clone());
        let clock_hands = hands.clone();
        let provider = Provider::with_clock(
            move || {
                let n = counted.fetch_add(1, Ordering::SeqCst) + 1;
                let expires_at = source_hands.now() + lifetime;
                async move { Ok(Credential::until(format!("token-{n}"), expires_at)) }
            },
            move || clock_hands.now(),
        );
        (provider, fetches)
    }

    const MINUTE: Duration = Duration::from_secs(60);

    #[tokio::test]
    async fn a_credential_is_kept_until_five_minutes_before_it_expires() {
        let hands = Hands::default();
        let (provider, fetches) = counting(&hands, 60 * MINUTE);
        assert_eq!(provider.credential().await.unwrap().secret, "token-1");
        hands.advance(54 * MINUTE);
        assert_eq!(provider.credential().await.unwrap().secret, "token-1");
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        // Exactly five minutes left is not more than five minutes.
        hands.advance(MINUTE);
        let renewed = provider.credential().await.unwrap();
        assert_eq!(renewed.secret, "token-2");
        assert_eq!(renewed.expires_at, Some(hands.now() + 60 * MINUTE));
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_short_lived_credential_is_not_fetched_by_every_request() {
        let hands = Hands::default();
        // Inside the last five minutes from the start.
        let (provider, fetches) = counting(&hands, 2 * MINUTE);
        for _ in 0..5 {
            assert_eq!(provider.credential().await.unwrap().secret, "token-1");
        }
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
        hands.advance(Duration::from_millis(100));
        assert_eq!(provider.credential().await.unwrap().secret, "token-2");
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_expired_credential_is_never_used() {
        let hands = Hands::default();
        let (provider, fetches) = counting(&hands, Duration::ZERO);
        for n in 1..=3 {
            let credential = provider.credential().await.unwrap();
            assert_eq!(credential.secret, format!("token-{n}"));
        }
        assert_eq!(fetches.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_failed_fetch_is_a_credential_failure_and_the_next_caller_tries_again() {
        let fetches = Arc::new(AtomicU64::new(0));
        let counted = fetches.clone();
        let provider = Provider::new(move || {
            let n = counted.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if n == 1 {
                    Err(Error::answered(
                        ::http::StatusCode::SERVICE_UNAVAILABLE,
                        None,
                    ))
                } else {
                    Ok(Credential::until(n, SystemTime::now() + 60 * MINUTE))
                }
            }
        });
        let error = provider.credential().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Credential);
        assert_eq!(
            error.to_string(),
            "the store answered 503 Service Unavailable"
        );
        // The source's own failure is under it, and a request is not sent
        // again for it.
        let under = std::error::Error::source(&error)
            .and_then(|source| source.downcast_ref::<Error>())
            .unwrap();
        assert_eq!(under.kind(), ErrorKind::Status);
        assert!(!error.retryable(crate::cloud::retry::Replay::Safe));
        assert_eq!(provider.credential().await.unwrap().secret, 2);
        assert_eq!(provider.credential().await.unwrap().secret, 2);
        assert_eq!(fetches.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_fixed_credential_is_never_fetched_and_never_shown() {
        let provider = Provider::fixed("s3cret-key".to_owned());
        let credential = provider.credential().await.unwrap();
        assert_eq!(credential, Credential::fixed("s3cret-key".to_owned()));
        assert_eq!(credential.expires_at, None);
        let shown = format!("{provider:?} {credential:?}");
        assert!(!shown.contains("s3cret"), "{shown}");
        assert_eq!(format!("{provider:?}"), "Provider { fixed: true, .. }");
    }

    /// Ways of signing in tried in order, as a driver writes them: the
    /// first that gives a credential is the one, and when none does the
    /// failure is the last one's.
    struct Chain(Vec<Box<dyn Source<String>>>);

    impl Source<String> for Chain {
        fn fetch(&self) -> BoxFuture<'_, Result<Credential<String>>> {
            Box::pin(async move {
                let mut failure = Error::new(ErrorKind::Credential, "no way of signing in is set");
                for source in &self.0 {
                    match source.fetch().await {
                        Ok(credential) => return Ok(credential),
                        Err(error) => failure = error,
                    }
                }
                Err(failure)
            })
        }
    }

    #[tokio::test]
    async fn ways_of_signing_in_are_tried_in_order_behind_one_provider() {
        let asked = Arc::new(AtomicU64::new(0));
        let no_keys = || async {
            Err::<Credential<String>, _>(Error::new(ErrorKind::Credential, "no keys are set"))
        };
        let no_service = || async {
            Err::<Credential<String>, _>(Error::new(
                ErrorKind::Connect,
                "no metadata service answers",
            ))
        };
        let token_file = || async { Ok(Credential::fixed("from the token file".to_owned())) };
        let later = {
            let asked = asked.clone();
            move || {
                asked.fetch_add(1, Ordering::SeqCst);
                async { Ok(Credential::fixed("from a later way".to_owned())) }
            }
        };

        let chain = Chain(vec![
            Box::new(no_keys),
            Box::new(token_file),
            Box::new(later),
        ]);
        let provider = Provider::new(chain);
        let credential = provider.credential().await.unwrap();
        assert_eq!(credential.secret, "from the token file");
        assert_eq!(asked.load(Ordering::SeqCst), 0);

        // When no way works, the failure is the last way's, as a failure to
        // sign in: no request is sent again for it.
        let chain = Chain(vec![Box::new(no_keys), Box::new(no_service)]);
        let error = Provider::new(chain).credential().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Credential);
        assert_eq!(error.to_string(), "no metadata service answers");
        let nothing = Provider::new(Chain(Vec::new()));
        let error = nothing.credential().await.unwrap_err();
        assert_eq!(error.to_string(), "no way of signing in is set");

        // A source picked at run time is boxed, and is a source.
        let picked: Box<dyn Source<String>> = Box::new(token_file);
        let provider = Provider::new(picked);
        assert_eq!(
            provider.credential().await.unwrap().secret,
            "from the token file"
        );
    }
}

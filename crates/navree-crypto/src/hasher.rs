//! [`Hasher`]: Argon2id password hashing off the async runtime threads.

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use argon2::password_hash::{self, PasswordHasher as _, PasswordVerifier as _};
use argon2::{Algorithm, Argon2, Params, PasswordHash, Version};
use navree_config::ConfigSection;
use serde::Deserialize;
use tokio::sync::Semaphore;

use crate::{CryptoError, Result};

/// Argon2id parameters and the hashing concurrency limit: section `hashing`
/// for `CryptoModule::from_config()`.
///
/// The defaults are OWASP's minimum Argon2id recommendation (19 MiB, 2
/// iterations, 1 lane). Raising any cost parameter makes
/// [`Hasher::needs_rehash`] true for existing hashes, so they are upgraded
/// at the next login.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(key = "hashing", docs = "/docs/security/encryption-and-hashing/", validate = Self::check_values)]
#[serde(default)]
pub struct HashingOptions {
    /// Memory cost in KiB (`m`; default 19 456 = 19 MiB).
    pub memory_kib: u32,
    /// Number of passes over the memory (`t`; default 2).
    pub iterations: u32,
    /// Number of lanes (`p`; default 1).
    pub parallelism: u32,
    /// How many hashes may run at once; `None` (the default) uses the number
    /// of CPUs. Each running hash holds `memory_kib` of memory.
    pub max_concurrent_hashes: Option<usize>,
}

impl HashingOptions {
    /// Checks the parameters (the boot calls this): the ones Argon2 refuses, and no
    /// permits.
    fn check_values(&self) -> std::result::Result<(), String> {
        Hasher::new(self)
            .map(drop)
            .map_err(|error| error.to_string())
    }
}

impl Default for HashingOptions {
    fn default() -> Self {
        Self {
            memory_kib: Params::DEFAULT_M_COST,
            iterations: Params::DEFAULT_T_COST,
            parallelism: Params::DEFAULT_P_COST,
            max_concurrent_hashes: None,
        }
    }
}

/// Hashes and verifies passwords with Argon2id (PHC string format,
/// `$argon2id$v=19$m=...,t=...,p=...$<salt>$<hash>`).
///
/// Hashing is CPU- and memory-heavy, so every hash and verification runs on
/// Tokio's blocking pool (`spawn_blocking`), never on an async worker, behind
/// a semaphore of [`max_concurrent_hashes`](HashingOptions::max_concurrent_hashes)
/// permits. A permit is held until the hash finishes, even if the caller's
/// future is dropped, so a burst of logins cannot exhaust memory.
///
/// `CryptoModule` provides it as a global singleton; clones share the
/// semaphore.
///
/// ```
/// use navree_crypto::{Hasher, HashingOptions};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_crypto::CryptoError> {
/// let hasher = Hasher::new(&HashingOptions::default())?;
/// let hash = hasher.hash("correct horse battery staple").await?;
/// assert!(hasher.verify("correct horse battery staple", &hash).await?);
/// assert!(!hasher.needs_rehash(&hash));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Hasher {
    params: Params,
    max_concurrent: usize,
    permits: Arc<Semaphore>,
}

impl Hasher {
    /// A hasher with `options`; [`CryptoError::InvalidHashingOptions`] when
    /// a cost is out of Argon2's range or the concurrency limit is 0.
    pub fn new(options: &HashingOptions) -> Result<Hasher> {
        let params = Params::new(
            options.memory_kib,
            options.iterations,
            options.parallelism,
            None,
        )
        .map_err(|error| CryptoError::InvalidHashingOptions {
            reason: error.to_string(),
        })?;
        let max_concurrent = match options.max_concurrent_hashes {
            Some(0) => {
                return Err(CryptoError::InvalidHashingOptions {
                    reason: "max_concurrent_hashes must be at least 1".to_owned(),
                });
            }
            Some(n) => n,
            None => std::thread::available_parallelism().map_or(1, NonZeroUsize::get),
        };
        Ok(Hasher {
            params,
            max_concurrent,
            permits: Arc::new(Semaphore::new(max_concurrent)),
        })
    }

    /// Hashes `password` with a fresh random salt and the configured
    /// parameters, returning the PHC string to store.
    pub async fn hash(&self, password: &str) -> Result<String> {
        let params = self.params.clone();
        let password = password.as_bytes().to_vec();
        self.run_blocking(move || {
            argon2id(params)
                .hash_password(&password)
                .map(|hash| hash.to_string())
                .map_err(hashing_error)
        })
        .await
    }

    /// Checks `password` against a stored `hash`, using the parameters
    /// stored in the hash (so old hashes keep verifying after the options
    /// change).
    ///
    /// `Ok(false)` when the password does not match;
    /// [`CryptoError::InvalidHash`] when `hash` is not an Argon2 PHC string.
    pub async fn verify(&self, password: &str, hash: &str) -> Result<bool> {
        let parsed = PasswordHash::new(hash).map_err(|_| CryptoError::InvalidHash)?;
        // Argon2's verifier reports a hash without salt or output, or with
        // unreadable parameters, as a mismatch: check the shape up front.
        if Algorithm::try_from(parsed.algorithm.as_str()).is_err()
            || parsed.salt.is_none()
            || parsed.hash.is_none()
            || Params::try_from(&parsed).is_err()
        {
            return Err(CryptoError::InvalidHash);
        }
        let password = password.as_bytes().to_vec();
        self.run_blocking(
            move || match Argon2::default().verify_password(&password, &parsed) {
                Ok(()) => Ok(true),
                Err(password_hash::Error::PasswordInvalid) => Ok(false),
                Err(
                    password_hash::Error::Algorithm
                    | password_hash::Error::EncodingInvalid
                    | password_hash::Error::OutputSize
                    | password_hash::Error::ParamsInvalid
                    | password_hash::Error::SaltInvalid
                    | password_hash::Error::Version,
                ) => Err(CryptoError::InvalidHash),
                Err(error) => Err(hashing_error(error)),
            },
        )
        .await
    }

    /// Whether `hash` should be replaced by a fresh [`hash`](Hasher::hash) of
    /// the password (after a successful [`verify`](Hasher::verify)): true when
    /// it does not parse, or is not an Argon2id v19 hash with exactly the
    /// configured memory, iterations, parallelism and output length.
    pub fn needs_rehash(&self, hash: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(hash) else {
            return true;
        };
        if parsed.algorithm.as_str() != Algorithm::Argon2id.as_str()
            || parsed.version != Some(Version::V0x13.into())
        {
            return true;
        }
        let Ok(stored) = Params::try_from(&parsed) else {
            return true;
        };
        let output_len = |p: &Params| p.output_len().unwrap_or(Params::DEFAULT_OUTPUT_LEN);
        stored.m_cost() != self.params.m_cost()
            || stored.t_cost() != self.params.t_cost()
            || stored.p_cost() != self.params.p_cost()
            || output_len(&stored) != output_len(&self.params)
    }

    /// Runs `f` on the blocking pool once a permit is free. The permit moves
    /// into the blocking task, so it is released only when `f` returns.
    async fn run_blocking<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let permit =
            self.permits
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| CryptoError::Hashing {
                    reason: "the hashing semaphore was closed".to_owned(),
                })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f()
        })
        .await
        .map_err(|error| CryptoError::Hashing {
            reason: if error.is_panic() {
                "the hashing task panicked".to_owned()
            } else {
                "the hashing task was cancelled".to_owned()
            },
        })?
    }
}

/// An Argon2id v19 context for `params`.
fn argon2id(params: Params) -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Wraps an Argon2 failure (its messages never contain the password).
fn hashing_error(error: password_hash::Error) -> CryptoError {
    CryptoError::Hashing {
        reason: error.to_string(),
    }
}

/// Shows the parameters and the concurrency limit.
impl fmt::Debug for Hasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hasher")
            .field("algorithm", &"argon2id")
            .field("memory_kib", &self.params.m_cost())
            .field("iterations", &self.params.t_cost())
            .field("parallelism", &self.params.p_cost())
            .field("max_concurrent_hashes", &self.max_concurrent)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    /// The semaphore bounds how many hashes run at once, and the work runs
    /// on blocking-pool threads, never on the runtime thread.
    #[tokio::test(flavor = "current_thread")]
    async fn hashing_runs_off_runtime_threads() {
        let hasher = Hasher::new(&HashingOptions {
            memory_kib: 1024,
            iterations: 1,
            parallelism: 1,
            max_concurrent_hashes: Some(2),
        })
        .expect("hasher");
        let runtime_thread = std::thread::current().id();
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let threads = Arc::new(Mutex::new(HashSet::new()));

        let mut jobs = tokio::task::JoinSet::new();
        for _ in 0..6 {
            let (hasher, active, peak, threads) = (
                hasher.clone(),
                active.clone(),
                peak.clone(),
                threads.clone(),
            );
            jobs.spawn(async move {
                hasher
                    .run_blocking(move || {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        threads.lock().unwrap().insert(std::thread::current().id());
                        std::thread::sleep(Duration::from_millis(40));
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .await
            });
        }
        while let Some(result) = jobs.join_next().await {
            result.expect("task").expect("job");
        }

        assert_eq!(peak.load(Ordering::SeqCst), 2, "semaphore bound");
        let threads = threads.lock().unwrap();
        assert!(!threads.is_empty());
        assert!(
            !threads.contains(&runtime_thread),
            "hashing ran on the runtime thread"
        );
    }

    #[tokio::test]
    async fn a_panicking_hash_job_is_an_error_and_releases_its_permit() {
        let hasher = Hasher::new(&HashingOptions {
            max_concurrent_hashes: Some(1),
            ..HashingOptions::default()
        })
        .expect("hasher");
        let error = hasher
            .run_blocking::<(), _>(|| panic!("boom"))
            .await
            .expect_err("panicked job");
        assert!(matches!(error, CryptoError::Hashing { .. }), "{error:?}");
        hasher
            .run_blocking(|| Ok(()))
            .await
            .expect("permit released");
    }
}

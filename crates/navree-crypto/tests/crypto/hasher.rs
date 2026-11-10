//! `Hasher`: Argon2id password hashes off the runtime threads.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use argon2::password_hash::PasswordHasher as _;
use argon2::{Algorithm, Argon2, Params, Version};
use navree_crypto::{CryptoError, Hasher, HashingOptions};

/// Cheap parameters so tests stay fast (production uses the defaults).
fn fast() -> HashingOptions {
    HashingOptions {
        memory_kib: 1024,
        iterations: 1,
        parallelism: 1,
        max_concurrent_hashes: Some(2),
    }
}

fn build(options: HashingOptions) -> Hasher {
    Hasher::new(&options).expect("valid hashing options")
}

#[test]
fn default_options_follow_owasp_argon2id_minimums() {
    let options = HashingOptions::default();
    assert_eq!(options.memory_kib, 19_456);
    assert_eq!(options.iterations, 2);
    assert_eq!(options.parallelism, 1);
    assert_eq!(options.max_concurrent_hashes, None);
    Hasher::new(&options).expect("defaults are valid");
}

#[tokio::test]
async fn hash_verify_roundtrip() {
    let hasher = build(fast());
    let hash = hasher
        .hash("correct horse battery staple")
        .await
        .expect("hash");
    assert!(hash.starts_with("$argon2id$v=19$m=1024,t=1,p=1$"), "{hash}");
    assert!(!hash.contains("correct horse"));
    assert!(
        hasher
            .verify("correct horse battery staple", &hash)
            .await
            .expect("verify")
    );
    // A fresh salt every time.
    let again = hasher
        .hash("correct horse battery staple")
        .await
        .expect("hash");
    assert_ne!(hash, again);
    assert!(
        hasher
            .verify("correct horse battery staple", &again)
            .await
            .expect("verify")
    );
}

#[tokio::test]
async fn wrong_password_fails() {
    let hasher = build(fast());
    let hash = hasher.hash("secret-password").await.expect("hash");
    assert!(
        !hasher
            .verify("secret-passwore", &hash)
            .await
            .expect("verify")
    );
    assert!(!hasher.verify("", &hash).await.expect("verify"));
}

#[tokio::test]
async fn verify_uses_the_parameters_stored_in_the_hash() {
    let old = build(fast());
    let hash = old.hash("pw").await.expect("hash");
    let new = build(HashingOptions {
        memory_kib: 2048,
        iterations: 2,
        ..fast()
    });
    assert!(new.verify("pw", &hash).await.expect("verify"));
}

#[tokio::test]
async fn verify_rejects_malformed_hashes() {
    let hasher = build(fast());
    for bad in [
        "",
        "not a hash",
        "$2y$10$abcdefghijklmnopqrstuv",
        "$argon2id$v=19$m=x",
    ] {
        let error = hasher.verify("pw", bad).await.expect_err(bad);
        assert!(
            matches!(error, CryptoError::InvalidHash),
            "{bad}: {error:?}"
        );
    }
}

#[tokio::test]
async fn needs_rehash_when_params_change() {
    let hasher = build(fast());
    let hash = hasher.hash("pw").await.expect("hash");
    assert!(!hasher.needs_rehash(&hash));

    for changed in [
        HashingOptions {
            memory_kib: 2048,
            ..fast()
        },
        HashingOptions {
            iterations: 2,
            ..fast()
        },
        HashingOptions {
            parallelism: 2,
            ..fast()
        },
    ] {
        assert!(build(changed.clone()).needs_rehash(&hash), "{changed:?}");
    }
    // The concurrency limit is not a hash parameter.
    assert!(
        !build(HashingOptions {
            max_concurrent_hashes: Some(7),
            ..fast()
        })
        .needs_rehash(&hash)
    );

    // Other Argon2 variants and unparseable hashes are rehashed too.
    let params = Params::new(1024, 1, 1, None).expect("params");
    let argon2i = Argon2::new(Algorithm::Argon2i, Version::V0x13, params)
        .hash_password(b"pw")
        .expect("argon2i")
        .to_string();
    assert!(hasher.needs_rehash(&argon2i));
    assert!(hasher.needs_rehash("garbage"));
    assert!(hasher.needs_rehash("$2y$10$abcdefghijklmnopqrstuv"));
}

#[test]
fn invalid_options_are_rejected() {
    for bad in [
        HashingOptions {
            memory_kib: 1,
            ..fast()
        },
        HashingOptions {
            iterations: 0,
            ..fast()
        },
        HashingOptions {
            parallelism: 0,
            ..fast()
        },
        HashingOptions {
            max_concurrent_hashes: Some(0),
            ..fast()
        },
    ] {
        let error = Hasher::new(&bad).expect_err("invalid options");
        assert!(
            matches!(error, CryptoError::InvalidHashingOptions { .. }),
            "{bad:?}: {error:?}"
        );
    }
}

/// On a single-threaded runtime, a hash computed inline would block every
/// other task until it finished. Here the other task completes first.
#[tokio::test(flavor = "current_thread")]
async fn hash_does_not_block_the_runtime_thread() {
    let hasher = build(HashingOptions::default());
    let order = Arc::new(AtomicUsize::new(0));

    let hashed = {
        let order = order.clone();
        async move {
            let hash = hasher.hash("pw").await.expect("hash");
            (hash, order.fetch_add(1, Ordering::SeqCst))
        }
    };
    let other = {
        let order = order.clone();
        async move {
            for _ in 0..3 {
                tokio::task::yield_now().await;
            }
            order.fetch_add(1, Ordering::SeqCst)
        }
    };

    let ((hash, hash_done), other_done) = tokio::join!(hashed, other);
    assert!(hash.starts_with("$argon2id$"));
    assert_eq!(
        other_done, 0,
        "the runtime thread was blocked while hashing"
    );
    assert_eq!(hash_done, 1);
}

#[test]
fn debug_shows_parameters() {
    let debug = format!("{:?}", build(fast()));
    assert!(debug.contains("1024"), "{debug}");
}

//! `CryptoModule`: `for_root`, `from_config`, global exports and `key:generate`.

use std::sync::Arc;

use navree_config::{ConfigModule, ConfigSources};
use navree_crypto::{
    AppKey, CryptoError, CryptoModule, CryptoOptions, Encrypter, Hasher, HashingOptions,
    generate_key,
};
use navree_kernel::{
    BootErrors, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, ProviderKey,
    Resolver,
};
use serde_json::json;

/// A provider in another module that injects the crypto services without
/// importing `CryptoModule` (it is global).
struct Vault {
    encrypter: Arc<Encrypter>,
    hasher: Arc<Hasher>,
}

impl Injectable for Vault {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Encrypter>(), ProviderKey::of::<Hasher>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Vault {
                encrypter: r.get::<Encrypter>().await?,
                hasher: r.get::<Hasher>().await?,
            })
        })
    }
}

struct VaultModule;

impl Module for VaultModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.provider::<Vault>().exports::<Vault>();
    }
}

/// A feature module that imports `CryptoModule` by type.
struct ImportsCrypto;

impl Module for ImportsCrypto {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(CryptoModule);
    }
}

fn fast_hashing() -> HashingOptions {
    HashingOptions {
        memory_kib: 1024,
        iterations: 1,
        parallelism: 1,
        max_concurrent_hashes: Some(2),
    }
}

async fn boot_with_config(sources: ConfigSources) -> navree_kernel::App {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(CryptoModule::from_config())
        .module(VaultModule)
        .boot()
        .await
        .expect("boot")
}

async fn boot_errors(sources: ConfigSources) -> BootErrors {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(CryptoModule::from_config())
        .boot()
        .await
        .expect_err("boot must fail")
}

#[tokio::test]
async fn for_root_exports_encrypter_and_hasher_globally() {
    let key = generate_key();
    let app = Navree::app()
        .module(CryptoModule::for_root(
            CryptoOptions::new(key.clone()).hashing(fast_hashing()),
        ))
        .module(VaultModule)
        .boot()
        .await
        .expect("boot");

    let vault = app.ctx().get::<Vault>().await.expect("vault");
    let payload = vault.encrypter.encrypt_string("hello").expect("encrypt");
    let standalone = Encrypter::new(AppKey::parse(&key).expect("key"));
    assert_eq!(
        standalone.decrypt_string(&payload).expect("decrypt"),
        "hello"
    );

    let hash = vault.hasher.hash("pw").await.expect("hash");
    assert!(hash.starts_with("$argon2id$v=19$m=1024,t=1,p=1$"), "{hash}");

    // The same singletons are visible through the context.
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert!(Arc::ptr_eq(&encrypter, &vault.encrypter));

    let report = Navree::app()
        .module(CryptoModule::for_root(CryptoOptions::new(key)))
        .validate()
        .expect("valid graph");
    let crypto = report
        .modules
        .iter()
        .find(|m| m.name.contains("CryptoModule"))
        .expect("crypto module in report");
    assert!(crypto.global);
}

#[tokio::test]
async fn for_root_options_rotate_keys() {
    let (old, new) = (generate_key(), generate_key());
    let old_payload = Encrypter::new(AppKey::parse(&old).expect("key"))
        .encrypt(b"old")
        .expect("encrypt");
    let app = Navree::app()
        .module(CryptoModule::for_root(
            CryptoOptions::new(new.clone()).previous_key(old),
        ))
        .boot()
        .await
        .expect("boot");
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert_eq!(encrypter.decrypt(&old_payload).expect("decrypt"), b"old");
    assert_eq!(
        encrypter.current_kid(),
        Some(AppKey::parse(&new).expect("key").kid())
    );
}

#[tokio::test]
async fn from_config_reads_app_key_and_previous_keys() {
    let (k0, k1, k2) = (generate_key(), generate_key(), generate_key());
    let payload_k0 = Encrypter::new(AppKey::parse(&k0).expect("k0"))
        .encrypt(b"zero")
        .expect("encrypt");
    let payload_k1 = Encrypter::new(AppKey::parse(&k1).expect("k1"))
        .encrypt(b"one")
        .expect("encrypt");

    let app = boot_with_config(
        ConfigSources::new()
            .set("app.key", k2.as_str())
            .set("app.previous_keys", json!([k1, k0])),
    )
    .await;
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert_eq!(
        encrypter.current_kid(),
        Some(AppKey::parse(&k2).expect("k2").kid())
    );
    assert_eq!(encrypter.decrypt(&payload_k0).expect("k0 payload"), b"zero");
    assert_eq!(encrypter.decrypt(&payload_k1).expect("k1 payload"), b"one");
}

#[tokio::test]
async fn from_config_reads_environment_variables() {
    let (k0, k1, k2) = (generate_key(), generate_key(), generate_key());
    let payload_k0 = Encrypter::new(AppKey::parse(&k0).expect("k0"))
        .encrypt(b"zero")
        .expect("encrypt");

    // `APP_APP__KEY` maps to `app.key`; previous keys are comma-separated.
    let app = boot_with_config(ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_APP__KEY", k2.as_str()),
            ("APP_APP__PREVIOUS_KEYS", &format!("{k1}, {k0}")),
        ],
    ))
    .await;
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert_eq!(
        encrypter.current_kid(),
        Some(AppKey::parse(&k2).expect("k2").kid())
    );
    assert_eq!(encrypter.decrypt(&payload_k0).expect("k0 payload"), b"zero");

    // Top-level `APP_KEY` / `APP_PREVIOUS_KEYS` (top-level `key`) work too.
    let app = boot_with_config(ConfigSources::new().env_vars(
        "APP_",
        [("APP_KEY", k1.as_str()), ("APP_PREVIOUS_KEYS", k0.as_str())],
    ))
    .await;
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert_eq!(
        encrypter.current_kid(),
        Some(AppKey::parse(&k1).expect("k1").kid())
    );
    assert_eq!(encrypter.decrypt(&payload_k0).expect("k0 payload"), b"zero");

    // `app.key` wins over the top-level `key`.
    let app = boot_with_config(ConfigSources::new().env_vars(
        "APP_",
        [("APP_KEY", k1.as_str()), ("APP_APP__KEY", k2.as_str())],
    ))
    .await;
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert_eq!(
        encrypter.current_kid(),
        Some(AppKey::parse(&k2).expect("k2").kid())
    );
}

#[tokio::test]
async fn plain_module_reads_config_like_from_config() {
    let key = generate_key();
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("app.key", key.as_str()),
        ))
        .module(CryptoModule)
        .boot()
        .await
        .expect("boot");
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert_eq!(
        encrypter.current_kid(),
        Some(AppKey::parse(&key).expect("key").kid())
    );
}

#[tokio::test]
async fn plain_import_reuses_the_configured_module() {
    // No ConfigModule: the plain import must reuse the `for_root` instance
    // rather than read configuration.
    let key = generate_key();
    let app = Navree::app()
        .module(CryptoModule::for_root(CryptoOptions::new(key.clone())))
        .module(ImportsCrypto)
        .boot()
        .await
        .expect("boot");
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert_eq!(
        encrypter.current_kid(),
        Some(AppKey::parse(&key).expect("key").kid())
    );
}

#[tokio::test]
async fn hashing_section_configures_the_hasher() {
    let app = boot_with_config(
        ConfigSources::new()
            .set("app.key", generate_key())
            .set("hashing", json!({"memory_kib": 1024, "iterations": 1}))
            .env_vars(
                "APP_",
                [
                    ("APP_HASHING__PARALLELISM", "2"),
                    ("APP_HASHING__MAX_CONCURRENT_HASHES", "3"),
                ],
            ),
    )
    .await;
    let hasher = app.ctx().get::<Hasher>().await.expect("hasher");
    let hash = hasher.hash("pw").await.expect("hash");
    assert!(hash.starts_with("$argon2id$v=19$m=1024,t=1,p=2$"), "{hash}");
    let debug = format!("{hasher:?}");
    assert!(debug.contains("max_concurrent_hashes: 3"), "{debug}");
}

#[tokio::test]
async fn invalid_key_fails_boot_without_echoing_it() {
    let short = "base64:c2hvcnQta2V5LXZhbHVl";
    let text = boot_errors(ConfigSources::new().set("app.key", short))
        .await
        .to_string();
    assert!(text.contains("app.key"), "{text}");
    assert!(text.contains("32 bytes"), "{text}");
    assert!(!text.contains("c2hvcnQta2V5LXZhbHVl"), "{text}");

    let errors = boot_errors(
        ConfigSources::new()
            .set("app.key", generate_key())
            .set("app.previous_keys", json!([generate_key(), short])),
    )
    .await;
    assert!(
        matches!(errors.0.as_slice(), [KernelError::Construction { provider, .. }] if provider.contains("Encrypter")),
        "{errors:?}"
    );
    let text = errors.to_string();
    assert!(text.contains("app.previous_keys"), "{text}");
    assert!(!text.contains("c2hvcnQta2V5LXZhbHVl"), "{text}");

    let errors = Navree::app()
        .module(CryptoModule::for_root(CryptoOptions::new(short)))
        .boot()
        .await
        .expect_err("invalid for_root key");
    assert!(errors.to_string().contains("32 bytes"), "{errors}");
}

#[tokio::test]
async fn invalid_hashing_options_fail_boot() {
    // Out of Argon2's range, and not a number: only the Hasher fails.
    for value in [json!(1), json!("lots")] {
        let errors = boot_errors(
            ConfigSources::new()
                .set("app.key", generate_key())
                .set("hashing.memory_kib", value.clone()),
        )
        .await;
        assert!(
            matches!(errors.0.as_slice(), [KernelError::Construction { provider, .. }] if provider.contains("Hasher")),
            "{value}: {errors:?}"
        );
        assert!(errors.to_string().contains("hashing"), "{errors}");
    }
}

#[tokio::test]
async fn missing_key_boots_but_encryption_fails() {
    let app = boot_with_config(ConfigSources::new()).await;
    let vault = app.ctx().get::<Vault>().await.expect("vault");
    assert_eq!(vault.encrypter.current_kid(), None);
    let error = vault.encrypter.encrypt(b"x").expect_err("no key");
    assert!(matches!(error, CryptoError::MissingKey), "{error:?}");
    assert!(error.to_string().contains("key:generate"), "{error}");
    assert!(matches!(
        vault.encrypter.derive_key("url-signing"),
        Err(CryptoError::MissingKey)
    ));
    // Hashing does not need the key.
    let hasher = Hasher::new(&fast_hashing()).expect("hasher");
    let hash = hasher.hash("pw").await.expect("hash");
    assert!(vault.hasher.verify("pw", &hash).await.expect("verify"));

    // An empty `APP_KEY=` line counts as missing, not as malformed.
    let app = boot_with_config(ConfigSources::new().env_vars("APP_", [("APP_KEY", "")])).await;
    let encrypter = app.ctx().get::<Encrypter>().await.expect("encrypter");
    assert_eq!(encrypter.current_kid(), None);
}

#[tokio::test]
async fn key_generate_command_is_registered_and_runs_without_a_key() {
    let report = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(CryptoModule)
        .validate()
        .expect("valid graph");
    let command = report
        .modules
        .iter()
        .flat_map(|m| m.commands.iter())
        .find(|c| c.name == "key:generate")
        .expect("key:generate registered");
    assert!(command.about.contains("key"), "{}", command.about);

    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(CryptoModule)
        .boot()
        .await
        .expect("boots without a key");
    app.run_command("key:generate", Vec::new())
        .await
        .expect("key:generate runs");
    app.shutdown().await.expect("shutdown");
}

#[test]
fn options_debug_redacts_keys() {
    let key = generate_key();
    let options = CryptoOptions::new(key.clone()).previous_key(generate_key());
    let debug = format!("{options:?}");
    assert!(!debug.contains(&key[7..]), "{debug}");
    assert!(debug.contains("redacted"), "{debug}");
}

#[test]
fn options_from_config_reads_keys_and_hashing() {
    let (k0, k1) = (generate_key(), generate_key());
    let config = navree_config::Config::load(
        &ConfigSources::new()
            .set("app.key", k1.as_str())
            .set("app.previous_keys", format!(" {k0} ,"))
            .set("hashing.iterations", 3),
    )
    .expect("config");
    let options = CryptoOptions::from_config(&config).expect("options");
    assert_eq!(
        options.key.as_ref().map(|k| k.expose().as_str()),
        Some(k1.as_str())
    );
    assert_eq!(
        options
            .previous_keys
            .iter()
            .map(|k| k.expose().as_str())
            .collect::<Vec<_>>(),
        [k0.as_str()]
    );
    assert_eq!(options.hashing.iterations, 3);
    assert_eq!(
        options.hashing.memory_kib,
        HashingOptions::default().memory_kib
    );

    let empty = CryptoOptions::from_config(
        &navree_config::Config::load(&ConfigSources::new()).expect("config"),
    )
    .expect("options");
    assert!(empty.key.is_none());
    assert!(empty.previous_keys.is_empty());
    assert_eq!(empty.hashing, HashingOptions::default());
}

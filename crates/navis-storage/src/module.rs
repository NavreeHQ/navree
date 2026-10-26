//! [`StorageModule`].

use std::sync::Arc;

use axum::extract::multipart::MultipartError;
use navis_config::{Config, ModuleBuilderConfigExt};
use navis_contracts::{BoxFuture, HealthCheck, HealthStatus};
use navis_crypto::{AppKey, Encrypter, random_token};
use navis_http::{AppError, ModuleBuilderHttpExt};
use navis_kernel::{BoxError, Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::serve::StorageController;
use crate::{Disk, Storage, StorageError, StorageOptions};

/// The purpose string temporary URL keys are derived for.
const SIGNING_PURPOSE: &str = "navis-storage/temporary-url/v1";

/// The global storage module: provides and exports [`Storage`] (every
/// configured disk) and [`Disk`] (the default disk), registers a `storage`
/// readiness check (`dyn HealthCheck`) and mounts the temporary URL route
/// `GET /storage/{disk}/{*path}` when the app has `HttpModule`.
///
/// ```no_run
/// use navis_kernel::Navis;
/// use navis_storage::{DiskConfig, StorageModule, StorageOptions};
///
/// # fn main() -> Result<(), navis_kernel::BoxError> {
/// let mut options = StorageOptions::default();
/// options
///     .disks
///     .insert("uploads".into(), DiskConfig::local("storage/uploads"));
/// Navis::app().module(StorageModule::for_root(options)).run()
/// # }
/// ```
///
/// [`from_config`](StorageModule::from_config), and the plain
/// `StorageModule` when no configured instance exists, read section
/// `storage` ([`StorageOptions`]) at boot, so the app must include
/// `navis_config::ConfigModule`. Local disk roots are created at boot; invalid
/// options fail the boot.
///
/// Temporary URLs are signed with a key derived from the app key
/// (`app.key` / `APP_KEY`, through `CryptoModule` when the app has it), so
/// they stay valid across restarts and instances. Without an app key a
/// random per-process key is used and links stop working on restart.
///
/// Handlers can propagate [`StorageError`]s with `?`: a missing file becomes
/// a 404 and an invalid path a 400. Multipart errors propagated with `?`
/// (`multipart.next_field().await?`) keep their client status (413 over
/// the body limit, 400 for a malformed body) instead of becoming a 500.
#[derive(Clone, Copy, Debug, Default)]
pub struct StorageModule;

impl StorageModule {
    /// The module configured with `options` (configure it once, in the app root).
    pub fn for_root(options: StorageOptions) -> impl Module {
        ConfiguredStorageModule {
            options: Some(options),
        }
    }

    /// The module configured from section `storage` of the app configuration.
    pub fn from_config() -> impl Module {
        ConfiguredStorageModule { options: None }
    }
}

impl Module for StorageModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `StorageModule::for_root(...)` / `from_config()`: same module id, configured.
struct ConfiguredStorageModule {
    /// `None`: read from configuration.
    options: Option<StorageOptions>,
}

impl Module for ConfiguredStorageModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<StorageModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<StorageOptions>) {
    m.global();
    match options {
        Some(options) => m.value(options),
        None => m.config::<StorageOptions>(),
    };
    m.factory(
        Factory::<Storage>::new(|r: Resolver| async move {
            let options = r.get::<StorageOptions>().await?;
            let key = signing_key(&r).await?;
            // Creating local roots touches the filesystem.
            let storage =
                tokio::task::spawn_blocking(move || Storage::new(&options, &key)).await??;
            Ok(Arc::new(storage))
        })
        .deps(vec![ProviderKey::of::<StorageOptions>()]),
    )
    .factory(
        Factory::<Disk>::new(|r: Resolver| async move {
            Ok(Arc::new(r.get::<Storage>().await?.default_disk()))
        })
        .deps(vec![ProviderKey::of::<Storage>()]),
    )
    .multi(
        Factory::<dyn HealthCheck>::new(|r: Resolver| async move {
            let storage = r.get::<Storage>().await?;
            Ok(Arc::new(StorageHealth { storage }) as Arc<dyn HealthCheck>)
        })
        .deps(vec![ProviderKey::of::<Storage>()]),
    )
    .exports::<Storage>()
    .exports::<Disk>()
    .controller::<StorageController>()
    .map_error::<StorageError>(storage_error)
    .map_error::<MultipartError>(crate::upload::client_error);
}

/// The temporary URL signing key: derived from the app key (the
/// `Encrypter`'s, or `app.key` / `key` from configuration), else random.
async fn signing_key(r: &Resolver) -> Result<Vec<u8>, BoxError> {
    let app = r.app();
    if let Some(encrypter) = app.try_get::<Encrypter>().await
        && let Ok(key) = encrypter.derive_key(SIGNING_PURPOSE)
    {
        return Ok(key.to_vec());
    }
    if let Some(config) = app.try_get::<Config>().await
        && let Some(raw) = ["app.key", "key"]
            .into_iter()
            .filter_map(|name| config.get_string(name))
            .find(|key| !key.trim().is_empty())
    {
        let key = AppKey::parse(&raw).map_err(|error| format!("`app.key`: {error}"))?;
        return Ok(Encrypter::new(key).derive_key(SIGNING_PURPOSE)?.to_vec());
    }
    tracing::warn!(
        target: "navis::storage",
        "no application key is configured (`app.key` / APP_KEY): temporary URLs use a random \
         key and stop working after a restart"
    );
    Ok(AppKey::generate().expose_bytes().to_vec())
}

/// Maps storage errors that reach a handler's `?`.
fn storage_error(error: &StorageError) -> Option<AppError> {
    match error {
        StorageError::NotFound { .. } => Some(AppError::not_found()),
        StorageError::InvalidPath { .. } => Some(AppError::bad_request("Invalid file path.")),
        _ => None,
    }
}

/// Readiness: every disk can write, read back and delete a small file.
struct StorageHealth {
    storage: Arc<Storage>,
}

impl StorageHealth {
    /// Writes, reads back and deletes a probe file; the error says what failed.
    async fn probe(disk: &Disk) -> Result<(), String> {
        let path = format!(".navis-health/{}", random_token(16));
        disk.put(&path, "ok")
            .await
            .map_err(|error| error.to_string())?;
        let read = disk.get(&path).await;
        disk.delete(&path)
            .await
            .map_err(|error| error.to_string())?;
        match read {
            Ok(bytes) if bytes == "ok" => Ok(()),
            Ok(_) => Err("the disk returned other contents than just written".to_owned()),
            Err(error) => Err(error.to_string()),
        }
    }
}

impl HealthCheck for StorageHealth {
    fn name(&self) -> &str {
        "storage"
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        Box::pin(async move {
            let mut down = Vec::new();
            for name in self.storage.disk_names() {
                let Ok(disk) = self.storage.disk(name) else {
                    continue;
                };
                if let Err(reason) = Self::probe(&disk).await {
                    down.push(format!("disk `{name}`: {reason}"));
                }
            }
            if down.is_empty() {
                HealthStatus::Up
            } else {
                HealthStatus::Down(down.join("; "))
            }
        })
    }
}

//! [`StorageModule`].

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::join_all;
use http::StatusCode;
use navree_config::__private::{build_driver, check_registrations, contribute_slot};
use navree_config::{Config, ModuleBuilderConfigExt};
use navree_contracts::{BoxFuture, HealthCheck, HealthStatus};
use navree_crypto::{AppKey, Encrypter};
use navree_http::{AppError, ModuleBuilderHttpExt};
use navree_kernel::{
    BoxError, ConfiguredModule, Factory, Module, ModuleBuilder, ProviderKey, Resolver,
};

use crate::options::SLOT;
use crate::serve::StorageController;
use crate::{Disk, DiskConfig, DiskDriver, Storage, StorageError, StorageOptions, UploadRejection};

/// The purpose string temporary URL keys are derived for.
const SIGNING_PURPOSE: &str = "navree-storage/temporary-url/v1";

/// The global storage module: provides and exports [`Storage`] (every
/// configured disk) and [`Disk`] (the default disk), registers a `storage`
/// readiness check (`dyn HealthCheck`, probing every disk) and the
/// `storage:sync` command, and mounts the routes of the app's own signed
/// links for local and memory disks, `GET /storage/{disk}/{*path}` and
/// `PUT /storage/{disk}/{*path}`, when the app has `HttpModule`.
///
/// ```no_run
/// use navree_kernel::Navree;
/// use navree_storage::{DiskConfig, StorageModule, StorageOptions};
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// let mut options = StorageOptions::default();
/// options
///     .disks
///     .insert("uploads".into(), DiskConfig::local("storage/uploads"));
/// Navree::app().module(StorageModule::for_root(options)).run()
/// # }
/// ```
///
/// [`from_config`](StorageModule::from_config), and the plain
/// `StorageModule` when no configured instance exists, read section
/// `storage` ([`StorageOptions`]) at boot, so the app must include
/// `navree_config::ConfigModule`. Local disk roots are created at boot; invalid
/// options fail the boot. A disk whose `driver` a module registers
/// ([`ModuleBuilderStorageExt`](crate::ModuleBuilderStorageExt)) is built at
/// boot by that driver's factory, and a disk naming a driver no module
/// registers fails the boot.
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
        ConfiguredModule::<StorageModule, _>::for_root(options, register)
    }

    /// The module configured from section `storage` of the app configuration.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<StorageModule, StorageOptions>::from_config(register)
    }
}

impl Module for StorageModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<StorageOptions>) {
    m.global();
    contribute_slot(m, SLOT);
    match options {
        Some(options) => m.value(options),
        None => m.config::<StorageOptions>(),
    };
    m.factory(
        Factory::<Storage>::new(|r: Resolver| async move {
            let options = r.get::<StorageOptions>().await?;
            let key = signing_key(&r).await?;
            let registered = registered_drivers(&r, &options).await?;
            // Creating local roots touches the filesystem.
            let storage = tokio::task::spawn_blocking(move || {
                Storage::with_drivers(&options, &key, registered)
            })
            .await??;
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
    .command(crate::sync::command());
}

/// The temporary URL signing key: derived from the app key (the
/// `Encrypter`'s, or `app.key` from configuration), else random.
async fn signing_key(r: &Resolver) -> Result<Vec<u8>, BoxError> {
    let app = r.app();
    if let Some(encrypter) = app.try_get::<Encrypter>().await
        && let Ok(key) = encrypter.derive_key(SIGNING_PURPOSE)
    {
        return Ok(key.to_vec());
    }
    if let Some(config) = app.try_get::<Config>().await
        && let Some(raw) = config.app_key()
    {
        let key = AppKey::parse(raw.expose()).map_err(|error| format!("`app.key`: {error}"))?;
        return Ok(Encrypter::new(key).derive_key(SIGNING_PURPOSE)?.to_vec());
    }
    tracing::warn!(
        target: "navree::storage",
        "no application key is configured (`app.key` / APP_KEY): temporary URLs use a random \
         key and stop working after a restart"
    );
    Ok(AppKey::generate().expose_bytes().to_vec())
}

/// Checks every registration of a disk driver, whichever disks use one,
/// then builds the driver of each disk whose `driver` a module registers,
/// by disk name: its settings are the disk's table the module runs with
/// (from the configuration or from `for_root`), and a driver no module
/// registers fails the boot naming `m.disk_driver`.
async fn registered_drivers(
    r: &Resolver,
    options: &StorageOptions,
) -> Result<BTreeMap<String, Arc<dyn DiskDriver>>, BoxError> {
    let app = r.app();
    check_registrations(&app, &SLOT, None)?;
    let mut registered = BTreeMap::new();
    for (disk, config) in &options.disks {
        let DiskConfig::Registered(table) = config else {
            continue;
        };
        let driver = build_driver::<dyn DiskDriver>(
            &app,
            &SLOT,
            &format!("storage.disks.{disk}.driver"),
            table.driver(),
            format!("storage.disks.{disk}"),
            Some(disk.clone()),
            Some(table.table()),
        )
        .await?;
        registered.insert(disk.clone(), driver);
    }
    Ok(registered)
}

/// Maps storage errors that reach a handler's `?`.
fn storage_error(error: &StorageError) -> Option<AppError> {
    match error {
        StorageError::NotFound { .. } => Some(AppError::not_found()),
        StorageError::InvalidPath { .. } => {
            Some(AppError::bad_request("Invalid file path.").with_code("invalid_file_path"))
        }
        StorageError::Rejected { rejection, .. } => Some(
            match rejection {
                UploadRejection::TooLarge { .. } => {
                    AppError::from_status(StatusCode::PAYLOAD_TOO_LARGE)
                        .with_detail(sentence(rejection))
                }
                _ => AppError::bad_request(sentence(rejection)),
            }
            .with_code(rejection.code()),
        ),
        _ => None,
    }
}

/// A rejection as a response message: `The file must ...`.
fn sentence(rejection: &UploadRejection) -> String {
    let message = rejection.to_string();
    let mut chars = message.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => message,
    }
}

/// How long one disk's probe may take; below the readiness report's own
/// limit, so a slow disk is named instead of failing the whole check.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1_500);

/// Readiness: every disk can write, read back and delete a small file. The
/// disks are probed at the same time, each with its own time limit.
struct StorageHealth {
    storage: Arc<Storage>,
}

impl HealthCheck for StorageHealth {
    fn name(&self) -> &str {
        "storage"
    }

    fn check(&self) -> BoxFuture<'_, HealthStatus> {
        Box::pin(async move {
            let probes = self.storage.disk_names().filter_map(|name| {
                let disk = self.storage.disk(name).ok()?;
                Some(async move {
                    let status = tokio::time::timeout(PROBE_TIMEOUT, disk.check_health())
                        .await
                        .unwrap_or_else(|_| {
                            HealthStatus::Down(format!(
                                "timed out after {} ms",
                                PROBE_TIMEOUT.as_millis()
                            ))
                        });
                    (disk.name().to_owned(), status)
                })
            });
            let down: Vec<String> = join_all(probes)
                .await
                .into_iter()
                .filter_map(|(name, status)| match status {
                    HealthStatus::Up => None,
                    HealthStatus::Down(reason) => Some(format!("disk `{name}`: {reason}")),
                })
                .collect();
            if down.is_empty() {
                HealthStatus::Up
            } else {
                HealthStatus::Down(down.join("; "))
            }
        })
    }
}

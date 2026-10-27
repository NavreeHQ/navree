//! [`MediaModule`] and [`ModuleBuilderMediaExt`].

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderJobsExt;
use navree_db::sea_orm::{DatabaseTransaction, DbErr};
use navree_db::{ChangeEvent, ChangeObserver, ChangeOp, Db, ModuleBuilderDbExt, Uow};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ModuleId, ProviderKey,
    Resolver, WeakAppContext,
};
use navree_storage::Storage;

use crate::collections::HasMedia;
use crate::commands::commands;
use crate::error::{map_archive_error, map_media_error};
use crate::jobs::GenerateConversions;
use crate::migration::migrations;
use crate::options::StoreKind;
use crate::service::{Registered, registration};
use crate::store::{DatabaseStore, MediaStore, MemoryStore};
#[cfg(feature = "ffmpeg")]
use crate::video::FfmpegRunner;
use crate::{ArchiveError, Media, MediaError, MediaOptions};

/// Media: files attached to models, with conversions.
///
/// Global; provides and exports [`Media`] and [`MediaOptions`], and
/// contributes:
///
/// - the `media` migration set (the `media` table);
/// - the job `media.conversions` ([`GenerateConversions`]);
/// - the command `media:regenerate`;
/// - a change observer that deletes a model's media when the model is
///   deleted (models registered with [`for_feature`](MediaModule::for_feature)
///   that name a `MEDIA_TABLE`, while `media.delete_with_model` is on);
/// - the mapping of [`MediaError`]s and [`ArchiveError`]s returned from
///   handlers to error responses (4xx with their own codes for refused
///   files, 500 otherwise).
///
/// The app must include `ConfigModule` (for
/// [`from_config`](MediaModule::from_config) and the plain module) and
/// `StorageModule` (the disks), and `DatabaseModule` unless `media.store`
/// is `memory`. With `QueueModule`, conversions are generated on the
/// queue; without it, while the file is added. With `HttpClientModule`,
/// remote files are downloaded through the app's client (a client with
/// the default SSRF policy otherwise). With the `ffmpeg` feature, an
/// `Arc<dyn FfmpegRunner>` in the app (an `FfmpegFake` in tests) runs the
/// tools instead of child processes.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_kernel::Navree;
/// use navree_media::MediaModule;
/// use navree_queue::QueueModule;
/// use navree_storage::StorageModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(DatabaseModule::from_config())
///     .module(StorageModule::from_config())
///     .module(QueueModule::from_config())
///     .module(MediaModule::from_config()) // section `media`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct MediaModule;

impl MediaModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: MediaOptions) -> impl Module {
        ConfiguredMediaModule {
            options: Some(options),
        }
    }

    /// The module configured from the `media` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredMediaModule { options: None }
    }

    /// Registers model `M`'s collections (import it in the feature module
    /// that owns the model): its collection and conversion names are
    /// checked at boot, `media:regenerate` knows its conversions before a
    /// file is added in the process, and, when `M` names a `MEDIA_TABLE`,
    /// deleting one of its rows deletes its media.
    pub fn for_feature<M: HasMedia + 'static>() -> impl Module {
        MediaFeature::<M>(PhantomData)
    }
}

impl Module for MediaModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `MediaModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredMediaModule {
    /// `None`: read from configuration.
    options: Option<MediaOptions>,
}

impl Module for ConfiguredMediaModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<MediaModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

/// A model type registered with [`MediaModule::for_feature`].
#[derive(Clone, Debug)]
pub(crate) struct MediaRegistration {
    pub(crate) model_type: String,
    pub(crate) registered: Registered,
}

fn register(m: &mut ModuleBuilder, options: Option<MediaOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<MediaOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|error| format!("invalid media options: {error}"))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<MediaOptions>(),
    };
    m.exports::<MediaOptions>()
        .factory(
            Factory::<Media>::new(|r: Resolver| async move {
                let options = r.get::<MediaOptions>().await?;
                let storage = r.get::<Storage>().await?;
                let app = r.app();
                let store: Arc<dyn MediaStore> = match options.store {
                    StoreKind::Database => {
                        let db = app.try_get::<Db>().await.ok_or(
                            "media.store = \"database\" needs DatabaseModule (or set \
                             media.store = \"memory\")",
                        )?;
                        Arc::new(DatabaseStore::new((*db).clone()))
                    }
                    StoreKind::Memory => Arc::new(MemoryStore::new()),
                };
                for disk in [&options.disk, &options.conversions_disk]
                    .into_iter()
                    .flatten()
                {
                    storage.disk(disk).map_err(|_| {
                        format!("media: `{disk}` is not a configured storage disk (storage.disks)")
                    })?;
                }
                let mut builder = Media::builder(storage)
                    .options((*options).clone())
                    .store(store)
                    .app(app.downgrade());
                #[cfg(feature = "ffmpeg")]
                if let Some(runner) = app.try_get::<dyn FfmpegRunner>().await {
                    builder = builder.ffmpeg_runner(runner);
                }
                for contributed in app.contributions::<MediaRegistration>() {
                    let registration = contributed.value.as_ref().clone();
                    builder = builder.registered(registration.model_type, registration.registered);
                }
                Ok(Arc::new(builder.build()?))
            })
            .deps(vec![
                ProviderKey::of::<MediaOptions>(),
                ProviderKey::of::<Storage>(),
            ]),
        )
        .exports::<Media>()
        .migrations(migrations())
        .change_observer::<DeleteWithModel>()
        .job::<GenerateConversions>()
        .map_error::<MediaError>(map_media_error)
        .map_error::<ArchiveError>(map_archive_error);
    for command in commands() {
        m.command(command);
    }
}

/// `MediaModule::for_feature::<M>()`.
struct MediaFeature<M>(PhantomData<fn() -> M>);

impl<M: HasMedia + 'static> Module for MediaFeature<M> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.media_model::<M>();
    }
}

/// Media registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderMediaExt {
    /// Registers model `M`'s collections from this module (what
    /// [`MediaModule::for_feature`] does, for a module's `register`).
    fn media_model<M: HasMedia + 'static>(&mut self) -> &mut Self;
}

impl ModuleBuilderMediaExt for ModuleBuilder {
    fn media_model<M: HasMedia + 'static>(&mut self) -> &mut Self {
        let (model_type, registered) = registration::<M>();
        self.contribute(MediaRegistration {
            model_type,
            registered,
        })
    }
}

/// Deletes a registered model's media when the model's row is deleted:
/// the rows in the same transaction, the files after the commit.
struct DeleteWithModel {
    app: WeakAppContext,
    /// Table → the model types stored in it.
    tables: HashMap<String, Vec<String>>,
}

impl Injectable for DeleteWithModel {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            let mut tables: HashMap<String, Vec<String>> = HashMap::new();
            for contributed in app.contributions::<MediaRegistration>() {
                let registration = &contributed.value;
                if !registration.registered.table.is_empty() {
                    let types = tables
                        .entry(registration.registered.table.to_owned())
                        .or_default();
                    if !types.contains(&registration.model_type) {
                        types.push(registration.model_type.clone());
                    }
                }
            }
            Ok(DeleteWithModel {
                app: app.downgrade(),
                tables,
            })
        })
    }
}

impl ChangeObserver for DeleteWithModel {
    fn observe<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move {
            if self.tables.is_empty() {
                return Ok(());
            }
            let owners: Vec<(&str, &str)> = changes
                .iter()
                .filter(|change| change.op == ChangeOp::Deleted)
                .filter_map(|change| {
                    self.tables
                        .get(&change.entity)
                        .map(|types| (types, change.id.as_str()))
                })
                .flat_map(|(types, id)| types.iter().map(move |t| (t.as_str(), id)))
                .collect();
            if owners.is_empty() {
                return Ok(());
            }
            let Some(app) = self.app.upgrade() else {
                return Ok(());
            };
            let Some(media) = app.try_get::<Media>().await else {
                return Ok(());
            };
            if !media.options().delete_with_model {
                return Ok(());
            }
            media
                .delete_owners_in(tx, uow, &owners)
                .await
                .map_err(|error| match error {
                    MediaError::Db(db) => db,
                    other => DbErr::Custom(other.to_string()),
                })
        })
    }
}

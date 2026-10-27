//! [`SheetsModule`] and [`ModuleBuilderSheetsExt`].

use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderJobsExt;
use navree_db::ModuleBuilderDbExt;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{Factory, Injectable, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::commands::commands;
use crate::error::map_sheets_error;
use crate::export::controller::ExportsController;
use crate::export::job::QueuedExport;
use crate::export::registry::ExportRegistration;
use crate::import::controller::ImportsController;
use crate::import::jobs::{ApplyImportChunk, FinishImport, StageImport};
use crate::import::registry::ImporterRegistration;
use crate::migration::migrations;
use crate::{
    Export, Exports, HeaderCatalog, HeaderTranslator, Import, Imports, SheetsError, SheetsOptions,
};

/// CSV and Excel exports and imports.
///
/// Global; provides and exports [`Exports`], [`Imports`] and
/// [`SheetsOptions`], and contributes:
///
/// - the `sheets` migrations (`imports`, `import_rows`, `import_mappings`);
/// - the jobs `sheets.export` ([`QueuedExport`]), `sheets.import.stage`
///   ([`StageImport`]), `sheets.import.apply` ([`ApplyImportChunk`]) and
///   `sheets.import.finish` ([`FinishImport`]);
/// - the routes under `{http.api_prefix}/exports` and
///   `{http.api_prefix}/imports` when `sheets.exports.routes` and
///   `sheets.imports.routes` are true and the app has an authenticator
///   ([`ExportsController`](crate::ExportsController),
///   [`ImportsController`](crate::ImportsController));
/// - the command `sheets:prune`;
/// - the mapping of [`SheetsError`]s returned from handlers to error
///   responses.
///
/// Downloads ([`Download`](crate::Download), [`Exports::csv`],
/// [`Exports::xlsx`]) need only this module. Queued exports and imports
/// need `DatabaseModule`, `QueueModule` and `StorageModule`.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_queue::QueueModule;
/// use navree_sheets::SheetsModule;
/// use navree_storage::StorageModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(QueueModule::from_config())
///     .module(StorageModule::from_config())
///     .module(SheetsModule::from_config()) // section `sheets`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SheetsModule;

impl SheetsModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: SheetsOptions) -> impl Module {
        ConfiguredSheetsModule {
            options: Some(options),
        }
    }

    /// The module configured from the `sheets` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredSheetsModule { options: None }
    }
}

impl Module for SheetsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `SheetsModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredSheetsModule {
    /// `None`: read from configuration.
    options: Option<SheetsOptions>,
}

impl Module for ConfiguredSheetsModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<SheetsModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<SheetsOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<SheetsOptions>::new(move |_| {
            let options = options.clone();
            async move {
                navree_config::ConfigSection::validate(&options)
                    .map_err(|message| format!("invalid sheets options: {message}"))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<SheetsOptions>(),
    };
    m.exports::<SheetsOptions>()
        .factory(
            Factory::<Exports>::new(|r: Resolver| async move {
                let options = r.get::<SheetsOptions>().await?;
                Ok(Arc::new(Exports::new(&r.app(), options).await?))
            })
            .deps(vec![ProviderKey::of::<SheetsOptions>()]),
        )
        .exports::<Exports>()
        .factory(
            Factory::<Imports>::new(|r: Resolver| async move {
                let options = r.get::<SheetsOptions>().await?;
                Ok(Arc::new(Imports::new(&r.app(), options)?))
            })
            .deps(vec![ProviderKey::of::<SheetsOptions>()]),
        )
        .exports::<Imports>()
        .job::<QueuedExport>()
        .job::<StageImport>()
        .job::<ApplyImportChunk>()
        .job::<FinishImport>()
        .controller::<ExportsController>()
        .controller::<ImportsController>()
        .migrations(migrations())
        .map_error::<SheetsError>(map_sheets_error);
    for command in commands() {
        m.command(command);
    }
}

/// Sheets registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderSheetsExt {
    /// Registers export `E` as a provider of this module and makes it
    /// available by name: [`Exports::download`], [`Exports::queue`] and
    /// `GET`/`POST /api/exports/{E::NAME}`.
    fn export<E: Export + Injectable>(&mut self) -> &mut Self;

    /// Registers importer `I` as a provider of this module and makes it
    /// available by name: [`Imports::upload`] and `POST /api/imports`
    /// with `importer = I::NAME`.
    fn importer<I: Import + Injectable>(&mut self) -> &mut Self;

    /// Translates export headers with `catalog` (a `dyn HeaderTranslator`
    /// for the app; register it once).
    fn header_catalog(&mut self, catalog: HeaderCatalog) -> &mut Self;
}

impl ModuleBuilderSheetsExt for ModuleBuilder {
    fn export<E: Export + Injectable>(&mut self) -> &mut Self {
        let module = self.module_id();
        self.provider::<E>()
            .contribute(ExportRegistration::of::<E>(module))
    }

    fn importer<I: Import + Injectable>(&mut self) -> &mut Self {
        let module = self.module_id();
        self.provider::<I>()
            .contribute(ImporterRegistration::of::<I>(module))
    }

    fn header_catalog(&mut self, catalog: HeaderCatalog) -> &mut Self {
        let catalog: Arc<dyn HeaderTranslator> = Arc::new(catalog);
        self.factory(Factory::<dyn HeaderTranslator>::new(move |_| {
            let catalog = catalog.clone();
            async move { Ok(catalog) }
        }))
        .exports::<dyn HeaderTranslator>()
    }
}

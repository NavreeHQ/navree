//! [`DocumentsModule`] and [`ModuleBuilderDocumentsExt`].

use std::sync::Arc;

use bytes::Bytes;
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::{Clock, Dispatcher, ModuleBuilderJobsExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Factory, Lifecycle, Module, ModuleBuilder, ModuleId,
    ProviderKey, Resolver,
};
use navree_storage::Storage;

use crate::commands::commands;
use crate::error::map_documents_error;
use crate::templates::normalize;
use crate::{Documents, DocumentsError, DocumentsOptions, RenderDocument, Templates};

/// Documents: PDF from Typst or HTML templates, DOCX, merge and stamps.
///
/// Global; provides and exports [`Documents`] and [`DocumentsOptions`],
/// and contributes:
///
/// - the job `documents.render` ([`RenderDocument`]), which
///   [`Documents::queue`] dispatches;
/// - the commands `documents:render` and `documents:fonts`;
/// - the mapping of [`DocumentsError`]s returned from handlers to error
///   responses (500, or 503 when an engine is unavailable or timed out);
/// - closing Chrome when the app stops.
///
/// The app must include `ConfigModule` (for
/// [`from_config`](DocumentsModule::from_config) and the plain module).
/// With `StorageModule` in the app, documents can be saved and queued to
/// its disks; with `QueueModule`, queued renders run on a worker; with a
/// `dyn Clock`, Typst's `datetime.today()` and the PDF's creation date
/// follow it. Nothing starts at boot: fonts load and Chrome starts on the
/// first render that needs them.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_documents::DocumentsModule;
/// use navree_kernel::Navree;
/// use navree_storage::StorageModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(StorageModule::from_config())
///     .module(DocumentsModule::from_config()) // section `documents`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct DocumentsModule;

impl DocumentsModule {
    /// The module with explicit options (configure it once, in the app
    /// root).
    pub fn for_root(options: DocumentsOptions) -> impl Module {
        ConfiguredDocumentsModule {
            options: Some(options),
        }
    }

    /// The module configured from the `documents` section of `Config` at
    /// boot.
    pub fn from_config() -> impl Module {
        ConfiguredDocumentsModule { options: None }
    }
}

impl Module for DocumentsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `DocumentsModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredDocumentsModule {
    /// `None`: read from configuration.
    options: Option<DocumentsOptions>,
}

impl Module for ConfiguredDocumentsModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<DocumentsModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

/// Template files a module embeds (see
/// [`ModuleBuilderDocumentsExt::document_files`]).
#[derive(Clone, Debug)]
pub(crate) struct DocumentFiles(pub(crate) Vec<(String, Bytes)>);

/// Closes Chrome when the app stops.
struct CloseEngines(Arc<Documents>);

impl Lifecycle for CloseEngines {
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.0.shutdown().await;
            Ok(())
        })
    }
}

fn register(m: &mut ModuleBuilder, options: Option<DocumentsOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<DocumentsOptions>::new(move |_| {
            let options = options.clone();
            async move { Ok(Arc::new(options)) }
        })),
        None => m.config::<DocumentsOptions>(),
    };
    m.exports::<DocumentsOptions>()
        .factory(
            Factory::<Documents>::new(|r: Resolver| async move {
                let options = r.get::<DocumentsOptions>().await?;
                let app = r.app();
                let embedded = app
                    .contributions::<DocumentFiles>()
                    .into_iter()
                    .flat_map(|contributed| contributed.value.0.clone());
                let templates = Templates::new(options.templates.clone(), embedded);
                let clock = app.try_get::<dyn Clock>().await;
                let mut documents = Documents::build(&options, templates, clock);
                if let Some(storage) = app.try_get::<Storage>().await {
                    if let Some(disk) = &options.disk {
                        storage.disk(disk).map_err(|_| {
                            format!(
                                "documents.disk = \"{disk}\" is not a configured storage disk \
                                 (storage.disks)"
                            )
                        })?;
                    }
                    documents = documents.storage(storage, None);
                }
                if let Some(dispatcher) = app.try_get::<dyn Dispatcher>().await {
                    documents = documents.dispatcher(dispatcher);
                }
                Ok(Arc::new(documents))
            })
            .deps(vec![ProviderKey::of::<DocumentsOptions>()]),
        )
        .exports::<Documents>()
        .factory(
            Factory::<CloseEngines>::new(|r: Resolver| async move {
                Ok(Arc::new(CloseEngines(r.get::<Documents>().await?)))
            })
            .deps(vec![ProviderKey::of::<Documents>()]),
        )
        .lifecycle::<CloseEngines>()
        .job::<RenderDocument>()
        .map_error::<DocumentsError>(map_documents_error);
    for command in commands() {
        m.command(command);
    }
}

/// Document registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderDocumentsExt {
    /// Embeds template files (templates, layouts, images, fonts' CSS) in
    /// the app, as `(path, contents)` pairs: `certificates/classic.typ`,
    /// `images/seal.png`. A file at the same path in a template directory
    /// wins, so an app can override what a module ships. Paths that would
    /// leave the template tree are ignored.
    ///
    /// ```ignore
    /// m.document_files([
    ///     ("certificates/course.typ", include_bytes!("../documents/certificates/course.typ").as_slice()),
    ///     ("images/seal.png", include_bytes!("../documents/images/seal.png").as_slice()),
    /// ]);
    /// ```
    fn document_files<I, P, B>(&mut self, files: I) -> &mut Self
    where
        I: IntoIterator<Item = (P, B)>,
        P: Into<String>,
        B: Into<Bytes>;
}

impl ModuleBuilderDocumentsExt for ModuleBuilder {
    fn document_files<I, P, B>(&mut self, files: I) -> &mut Self
    where
        I: IntoIterator<Item = (P, B)>,
        P: Into<String>,
        B: Into<Bytes>,
    {
        let files = files
            .into_iter()
            .filter_map(|(path, contents)| {
                let path = path.into();
                match normalize(&path) {
                    Some(path) => Some((path, contents.into())),
                    None => {
                        tracing::warn!(target: "navree_documents", path, "ignored an embedded document file with an invalid path");
                        None
                    }
                }
            })
            .collect();
        self.contribute(DocumentFiles(files))
    }
}

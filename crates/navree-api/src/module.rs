//! [`ApiModule`] and the [`ApiDocs`] provider.

use std::path::PathBuf;
use std::sync::Arc;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_contracts::ModuleBuilderConformityExt;
use navree_http::body::Bytes;
use navree_http::{HttpModule, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Injectable, KernelError, Lifecycle, Module,
    ModuleBuilder, ModuleId, ProviderKey, Resolver, WeakAppContext,
};
use navree_openapi::Document;
use serde_json::Value;
use tokio::sync::OnceCell;

use crate::conformity::{docs_check, documented_routes, require_docs_error, undocumented};
use crate::docs::DocsController;
use crate::document::build;
use crate::ext::ApiSchema;
use crate::{ApiOptions, DEFAULT_SESSION_COOKIE};

/// The API module: builds the app's OpenAPI 3.1 document at boot and
/// serves it.
///
/// - Imports `HttpModule` (a configured `HttpModule::for_root` in the app
///   wins; otherwise it reads the `http` section, so the app needs
///   `ConfigModule`).
/// - Provides and exports [`ApiOptions`] and [`ApiDocs`]; the document is
///   built when the app boots, so a schema conflict or a missing schema
///   stops the boot (see [`openapi_document`](crate::openapi_document)).
/// - Routes (public, under the API prefix, left out of the document):
///   with [`openapi_json`](ApiOptions::openapi_json),
///   `GET {api_prefix}/openapi.json`, and with it and
///   [`docs_ui`](ApiOptions::docs_ui), the API reference page at
///   `GET {api_prefix}/docs`. Unset, both are off in the `production`
///   environment and on elsewhere (see
///   [`serves_openapi_json_in`](ApiOptions::serves_openapi_json_in)).
/// - Command `openapi:export [path]`: writes the document (default
///   `openapi.json`), creating missing directories.
/// - The conformity check `api.docs` (`navree check`): every route has
///   `#[api]` documentation, unless [`undocumented`](ApiOptions::undocumented)
///   allows it. With [`require_docs`](ApiOptions::require_docs), an
///   undocumented route also stops the boot.
///
/// ```no_run
/// use navree_api::ApiModule;
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// // `api` section: title, version, servers, docs_ui
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(ApiModule::from_config())
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(ApiModule)`), it reads the `api` section like
/// [`from_config`](ApiModule::from_config).
#[derive(Clone, Copy, Debug, Default)]
pub struct ApiModule;

impl ApiModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: ApiOptions) -> impl Module {
        ConfiguredApiModule {
            options: Some(options),
        }
    }

    /// The module configured from the `api` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredApiModule { options: None }
    }
}

impl Module for ApiModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `ApiModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredApiModule {
    options: Option<ApiOptions>,
}

impl Module for ConfiguredApiModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<ApiModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<ApiOptions>) {
    m.import(HttpModule);
    match options {
        Some(options) => m.value(options),
        None => m.config::<ApiOptions>(),
    };
    m.exports::<ApiOptions>()
        .provider::<ApiDocs>()
        .exports::<ApiDocs>()
        .lifecycle::<ApiDocs>()
        .controller::<DocsController>()
        .command(CommandSpec::new(
            "openapi:export",
            "Write the OpenAPI document to a file ([path], default openapi.json)",
            export,
        ))
        .conformity_check(docs_check());
}

/// `openapi:export [path]`: writes the pretty-printed document, creating
/// missing parent directories.
async fn export(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let path = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map_or_else(|| PathBuf::from("openapi.json"), PathBuf::from);
    let docs = ctx.get::<ApiDocs>().await?;
    let json = docs.json().await?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    tokio::fs::write(&path, &json)
        .await
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    println!("OpenAPI document written to {}", path.display());
    Ok(())
}

/// The app's OpenAPI document, built once from the route table (when the
/// app boots) and cached with its JSON form.
///
/// The security scheme `cookie` names the session cookie from
/// `session.cookie` in the configuration, else
/// [`DEFAULT_SESSION_COOKIE`]; module registrations made with
/// [`api_schema`](crate::ModuleBuilderApiExt::api_schema) are component
/// schemas.
pub struct ApiDocs {
    options: Arc<ApiOptions>,
    /// The app's environment (`env` / `app.env`), if it names one.
    environment: Option<String>,
    app: WeakAppContext,
    built: OnceCell<Built>,
}

/// The document and its serialized forms.
struct Built {
    document: Arc<Value>,
    json: Bytes,
    model: Document,
}

impl ApiDocs {
    /// The document and docs UI options.
    pub fn options(&self) -> &ApiOptions {
        &self.options
    }

    /// Whether this app serves `{api_prefix}/openapi.json`
    /// ([`ApiOptions::serves_openapi_json_in`] its environment).
    pub fn serves_openapi_json(&self) -> bool {
        self.options
            .serves_openapi_json_in(self.environment.as_deref())
    }

    /// Whether this app serves the reference page at `{api_prefix}/docs`
    /// ([`ApiOptions::serves_docs_ui_in`] its environment).
    pub fn serves_docs_ui(&self) -> bool {
        self.options.serves_docs_ui_in(self.environment.as_deref())
    }

    /// The OpenAPI document.
    ///
    /// # Errors
    ///
    /// When the document cannot be built: the route table cannot be read
    /// (e.g. `HttpModule` is missing) or an [`ApiError`](crate::ApiError).
    /// The app builds it at boot, so after a successful boot this does not
    /// fail.
    pub async fn document(&self) -> Result<Arc<Value>, BoxError> {
        Ok(self.built().await?.document.clone())
    }

    /// The document, in its model's form.
    ///
    /// # Errors
    ///
    /// As [`document`](ApiDocs::document).
    pub async fn model(&self) -> Result<&Document, BoxError> {
        Ok(&self.built().await?.model)
    }

    /// The document as pretty-printed JSON, with a trailing newline (what
    /// `openapi.json` serves and `openapi:export` writes).
    ///
    /// # Errors
    ///
    /// As [`document`](ApiDocs::document).
    pub async fn json(&self) -> Result<Bytes, BoxError> {
        Ok(self.built().await?.json.clone())
    }

    async fn built(&self) -> Result<&Built, BoxError> {
        self.built
            .get_or_try_init(|| async {
                let ctx = self
                    .app
                    .upgrade()
                    .ok_or("the app was dropped; its OpenAPI document cannot be built")?;
                let routes = documented_routes(&ctx).await?;
                if self.options.require_docs {
                    let missing = undocumented(&routes, &self.options);
                    if !missing.is_empty() {
                        return Err(require_docs_error(&missing).into());
                    }
                }
                let extra: Vec<_> = ctx
                    .contributions::<ApiSchema>()
                    .into_iter()
                    .map(|c| (c.value.0, format!("module `{}`", c.module.name)))
                    .collect();
                let cookie = ctx
                    .try_get::<Config>()
                    .await
                    .and_then(|config| config.get_string("session.cookie"))
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_SESSION_COOKIE.to_owned());
                let document = build(&routes, &extra, &self.options, &cookie)?;
                let value = serde_json::to_value(&document)?;
                let mut json = serde_json::to_vec_pretty(&value)?;
                json.push(b'\n');
                Ok::<_, BoxError>(Built {
                    document: Arc::new(value),
                    json: Bytes::from(json),
                    model: document,
                })
            })
            .await
    }
}

impl Injectable for ApiDocs {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<ApiOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let environment = r
                .app()
                .try_get::<Config>()
                .await
                .and_then(|config| environment(&config));
            Ok(Self {
                options: r.get::<ApiOptions>().await?,
                environment,
                app: r.app().downgrade(),
                built: OnceCell::new(),
            })
        })
    }
}

/// The app's environment: top-level `env` (`APP_ENV`), else `app.env`.
fn environment(config: &Config) -> Option<String> {
    config
        .get_string("env")
        .or_else(|| config.get_string("app.env"))
}

/// Builds the document at boot, so a schema conflict stops the app.
impl Lifecycle for ApiDocs {
    fn on_bootstrap(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move { self.built().await.map(drop) })
    }
}

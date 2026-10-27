//! [`GraphqlModule`] and [`ModuleBuilderGraphqlExt`].

use std::sync::Arc;

use navree_config::ModuleBuilderConfigExt;
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{Factory, Injectable, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::command::schema_command;
use crate::endpoint::GraphqlController;
use crate::loader::LoaderRegistration;
use crate::options::GraphqlOptions;
use crate::schema::{GraphqlResolver, GraphqlSchema, ResolverRegistration};

/// A GraphQL API served from the app's modules.
///
/// Global; provides and exports [`GraphqlSchema`] (built at boot from every
/// resolver registered with [`ModuleBuilderGraphqlExt::resolver`]) and
/// [`GraphqlOptions`], and contributes:
///
/// - `POST <path>` (default `/graphql`): one operation as JSON, or a batch
///   with `graphql.batching`; open to everyone, each field checks its own
///   access;
/// - `GET <path>`: GraphiQL for browsers when `graphql.graphiql` is on
///   (debug builds by default), else 405;
/// - `GET <path>/ws`: subscriptions over WebSockets (`graphql-transport-ws`
///   and `graphql-ws`) unless `graphql.subscriptions` is off;
/// - the command `graphql:schema [path] [--check]`.
///
/// [`from_config`](GraphqlModule::from_config) and a plain
/// `.module(GraphqlModule)` read the `graphql` section, so the app includes
/// `ConfigModule`; [`for_root`](GraphqlModule::for_root) takes the options.
/// Fields that declare roles, permissions or `can` need navree-access's
/// `AccessModule`; non-public fields need an authenticator (navree-auth's
/// `AuthModule`).
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_graphql::GraphqlModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(GraphqlModule::from_config()) // section `graphql`
///     // the modules registering resolvers, AuthModule, AccessModule, ...
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct GraphqlModule;

impl GraphqlModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: GraphqlOptions) -> impl Module {
        ConfiguredGraphqlModule {
            options: Some(options),
        }
    }

    /// The module configured from the `graphql` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredGraphqlModule { options: None }
    }
}

impl Module for GraphqlModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `GraphqlModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredGraphqlModule {
    /// `None`: read from configuration.
    options: Option<GraphqlOptions>,
}

impl Module for ConfiguredGraphqlModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<GraphqlModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<GraphqlOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<GraphqlOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check().map_err(|problems| {
                    format!("invalid graphql options: {}", problems.join("; "))
                })?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<GraphqlOptions>(),
    };
    m.exports::<GraphqlOptions>()
        .factory(
            Factory::<GraphqlSchema>::new(|r: Resolver| async move {
                let options = r.get::<GraphqlOptions>().await?;
                Ok(Arc::new(GraphqlSchema::build(r.app(), options).await?))
            })
            .deps(vec![ProviderKey::of::<GraphqlOptions>()]),
        )
        .exports::<GraphqlSchema>()
        .controller::<GraphqlController>()
        .command(schema_command());
}

/// GraphQL registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderGraphqlExt {
    /// Registers resolver `R` (an injectable type whose impl block carries
    /// `#[resolver]`) as a singleton provider of this module and adds its
    /// root fields to the app's schema.
    fn resolver<R: GraphqlResolver + Injectable>(&mut self) -> &mut Self;

    /// Registers loader `L` (an injectable type implementing
    /// [`Loader<K>`](crate::Loader)) as a singleton provider of this module;
    /// every operation gets its own dataloader of it (`ctx.loader::<L>()`).
    fn dataloader<L: Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderGraphqlExt for ModuleBuilder {
    fn resolver<R: GraphqlResolver + Injectable>(&mut self) -> &mut Self {
        let registration = ResolverRegistration::of::<R>(self.module_id());
        self.provider::<R>().contribute(registration)
    }

    fn dataloader<L: Injectable>(&mut self) -> &mut Self {
        let registration = LoaderRegistration::of::<L>(self.module_id());
        self.provider::<L>().contribute(registration)
    }
}

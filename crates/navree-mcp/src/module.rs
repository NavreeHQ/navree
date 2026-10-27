//! [`McpModule`] and [`ModuleBuilderMcpExt`].

use std::sync::Arc;

use navree_config::{Config, ModuleBuilderConfigExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{Factory, Injectable, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};

use crate::http::{McpController, challenge_layer};
use crate::item::McpServer;
use crate::notifier::McpNotifier;
use crate::options::{McpOptions, McpSettings};
use crate::registry::{McpRegistry, ServerRegistration};
use crate::stdio::serve_command;

/// The MCP server of the app: the tools, resources and prompts of every
/// server the modules register ([`ModuleBuilderMcpExt::mcp_server`]),
/// served over streamable HTTP at `mcp.path` (default `/mcp`) on the app's
/// own server, and over stdio by the `mcp:serve` command.
///
/// Global; provides and exports [`McpSettings`], [`McpRegistry`] (built at
/// boot: duplicate names and items without an access declaration fail it)
/// and [`McpNotifier`]. The endpoint is authenticated, so the app needs
/// `HttpModule` and an authenticator (navree-auth's `AuthModule`); items
/// that declare roles, permissions or `can` need `AccessModule`, and
/// tools that write need `DatabaseModule` (each call runs in its own unit
/// of work). With `CacheModule`, sessions are kept in the cache, so any
/// process can serve a session's next request.
///
/// ```no_run
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
/// use navree_mcp::McpModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     // DatabaseModule, AuthModule, AccessModule, ...
///     .module(McpModule::from_config()) // section `mcp`; serves /mcp
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct McpModule;

impl McpModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: McpOptions) -> impl Module {
        ConfiguredMcpModule {
            options: Some(options),
        }
    }

    /// The module configured from the `mcp` section of `Config` at boot
    /// (the same as a plain `McpModule`).
    pub fn from_config() -> impl Module {
        ConfiguredMcpModule { options: None }
    }
}

impl Module for McpModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `McpModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredMcpModule {
    options: Option<McpOptions>,
}

impl Module for ConfiguredMcpModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<McpModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<McpOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<McpOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid mcp options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<McpOptions>(),
    };
    m.exports::<McpOptions>()
        .factory(
            Factory::<McpSettings>::new(|r: Resolver| async move {
                let options = r.get::<McpOptions>().await?;
                let config = r.app().try_get::<Config>().await;
                let settings = McpSettings::resolve(&options, config.as_deref())?;
                Ok(Arc::new(settings))
            })
            .deps(vec![ProviderKey::of::<McpOptions>()]),
        )
        .exports::<McpSettings>()
        .value(McpNotifier::default())
        .exports::<McpNotifier>()
        .factory(Factory::<McpRegistry>::new(|r: Resolver| async move {
            let registry = McpRegistry::collect(&r.app()).await?;
            Ok(Arc::new(registry))
        }))
        .exports::<McpRegistry>()
        .controller::<McpController>()
        .http_layer(challenge_layer())
        .command(serve_command());
}

/// MCP registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderMcpExt {
    /// Registers server `S` as a singleton provider of this module and
    /// offers its tools, resources and prompts. Servers registered by
    /// several modules merge into the app's one endpoint.
    fn mcp_server<S: McpServer + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderMcpExt for ModuleBuilder {
    fn mcp_server<S: McpServer + Injectable>(&mut self) -> &mut Self {
        let registration = ServerRegistration::of::<S>(self.module_id());
        self.provider::<S>().contribute(registration)
    }
}

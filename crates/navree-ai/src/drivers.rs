//! AI provider drivers a module registers under a name:
//! [`ModuleBuilderAiProviderExt`].

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

use navree_config::__private::{build_driver, check_registrations, register_driver};
use navree_config::{DriverContext, DriverName, DriverSlot};
use navree_kernel::{AppContext, BoxError, ModuleBuilder, Resolver};
use serde_json::{Map, Value as Json};

use crate::options::DRIVERS;
use crate::{AiOptions, ProviderConfig};
use crate::provider::Driver;
use crate::provider::public::{AiDriver, PublicDriver};

/// The selector of every provider table, `ai.providers.{name}.driver`
/// (the table's own name when it has none), its built-in drivers and the
/// method that registers more.
pub(crate) const SLOT: DriverSlot = DriverSlot::new(
    "ai.providers.*.driver",
    &[
        "anthropic",
        "openai",
        "gemini",
        "mistral",
        "ollama",
        "bedrock",
    ],
    "m.ai_provider",
    "AI provider driver",
)
.defaults_to_table_name();

/// What the boot builds before `Ai`: it holds that every registration of a
/// provider driver was checked. A provider of its own, so an app that
/// replaces `Ai` still has its registrations checked.
pub(crate) struct CheckedDrivers;

/// Registers AI provider drivers on a [`ModuleBuilder`]: `use
/// navree::plugin::prelude::*;` (or this trait) brings the method.
///
/// ```no_run
/// use navree_ai::{
///     AiCapabilities, AiDriver, AiDriverAnswer, AiDriverRequest, AiDriverStream, AiError,
///     AiModule, Finish, ModuleBuilderAiProviderExt, Part, ProviderConfig, Usage,
/// };
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::{BoxFuture, Module, ModuleBuilder, Navree};
///
/// /// A provider that greets, at its table's `url`.
/// struct Vaani {
///     url: String,
/// }
///
/// impl AiDriver for Vaani {
///     fn capabilities(&self) -> AiCapabilities {
///         AiCapabilities::default()
///     }
///
///     fn chat<'a>(
///         &'a self,
///         _request: &'a AiDriverRequest,
///     ) -> BoxFuture<'a, Result<AiDriverAnswer, AiError>> {
///         Box::pin(async move {
///             let text = Part::Text { text: format!("Namaste from {}", self.url) };
///             Ok(AiDriverAnswer::new(vec![text], Usage::new(3, 1), Finish::Stop))
///         })
///     }
///
///     fn stream<'a>(
///         &'a self,
///         _request: &'a AiDriverRequest,
///     ) -> BoxFuture<'a, Result<AiDriverStream, AiError>> {
///         Box::pin(async { Err(AiError::unavailable("vaani")) })
///     }
/// }
///
/// struct VaaniModule;
///
/// impl Module for VaaniModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // Selected by `[ai.providers.vaani]`, or by a table with `driver = "vaani"`.
///         m.ai_provider("vaani", |ctx| async move {
///             let table: ProviderConfig = ctx.settings()?;
///             Ok(Vaani { url: table.url.unwrap_or_default() })
///         });
///     }
/// }
///
/// fn main() -> Result<(), navree_kernel::BoxError> {
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(AiModule::from_config())
///         .module(VaaniModule)
///         .run()
/// }
/// ```
pub trait ModuleBuilderAiProviderExt {
    /// Registers an AI provider driver under `name` (a lower-case letter
    /// followed by up to 63 lower-case letters, digits or `_`): a provider
    /// table selects it with `driver = "{name}"`, or by being named after
    /// it (`[ai.providers.{name}]`), and `factory` builds it at boot once
    /// per such table from its [`DriverContext`]: its settings are the
    /// table (`ai.providers.{table}`, which `ctx.settings()` reads as a
    /// `ProviderConfig` or a type of the driver's own) and its `instance()`
    /// the table's name. Given to `AiModule::for_root` in code, the table
    /// is the `ProviderConfig` given, its keys included.
    ///
    /// A name that is not such a word, a built-in driver's name
    /// (`anthropic`, `openai`, `gemini`, `mistral`, `ollama`, `bedrock`) and
    /// a name two modules register fail the boot, naming `m.ai_provider`
    /// and the module; so does a table naming a driver no module registers.
    /// `config:check` says the same. A factory that fails fails the boot,
    /// naming the driver and the module. A factory's error is shown in the
    /// boot error as written, so it must not contain a secret from the
    /// driver's settings.
    fn ai_provider<A, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        A: AiDriver,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<A, BoxError>> + Send + 'static;
}

impl ModuleBuilderAiProviderExt for ModuleBuilder {
    fn ai_provider<A, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        A: AiDriver,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<A, BoxError>> + Send + 'static,
    {
        register_driver::<dyn AiDriver, _, _>(self, &SLOT, None, name, move |ctx| {
            let built = factory(ctx);
            async move {
                built
                    .await
                    .map(|driver| Arc::new(driver) as Arc<dyn AiDriver>)
            }
        })
    }
}

/// Checks every registration of a provider driver, whichever tables select
/// one: what [`CheckedDrivers`]'s factory runs.
pub(crate) fn check_drivers(app: &AppContext) -> Result<(), BoxError> {
    check_registrations(app, &SLOT, None)
}

/// Builds the driver of each provider table of `options` whose driver is
/// not built in, from the registration of that name, keyed by the table's
/// name. A table naming a driver no module registers is the boot error
/// naming its key. With `in_code` (options given to `AiModule::for_root`),
/// each driver reads the table it was given; otherwise the table as the
/// configuration writes it.
pub(crate) async fn registered_drivers(
    r: &Resolver,
    options: &AiOptions,
    in_code: bool,
) -> Result<BTreeMap<String, Arc<dyn Driver>>, BoxError> {
    let mut drivers: BTreeMap<String, Arc<dyn Driver>> = BTreeMap::new();
    for (table, config) in &options.providers {
        let driver = config.driver_for(table);
        if DRIVERS.contains(&driver) {
            continue;
        }
        // A value of the app's configuration (or of its code), never of a
        // request: the open check let only driver names through.
        let name = DriverName::new(driver)?;
        let built = build_driver::<dyn AiDriver>(
            r,
            &SLOT,
            &format!("ai.providers.{table}.driver"),
            name,
            format!("ai.providers.{table}"),
            Some(table.clone()),
            in_code.then(|| provider_table(config)),
        )
        .await?;
        drivers.insert(
            table.clone(),
            Arc::new(PublicDriver(built)) as Arc<dyn Driver>,
        );
    }
    Ok(drivers)
}

/// A provider table given in code, as the settings its driver reads: every
/// field that is set, its keys included (a `ProviderConfig` never
/// serializes them).
fn provider_table(config: &ProviderConfig) -> Json {
    let mut table = match serde_json::to_value(config) {
        Ok(Json::Object(table)) => table,
        _ => Map::new(),
    };
    table.retain(|_, value| !value.is_null());
    let secrets = [
        ("api_key", &config.api_key),
        ("secret_access_key", &config.secret_access_key),
        ("session_token", &config.session_token),
    ];
    for (key, secret) in secrets {
        if let Some(secret) = secret {
            table.insert(key.to_owned(), Json::String(secret.expose().clone()));
        }
    }
    Json::Object(table)
}

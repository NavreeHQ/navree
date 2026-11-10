//! Providers registered by name: [`BillingModule::provider`] contributes a
//! [`ProviderRegistration`], and `billing.provider = "<name>"` picks it at
//! boot.
//!
//! [`BillingModule::provider`]: crate::BillingModule::provider

use std::future::Future;
use std::sync::Arc;

use navree_config::Config;
use navree_contracts::Clock;
use navree_http_client::HttpClient;
use navree_kernel::{AppContext, BoxError, BoxFuture};
use serde::de::DeserializeOwned;

use crate::config::BillingOptions;
use crate::provider::BillingProvider;

/// The names of the providers that ship with navree-billing; a registration
/// may not take one.
pub(crate) const BUILT_IN: &[&str] = &["stripe", "paddle", "fake"];

/// What a provider factory gets to build its provider: the billing options,
/// the app's HTTP client (with its retries, timeouts and SSRF policy), the
/// clock and the configuration.
#[derive(Clone)]
pub struct ProviderContext {
    app: AppContext,
    name: String,
    options: Arc<BillingOptions>,
    http: HttpClient,
    clock: Arc<dyn Clock>,
    config: Option<Arc<Config>>,
}

impl std::fmt::Debug for ProviderContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderContext")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl ProviderContext {
    pub(crate) fn new(
        app: AppContext,
        name: String,
        options: Arc<BillingOptions>,
        http: HttpClient,
        clock: Arc<dyn Clock>,
        config: Option<Arc<Config>>,
    ) -> ProviderContext {
        ProviderContext {
            app,
            name,
            options,
            http,
            clock,
            config,
        }
    }

    /// The name the provider is registered and selected under.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The `billing` options.
    pub fn options(&self) -> &BillingOptions {
        &self.options
    }

    /// The app's HTTP client (`HttpClientModule`'s, or a default one): send
    /// writes with `idempotency_key` so they are retried safely.
    pub fn http_client(&self) -> &HttpClient {
        &self.http
    }

    /// The clock (the app's `dyn Clock`, or the system clock).
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// The app, to resolve anything else the provider needs.
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// The provider's own configuration section, typed: by convention
    /// `billing.<name>` (`billing.razorpay`), with its secrets from the
    /// environment (`APP_BILLING__RAZORPAY__KEY_SECRET`). A missing section
    /// is the type's default.
    ///
    /// Fails when the app has no `ConfigModule` or the section does not
    /// match `T`, naming the key.
    pub fn section<T: DeserializeOwned + Default>(&self, key: &str) -> Result<T, BoxError> {
        let config = self
            .config
            .as_ref()
            .ok_or("the billing provider needs ConfigModule to read its section")?;
        config
            .section_or_default::<T>(key)
            .map_err(|error| format!("{key}: {error}").into())
    }
}

/// Builds a provider at boot.
pub(crate) type ProviderFactory = Arc<
    dyn Fn(ProviderContext) -> BoxFuture<'static, Result<Arc<dyn BillingProvider>, BoxError>>
        + Send
        + Sync,
>;

/// A provider registered under a name.
#[derive(Clone)]
pub(crate) struct ProviderRegistration {
    pub(crate) name: String,
    pub(crate) factory: ProviderFactory,
}

impl std::fmt::Debug for ProviderRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistration")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl ProviderRegistration {
    pub(crate) fn new<F, Fut>(name: &str, factory: F) -> ProviderRegistration
    where
        F: Fn(ProviderContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Arc<dyn BillingProvider>, BoxError>> + Send + 'static,
    {
        ProviderRegistration {
            name: name.to_owned(),
            factory: Arc::new(move |context| Box::pin(factory(context))),
        }
    }
}

/// Builds the provider registered as `name`, checking the registrations.
pub(crate) async fn build(
    name: &str,
    registrations: &[ProviderRegistration],
    context: ProviderContext,
) -> Result<Arc<dyn BillingProvider>, BoxError> {
    if let Some(taken) = registrations
        .iter()
        .find(|r| BUILT_IN.contains(&r.name.as_str()))
    {
        return Err(format!(
            "BillingModule::provider(\"{}\"): `{}` is a built-in provider; register yours under another name",
            taken.name, taken.name
        )
        .into());
    }
    let matching: Vec<&ProviderRegistration> =
        registrations.iter().filter(|r| r.name == name).collect();
    let registration = match matching.as_slice() {
        [only] => *only,
        [] => {
            let mut names: Vec<&str> = BUILT_IN.to_vec();
            names.extend(registrations.iter().map(|r| r.name.as_str()));
            return Err(format!(
                "billing.provider is `{name}`, but no module registers that provider with BillingModule::provider (known: {})",
                names.join(", ")
            )
            .into());
        }
        _ => {
            return Err(format!(
                "billing provider `{name}` is registered {} times; register it once",
                matching.len()
            )
            .into());
        }
    };
    let provider = (registration.factory)(context).await?;
    if provider.name() != name {
        return Err(format!(
            "the billing provider registered as `{name}` names itself `{}`: BillingProvider::name must return the registered name, which the billing tables store",
            provider.name()
        )
        .into());
    }
    Ok(provider)
}

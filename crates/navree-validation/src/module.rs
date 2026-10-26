//! [`ValidationModule`] and [`ValidationOptions`].

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_config::Config;
use navree_kernel::{
    Factory, Injectable, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::{Country, HtmlPolicy, InputContext, Messages, RegionSource};

/// The configuration section of [`ValidationModule::from_config`].
pub const CONFIG_SECTION: &str = "validation";

/// Message and attribute-name overrides, the default phone region and the
/// HTML policy (section `validation`).
///
/// ```toml
/// [validation.messages]
/// required = ":attribute est obligatoire"
/// "email.unique" = "This e-mail is already registered"
///
/// [validation.attributes]
/// first_name = "first name"
///
/// [validation.phone]
/// default_region = "GB"
///
/// [validation.html]
/// tags = ["p", "br", "strong", "em", "a", "ul", "ol", "li"]
/// ```
///
/// Nested tables flatten to dotted keys, so `[validation.messages.min]
/// string = "..."` sets `min.string`. In code, start from the defaults:
/// `ValidationOptions { messages, ..ValidationOptions::default() }`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationOptions {
    /// Templates by rule (`"required"`, `"min.string"`) or by field and rule
    /// (`"email.unique"`); see [`DEFAULT_MESSAGES`](crate::DEFAULT_MESSAGES).
    #[serde(deserialize_with = "flat_strings")]
    pub messages: BTreeMap<String, String>,
    /// Display names by field path (`"address.city"`), used for `:attribute`.
    #[serde(deserialize_with = "flat_strings")]
    pub attributes: BTreeMap<String, String>,
    /// Phone numbers (`validation.phone`).
    pub phone: PhoneOptions,
    /// What `SafeHtml` keeps (`validation.html`).
    pub html: HtmlPolicy,
}

/// Phone numbers (section `validation.phone`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhoneOptions {
    /// The region of numbers written without a country code (`"GB"`, from
    /// `APP_VALIDATION__PHONE__DEFAULT_REGION`), unless the app's
    /// [`RegionSource`] gives one for the request. Unset, such numbers are
    /// invalid.
    pub default_region: Option<Country>,
}

impl From<ValidationOptions> for Messages {
    fn from(options: ValidationOptions) -> Self {
        Messages::new(options.messages, options.attributes)
    }
}

impl InputContext {
    /// The context of `options`, refusing an unsafe HTML policy.
    fn from_options(options: &ValidationOptions) -> Result<InputContext, String> {
        options.html.check()?;
        Ok(InputContext::new()
            .with_default_region(options.phone.default_region)
            .with_html(options.html.clone()))
    }
}

/// A string map whose nested maps flatten to dotted keys.
fn flat_strings<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    fn flatten(
        prefix: &str,
        value: Value,
        out: &mut BTreeMap<String, String>,
    ) -> Result<(), String> {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    let key = if prefix.is_empty() {
                        key
                    } else {
                        format!("{prefix}.{key}")
                    };
                    flatten(&key, value, out)?;
                }
                Ok(())
            }
            Value::String(text) => {
                out.insert(prefix.to_owned(), text);
                Ok(())
            }
            Value::Null => Ok(()),
            other => Err(format!("`{prefix}` must be a string, found {other}")),
        }
    }
    let value = Value::deserialize(deserializer)?;
    let mut out = BTreeMap::new();
    flatten("", value, &mut out).map_err(serde::de::Error::custom)?;
    Ok(out)
}

/// The validation module. Global; provides and exports the app's
/// [`Messages`], which the [`Valid`](crate::Valid) and
/// [`ValidAsync`](crate::ValidAsync) extractors render errors with, and its
/// [`InputContext`] (default phone region, HTML policy), which the
/// international types read. Without the module the extractors use the
/// built-in messages and the default context. An unsafe HTML policy
/// ([`HtmlPolicy::check`]) fails the boot.
///
/// - [`for_root`](ValidationModule::for_root): the built-in messages.
/// - [`with_options`](ValidationModule::with_options): explicit overrides.
/// - [`from_config`](ValidationModule::from_config) or a plain
///   `.module(ValidationModule)`: overrides from the `validation`
///   configuration section, so the app must include `ConfigModule`.
///
/// ```no_run
/// use navree_kernel::Navree;
/// use navree_validation::ValidationModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app().module(ValidationModule::for_root()).run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct ValidationModule;

impl ValidationModule {
    /// The module with the built-in English messages.
    pub fn for_root() -> impl Module {
        Self::with_options(ValidationOptions::default())
    }

    /// The module with explicit message and attribute overrides.
    pub fn with_options(options: ValidationOptions) -> impl Module {
        ConfiguredValidationModule {
            options: Some(options),
        }
    }

    /// The module configured from the `validation` section of `Config`.
    pub fn from_config() -> impl Module {
        ConfiguredValidationModule { options: None }
    }
}

impl Module for ValidationModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `ValidationModule::for_root` / `with_options` / `from_config`: same module id, configured.
struct ConfiguredValidationModule {
    options: Option<ValidationOptions>,
}

impl Module for ConfiguredValidationModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<ValidationModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<ValidationOptions>) {
    m.global();
    match options {
        Some(options) => m.value(options),
        None => m.factory(
            Factory::<ValidationOptions>::new(|r: Resolver| async move {
                let config = r.get::<Config>().await?;
                let options: ValidationOptions = config.section_or_default(CONFIG_SECTION)?;
                Ok(Arc::new(options))
            })
            .deps(vec![ProviderKey::of::<Config>()]),
        ),
    };
    m.factory(
        Factory::<Messages>::new(|r: Resolver| async move {
            let options = r.get::<ValidationOptions>().await?;
            Ok(Arc::new(Messages::from(ValidationOptions::clone(&options))))
        })
        .deps(vec![ProviderKey::of::<ValidationOptions>()]),
    )
    .exports::<Messages>()
    .factory(
        Factory::<InputContext>::new(|r: Resolver| async move {
            let options = r.get::<ValidationOptions>().await?;
            Ok(Arc::new(InputContext::from_options(&options)?))
        })
        .deps(vec![ProviderKey::of::<ValidationOptions>()]),
    )
    .exports::<InputContext>();
}

/// Validation registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderValidationExt {
    /// Registers provider `R` as the app's `dyn RegionSource`, which picks
    /// the default region of each request's phone numbers.
    ///
    /// ```
    /// use http::request::Parts;
    /// use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
    /// use navree_validation::{Country, ModuleBuilderValidationExt, RegionSource};
    ///
    /// /// The country of the signed-in user's workspace, cached in the request.
    /// struct WorkspaceRegion;
    ///
    /// # #[derive(Clone)] struct WorkspaceCountry(Country);
    /// impl RegionSource for WorkspaceRegion {
    ///     fn region<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Option<Country>> {
    ///         let country = parts.extensions.get::<WorkspaceCountry>().map(|c| c.0);
    ///         Box::pin(async move { country })
    ///     }
    /// }
    ///
    /// impl Injectable for WorkspaceRegion {
    ///     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
    ///         Box::pin(async { Ok(WorkspaceRegion) })
    ///     }
    /// }
    ///
    /// struct ContactsModule;
    ///
    /// impl Module for ContactsModule {
    ///     fn register(&self, m: &mut ModuleBuilder) {
    ///         m.region_source::<WorkspaceRegion>();
    ///     }
    /// }
    /// ```
    fn region_source<R: RegionSource + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderValidationExt for ModuleBuilder {
    fn region_source<R: RegionSource + Injectable>(&mut self) -> &mut Self {
        self.provider::<R>()
            .factory(
                Factory::<dyn RegionSource>::new(|r: Resolver| async move {
                    Ok(r.get::<R>().await? as Arc<dyn RegionSource>)
                })
                .deps(vec![ProviderKey::of::<R>()]),
            )
            .exports::<dyn RegionSource>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_flatten_nested_tables() {
        let options: ValidationOptions = serde_json::from_value(serde_json::json!({
            "messages": { "required": "R", "min": { "string": "S" }, "email": { "unique": "U" } },
        }))
        .unwrap();
        assert_eq!(
            options.messages,
            BTreeMap::from([
                ("email.unique".to_owned(), "U".to_owned()),
                ("min.string".to_owned(), "S".to_owned()),
                ("required".to_owned(), "R".to_owned()),
            ])
        );
        assert!(options.attributes.is_empty());
        let error = serde_json::from_value::<ValidationOptions>(serde_json::json!({
            "messages": { "required": 5 },
        }))
        .unwrap_err();
        assert!(
            error.to_string().contains("`required` must be a string"),
            "{error}"
        );
    }

    #[test]
    fn options_read_the_phone_and_html_sections() {
        let options: ValidationOptions = serde_json::from_value(serde_json::json!({
            "phone": { "default_region": "gb" },
            "html": { "tags": ["p"], "link_rel": "" },
        }))
        .unwrap();
        assert_eq!(options.phone.default_region, Country::parse("GB").ok());
        assert_eq!(options.html.tags, ["p".to_owned()].into());
        assert_eq!(options.html.link_rel, "");
        assert_eq!(
            options.html.url_schemes,
            HtmlPolicy::default().url_schemes
        );
        let context = InputContext::from_options(&options).unwrap();
        assert_eq!(context.default_region(), options.phone.default_region);
        assert_eq!(context.html(), &options.html);

        let error = serde_json::from_value::<ValidationOptions>(serde_json::json!({
            "phone": { "default_region": "UK" },
        }))
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("`UK` is not an ISO 3166-1 alpha-2 country code"),
            "{error}"
        );

        let unsafe_html = ValidationOptions {
            html: HtmlPolicy::default().allow_tags(["script"]),
            ..ValidationOptions::default()
        };
        assert!(
            InputContext::from_options(&unsafe_html)
                .unwrap_err()
                .starts_with("validation.html.tags")
        );
    }
}

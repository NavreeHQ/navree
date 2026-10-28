//! [`BillingOptions`]: the `billing` configuration section.

use std::collections::BTreeMap;
use std::fmt;

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::Deserialize;

/// Which payment provider [`Billing`](crate::Billing) talks to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Stripe (`billing.stripe.*`).
    #[default]
    Stripe,
    /// Paddle Billing (`billing.paddle.*`).
    Paddle,
    /// The in-memory [`BillingFake`](crate::BillingFake), for tests.
    Fake,
}

impl ProviderKind {
    /// `stripe`, `paddle` or `fake`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Stripe => "stripe",
            ProviderKind::Paddle => "paddle",
            ProviderKind::Fake => "fake",
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Stripe settings (`billing.stripe`). Put the secrets in `.env`
/// (`APP_BILLING__STRIPE__SECRET_KEY`, `APP_BILLING__STRIPE__WEBHOOK_SECRET`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct StripeOptions {
    /// The secret API key (`sk_live_...`, `sk_test_...`).
    pub secret_key: Option<Secret<String>>,
    /// The signing secret of the webhook endpoint (`whsec_...`).
    pub webhook_secret: Option<Secret<String>>,
    /// The API base URL (default `https://api.stripe.com`); point it at a
    /// mock server or a proxy.
    pub api_base: String,
}

impl Default for StripeOptions {
    fn default() -> Self {
        Self {
            secret_key: None,
            webhook_secret: None,
            api_base: "https://api.stripe.com".to_owned(),
        }
    }
}

/// Paddle's environments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaddleEnvironment {
    /// `https://sandbox-api.paddle.com` (the default).
    #[default]
    Sandbox,
    /// `https://api.paddle.com`.
    Production,
}

impl PaddleEnvironment {
    /// The API base URL of the environment.
    pub fn api_base(self) -> &'static str {
        match self {
            PaddleEnvironment::Sandbox => "https://sandbox-api.paddle.com",
            PaddleEnvironment::Production => "https://api.paddle.com",
        }
    }
}

/// Paddle settings (`billing.paddle`). Put the secrets in `.env`
/// (`APP_BILLING__PADDLE__API_KEY`, `APP_BILLING__PADDLE__WEBHOOK_SECRET`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PaddleOptions {
    /// The API key (`pdl_live_apikey_...`, `pdl_sdbx_apikey_...`).
    pub api_key: Option<Secret<String>>,
    /// The secret key of the notification destination (`pdl_ntfset_...`).
    pub webhook_secret: Option<Secret<String>>,
    /// `sandbox` (the default) or `production`.
    pub environment: PaddleEnvironment,
    /// Overrides the environment's API base URL (a mock server or a proxy).
    pub api_base: Option<String>,
}

impl PaddleOptions {
    /// The API base URL: [`api_base`](Self::api_base) or the environment's.
    pub fn base_url(&self) -> String {
        self.api_base
            .clone()
            .unwrap_or_else(|| self.environment.api_base().to_owned())
    }
}

/// The provider's price ids of a plan (`billing.plans.<key>.prices`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PlanPrices {
    /// The monthly price id.
    pub monthly: Option<String>,
    /// The yearly price id.
    pub yearly: Option<String>,
}

/// A plan (`billing.plans.<key>`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PlanOptions {
    /// The display name (default: the key).
    pub name: Option<String>,
    /// The monthly and yearly price ids; at least one.
    pub prices: PlanPrices,
    /// The trial on checkout, in days (0: none).
    pub trial_days: u32,
    /// Whether the quantity follows the workspace's member count.
    pub seats: bool,
    /// Meter key → the provider's metered price id.
    pub meters: BTreeMap<String, String>,
}

/// Options of the billing module (configuration section `billing`).
///
/// ```toml
/// [billing]
/// provider = "stripe"                       # stripe | paddle | fake
/// return_url = "https://app.acme.test/settings/billing"
///
/// [billing.plans.pro]
/// name = "Pro"
/// prices = { monthly = "price_1Q8ProMonthly", yearly = "price_1Q8ProYearly" }
/// trial_days = 14
/// seats = true
///
/// [billing.plans.pro.meters]
/// ai_summaries = "price_1Q8AiSummaries"
/// ```
///
/// Secrets come from the environment: `APP_BILLING__STRIPE__SECRET_KEY`,
/// `APP_BILLING__STRIPE__WEBHOOK_SECRET`, `APP_BILLING__PADDLE__API_KEY`,
/// `APP_BILLING__PADDLE__WEBHOOK_SECRET`. [`check`](Self::check) names the
/// key of every problem and never prints a secret.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct BillingOptions {
    /// The payment provider.
    pub provider: ProviderKind,
    /// The currency of the fake provider's invoices (lower-case ISO code).
    /// Stripe and Paddle take the currency from the price.
    pub currency: String,
    /// Where checkout and the portal return; `app.url` when unset.
    pub return_url: Option<String>,
    /// The queue of the webhook jobs.
    pub queue: String,
    /// Days a past-due subscription stays valid.
    pub past_due_grace_days: u32,
    /// The plan a trial without a subscription grants (default: the first
    /// plan by key).
    pub trial_plan: Option<String>,
    /// How long invoice lists are cached, in seconds (0: not cached).
    pub invoice_cache_secs: u64,
    /// Mounts the `/api/billing` routes.
    pub routes: bool,
    /// The billable type the routes and `Subscribed` act on (`workspace`,
    /// `user`, or an app-defined `Billable::BILLABLE_TYPE`); default: the
    /// only registered billable.
    pub routes_billable: Option<String>,
    /// Mounts `POST /api/billing/webhook`.
    pub webhook_route: bool,
    /// Accepted clock skew of webhook signatures, in seconds.
    pub webhook_tolerance_secs: u64,
    /// Stripe settings.
    pub stripe: StripeOptions,
    /// Paddle settings.
    pub paddle: PaddleOptions,
    /// The plans, by key.
    pub plans: BTreeMap<String, PlanOptions>,
}

impl Default for BillingOptions {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Stripe,
            currency: "usd".to_owned(),
            return_url: None,
            queue: "default".to_owned(),
            past_due_grace_days: 7,
            trial_plan: None,
            invoice_cache_secs: 300,
            routes: true,
            routes_billable: None,
            webhook_route: true,
            webhook_tolerance_secs: 300,
            stripe: StripeOptions::default(),
            paddle: PaddleOptions::default(),
            plans: BTreeMap::new(),
        }
    }
}

impl BillingOptions {
    /// Every problem with the options, naming the configuration keys.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let missing = |secret: &Option<Secret<String>>| {
            secret
                .as_ref()
                .is_none_or(|secret| secret.expose().trim().is_empty())
        };
        match self.provider {
            ProviderKind::Stripe => {
                if missing(&self.stripe.secret_key) {
                    errors.push(
                        "billing.stripe.secret_key is required with billing.provider = \"stripe\" (APP_BILLING__STRIPE__SECRET_KEY)"
                            .to_owned(),
                    );
                }
                if self.webhook_route && missing(&self.stripe.webhook_secret) {
                    errors.push(
                        "billing.stripe.webhook_secret is required with billing.webhook_route on (APP_BILLING__STRIPE__WEBHOOK_SECRET)"
                            .to_owned(),
                    );
                }
                if !is_http_url(&self.stripe.api_base) {
                    errors
                        .push("billing.stripe.api_base must be an absolute http(s) URL".to_owned());
                }
            }
            ProviderKind::Paddle => {
                if missing(&self.paddle.api_key) {
                    errors.push(
                        "billing.paddle.api_key is required with billing.provider = \"paddle\" (APP_BILLING__PADDLE__API_KEY)"
                            .to_owned(),
                    );
                }
                if self.webhook_route && missing(&self.paddle.webhook_secret) {
                    errors.push(
                        "billing.paddle.webhook_secret is required with billing.webhook_route on (APP_BILLING__PADDLE__WEBHOOK_SECRET)"
                            .to_owned(),
                    );
                }
                if self
                    .paddle
                    .api_base
                    .as_deref()
                    .is_some_and(|url| !is_http_url(url))
                {
                    errors
                        .push("billing.paddle.api_base must be an absolute http(s) URL".to_owned());
                }
            }
            ProviderKind::Fake => {}
        }
        if self.currency.len() != 3 || !self.currency.chars().all(|c| c.is_ascii_lowercase()) {
            errors.push(
                "billing.currency must be a lower-case ISO 4217 code such as `usd`".to_owned(),
            );
        }
        if self
            .return_url
            .as_deref()
            .is_some_and(|url| !is_http_url(url))
        {
            errors.push("billing.return_url must be an absolute http(s) URL".to_owned());
        }
        if self.queue.trim().is_empty() {
            errors.push("billing.queue must not be empty".to_owned());
        }
        if self.webhook_tolerance_secs == 0 {
            errors.push("billing.webhook_tolerance_secs must be at least 1".to_owned());
        }
        if self
            .routes_billable
            .as_deref()
            .is_some_and(|kind| kind.trim().is_empty())
        {
            errors.push("billing.routes_billable must name a billable type".to_owned());
        }
        for (key, plan) in &self.plans {
            if !is_key(key) {
                errors.push(format!(
                    "billing.plans: `{key}` must be lower-case letters, digits, `_` or `-`"
                ));
            }
            let blank =
                |price: &Option<String>| price.as_deref().is_some_and(|p| p.trim().is_empty());
            if plan.prices.monthly.is_none() && plan.prices.yearly.is_none() {
                errors.push(format!(
                    "billing.plans.{key}.prices needs a monthly or a yearly price id"
                ));
            }
            if blank(&plan.prices.monthly) {
                errors.push(format!(
                    "billing.plans.{key}.prices.monthly must not be empty"
                ));
            }
            if blank(&plan.prices.yearly) {
                errors.push(format!(
                    "billing.plans.{key}.prices.yearly must not be empty"
                ));
            }
            for (meter, price) in &plan.meters {
                if !is_key(meter) {
                    errors.push(format!(
                        "billing.plans.{key}.meters: `{meter}` must be lower-case letters, digits, `_` or `-`"
                    ));
                }
                if price.trim().is_empty() {
                    errors.push(format!(
                        "billing.plans.{key}.meters.{meter} must name a price id"
                    ));
                }
            }
        }
        if let Some(plan) = &self.trial_plan
            && !self.plans.contains_key(plan)
        {
            errors.push(format!(
                "billing.trial_plan names `{plan}`, which is not under billing.plans"
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl ConfigSection for BillingOptions {
    const KEY: &'static str = "billing";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// Whether `url` is an absolute `http://` or `https://` URL with a host.
pub(crate) fn is_http_url(url: &str) -> bool {
    ["https://", "http://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
}

/// A plan or meter key: lower-case letters, digits, `_` and `-`.
fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};
    use serde_json::json;

    use super::*;

    fn load(sources: ConfigSources) -> Result<BillingOptions, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<BillingOptions>()
    }

    fn fake() -> ConfigSources {
        ConfigSources::new().set("billing.provider", "fake")
    }

    #[test]
    fn defaults() {
        let options = BillingOptions::default();
        assert_eq!(options.provider, ProviderKind::Stripe);
        assert_eq!(options.currency, "usd");
        assert_eq!(options.return_url, None);
        assert_eq!(options.queue, "default");
        assert_eq!(options.past_due_grace_days, 7);
        assert_eq!(options.trial_plan, None);
        assert_eq!(options.invoice_cache_secs, 300);
        assert!(options.routes);
        assert_eq!(options.routes_billable, None);
        assert!(options.webhook_route);
        assert_eq!(options.webhook_tolerance_secs, 300);
        assert_eq!(options.stripe.api_base, "https://api.stripe.com");
        assert_eq!(options.paddle.environment, PaddleEnvironment::Sandbox);
        assert_eq!(options.paddle.base_url(), "https://sandbox-api.paddle.com");
        assert!(options.plans.is_empty());
    }

    #[test]
    fn plans_and_secrets_come_from_config_and_environment() {
        let options = load(
            ConfigSources::new()
                .set(
                    "billing.return_url",
                    "https://app.acme.test/settings/billing",
                )
                .set(
                    "billing.plans.pro",
                    json!({
                        "name": "Pro",
                        "prices": { "monthly": "price_pro_m", "yearly": "price_pro_y" },
                        "seats": true,
                        "meters": { "ai_summaries": "price_ai" }
                    }),
                )
                .env_vars(
                    "APP_",
                    [
                        ("APP_BILLING__STRIPE__SECRET_KEY", "sk_test_123"),
                        ("APP_BILLING__STRIPE__WEBHOOK_SECRET", "whsec_abc"),
                        ("APP_BILLING__PLANS__PRO__TRIAL_DAYS", "30"),
                        ("APP_BILLING__PAST_DUE_GRACE_DAYS", "3"),
                        ("APP_BILLING__ROUTES", "false"),
                    ],
                ),
        )
        .unwrap();
        assert_eq!(options.provider, ProviderKind::Stripe);
        assert_eq!(
            options
                .stripe
                .secret_key
                .as_ref()
                .map(|s| s.expose().as_str()),
            Some("sk_test_123")
        );
        let pro = &options.plans["pro"];
        assert_eq!(pro.name.as_deref(), Some("Pro"));
        assert_eq!(pro.prices.monthly.as_deref(), Some("price_pro_m"));
        assert_eq!(pro.prices.yearly.as_deref(), Some("price_pro_y"));
        assert_eq!(pro.trial_days, 30);
        assert!(pro.seats);
        assert_eq!(pro.meters["ai_summaries"], "price_ai");
        assert_eq!(options.past_due_grace_days, 3);
        assert!(!options.routes);
    }

    #[test]
    fn paddle_takes_its_own_keys() {
        let options = load(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_BILLING__PROVIDER", "paddle"),
                ("APP_BILLING__PADDLE__API_KEY", "pdl_live_apikey_01j"),
                ("APP_BILLING__PADDLE__WEBHOOK_SECRET", "pdl_ntfset_01j"),
                ("APP_BILLING__PADDLE__ENVIRONMENT", "production"),
            ],
        ))
        .unwrap();
        assert_eq!(options.provider, ProviderKind::Paddle);
        assert_eq!(options.paddle.base_url(), "https://api.paddle.com");
    }

    #[test]
    fn a_missing_secret_fails_naming_the_key_and_never_the_value() {
        let error = load(ConfigSources::new().set("billing.stripe.webhook_secret", "whsec_secret"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("billing.stripe.secret_key"), "{error}");
        assert!(!error.contains("whsec_secret"), "{error}");

        let error = load(
            ConfigSources::new()
                .set("billing.provider", "paddle")
                .set("billing.paddle.api_key", "pdl_key"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("billing.paddle.webhook_secret"), "{error}");
        assert!(!error.contains("pdl_key"), "{error}");

        // Without the webhook route, the webhook secret is optional.
        load(
            ConfigSources::new()
                .set("billing.stripe.secret_key", "sk_test_1")
                .set("billing.webhook_route", false),
        )
        .unwrap();
    }

    #[test]
    fn invalid_values_fail_validation() {
        for (key, value) in [
            ("billing.provider", json!("braintree")),
            ("billing.currency", json!("USD")),
            ("billing.return_url", json!("settings/billing")),
            ("billing.queue", json!(" ")),
            ("billing.webhook_tolerance_secs", json!(0)),
            ("billing.routes_billable", json!("")),
            ("billing.plans.pro.trial_days", json!(14)),
            ("billing.plans.Pro.prices.monthly", json!("price_1")),
            ("billing.plans.pro.prices.monthly", json!("")),
            ("billing.trial_plan", json!("enterprise")),
        ] {
            let result = load(fake().set(key, value.clone()));
            assert!(result.is_err(), "{key} = {value} should be rejected");
        }
        let error = load(fake().set("billing.plans.pro.trial_days", 14))
            .unwrap_err()
            .to_string();
        assert!(error.contains("billing.plans.pro.prices"), "{error}");
    }

    #[test]
    fn an_unknown_provider_names_the_key() {
        let error = load(ConfigSources::new().set("billing.provider", "braintree"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("provider"), "{error}");
        assert!(error.contains("`stripe`"), "{error}");
        // Configured values never appear in errors; the key and the
        // accepted values do.
        assert!(!error.contains("braintree"), "{error}");
    }

    #[test]
    fn the_fake_provider_needs_no_keys() {
        let options = load(
            fake()
                .set("billing.plans.pro.prices.monthly", "price_pro")
                .set("billing.trial_plan", "pro"),
        )
        .unwrap();
        assert_eq!(options.provider, ProviderKind::Fake);
        assert_eq!(options.trial_plan.as_deref(), Some("pro"));
    }

    #[test]
    fn urls_are_checked() {
        assert!(is_http_url("https://app.test/billing"));
        assert!(is_http_url("http://127.0.0.1:4010"));
        assert!(!is_http_url("https:///x"));
        assert!(!is_http_url("app.test"));
    }
}

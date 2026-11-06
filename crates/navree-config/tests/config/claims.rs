//! Keys a module reads by itself (`billing.<provider>`) are claimed, so they
//! are not reported as unknown.

use navree_config::check::{CheckOptions, CheckReport};
use navree_config::{
    ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt, SectionRegistration,
};
use navree_kernel::{Module, ModuleBuilder, Navree};
use serde::Deserialize;

/// A billing section that knows no provider by name.
#[derive(Debug, Default, Deserialize, ConfigSection)]
#[config(key = "billing")]
#[serde(default)]
struct Billing {
    /// The currency of invoices.
    currency: String,
}

fn sources() -> ConfigSources {
    ConfigSources::new()
        .set("billing.currency", "INR")
        .set("billing.razorpay.key_id", "rzp_test")
}

#[test]
fn a_provider_key_is_unknown_until_it_is_claimed() {
    let registrations = [SectionRegistration::of::<Billing>()];
    let report =
        CheckReport::run(&sources(), &registrations, &CheckOptions::default()).expect("load");
    let keys: Vec<&str> = report.unknown.iter().map(|u| u.key.as_str()).collect();
    assert_eq!(keys, ["billing.razorpay"]);

    let claims = ["billing.razorpay".to_owned()];
    let report = CheckReport::run_with_claims(
        &sources(),
        &registrations,
        &claims,
        &CheckOptions::default(),
    )
    .expect("load");
    assert!(report.unknown.is_empty(), "{}", report.render());
    assert!(report.is_ok());
}

#[test]
fn a_claim_does_not_hide_typos_next_to_it() {
    let registrations = [SectionRegistration::of::<Billing>()];
    let claims = ["billing.razorpay".to_owned()];
    let report = CheckReport::run_with_claims(
        &sources().set("billing.curency", "INR"),
        &registrations,
        &claims,
        &CheckOptions::default(),
    )
    .expect("load");
    let keys: Vec<&str> = report.unknown.iter().map(|u| u.key.as_str()).collect();
    assert_eq!(keys, ["billing.curency"]);
}

struct Claiming;

impl Module for Claiming {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<Billing>().config_claim("billing.razorpay");
    }
}

struct Plain;

impl Module for Plain {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<Billing>();
    }
}

async fn boots(module: impl Module) -> Result<(), String> {
    let sources = sources().set("app.strict_config", true);
    match Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(module)
        .boot()
        .await
    {
        Ok(app) => {
            app.shutdown().await.expect("shutdown");
            Ok(())
        }
        Err(errors) => Err(errors.to_string()),
    }
}

#[tokio::test]
async fn the_boot_leaves_claimed_keys_alone() {
    let error = boots(Plain).await.expect_err("billing.razorpay is unknown");
    assert!(error.contains("billing.razorpay"), "{error}");
    assert_eq!(boots(Claiming).await, Ok(()));
}

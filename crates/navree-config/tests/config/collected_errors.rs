//! A boot that fails on a section names every invalid section,
//! not only the first.

use std::sync::Arc;

use navree_config::{
    Config, ConfigError, ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt,
    SectionRegistration,
};
use navree_kernel::{Module, ModuleBuilder, Navree};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Billing {
    currency: String,
}

impl ConfigSection for Billing {
    const KEY: &'static str = "billing";

    fn validate(&self) -> Result<(), String> {
        if self.currency.len() == 3 {
            Ok(())
        } else {
            Err("currency must be a 3-letter ISO code".to_owned())
        }
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Queue {
    workers: u32,
}

impl ConfigSection for Queue {
    const KEY: &'static str = "queue";

    fn validate(&self) -> Result<(), String> {
        if self.workers >= 1 {
            Ok(())
        } else {
            Err("workers must be at least 1".to_owned())
        }
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Mail {
    transport: String,
}

impl ConfigSection for Mail {
    const KEY: &'static str = "mail";
}

struct BillingModule;
struct QueueModule;
struct MailModule;

impl Module for BillingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<Billing>();
    }
}

impl Module for QueueModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<Queue>();
    }
}

impl Module for MailModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<Mail>();
    }
}

async fn boot(sources: ConfigSources) -> Result<navree_kernel::App, navree_kernel::BootErrors> {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(BillingModule)
        .module(QueueModule)
        .module(MailModule)
        .boot()
        .await
}

#[tokio::test]
async fn three_bad_sections_give_three_messages_at_once() {
    let errors = boot(
        ConfigSources::new()
            .set("billing.currency", "EURO")
            .set("queue.workers", 0)
            .set("mail.transport", json_null()),
    )
    .await
    .map(drop)
    .expect_err("boot must fail");
    let text = errors.to_string();
    assert!(text.contains("configuration has 3 problems"), "{text}");
    assert!(
        text.contains("`billing`") && text.contains("3-letter ISO code"),
        "{text}"
    );
    assert!(
        text.contains("`queue`") && text.contains("workers must be at least 1"),
        "{text}"
    );
    assert!(text.contains("`mail`"), "{text}");
}

fn json_null() -> serde_json::Value {
    serde_json::Value::Null
}

#[tokio::test]
async fn one_bad_section_keeps_its_own_message() {
    let errors = boot(
        ConfigSources::new()
            .set("billing.currency", "EURO")
            .set("queue.workers", 2)
            .set("mail.transport", "log"),
    )
    .await
    .map(drop)
    .expect_err("boot must fail");
    let text = errors.to_string();
    assert!(!text.contains("problems"), "{text}");
    assert!(
        text.contains("configuration section `billing` failed validation"),
        "{text}"
    );
    assert!(
        !text.contains("`queue`") && !text.contains("`mail`"),
        "{text}"
    );
}

#[tokio::test]
async fn valid_sections_boot_as_before() {
    let app = boot(
        ConfigSources::new()
            .set("billing.currency", "EUR")
            .set("queue.workers", 2)
            .set("mail.transport", "log"),
    )
    .await
    .expect("boot");
    assert_eq!(app.ctx().get::<Queue>().await.expect("queue").workers, 2);
}

#[tokio::test]
async fn the_app_section_is_one_of_the_problems() {
    let errors = boot(
        ConfigSources::new()
            .set("billing.currency", "EURO")
            .set("app.url", "nowhere"),
    )
    .await
    .map(drop)
    .expect_err("boot must fail");
    let text = errors.to_string();
    assert!(text.contains("configuration has"), "{text}");
    assert!(
        text.contains("app.url must be an absolute http or https address"),
        "{text}"
    );
    assert!(text.contains("3-letter ISO code"), "{text}");
}

#[test]
fn registrations_name_their_section_and_check_it_without_booting() {
    let registration = SectionRegistration::of::<Queue>();
    assert_eq!(registration.key(), "queue");
    assert!(registration.type_name().ends_with("Queue"));
    let good = Config::load(&ConfigSources::new().set("queue.workers", 1)).expect("load");
    assert!(registration.check(&good).is_ok());
    let bad = Config::load(&ConfigSources::new().set("queue.workers", 0)).expect("load");
    assert!(matches!(
        registration.check(&bad),
        Err(ConfigError::Validation { .. })
    ));
    let missing = Config::load(&ConfigSources::new()).expect("load");
    assert!(matches!(
        registration.check(&missing),
        Err(ConfigError::Section { .. })
    ));
    assert!(registration.defaults().is_none());
    let _ = Arc::new(registration);
}

#[test]
fn the_invalid_error_lists_one_problem_per_line() {
    let error = ConfigError::Invalid {
        problems: vec![
            ConfigError::MissingSection {
                key: "a".to_owned(),
            },
            ConfigError::MissingSection {
                key: "b".to_owned(),
            },
        ],
    };
    assert_eq!(
        error.to_string(),
        "configuration has 2 problems:\n  - configuration section `a` is missing\n  - configuration section `b` is missing"
    );
}

//! `MailModule`: `for_root`, `from_config`, global exports, the transport
//! as a swappable trait object, and boot-time failures.

use std::sync::Arc;

use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::Job;
use navree_contracts::JobRegistration;
use navree_kernel::{
    App, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, ProviderKey, Resolver,
};
use navree_mail::{ArrayTransport, MailModule, MailTransport, Mailer, SendMailJob, TemplateMail};
use serde_json::json;

use crate::support::{FROM, MailTemplates, array_options, view_module};

/// A provider in another module that injects `Mailer` without importing
/// `MailModule` (it is global).
struct Welcomer {
    mailer: Arc<Mailer>,
}

impl Injectable for Welcomer {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Mailer>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Welcomer {
                mailer: r.get::<Mailer>().await?,
            })
        })
    }
}

struct UsersModule;

impl Module for UsersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.provider::<Welcomer>().exports::<Welcomer>();
    }
}

fn welcome() -> TemplateMail {
    TemplateMail::new("anu@example.com", "Welcome", "mail/welcome")
        .context(json!({ "name": "Anu" }))
}

async fn mailer(app: &App) -> Arc<Mailer> {
    app.ctx().get::<Mailer>().await.expect("Mailer")
}

#[tokio::test]
async fn for_root_exports_mailer_to_every_module() {
    let app = Navree::app()
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::for_root(array_options()))
        .module(UsersModule)
        .boot()
        .await
        .expect("boot");

    let welcomer = app.ctx().get::<Welcomer>().await.expect("Welcomer");
    welcomer.mailer.send(&welcome()).await.expect("send");

    let array = welcomer.mailer.array_transport().expect("array transport");
    array.assert_sent(|m| {
        m.from.as_deref() == Some(FROM)
            && m.to == ["anu@example.com"]
            && m.html.as_deref() == Some("<p>Welcome, Anu!</p>")
            && m.text.as_deref() == Some("Welcome, Anu!")
    });
}

/// Boots `module` with section `mail` set and checks the mailer uses it.
async fn assert_reads_section_mail(module: impl Module) {
    let config = ConfigSources::new().set(
        "mail",
        json!({ "from": "Agra <hello@acme.test>", "transport": "array" }),
    );
    let app = Navree::app()
        .module(ConfigModule::for_root(config))
        .module(view_module())
        .module(MailTemplates)
        .module(module)
        .boot()
        .await
        .expect("boot");

    let mailer = mailer(&app).await;
    mailer.send(&welcome()).await.expect("send");

    let array = mailer.array_transport().expect("array transport");
    array.assert_sent(|m| m.from.as_deref() == Some("Agra <hello@acme.test>"));
}

#[tokio::test]
async fn from_config_reads_section_mail() {
    assert_reads_section_mail(MailModule::from_config()).await;
}

#[tokio::test]
async fn plain_module_reads_section_mail() {
    assert_reads_section_mail(MailModule).await;
}

#[tokio::test]
async fn transport_is_exported_and_can_be_overridden() {
    let fake = ArrayTransport::new();
    let app = Navree::app()
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::for_root(navree_mail::MailOptions {
            transport: navree_mail::TransportConfig::smtp("smtp://localhost:1025"),
            ..array_options()
        }))
        .override_value::<dyn MailTransport>(Arc::new(fake.clone()))
        .boot()
        .await
        .expect("boot");

    app.ctx()
        .get::<dyn MailTransport>()
        .await
        .expect("exported transport");
    mailer(&app).await.send(&welcome()).await.expect("send");

    fake.assert_sent(|m| m.subject == "Welcome");
}

#[tokio::test]
async fn mail_module_registers_the_send_job() {
    let app = Navree::app()
        .module(view_module())
        .module(MailModule::for_root(array_options()))
        .boot()
        .await
        .expect("boot");

    let registrations = app.ctx().contributions::<JobRegistration>();

    assert!(
        registrations
            .iter()
            .any(|r| r.value.name == SendMailJob::NAME && r.value.name == "mail.send"),
        "registered: {:?}",
        registrations
            .iter()
            .map(|r| r.value.name)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn boot_fails_without_view_module() {
    let error = Navree::app()
        .module(MailModule::for_root(array_options()))
        .boot()
        .await
        .expect_err("Views is missing");

    let message = error.to_string();
    assert!(message.contains("navree_view::views::Views"), "{message}");
}

#[tokio::test]
async fn boot_fails_for_invalid_mail_config() {
    let config = ConfigSources::new().set(
        "mail",
        json!({ "transport": "smtp://mailer:s3cret@smtp.example.com?tls=never" }),
    );
    let error = Navree::app()
        .module(ConfigModule::for_root(config))
        .module(view_module())
        .module(MailModule::from_config())
        .boot()
        .await
        .expect_err("invalid transport");

    let message = error.to_string();
    assert!(message.contains("mail"), "{message}");
    assert!(message.contains("tls"), "{message}");
    assert!(!message.contains("s3cret"), "{message}");

    let error = Navree::app()
        .module(view_module())
        .module(MailModule::for_root(navree_mail::MailOptions {
            from: "not an address".to_owned(),
            ..array_options()
        }))
        .boot()
        .await
        .expect_err("invalid sender");
    assert!(error.to_string().contains("from"), "{error}");
}

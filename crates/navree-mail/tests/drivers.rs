//! Mail transports a module registers under a name: `mail.transport`, a
//! failover's list and `mail.transports` select one, a name read from a
//! request is only looked up, and a registration that resolves to two
//! things fails the boot and `config:check` alike.

use std::sync::{Arc, Mutex};

use navree_config::check::CheckOptions;
use navree_config::{ConfigModule, ConfigSection, ConfigSources, DriverName};
use navree_contracts::Secret;
use navree_kernel::{AppBuilder, BoxFuture, CommandSpec, Module, ModuleBuilder, Navree};
use navree_mail::{
    MailError, MailModule, MailOptions, MailTransport, Mailer, Message,
    ModuleBuilderMailTransportExt, PostmarkConfig, SesConfig, TransportConfig,
};
use navree_view::{ViewModule, ViewOptions};
use serde_json::{Value, json};

/// Keeps the subject of every message it is handed.
#[derive(Clone, Default)]
struct Dakghar {
    subjects: Arc<Mutex<Vec<String>>>,
}

impl Dakghar {
    fn subjects(&self) -> Vec<String> {
        self.subjects.lock().unwrap().clone()
    }
}

impl MailTransport for Dakghar {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        self.subjects.lock().unwrap().push(message.subject.clone());
        Box::pin(async { Ok(()) })
    }

    fn name(&self) -> &'static str {
        "dakghar"
    }
}

/// Registers the mail transport `dakghar` over a transport the test keeps.
#[derive(Default)]
struct DakgharModule {
    transport: Dakghar,
}

impl Module for DakgharModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let transport = self.transport.clone();
        m.mail_transport("dakghar", move |_| {
            let transport = transport.clone();
            async move { Ok(transport) }
        });
    }
}

/// Registers `dakghar` a second time.
struct SecondDakgharModule;

impl Module for SecondDakgharModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mail_transport("dakghar", |_| async { Ok(Dakghar::default()) });
    }
}

/// Registers a transport under the built-in type `smtp`.
struct SmtpLikeModule;

impl Module for SmtpLikeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mail_transport("smtp", |_| async { Ok(Dakghar::default()) });
    }
}

/// A module that registers one command.
struct CommandModule(CommandSpec);

impl Module for CommandModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.command(self.0.clone());
    }
}

/// `ViewModule` with no template directories.
fn views() -> impl Module {
    ViewModule::for_root(ViewOptions {
        dirs: Vec::new(),
        reload: false,
    })
}

/// Section `mail`: `fields` and the sender.
fn mail(fields: Value) -> ConfigSources {
    let mut section = fields;
    section["from"] = json!("Meera Sharma <meera@ludhiana.example>");
    ConfigSources::new().set("mail", section)
}

/// A message with `subject`.
fn message(subject: &str) -> Message {
    Message::new()
        .to("harpreet.sandhu@amritsar.example")
        .subject(subject)
        .text("Your invoice of ₹1,00,000 is ready.")
}

/// What `config:check` prints for an app built by `add`, without booting it.
async fn check_text(sources: ConfigSources, add: impl FnOnce(AppBuilder) -> AppBuilder) -> String {
    let out = Arc::new(Mutex::new(String::new()));
    let sink = out.clone();
    let probe = CommandModule(
        CommandSpec::new("probe:check", "config:check's report", move |ctx, _| {
            let sink = sink.clone();
            async move {
                let report =
                    navree_config::__private::check_report(&ctx, &CheckOptions::default()).await?;
                *sink.lock().unwrap() = report.render();
                Ok(())
            }
        })
        .offline(),
    );
    add(Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(probe))
    .run_with_args(vec!["probe:check".to_owned()])
    .await
    .unwrap();
    out.lock().unwrap().clone()
}

/// The boot errors of an app built by `add` over `ConfigModule::for_root(sources)`.
async fn boot(sources: ConfigSources, add: impl FnOnce(AppBuilder) -> AppBuilder) -> String {
    match add(Navree::app().module(ConfigModule::for_root(sources)))
        .boot()
        .await
    {
        Ok(_) => panic!("the app booted"),
        Err(errors) => errors.to_string(),
    }
}

#[tokio::test]
async fn a_registered_mail_transport_sends_and_joins_a_failover() {
    let module = DakgharModule::default();
    let transport = module.transport.clone();
    let app = Navree::app()
        .module(ConfigModule::for_root(mail(json!({
            "transport": "dakghar",
            "transports": {"otp": "dakghar"},
        }))))
        .module(views())
        .module(MailModule::from_config())
        .module(module)
        .boot()
        .await
        .unwrap();
    // The registration put the name in the process's names, so the
    // configuration reads it.
    let options: MailOptions = serde_json::from_value(json!({"transport": "dakghar"})).unwrap();
    assert_eq!(
        options.transport,
        TransportConfig::Registered(DriverName::from_static("dakghar"))
    );
    assert_eq!(options.transport.kind(), "dakghar");
    let mailer = app.ctx().get::<Mailer>().await.unwrap();
    mailer.send(&message("Invoice INV-1101")).await.unwrap();
    mailer
        .via("otp")
        .send(&message("Your sign-in code"))
        .await
        .unwrap();
    assert_eq!(
        transport.subjects(),
        ["Invoice INV-1101", "Your sign-in code"]
    );

    // A failover takes the name in its list.
    let module = DakgharModule::default();
    let transport = module.transport.clone();
    let app = Navree::app()
        .module(ConfigModule::for_root(mail(json!({
            "transport": {"type": "failover", "transports": ["dakghar", "log"]},
        }))))
        .module(views())
        .module(MailModule::from_config())
        .module(module)
        .boot()
        .await
        .unwrap();
    let mailer = app.ctx().get::<Mailer>().await.unwrap();
    mailer.send(&message("Invoice INV-1102")).await.unwrap();
    assert_eq!(transport.subjects(), ["Invoice INV-1102"]);

    // Built outside the module, a registered transport is refused.
    let error = TransportConfig::Registered(DriverName::from_static("dakghar"))
        .build()
        .expect_err("a registered transport is built by MailModule");
    assert_eq!(
        error.to_string(),
        "invalid mail configuration: mail transport `dakghar` is registered by a module; MailModule builds it at boot"
    );
    assert!(
        TransportConfig::Failover(vec![
            TransportConfig::Registered(DriverName::from_static("dakghar")),
            TransportConfig::Log,
        ])
        .build()
        .is_err()
    );
}

#[test]
fn a_made_up_transport_name_from_a_request_is_refused_and_not_kept() {
    for body in [
        json!("patra_post"),
        json!({"type": "failover", "transports": ["patra_post"]}),
        json!({"type": "failover", "transports": "log,patra_post"}),
    ] {
        let error = serde_json::from_value::<TransportConfig>(body.clone())
            .expect_err(&body.to_string())
            .to_string();
        assert!(error.contains("unknown mail transport (expected"), "{error}");
    }
    assert_eq!(DriverName::lookup("patra_post"), None, "reading adds no name");

    // A name the process knows reads as that transport; outside MailModule
    // it does not build, so a tenant's transport cannot be one.
    DriverName::register("dakghar").unwrap();
    let read: TransportConfig = serde_json::from_value(json!("dakghar")).unwrap();
    assert_eq!(
        read,
        TransportConfig::Registered(DriverName::from_static("dakghar"))
    );
    assert!(read.build().is_err());
    assert_eq!(read.to_json(), json!("dakghar"));
}

#[test]
fn mail_config_of_today_reads_as_before() {
    let read = |value: Value| serde_json::from_value::<TransportConfig>(value);
    assert_eq!(read(json!("log")).unwrap(), TransportConfig::Log);
    assert_eq!(read(json!("array")).unwrap(), TransportConfig::Array);
    assert_eq!(
        read(json!("smtp://localhost:1025")).unwrap(),
        TransportConfig::smtp("smtp://localhost:1025")
    );
    assert_eq!(
        read(json!("ses://KEY:SECRET@ap-south-1")).unwrap(),
        TransportConfig::Ses(SesConfig {
            region: "ap-south-1".to_owned(),
            access_key_id: "KEY".to_owned(),
            secret_access_key: Secret::new("SECRET".to_owned()),
            ..SesConfig::default()
        })
    );
    assert_eq!(
        read(json!({"type": "postmark", "server_token": "pm-token-ludhiana"})).unwrap(),
        TransportConfig::Postmark(PostmarkConfig {
            server_token: Secret::new("pm-token-ludhiana".to_owned()),
            ..PostmarkConfig::default()
        })
    );
    assert_eq!(
        read(json!({"type": "failover", "transports": "smtp://localhost:1025,log"})).unwrap(),
        TransportConfig::Failover(vec![
            TransportConfig::smtp("smtp://localhost:1025"),
            TransportConfig::Log,
        ])
    );
    assert_eq!(MailOptions::defaults().unwrap()["transport"], "log");
    assert_eq!(TransportConfig::Log.to_json(), json!({"type": "log"}));

    // A bare name no module registers keeps today's text, and so does a
    // bare built-in type that is not a short form.
    for bare in ["kabutar_dak", "smtp", "ses", "failover"] {
        let error = read(json!(bare)).unwrap_err().to_string();
        assert!(error.contains("unknown mail transport (expected"), "{error}");
    }
}

#[tokio::test]
async fn an_unregistered_or_doubly_registered_mail_transport_fails_the_boot_naming_m_mail_transport()
 {
    // A name the process knows (another kind's, say) that no module
    // registers as a mail transport.
    DriverName::register("chitthi_dak").unwrap();
    let unregistered = "mail.transport is `chitthi_dak`, but no module registers a mail transport under that name with m.mail_transport (known: log, array, smtp, smtps, ses, mailgun, postmark, resend, failover)";
    let sources = || mail(json!({"transport": "chitthi_dak"}));
    let add = |app: AppBuilder| app.module(views()).module(MailModule::from_config());
    let text = boot(sources(), add).await;
    assert!(text.contains(unregistered), "{text}");
    let text = check_text(sources(), add).await;
    assert!(text.contains(&format!("error    {unregistered}")), "{text}");

    let sources = || mail(json!({"transport": "log"}));

    let twice ="mail transport `dakghar` is registered by both `DakgharModule` and `SecondDakgharModule`; register it once";
    let add = |app: AppBuilder| {
        app.module(views())
            .module(MailModule::from_config())
            .module(DakgharModule::default())
            .module(SecondDakgharModule)
    };
    let text = boot(sources(), add).await;
    assert!(text.contains(twice), "checked although log is selected: {text}");
    let text = check_text(sources(), add).await;
    assert!(text.contains(&format!("error    {twice}")), "{text}");

    let built_in = "m.mail_transport(\"smtp\") in `SmtpLikeModule`: `smtp` is a built-in mail transport; register yours under another name";
    let add = |app: AppBuilder| {
        app.module(views())
            .module(MailModule::from_config())
            .module(SmtpLikeModule)
    };
    let text = boot(sources(), add).await;
    assert!(text.contains(built_in), "{text}");
    let text = check_text(sources(), add).await;
    assert!(text.contains(&format!("error    {built_in}")), "{text}");

    // Swapping the transport in a test still checks the registrations.
    let text = match Navree::app()
        .module(ConfigModule::for_root(sources()))
        .module(views())
        .module(MailModule::from_config())
        .module(DakgharModule::default())
        .module(SecondDakgharModule)
        .override_value::<dyn MailTransport>(Arc::new(Dakghar::default()))
        .boot()
        .await
    {
        Ok(_) => panic!("the app booted"),
        Err(errors) => errors.to_string(),
    };
    assert!(text.contains(twice), "{text}");

    // An app that registers nothing sees nothing new.
    let add = |app: AppBuilder| app.module(views()).module(MailModule::from_config());
    let text = check_text(sources(), add).await;
    assert!(text.contains("config:check: ok ("), "{text}");
}

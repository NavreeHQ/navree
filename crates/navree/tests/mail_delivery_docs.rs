//! The examples of `content/docs/digging-deeper/mail-delivery.mdx`, written
//! as an app writes them (the facade, the prelude and the macros), compiled
//! and run.

use std::sync::Mutex;

use navree::BoxFuture;
use navree::db::Db;
use navree::events::Listener;
use navree::mail::events::MailBounced;
use navree::mail::{
    CheckStatus, DeliveryStatus, DkimSigner, DnsResolver, DomainExpectations, MailEventKind,
    MailFake, MailScope, MailTransport, Mailer, Message, Outbox, ProviderEvent, SendingDomains,
    StaticResolver, StoredTransport, SuppressionReason, Suppressions, TransportConfig,
    TransportOwner, TransportResolver, TransportTier,
};
use navree::prelude::*;
use navree::testing::TestApp;
use navree::workspace::{CurrentWorkspace, InWorkspace};
use uuid::Uuid;

/// "Saving a tenant's transport": a workspace settings route.
#[injectable]
pub struct MailSettings {
    transports: Arc<TransportResolver>,
}

#[controller("/settings/mail")]
#[guard(InWorkspace::guard())]
impl MailSettings {
    #[put("/transport")]
    #[permissions("settings.mail")]
    async fn save(
        &self,
        uow: Uow,
        workspace: CurrentWorkspace,
        Json(config): Json<TransportConfig>,
    ) -> Result<Json<StoredTransport>> {
        self.transports.usable(&config).await?;
        let stored = self
            .transports
            .set(&uow, TransportOwner::workspace(workspace.id()), &config)
            .await?;
        Ok(Json(stored))
    }
}

/// Mounts the settings route.
#[module(controllers = [MailSettings])]
pub struct SettingsModule;

/// The contacts the listener flags.
#[derive(Default)]
pub struct ContactService {
    bounced: Mutex<Vec<(Option<Uuid>, String)>>,
}

impl ContactService {
    async fn mark_bounced(&self, workspace: Option<Uuid>, email: &str) -> Result<(), BoxError> {
        self.bounced
            .lock()
            .unwrap()
            .push((workspace, email.to_owned()));
        Ok(())
    }
}

/// "Provider events": flag a contact whose address bounced.
#[injectable]
pub struct FlagBouncedContact {
    contacts: Arc<ContactService>,
}

impl Listener<MailBounced> for FlagBouncedContact {
    fn handle<'a>(
        &'a self,
        event: &'a MailBounced,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            if let (true, Some(email)) = (event.permanent, &event.recipient) {
                self.contacts
                    .mark_bounced(event.workspace_id, email)
                    .await?;
            }
            Ok(())
        })
    }
}

/// Listens to `MailBounced`.
#[module(register = Self::wire)]
pub struct ContactsModule;

impl ContactsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.listen::<MailBounced, FlagBouncedContact>();
    }
}

async fn app(fake: &MailFake, contacts: &Arc<ContactService>) -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("mail.from", "Agra <hello@acme.test>")
        .config("mail.delivery.enabled", true)
        .config("mail.transports.campaigns", "array")
        .config("mail.delivery.routes", false)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(ContactsModule)
        .override_value::<dyn MailTransport>(fake.transport())
        .override_value(contacts.clone())
        .build()
        .await
}

fn newsletter(to: &str) -> Message {
    Message::new()
        .to(to)
        .subject("Spring news")
        .html("<p>Hi</p>")
        .header("List-Unsubscribe", "<https://acme.test/unsubscribe>")
        .metadata("campaign", "spring")
}

#[tokio::test]
async fn bounces_flag_the_contact_and_suppress_the_address() {
    let fake = MailFake::new();
    let contacts = Arc::new(ContactService::default());
    let app = app(&fake, &contacts).await;
    let mailer = app.get::<Mailer>().await;
    let workspace = Uuid::now_v7();

    let report = mailer
        .for_scope(&MailScope::workspace(workspace))
        .deliver(&newsletter("anu@example.com"))
        .await
        .unwrap();
    assert_eq!(report.status, DeliveryStatus::Sent);
    assert_eq!(report.tier, Some(TransportTier::Config));
    assert!(report.delivery_id.is_some());

    fake.bounce(app.ctx(), "anu@example.com").await.unwrap();

    assert_eq!(
        contacts.bounced.lock().unwrap().clone(),
        [(Some(workspace), "anu@example.com".to_owned())]
    );
    let suppressions = app.get::<Suppressions>().await;
    assert!(
        suppressions
            .is_suppressed(Some(workspace), "anu@example.com")
            .await
            .unwrap()
    );
    let again = mailer
        .for_scope(&MailScope::workspace(workspace))
        .deliver(&newsletter("anu@example.com"))
        .await
        .unwrap();
    assert_eq!(again.status, DeliveryStatus::Suppressed);
    assert_eq!(fake.sent_to("anu@example.com").len(), 1);
}

#[tokio::test]
async fn named_transports_suppressions_and_held_mail() {
    let fake = MailFake::new();
    let app = app(&fake, &Arc::new(ContactService::default())).await;
    let mailer = app.get::<Mailer>().await;

    // Named transports.
    let report = mailer
        .via("campaigns")
        .deliver(&newsletter("bal@example.com"))
        .await
        .unwrap();
    assert_eq!(report.transport, Some("array"));

    // Suppressions: an unsubscribe link.
    let suppressions = app.get::<Suppressions>().await;
    let uow = app.get::<Db>().await.uow(None, None).await;
    let workspace_id = Uuid::now_v7();
    suppressions
        .add(
            &uow,
            Some(workspace_id),
            "ela@example.com",
            SuppressionReason::Unsubscribed,
            None,
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert!(
        suppressions
            .is_suppressed(Some(workspace_id), "ela@example.com")
            .await
            .unwrap()
    );

    // Hold for approval.
    let mail = newsletter("all@acme.test").hold("Mail to every employee needs approval");
    let held = mailer.deliver(&mail).await.unwrap();
    assert_eq!(held.status, DeliveryStatus::Held);
    let outbox = app.get::<Outbox>().await;
    let released = outbox.release(held.outbox_id.unwrap(), None).await.unwrap();
    assert_eq!(released.status, DeliveryStatus::Sent);

    // Your own sources.
    let deliveries = app.get::<navree::mail::Deliveries>().await;
    let message_id = "relay-1".to_owned();
    let event = ProviderEvent::new("relay", message_id, MailEventKind::Bounced)
        .recipient("zoe@example.com")
        .permanent()
        .detail("550 5.1.1 user unknown");
    deliveries.record(event).await.unwrap();
    assert!(
        suppressions
            .is_suppressed(None, "zoe@example.com")
            .await
            .unwrap()
    );

    // Reading the tables directly: the records of `navree::mail::entity`.
    use navree::db::Record as _;
    use navree::mail::entity::DeliveryRow;
    let db = app.get::<Db>().await;
    let sent = DeliveryRow::select()
        .eq("status", "sent")
        .count(&*db)
        .await
        .unwrap();
    assert!(sent >= 1);
}

#[tokio::test]
async fn tenant_transports_resolve_most_specific_first() {
    let fake = MailFake::new();
    let app = app(&fake, &Arc::new(ContactService::default())).await;
    let transports = app.get::<TransportResolver>().await;
    let workspace = Uuid::now_v7();
    transports
        .usable(&TransportConfig::Array)
        .await
        .expect("usable");
    let uow = app.get::<Db>().await.uow(None, None).await;
    transports
        .set(
            &uow,
            TransportOwner::workspace(workspace),
            &TransportConfig::Array,
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let report = app
        .get::<Mailer>()
        .await
        .for_scope(&MailScope::workspace(workspace))
        .deliver(&newsletter("anu@example.com"))
        .await
        .unwrap();

    assert_eq!(report.tier, Some(TransportTier::Workspace));
    let _ = SettingsModule;
}

#[tokio::test]
async fn dkim_records_and_domain_checks() {
    let pem = navree::crypto::jose::SigningKey::generate_ed25519()
        .to_pkcs8_pem()
        .unwrap();
    let signer = DkimSigner::from_pem("acme.test", "mail", &pem).unwrap();
    assert_eq!(signer.record_name(), "mail._domainkey.acme.test");
    let record = signer.dns_record();
    assert!(record.starts_with("v=DKIM1; k=ed25519; p="));

    let dns = StaticResolver::new()
        .record("acme.test", "v=spf1 include:amazonses.com ~all")
        .record("mail._domainkey.acme.test", &record)
        .record(
            "_dmarc.acme.test",
            "v=DMARC1; p=none; rua=mailto:dmarc@acme.test",
        );
    let domains = SendingDomains::new(Arc::new(dns) as Arc<dyn DnsResolver>);
    let report = domains
        .check(
            "acme.test",
            &DomainExpectations::new()
                .selector("mail")
                .spf_include("amazonses.com"),
        )
        .await
        .unwrap();
    assert!(report.ready());
    assert_eq!(report.dmarc.status, CheckStatus::Warn);
    assert_eq!(
        report.dmarc.problems,
        ["`p=none` only monitors: move to `quarantine` or `reject`"]
    );
}

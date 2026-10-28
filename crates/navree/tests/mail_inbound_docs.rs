//! The examples of `content/docs/digging-deeper/inbound-mail.mdx`, written
//! as an app writes them (the facade, the prelude and the macros), compiled
//! and run.

use std::sync::Mutex;

use navree::BoxFuture;
use navree::events::Listener;
use navree::integrations::{MailboxWatch, MailboxWatches};
use navree::mail::events::InboundMailReceived;
use navree::mail::inbound::{InboundStatus, StoredMail, Verdict, Verdicts};
use navree::mail::{
    Inbound, InboundError, InboundMail, Incoming, MailFake, MailTransport, MailboxContext,
    MailboxHandler, Mailer, Message, ReplyTokens,
};
use navree::prelude::*;
use navree::testing::TestApp;
use navree::webhooks::{AwsSns, SnsMessage, VerifiedWebhook};
use uuid::Uuid;

// ---------------------------------------------------------------------
// "Mailboxes": a closure mailbox.
// ---------------------------------------------------------------------

/// The support inbox the closure mailbox opens conversations in.
#[injectable]
pub struct SupportInbox {
    #[inject(default)]
    opened: Mutex<Vec<String>>,
}

impl SupportInbox {
    async fn open_conversation(&self, mail: &InboundMail) -> Result<(), BoxError> {
        self.opened
            .lock()
            .unwrap()
            .push(mail.subject.clone().unwrap_or_default());
        Ok(())
    }
}

/// Registers the support mailbox.
#[module(providers = [SupportInbox], exports = [SupportInbox], register = Self::wire)]
pub struct SupportModule;

impl SupportModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.mailbox(
            "support@acme.test",
            |mail: InboundMail, ctx: MailboxContext| async move {
                let inbox = ctx.get::<SupportInbox>().await?;
                inbox.open_conversation(&mail).await?;
                Ok(())
            },
        );
    }
}

// ---------------------------------------------------------------------
// "Handlers with dependencies": the ticket replies.
// ---------------------------------------------------------------------

/// The tickets service the reply mailbox writes to.
#[injectable]
pub struct Tickets {
    #[inject(default)]
    replies: Mutex<Vec<(Uuid, String, String)>>,
}

impl Tickets {
    async fn add_reply(
        &self,
        ticket: Uuid,
        from: &str,
        text: &str,
        _message_id: Option<&str>,
    ) -> Result<(), BoxError> {
        self.replies
            .lock()
            .unwrap()
            .push((ticket, from.to_owned(), text.to_owned()));
        Ok(())
    }

    async fn replies(&self, ticket: Uuid) -> Vec<String> {
        self.replies
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _, _)| *id == ticket)
            .map(|(_, _, text)| text.clone())
            .collect()
    }
}

/// The reply mailbox of tickets.
#[injectable]
pub struct TicketReplies {
    tickets: Arc<Tickets>,
}

impl MailboxHandler for TicketReplies {
    fn handle(
        &self,
        mail: InboundMail,
        ctx: MailboxContext,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if mail.is_auto_reply() {
                return Ok(()); // an out-of-office answer adds nothing to the ticket
            }
            let ticket = ctx.verified_uuid("token")?;
            let Some(text) = mail.reply_text().filter(|text| !text.is_empty()) else {
                return Err(ctx.reject("the reply is empty"));
            };
            let from = mail
                .from
                .as_ref()
                .map(|from| from.email.clone())
                .unwrap_or_default();
            self.tickets
                .add_reply(ticket, &from, &text, mail.message_id.as_deref())
                .await?;
            Ok(())
        })
    }
}

/// Registers the reply mailbox.
#[module(providers = [Tickets], exports = [Tickets], register = Self::wire)]
pub struct TicketsModule;

impl TicketsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.mailbox_with::<TicketReplies>("reply+{token}@in.acme.test");
    }
}

// ---------------------------------------------------------------------
// "Attachments and forwarded messages": reported phish.
// ---------------------------------------------------------------------

/// Analyzes reported messages.
#[injectable]
pub struct PhishAnalyzer {
    #[inject(default)]
    analyzed: Mutex<Vec<(String, String)>>,
}

impl PhishAnalyzer {
    async fn analyze(&self, reporter: &str, original: &InboundMail) -> Result<(), BoxError> {
        self.analyzed.lock().unwrap().push((
            reporter.to_owned(),
            original.subject.clone().unwrap_or_default(),
        ));
        Ok(())
    }
}

/// The reporting mailbox: analyzes each forwarded message.
pub async fn reported_phish(mail: InboundMail, ctx: MailboxContext) -> Result<(), BoxError> {
    let analyzer = ctx.get::<PhishAnalyzer>().await?;
    let reporter = mail
        .from
        .as_ref()
        .map(|from| from.email.clone())
        .unwrap_or_default();
    let mut found = false;
    for attachment in mail.attachments.iter().filter(|a| a.is_message()) {
        let original = attachment.as_mail()?;
        analyzer.analyze(&reporter, &original).await?;
        found = true;
    }
    if !found {
        return Err(ctx.reject("forward the suspicious message as an attachment"));
    }
    Ok(())
}

/// Registers the reporting mailbox.
#[module(providers = [PhishAnalyzer], exports = [PhishAnalyzer], register = Self::wire)]
pub struct SecurityModule;

impl SecurityModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.mailbox("phish@in.acme.test", reported_phish);
    }
}

// ---------------------------------------------------------------------
// "Reply tokens": the notification with a reply address.
// ---------------------------------------------------------------------

/// Ticket notifications with a reply address.
#[injectable]
pub struct TicketMail {
    mailer: Arc<Mailer>,
    tokens: Arc<ReplyTokens>,
}

impl TicketMail {
    /// Mails the agent's answer with the ticket's reply address.
    pub async fn agent_replied(&self, ticket: Uuid, to: &str, text: &str) -> Result<(), BoxError> {
        let reply_to = self
            .tokens
            .address_for("reply+{token}@in.acme.test", ticket)?;
        let message = Message::new()
            .to(to)
            .reply_to(reply_to)
            .subject("Re: your support request")
            .text(text);
        self.mailer.send_message(message).await?;
        Ok(())
    }
}

/// Provides the notification service.
#[module(providers = [TicketMail], exports = [TicketMail])]
pub struct NotificationsModule;

// ---------------------------------------------------------------------
// "Amazon SES": a verified SNS route of your own.
// ---------------------------------------------------------------------

/// Received SES mail through a verified SNS topic.
#[injectable]
pub struct SesInboundController {
    inbound: Arc<Inbound>,
}

#[controller("/mail/ses-inbound")]
impl SesInboundController {
    /// `POST /api/mail/ses-inbound`: received mail from SES, through a verified SNS topic.
    #[post("/")]
    #[public]
    async fn receive(&self, uow: Uow, hook: VerifiedWebhook<AwsSns>) -> Result<NoContent> {
        if !hook.mark_processed(&uow).await? {
            return Ok(NoContent);
        }
        let message: SnsMessage = hook.json()?;
        let notification: serde_json::Value = message
            .message_json()
            .map_err(|error| AppError::bad_request(error.to_string()))?;
        self.inbound.receive_ses(&notification).await?;
        Ok(NoContent)
    }
}

/// "Your own sources": `Inbound::receive` from an IMAP poller.
#[allow(dead_code)]
async fn from_imap(
    inbound: &Inbound,
    raw_bytes: Vec<u8>,
    uid: u32,
) -> Result<navree::mail::Received, InboundError> {
    let received = inbound
        .receive(
            Incoming::new("imap", raw_bytes)
                .provider_id(uid.to_string())
                .recipient("support@acme.test"),
        )
        .await?;
    Ok(received)
}

// ---------------------------------------------------------------------
// "Microsoft 365 and Gmail mailboxes".
// ---------------------------------------------------------------------

/// Watches a customer's reporting mailbox.
#[injectable]
pub struct ReportedMailboxes {
    watches: Arc<MailboxWatches>,
}

impl ReportedMailboxes {
    /// Starts watching `mailbox` with `connection`.
    pub async fn watch(&self, connection: Uuid, mailbox: &str) -> Result<MailboxWatch, BoxError> {
        Ok(self.watches.watch(connection, mailbox).await?)
    }
}

// ---------------------------------------------------------------------
// "The event": a queued listener that reads the stored message.
// ---------------------------------------------------------------------

/// The archive the listener indexes into.
#[injectable]
pub struct MailArchive {
    #[inject(default)]
    indexed: Mutex<Vec<(Uuid, String)>>,
}

impl MailArchive {
    async fn index(&self, id: Uuid, mail: &InboundMail) -> Result<(), BoxError> {
        self.indexed
            .lock()
            .unwrap()
            .push((id, mail.subject.clone().unwrap_or_default()));
        Ok(())
    }
}

/// Indexes every stored message.
#[injectable]
pub struct IndexInboundMail {
    inbound: Arc<Inbound>,
    archive: Arc<MailArchive>,
}

impl Listener<InboundMailReceived> for IndexInboundMail {
    fn handle<'a>(
        &'a self,
        event: &'a InboundMailReceived,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let Some(stored) = &event.stored else {
                return Ok(()); // mail.inbound.disk is not set
            };
            let mail = self.inbound.read(stored).await?;
            self.archive.index(event.id, &mail).await?;
            Ok(())
        })
    }

    fn queued(&self) -> bool {
        true
    }
}

/// Listens to `InboundMailReceived`.
#[module(providers = [MailArchive], exports = [MailArchive], register = Self::wire)]
pub struct ArchiveModule;

impl ArchiveModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.listen::<InboundMailReceived, IndexInboundMail>();
    }
}

// ---------------------------------------------------------------------
// The runs.
// ---------------------------------------------------------------------

/// The app of the page: tickets, support, security, the archive, a memory
/// disk for raw messages.
async fn app(fake: &MailFake) -> TestApp {
    TestApp::builder()
        .config("app.key", navree::crypto::generate_key())
        .config("mail.from", "Acme Support <support@acme.test>")
        .config("mail.inbound.disk", "mail")
        .config("storage.default", "mail")
        .config("storage.disks.mail.driver", "memory")
        .module(CryptoModule::from_config())
        .module(EventsModule::for_root())
        .module(StorageModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(TicketsModule)
        .module(SupportModule)
        .module(SecurityModule)
        .module(ArchiveModule)
        .module(NotificationsModule)
        .override_value::<dyn MailTransport>(fake.transport())
        .build()
        .await
}

/// "Testing": a reply lands on its ticket.
#[tokio::test]
async fn a_reply_lands_on_its_ticket() {
    let fake = MailFake::new();
    let app = TestApp::builder()
        .config("app.key", navree::crypto::generate_key())
        .module(CryptoModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(TicketsModule)
        .build()
        .await;
    let ticket = Uuid::now_v7();
    let reply_to = app
        .get::<ReplyTokens>()
        .await
        .address_for("reply+{token}@in.acme.test", ticket)
        .unwrap();

    let received = fake
        .receive(
            app.ctx(),
            Incoming::compose()
                .from("Ada Lovelace <ada@example.com>")
                .to(reply_to)
                .subject("Re: your support request")
                .text("Thanks, that fixed it.\n\nOn Monday, Acme Support wrote:\n> Try turning it off and on.")
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(received.status, InboundStatus::Handled);
    let replies = app.get::<Tickets>().await.replies(ticket).await;
    assert_eq!(replies, vec!["Thanks, that fixed it."]);
}

/// "Reply tokens": the notification's reply address, answered.
#[tokio::test]
async fn the_reply_address_of_a_notification_leads_back_to_its_ticket() {
    let fake = MailFake::new();
    let app = app(&fake).await;
    let ticket = Uuid::now_v7();

    app.get::<TicketMail>()
        .await
        .agent_replied(ticket, "ada@example.com", "We are on it.")
        .await
        .unwrap();
    let sent = fake.sent();
    let reply_to = sent[0].reply_to.clone().expect("reply-to");
    assert!(reply_to.starts_with("reply+"), "{reply_to}");

    let inbound = app.get::<Inbound>().await;
    let received = inbound
        .receive(
            Incoming::compose()
                .from("ada@example.com")
                .to(reply_to.to_ascii_uppercase())
                .text("Thank you!")
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(received.status, InboundStatus::Handled);
    assert_eq!(
        app.get::<Tickets>().await.replies(ticket).await,
        vec!["Thank you!"]
    );

    // An auto-reply is accepted and adds nothing; a forged token rejects.
    let auto = inbound
        .receive(
            Incoming::compose()
                .from("ada@example.com")
                .to(reply_to.clone())
                .header("Auto-Submitted", "auto-replied")
                .text("I am away.")
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(auto.status, InboundStatus::Handled);
    let forged = inbound
        .receive(
            Incoming::compose()
                .from("mallory@example.com")
                .to("reply+forged@in.acme.test")
                .text("Close it.")
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(forged.status, InboundStatus::Rejected);
    assert_eq!(
        app.get::<Tickets>().await.replies(ticket).await.len(),
        1,
        "only the real reply landed"
    );
}

/// "Mailboxes" and "The event": the closure mailbox, the stored message
/// and the queued listener (handled inline without a queue).
#[tokio::test]
async fn support_mail_is_stored_routed_and_archived() {
    let fake = MailFake::new();
    let app = app(&fake).await;

    let received = app
        .get::<Inbound>()
        .await
        .receive(
            Incoming::compose()
                .from("Grace <grace@example.com>")
                .to("Support@Acme.test")
                .subject("Invoice question")
                .text("Where is my invoice?")
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(received.mailbox.as_deref(), Some("support@acme.test"));
    let stored: StoredMail = received.stored.clone().expect("stored");
    assert_eq!(stored.disk, "mail");
    assert!(stored.path.starts_with("inbound/"));
    let archived = app
        .get::<MailArchive>()
        .await
        .indexed
        .lock()
        .unwrap()
        .clone();
    assert_eq!(archived, vec![(received.id, "Invoice question".to_owned())]);
    assert_eq!(
        app.get::<Inbound>()
            .await
            .read(&stored)
            .await
            .unwrap()
            .subject
            .as_deref(),
        Some("Invoice question")
    );
}

/// "Attachments and forwarded messages": a reported phish, and a report
/// without the message attached.
#[tokio::test]
async fn reported_phish_is_analyzed_from_the_forwarded_message() {
    let fake = MailFake::new();
    let app = app(&fake).await;
    let original = Incoming::compose()
        .from("IT Desk <helpdesk@bad.example>")
        .to("joan@acme.test")
        .subject("Your mailbox is full")
        .html("<a href=\"https://bad.example/login\">Keep your mail</a>")
        .verdicts(Verdicts {
            spam: Some(Verdict::Fail),
            ..Verdicts::default()
        })
        .to_bytes();

    let received = fake
        .receive(
            app.ctx(),
            Incoming::compose()
                .from("Joan Clarke <joan@acme.test>")
                .to("phish@in.acme.test")
                .subject("Fwd: Your mailbox is full")
                .text("This looks suspicious.")
                .forward("Your mailbox is full.eml", original)
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(received.status, InboundStatus::Handled);
    assert_eq!(
        app.get::<PhishAnalyzer>()
            .await
            .analyzed
            .lock()
            .unwrap()
            .clone(),
        vec![(
            "joan@acme.test".to_owned(),
            "Your mailbox is full".to_owned()
        )]
    );

    let without = fake
        .receive(
            app.ctx(),
            Incoming::compose()
                .from("joan@acme.test")
                .to("phish@in.acme.test")
                .text("I got a weird mail.")
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(without.status, InboundStatus::Rejected);
}

/// "Sources": the raw route answers with what became of the message.
#[tokio::test]
async fn the_raw_route_receives_mime() {
    let fake = MailFake::new();
    let app = TestApp::builder()
        .config("mail.inbound.raw.token", "s3cret")
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(SupportModule)
        .build()
        .await;
    let _ = fake;

    let raw = Incoming::compose()
        .from("ada@example.com")
        .to("support@acme.test")
        .subject("Hello")
        .text("Hi")
        .to_bytes();
    let response = app
        .client()
        .post("/api/mail/inbound/raw")
        .bearer("s3cret")
        .header("content-type", "message/rfc822")
        .header("x-envelope-to", "support@acme.test")
        .body(raw)
        .send()
        .await
        .assert_status(200);

    let body = response.json_value();
    assert_eq!(body["status"], "handled");
    assert_eq!(body["mailbox"], "support@acme.test");
    assert_eq!(body["reason"], serde_json::Value::Null);
}

/// The other examples compile: the SES controller, `Inbound::receive`
/// from your own source, and the mailbox watches.
#[test]
fn the_other_examples_compile() {
    let _ = from_imap;
    let _ = ReportedMailboxes::watch;
    let _: Option<SesInboundController> = None;
    let _ = <Inbound>::receive_ses;
}

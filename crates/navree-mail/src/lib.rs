//! # navree-mail
//!
//! `MailModule`: mailables rendered from templates, transports for SMTP and
//! the email APIs, queued sending, and the delivery pipeline of a product
//! that sends mail for its customers.
//!
//! - [`MailModule`] is global: it provides and exports [`Mailer`] and
//!   `Arc<dyn MailTransport>`, configured with [`MailOptions`] (section
//!   `mail`), and registers job `mail.send` ([`SendMailJob`]). The app must
//!   include `ViewModule`.
//! - A [`Mailable`] renders into a [`Message`] with the app's
//!   [`Views`](navree_view::Views); [`TemplateMail`] renders the
//!   `{template}.html` / `{template}.txt` pair. The mailer inlines CSS,
//!   generates the text body of HTML-only mail and sets `lang`/`dir` from
//!   the message's locale ([`render`], [`RenderOptions`]).
//! - [`Mailer::send`] delivers now and [`Mailer::deliver`] also returns a
//!   [`SendReport`]; [`Mailer::queue`] renders and validates now, then
//!   dispatches `mail.send` through the app's
//!   [`Dispatcher`](navree_contracts::Dispatcher) (or sends now when the app
//!   has none). [`Mailer::via`] picks a named transport
//!   (`mail.transports`), [`Mailer::for_scope`] sends for a tenant
//!   ([`MailScope`]).
//! - Transports ([`TransportConfig`]): [`SmtpTransport`] (lettre on Tokio
//!   with rustls + ring, DKIM-signed with a [`DkimSigner`]),
//!   [`SesTransport`] (SigV4), [`MailgunTransport`], [`PostmarkTransport`],
//!   [`ResendTransport`], [`LogTransport`] (development),
//!   [`ArrayTransport`] (tests) and [`FailoverTransport`]. Implement
//!   [`MailTransport`] for your own and swap it in with
//!   `AppBuilder::override_value::<dyn MailTransport>`.
//! - With `mail.delivery.enabled` (and `DatabaseModule`; migration set
//!   `mail`): [`TransportResolver`] (transports per domain, workspace,
//!   organization and platform, stored encrypted), [`Deliveries`] (the
//!   delivery log and provider events, emitted as [`events`]),
//!   [`Suppressions`], [`Outbox`] (mail held for approval), open and click
//!   tracking, the provider webhook and admin routes of [`MailController`]
//!   and the commands `mail:test` and `mail:suppressions`. The tables
//!   (PostgreSQL, MySQL and SQLite) are navree-db records in [`entity`]
//!   (`DeliveryRow`, `EventRow`, ...): read them freely, change them
//!   through the services.
//! - [`SendingDomains`] checks SPF, DKIM and DMARC (`mail:domains`);
//!   [`MailFake`] records mail and plays the provider in tests.
//! - Inbound mail (crate feature `inbound`, module [`inbound`]): received
//!   messages parsed into [`InboundMail`](inbound::InboundMail), routed to
//!   mailboxes by address (`m.mailbox("reply+{token}@in.acme.test", handler)`,
//!   [`ModuleBuilderMailExt`](inbound::ModuleBuilderMailExt)) through
//!   [`Inbound`](inbound::Inbound), with signed reply addresses
//!   ([`ReplyTokens`](inbound::ReplyTokens)), raw `.eml` files on a
//!   navree-storage disk, the SES, Mailgun, Postmark and raw MIME source
//!   routes under `/api/mail/inbound`, and `InboundMailReceived`.
//!
//! Every message is validated before it reaches a transport (sender, at
//! least one recipient, parseable addresses, no line break in the subject
//! or a header). Message bodies and credentials never appear in errors or
//! in `INFO`-level logs.
//!
//! ```
//! use navree_kernel::{Navree, Module, ModuleBuilder};
//! use navree_mail::{MailModule, MailOptions, Mailer, TemplateMail, TransportConfig};
//! use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions};
//! use serde_json::json;
//!
//! struct Templates;
//!
//! impl Module for Templates {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.templates(&[
//!             ("mail/welcome.html", "<p>Welcome, {{ name }}!</p>"),
//!             ("mail/welcome.txt", "Welcome, {{ name }}!"),
//!         ]);
//!     }
//! }
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! let app = Navree::app()
//!     .module(ViewModule::for_root(ViewOptions { dirs: vec![], reload: false }))
//!     .module(Templates)
//!     .module(MailModule::for_root(MailOptions {
//!         from: "Acme <hello@acme.test>".into(),
//!         transport: TransportConfig::Array,
//!         ..MailOptions::default()
//!     }))
//!     .boot()
//!     .await?;
//!
//! let mailer = app.ctx().get::<Mailer>().await?;
//! mailer
//!     .send(
//!         &TemplateMail::new("ada@example.com", "Welcome", "mail/welcome")
//!             .context(json!({ "name": "Ada" })),
//!     )
//!     .await?;
//!
//! let sent = mailer.array_transport().expect("array transport").sent();
//! assert_eq!(sent[0].from.as_deref(), Some("Acme <hello@acme.test>"));
//! assert_eq!(sent[0].html.as_deref(), Some("<p>Welcome, Ada!</p>"));
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod api;
mod array;
mod attachment;
mod commands;
mod delivery;
mod dkim;
mod domains;
mod error;
mod fake;
#[cfg(feature = "inbound")]
pub mod inbound;
mod job;
mod mailable;
mod mailer;
mod message;
mod module;
mod options;
pub mod render;
mod report;
mod smtp;
mod sns;
mod transport;
mod transport_config;

pub use api::mailgun::{MailgunConfig, MailgunRegion, MailgunTransport};
pub use api::postmark::{PostmarkConfig, PostmarkTransport};
pub use api::resend::{ResendConfig, ResendTransport};
pub use api::ses::{SesConfig, SesTransport};
pub use array::ArrayTransport;
pub use attachment::{
    Attachment, BLOCKED_EXTENSIONS, MAILGUN_MAX_MESSAGE_BYTES, POSTMARK_MAX_MESSAGE_BYTES,
    RESEND_MAX_MESSAGE_BYTES, SES_MAX_MESSAGE_BYTES, SMTP_MAX_MESSAGE_BYTES, encoded_size,
};
pub use delivery::controller::{
    AddSuppressionInput, DeliveryWithEvents, INVALID_SIGNATURE_CODE, MANAGE_PERMISSION,
    MailController, ReleasedMail, UNUSABLE_TRANSPORT_CODE, UsableTransport,
};
pub use delivery::ingest::{MailEventKind, ProviderEvent};
pub use delivery::log::{Deliveries, Delivery, DeliveryEvent, DeliveryFilter};
pub use delivery::migration::MAIL_MIGRATIONS;
pub use delivery::outbox::{HeldMail, Outbox, OutboxStatus};
pub use delivery::resolver::{
    ResolvedTransport, StoredTransport, TransportOwner, TransportResolver,
};
pub use delivery::suppressions::{Suppression, SuppressionReason, Suppressions};
pub use delivery::{entity, events};
pub use dkim::{DEFAULT_SIGNED_HEADERS, DkimAlgorithm, DkimConfig, DkimSigner};
pub use domains::{
    CheckStatus, DkimCheck, DnsResolver, DomainExpectations, DomainReport, RecordCheck,
    SendingDomains, StaticResolver, SystemResolver,
};
pub use error::MailError;
pub use fake::MailFake;
#[cfg(feature = "inbound")]
pub use inbound::{
    Inbound, InboundError, InboundMail, InboundOptions, Incoming, MailboxContext, MailboxHandler,
    ModuleBuilderMailExt, Received, ReplyTokens,
};
pub use job::SendMailJob;
pub use mailable::{Held, Mailable, TemplateMail};
pub use mailer::{Mailer, ScopedMailer};
pub use message::Message;
pub use module::MailModule;
pub use options::{
    DeliveryOptions, MailOptions, MailgunWebhookOptions, PostmarkWebhookOptions,
    ResendWebhookOptions, SesWebhookOptions, TrackingOptions, WebhookOptions,
};
pub use render::RenderOptions;
pub use report::{DeliveryStatus, MailScope, SendReport, TransportTier};
pub use smtp::{SmtpTls, SmtpTransport};
pub use transport::{FailoverTransport, LogTransport, MailTransport, Sent};
pub use transport_config::TransportConfig;

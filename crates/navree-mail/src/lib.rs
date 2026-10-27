//! # navree-mail
//!
//! `MailModule`: mailables rendered from templates, SMTP, log, array and
//! failover transports, and queued sending.
//!
//! - [`MailModule`] is global: it provides and exports [`Mailer`] and
//!   `Arc<dyn MailTransport>`, configured with [`MailOptions`] (section
//!   `mail`), and registers job `mail.send` ([`SendMailJob`]). The app must
//!   include `ViewModule`.
//! - A [`Mailable`] renders into a [`Message`] with the app's
//!   [`Views`](navree_view::Views); [`TemplateMail`] renders the
//!   `{template}.html` / `{template}.txt` pair.
//! - [`Mailer::send`] delivers now; [`Mailer::queue`] renders and validates
//!   now, then dispatches `mail.send` through the app's
//!   [`Dispatcher`](navree_contracts::Dispatcher) (or sends now when the app has
//!   none).
//! - Transports ([`TransportConfig`]): [`SmtpTransport`] (lettre on Tokio
//!   with rustls + ring), [`LogTransport`] (development), [`ArrayTransport`]
//!   (tests) and [`FailoverTransport`]. Implement [`MailTransport`] for your
//!   own and swap it in with `AppBuilder::override_value::<dyn MailTransport>`.
//!
//! Every message is validated before it reaches a transport (sender, at
//! least one recipient, parseable addresses, no line break in the subject).
//! Message bodies and SMTP credentials never appear in errors or in
//! `INFO`-level logs.
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

mod array;
mod api;
mod delivery;
mod dkim;
mod domains;
mod error;
mod job;
mod mailable;
mod mailer;
mod message;
mod module;
mod options;
pub mod render;
mod report;
mod smtp;
mod transport;
mod transport_config;

pub use api::mailgun::{MailgunConfig, MailgunRegion, MailgunTransport};
pub use api::postmark::{PostmarkConfig, PostmarkTransport};
pub use api::resend::{ResendConfig, ResendTransport};
pub use api::ses::{SesConfig, SesTransport};
pub use array::ArrayTransport;
pub use dkim::{DEFAULT_SIGNED_HEADERS, DkimAlgorithm, DkimConfig, DkimSigner};
pub use domains::{
    CheckStatus, DkimCheck, DnsResolver, DomainExpectations, DomainReport, RecordCheck,
    SendingDomains, StaticResolver, SystemResolver,
};
pub use error::MailError;
pub use job::SendMailJob;
pub use mailable::{Held, Mailable, TemplateMail};
pub use mailer::{Mailer, ScopedMailer};
pub use message::Message;
pub use module::MailModule;
pub use options::{
    DeliveryOptions, MailOptions, MailgunWebhookOptions, PostmarkWebhookOptions,
    ResendWebhookOptions, SesWebhookOptions, TrackingOptions, WebhookOptions,
};
pub use transport_config::TransportConfig;
pub use render::RenderOptions;
pub use report::{DeliveryStatus, MailScope, SendReport, TransportTier};
pub use smtp::{SmtpTls, SmtpTransport};
pub use transport::{FailoverTransport, LogTransport, MailTransport, Sent};

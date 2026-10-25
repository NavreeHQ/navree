//! # keel-mail
//!
//! `MailModule`: mailables rendered from templates, SMTP, log, array and
//! failover transports, and queued sending.
//!
//! - [`MailModule`] is global: it provides and exports [`Mailer`] and
//!   `Arc<dyn MailTransport>`, configured with [`MailOptions`] (section
//!   `mail`), and registers job `mail.send` ([`SendMailJob`]). The app must
//!   include `ViewModule`.
//! - A [`Mailable`] renders into a [`Message`] with the app's
//!   [`Views`](keel_view::Views); [`TemplateMail`] renders the
//!   `{template}.html` / `{template}.txt` pair.
//! - [`Mailer::send`] delivers now; [`Mailer::queue`] renders and validates
//!   now, then dispatches `mail.send` through the app's
//!   [`Dispatcher`](keel_contracts::Dispatcher) (or sends now when the app has
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
//! use keel_kernel::{Keel, Module, ModuleBuilder};
//! use keel_mail::{MailModule, MailOptions, Mailer, TemplateMail, TransportConfig};
//! use keel_view::{ModuleBuilderViewExt, ViewModule, ViewOptions};
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
//! let app = Keel::app()
//!     .module(ViewModule::for_root(ViewOptions { dirs: vec![], reload: false }))
//!     .module(Templates)
//!     .module(MailModule::for_root(MailOptions {
//!         from: "Acme <hello@acme.test>".into(),
//!         transport: TransportConfig::Array,
//!         queue: None,
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
mod error;
mod job;
mod mailable;
mod mailer;
mod message;
mod module;
mod options;
mod smtp;
mod transport;

pub use array::ArrayTransport;
pub use error::MailError;
pub use job::SendMailJob;
pub use mailable::{Mailable, TemplateMail};
pub use mailer::Mailer;
pub use message::Message;
pub use module::MailModule;
pub use options::{MailOptions, TransportConfig};
pub use smtp::{SmtpTls, SmtpTransport};
pub use transport::{FailoverTransport, LogTransport, MailTransport};

//! [`SmtpTransport`]: delivers over SMTP with lettre (Tokio, rustls + ring).

use std::fmt;
use std::sync::OnceLock;

use navree_kernel::BoxFuture;
use lettre::message::{MultiPart, SinglePart};
use lettre::transport::smtp::AsyncSmtpTransportBuilder;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use url::Url;

use crate::message::mailbox;
use crate::{MailError, MailTransport, Message};

/// How an [`SmtpTransport`] secures its connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpTls {
    /// Plain text (`smtp://host`, default port 25): only for local relays
    /// and development catchers such as Mailpit. Credentials, if any, would
    /// travel unencrypted.
    None,
    /// STARTTLS when the server offers it, plain text otherwise
    /// (`smtp://host?tls=opportunistic`, default port 587).
    Opportunistic,
    /// STARTTLS, required (`smtp://host?tls=required`, default port 587).
    Required,
    /// TLS from the first byte (`smtps://host`, default port 465).
    Implicit,
}

/// Delivers messages over SMTP (lettre's async transport on Tokio, with
/// rustls and the ring provider, and a connection pool).
///
/// Configured with a URL, `scheme://user:password@host:port/ehlo-name?tls=mode`:
///
/// | URL | Security | Default port |
/// |---|---|---|
/// | `smtps://user:pass@smtp.example.com` | TLS ([`SmtpTls::Implicit`]) | 465 |
/// | `smtp://user:pass@smtp.example.com?tls=required` | STARTTLS ([`SmtpTls::Required`]) | 587 |
/// | `smtp://smtp.example.com?tls=opportunistic` | STARTTLS if offered | 587 |
/// | `smtp://localhost:1025` | none | 25 |
///
/// Percent-encode reserved characters in the user name and password. The
/// password never appears in `Debug` output or error messages.
///
/// The connection (and the pool's background task) is created on the first
/// send, so building the transport needs no network and no runtime.
pub struct SmtpTransport {
    builder: AsyncSmtpTransportBuilder,
    inner: OnceLock<AsyncSmtpTransport<Tokio1Executor>>,
    host: String,
    port: u16,
    tls: SmtpTls,
    authenticated: bool,
}

impl SmtpTransport {
    /// A transport for the SMTP server at `url` (see the
    /// [URL format](SmtpTransport)). Errors are [`MailError::Config`] and do
    /// not repeat the URL.
    pub fn from_url(url: &str) -> Result<Self, MailError> {
        let invalid = |reason: &str| MailError::Config(format!("invalid SMTP URL: {reason}"));
        let parsed = Url::parse(url).map_err(|error| invalid(&error.to_string()))?;
        let host = match parsed.host_str() {
            Some(host) if !host.is_empty() => host.to_owned(),
            _ => return Err(invalid("missing host")),
        };
        let tls_param = parsed
            .query_pairs()
            .find(|(key, _)| key == "tls")
            .map(|(_, value)| value.into_owned());
        let (tls, default_port) = match (parsed.scheme(), tls_param.as_deref()) {
            ("smtp", None) => (SmtpTls::None, 25),
            ("smtp", Some("opportunistic")) => (SmtpTls::Opportunistic, 587),
            ("smtp", Some("required")) => (SmtpTls::Required, 587),
            ("smtps", None) => (SmtpTls::Implicit, 465),
            ("smtp" | "smtps", Some(_)) => {
                return Err(invalid(
                    "the `tls` parameter must be `required` or `opportunistic` (with `smtp://`)",
                ));
            }
            _ => return Err(invalid("the scheme must be `smtp` or `smtps`")),
        };
        let builder = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|error| invalid(&error.to_string()))?;
        Ok(Self {
            builder,
            inner: OnceLock::new(),
            host,
            port: parsed.port().unwrap_or(default_port),
            tls,
            authenticated: !parsed.username().is_empty(),
        })
    }

    /// The server's host name.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The server's port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// How the connection is secured.
    pub fn tls(&self) -> SmtpTls {
        self.tls
    }

    /// The lettre transport, built (with its pool) on first use.
    fn transport(&self) -> &AsyncSmtpTransport<Tokio1Executor> {
        self.inner.get_or_init(|| self.builder.clone().build())
    }
}

impl MailTransport for SmtpTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move {
            let email = to_lettre(message)?;
            self.transport()
                .send(email)
                .await
                .map_err(|error| MailError::Transport {
                    transport: "smtp",
                    source: Box::new(error),
                })?;
            Ok(())
        })
    }
}

impl fmt::Debug for SmtpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpTransport")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("authenticated", &self.authenticated)
            .finish_non_exhaustive()
    }
}

/// The lettre message for `message`: `text` and `html` become a
/// `multipart/alternative` body when both are set. Bcc recipients are in the
/// envelope only, never in the headers.
fn to_lettre(message: &Message) -> Result<lettre::Message, MailError> {
    let from = message.from.as_deref().ok_or(MailError::MissingSender)?;
    let mut builder = lettre::Message::builder()
        .from(mailbox("from", from)?)
        .subject(message.subject.as_str());
    for to in &message.to {
        builder = builder.to(mailbox("to", to)?);
    }
    for cc in &message.cc {
        builder = builder.cc(mailbox("cc", cc)?);
    }
    for bcc in &message.bcc {
        builder = builder.bcc(mailbox("bcc", bcc)?);
    }
    if let Some(reply_to) = &message.reply_to {
        builder = builder.reply_to(mailbox("reply_to", reply_to)?);
    }
    let built = match (&message.html, &message.text) {
        (Some(html), Some(text)) => builder.multipart(MultiPart::alternative_plain_html(
            text.clone(),
            html.clone(),
        )),
        (Some(html), None) => builder.singlepart(SinglePart::html(html.clone())),
        (None, Some(text)) => builder.singlepart(SinglePart::plain(text.clone())),
        (None, None) => builder.singlepart(SinglePart::plain(String::new())),
    };
    built.map_err(|error| MailError::Transport {
        transport: "smtp",
        source: Box::new(error),
    })
}

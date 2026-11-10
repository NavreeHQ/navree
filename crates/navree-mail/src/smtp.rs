//! [`SmtpTransport`]: delivers over SMTP with lettre (Tokio, rustls + ring).

use std::fmt;
use std::sync::OnceLock;

use lettre::message::header::{ContentType, HeaderName, HeaderValue};
use lettre::message::{Attachment as LettreAttachment, MultiPart, SinglePart};
use lettre::transport::smtp::AsyncSmtpTransportBuilder;
use lettre::transport::smtp::client::{Certificate, Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use navree_kernel::BoxFuture;
use url::Url;

use crate::attachment::{SMTP_MAX_MESSAGE_BYTES, check_size};
use crate::message::mailbox;
use crate::{DkimSigner, MailError, MailTransport, Message};

/// How an [`SmtpTransport`] secures its connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
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
///
/// A server whose certificate no public authority signed (a private
/// authority, or a self-signed certificate on a mail relay inside your
/// network) is trusted with
/// [`with_root_certificate`](SmtpTransport::with_root_certificate), or `ca`
/// in the transport's settings. The certificate is trusted besides the
/// public authorities, for this transport only; the server's name is still
/// checked.
///
/// With a [`DkimSigner`] ([`with_dkim`](SmtpTransport::with_dkim), or
/// `dkim` in the transport's settings), mail whose sender is in the
/// signing domain (or a subdomain) gets a `DKIM-Signature` header.
pub struct SmtpTransport {
    builder: AsyncSmtpTransportBuilder,
    inner: OnceLock<AsyncSmtpTransport<Tokio1Executor>>,
    host: String,
    port: u16,
    tls: SmtpTls,
    authenticated: bool,
    dkim: Option<DkimSigner>,
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
            dkim: None,
        })
    }

    /// This transport, DKIM-signing mail from `signer`'s domain.
    #[must_use]
    pub fn with_dkim(mut self, signer: DkimSigner) -> Self {
        self.dkim = Some(signer);
        self
    }

    /// The DKIM signer, if any.
    pub fn dkim(&self) -> Option<&DkimSigner> {
        self.dkim.as_ref()
    }

    /// This transport, also trusting the certificate (or certificate
    /// authority) in `pem`, a PEM text with one or more `CERTIFICATE`
    /// blocks. For a server with a private authority's certificate or a
    /// self-signed one; the public authorities stay trusted, and the
    /// server's name must still match its certificate.
    ///
    /// The transport must use TLS (`smtps://`, or `?tls=required` or
    /// `?tls=opportunistic`); plain text has no certificate to trust. An
    /// opportunistic transport still refuses a server whose certificate it
    /// does not trust, rather than falling back to plain text.
    ///
    /// Errors are [`MailError::Config`] and never repeat the text.
    pub fn with_root_certificate(mut self, pem: &str) -> Result<Self, MailError> {
        let invalid =
            |reason: &str| MailError::Config(format!("invalid SMTP certificate: {reason}"));
        if !pem.contains("-----BEGIN CERTIFICATE-----") {
            return Err(invalid("expected a PEM text with a CERTIFICATE block"));
        }
        let certificate = Certificate::from_pem(pem.as_bytes())
            .map_err(|_| invalid("the certificate cannot be read"))?;
        let parameters = TlsParameters::builder(self.host.clone())
            .add_root_certificate(certificate)
            .build_rustls()
            .map_err(|_| invalid("the certificate cannot be trusted"))?;
        let tls = match self.tls {
            SmtpTls::None => {
                return Err(invalid(
                    "the transport uses no TLS; use `smtps://` or `?tls=required`",
                ));
            }
            SmtpTls::Opportunistic => Tls::Opportunistic(parameters),
            SmtpTls::Required => Tls::Required(parameters),
            SmtpTls::Implicit => Tls::Wrapper(parameters),
        };
        self.builder = self.builder.tls(tls);
        Ok(self)
    }

    /// The message as it goes on the wire: lettre's formatting, with a
    /// `DKIM-Signature` header first when the sender's domain is signed.
    /// `None` when nothing is signed (lettre then sends the message itself).
    pub fn signed_bytes(&self, message: &Message) -> Result<Option<Vec<u8>>, MailError> {
        self.sign_email(message, &to_lettre(message, "smtp")?)
    }

    /// `email` (built from `message`) signed, when its sender's domain is.
    fn sign_email(
        &self,
        message: &Message,
        email: &lettre::Message,
    ) -> Result<Option<Vec<u8>>, MailError> {
        let Some(signer) = &self.dkim else {
            return Ok(None);
        };
        if !message
            .sender_domain()
            .is_some_and(|domain| signer.applies_to(&domain))
        {
            return Ok(None);
        }
        let formatted = email.formatted();
        let header = signer.sign(&formatted)?;
        let mut bytes = header.into_bytes();
        bytes.extend_from_slice(&formatted);
        Ok(Some(bytes))
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
            check_size("smtp", message, SMTP_MAX_MESSAGE_BYTES)?;
            let email = to_lettre(message, "smtp")?;
            let result = match self.sign_email(message, &email)? {
                Some(bytes) => self.transport().send_raw(email.envelope(), &bytes).await,
                None => self.transport().send(email).await,
            };
            result.map_err(|error| MailError::Transport {
                transport: "smtp",
                source: Box::new(error),
            })?;
            Ok(())
        })
    }

    fn name(&self) -> &'static str {
        "smtp"
    }

    /// Connects and authenticates (no message is sent).
    fn check(&self) -> BoxFuture<'_, Result<(), MailError>> {
        Box::pin(async move {
            match self.transport().test_connection().await {
                Ok(true) => Ok(()),
                Ok(false) => Err(MailError::Unusable(format!(
                    "the SMTP server at {}:{} did not accept the connection",
                    self.host, self.port
                ))),
                Err(error) => Err(MailError::Unusable(format!(
                    "cannot connect to the SMTP server at {}:{}: {error}",
                    self.host, self.port
                ))),
            }
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

/// The lettre message for `message` (built for `transport`, which errors
/// name): `text` and `html` become a `multipart/alternative` body when both
/// are set, and with attachments the body and the files go in a
/// `multipart/mixed`. Bcc recipients are in the envelope only, never in the
/// headers.
pub(crate) fn to_lettre(
    message: &Message,
    transport: &'static str,
) -> Result<lettre::Message, MailError> {
    let from = message.from.as_deref().ok_or(MailError::MissingSender)?;
    let sender = mailbox("from", from)?;
    let mut builder = lettre::Message::builder()
        .from(sender.clone())
        .subject(message.subject.as_str());
    // Every message carries a Message-ID (RFC 5322, section 3.6.4) unless the app set its
    // own: a random id on the sender's domain, which names no machine.
    if !message
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("message-id"))
    {
        builder = builder.message_id(Some(format!(
            "<{}@{}>",
            uuid::Uuid::new_v4().simple(),
            sender.email.domain()
        )));
    }
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
    for (name, value) in &message.headers {
        let header_name =
            HeaderName::new_from_ascii(name.clone()).map_err(|_| MailError::Header {
                name: name.clone(),
                reason: "a header name is printable ASCII without `:` or spaces",
            })?;
        builder = builder.raw_header(HeaderValue::new(header_name, value.clone()));
    }
    let body = match (&message.html, &message.text) {
        (Some(html), Some(text)) => Body::Alternative(MultiPart::alternative_plain_html(
            text.clone(),
            html.clone(),
        )),
        (Some(html), None) => Body::Single(SinglePart::html(html.clone())),
        (None, Some(text)) => Body::Single(SinglePart::plain(text.clone())),
        (None, None) => Body::Single(SinglePart::plain(String::new())),
    };
    let built = if message.attachments.is_empty() {
        match body {
            Body::Alternative(part) => builder.multipart(part),
            Body::Single(part) => builder.singlepart(part),
        }
    } else {
        let mut mixed = match body {
            Body::Alternative(part) => MultiPart::mixed().multipart(part),
            Body::Single(part) => MultiPart::mixed().singlepart(part),
        };
        for file in &message.attachments {
            let content_type =
                ContentType::parse(&file.content_type).map_err(|_| MailError::Attachment {
                    filename: file.filename.clone(),
                    reason: "the content type must be a MIME type such as `application/pdf`",
                })?;
            mixed = mixed.singlepart(
                LettreAttachment::new(file.filename.clone())
                    .body(file.content.clone(), content_type),
            );
        }
        builder.multipart(mixed)
    };
    built.map_err(|error| MailError::Transport {
        transport,
        source: Box::new(error),
    })
}

/// A message's body before attachments join it.
enum Body {
    Alternative(MultiPart),
    Single(SinglePart),
}

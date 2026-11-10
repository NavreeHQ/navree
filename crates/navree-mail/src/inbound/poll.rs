//! [`MailboxPoller`]: reads a mailbox over IMAP or POP3 and hands each
//! message to [`Inbound::receive`](super::Inbound::receive), for mail that
//! arrives in a mailbox and not through a provider's webhook.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use navree_contracts::Secret;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

use super::{Inbound, InboundError, Incoming, Received};

/// The longest line read from a server outside a message.
const LINE: u64 = 64 * 1024;

/// A step of a poll: an error is the reason, without secrets.
type Step<T> = Result<T, String>;

/// The protocol a mailbox is read with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PollProtocol {
    /// IMAP: messages are flagged `\Seen` (or deleted) when they are taken.
    Imap,
    /// POP3: a message has no flags, so a message taken is deleted.
    Pop3,
}

/// How the connection to the mailbox is secured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PollSecurity {
    /// TLS from the first byte: IMAPS on port 993, POP3S on 995.
    Tls,
    /// Plain text first, then TLS before the login (IMAP `STARTTLS`, POP3
    /// `STLS`): ports 143 and 110. A server that does not offer it is not
    /// logged in to.
    StartTls,
    /// Plain text, the password included. Only for a server on the same
    /// machine or in a test.
    None,
}

/// What happens to a message on the server once it was received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PollAfter {
    /// IMAP: the message is flagged `\Seen` and a later poll skips it.
    MarkSeen,
    /// The message is deleted from the server (IMAP: flagged `\Deleted` and
    /// the folder expunged).
    Delete,
}

/// A mailbox to read with [`Inbound::poll`]: the server, the login and what
/// to do with the messages.
///
/// ```
/// use navree_mail::inbound::{MailboxPoller, PollAfter};
///
/// let poller = MailboxPoller::imap("imap.example.com")
///     .login("support@example.com", "app-password")
///     .folder("INBOX")
///     .after(PollAfter::MarkSeen);
/// ```
///
/// Each poll reads the messages that are not yet taken (IMAP: unseen and not
/// deleted; POP3: all of them), up to [`max_messages`](Self::max_messages),
/// and runs them through [`Inbound::receive`](super::Inbound::receive). The
/// source of the messages is `imap` or `pop3` (change it with
/// [`source`](Self::source)), their `provider_id` the IMAP UID or the POP3
/// unique id.
///
/// Run one poll of a mailbox at a time: two at once may read the same
/// message twice. Call [`Inbound::poll`] from a scheduled job, as often as
/// the mail should be seen.
#[derive(Clone)]
pub struct MailboxPoller {
    protocol: PollProtocol,
    host: String,
    port: Option<u16>,
    security: PollSecurity,
    username: String,
    password: Secret<String>,
    folder: String,
    after: PollAfter,
    recipient: Option<String>,
    source: Option<String>,
    max_messages: usize,
    timeout: Duration,
    roots: Vec<CertificateDer<'static>>,
}

impl MailboxPoller {
    /// An IMAP mailbox at `host`: TLS on port 993, folder `INBOX`, messages
    /// flagged `\Seen` once received.
    pub fn imap(host: impl Into<String>) -> Self {
        Self::new(PollProtocol::Imap, host, PollAfter::MarkSeen)
    }

    /// A POP3 mailbox at `host`: TLS on port 995, messages deleted once
    /// received.
    pub fn pop3(host: impl Into<String>) -> Self {
        Self::new(PollProtocol::Pop3, host, PollAfter::Delete)
    }

    fn new(protocol: PollProtocol, host: impl Into<String>, after: PollAfter) -> Self {
        Self {
            protocol,
            host: host.into(),
            port: None,
            security: PollSecurity::Tls,
            username: String::new(),
            password: Secret::new(String::new()),
            folder: "INBOX".to_owned(),
            after,
            recipient: None,
            source: None,
            max_messages: 100,
            timeout: Duration::from_secs(30),
            roots: Vec::new(),
        }
    }

    /// The server's port (default: by protocol and [`security`](Self::security)).
    #[must_use]
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// How the connection is secured (default [`PollSecurity::Tls`]).
    #[must_use]
    pub fn security(mut self, security: PollSecurity) -> Self {
        self.security = security;
        self
    }

    /// The user name and password of the mailbox.
    #[must_use]
    pub fn login(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = username.into();
        self.password = Secret::new(password.into());
        self
    }

    /// The IMAP folder to read (default `INBOX`; ASCII names only).
    #[must_use]
    pub fn folder(mut self, folder: impl Into<String>) -> Self {
        self.folder = folder.into();
        self
    }

    /// What to do with a message once it was received (see [`PollAfter`]).
    /// POP3 can only delete.
    #[must_use]
    pub fn after(mut self, after: PollAfter) -> Self {
        self.after = after;
        self
    }

    /// The address the messages were delivered to, which routes them to a
    /// mailbox when the message's own headers do not name it (a blind copy,
    /// a list). Default: the user name, when it is an address.
    #[must_use]
    pub fn recipient(mut self, address: impl Into<String>) -> Self {
        self.recipient = Some(address.into());
        self
    }

    /// The `source` of the received messages (default `imap` or `pop3`).
    #[must_use]
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// The most messages one poll reads (default 100); the rest wait for the
    /// next poll.
    #[must_use]
    pub fn max_messages(mut self, max: usize) -> Self {
        self.max_messages = max;
        self
    }

    /// How long the server may take to answer one command (default 30
    /// seconds).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Trusts one more certificate or authority (PEM) besides the public
    /// authorities, for a server with a private certificate. The server's
    /// name is still checked.
    ///
    /// # Errors
    ///
    /// [`InboundError::Config`] when `pem` holds no certificate.
    pub fn root_certificate(mut self, pem: impl AsRef<[u8]>) -> Result<Self, InboundError> {
        let found: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(pem.as_ref())
            .collect::<Result<_, _>>()
            .map_err(|error| {
                InboundError::Config(format!("the root certificate cannot be read: {error}"))
            })?;
        if found.is_empty() {
            return Err(InboundError::Config(
                "the root certificate holds no certificate".to_owned(),
            ));
        }
        self.roots.extend(found);
        Ok(self)
    }

    fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(match (self.protocol, self.security) {
            (PollProtocol::Imap, PollSecurity::Tls) => 993,
            (PollProtocol::Imap, _) => 143,
            (PollProtocol::Pop3, PollSecurity::Tls) => 995,
            (PollProtocol::Pop3, _) => 110,
        })
    }

    fn source_name(&self) -> &str {
        self.source.as_deref().unwrap_or(match self.protocol {
            PollProtocol::Imap => "imap",
            PollProtocol::Pop3 => "pop3",
        })
    }

    /// The address the messages count as delivered to.
    fn delivered_to(&self) -> Option<&str> {
        self.recipient.as_deref().or_else(|| {
            self.username
                .contains('@')
                .then_some(self.username.as_str())
        })
    }

    fn check(&self) -> Result<(), InboundError> {
        let invalid = |reason: &str| InboundError::Config(format!("the mailbox poller: {reason}"));
        if self.host.trim().is_empty() {
            return Err(invalid("a host is needed"));
        }
        if self.username.is_empty() || self.password.expose().is_empty() {
            return Err(invalid("a user name and a password are needed"));
        }
        for (what, value) in [
            ("user name", self.username.as_str()),
            ("password", self.password.expose().as_str()),
            ("folder", self.folder.as_str()),
        ] {
            if value.contains(['\r', '\n', '\0']) {
                return Err(invalid(&format!("the {what} has a line break in it")));
            }
        }
        if self.protocol == PollProtocol::Pop3 && self.after == PollAfter::MarkSeen {
            return Err(invalid(
                "POP3 has no flag to mark a message read: use PollAfter::Delete",
            ));
        }
        if self.max_messages == 0 {
            return Err(invalid("max_messages is 0"));
        }
        Ok(())
    }
}

impl fmt::Debug for MailboxPoller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailboxPoller")
            .field("protocol", &self.protocol)
            .field("host", &self.host)
            .field("port", &self.port_or_default())
            .field("security", &self.security)
            .field("username", &self.username)
            .field("folder", &self.folder)
            .field("after", &self.after)
            .finish_non_exhaustive()
    }
}

/// What became of a message a poll handed to `receive`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Taken {
    /// Received: it may leave the mailbox.
    Done,
    /// It can never be received (too large, not a message).
    Refused,
    /// A handler or the storage failed: try again next time.
    Failed,
}

/// A message a poll could not receive, or left alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PollIssue {
    /// The IMAP UID or POP3 unique id.
    pub id: String,
    /// Why, without the message's content.
    pub reason: String,
}

/// What one [`Inbound::poll`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PollReport {
    /// What [`Inbound::receive`](super::Inbound::receive) answered for each
    /// message it took, in the order they were read.
    pub received: Vec<Received>,
    /// Messages that can never be received (over `mail.inbound.max_size`,
    /// not a message). They are never deleted, even with
    /// [`PollAfter::Delete`], so that someone can look at them: IMAP flags
    /// them `\Seen`, so that they are not read again, and POP3, which has no
    /// flag, leaves them to be listed again by every poll.
    pub refused: Vec<PollIssue>,
    /// Messages a mailbox handler or the storage failed on. They stay in the
    /// mailbox, so the next poll tries them again.
    pub failed: Vec<PollIssue>,
    /// Messages waiting that this poll did not read, because
    /// [`max_messages`](MailboxPoller::max_messages) was reached.
    pub left: usize,
}

impl Inbound {
    /// Reads the mailbox: connects, logs in, receives each waiting message
    /// ([`Inbound::receive`]) and then flags or deletes it, as
    /// [`PollAfter`] says. A message whose handler fails is left in the
    /// mailbox for the next poll (it is in [`PollReport::failed`]); a message
    /// that can never be received is in [`PollReport::refused`].
    ///
    /// # Errors
    ///
    /// [`InboundError::Config`] for a poller that cannot work, and
    /// [`InboundError::Mailbox`] when the server cannot be reached, refuses
    /// the login or breaks the protocol. Messages received before the error
    /// are handled, and may be read again by the next poll if they were not
    /// flagged yet.
    pub async fn poll(&self, poller: &MailboxPoller) -> Result<PollReport, InboundError> {
        poller.check()?;
        let max_size = self.options().max_size;
        let mailbox = |reason: String| InboundError::Mailbox {
            host: poller.host.clone(),
            reason,
        };
        let report = match poller.protocol {
            PollProtocol::Imap => self.poll_imap(poller, max_size).await,
            PollProtocol::Pop3 => self.poll_pop3(poller, max_size).await,
        }
        .map_err(mailbox)?;
        tracing::info!(
            target: "navree::mail",
            host = %poller.host,
            source = poller.source_name(),
            received = report.received.len(),
            refused = report.refused.len(),
            failed = report.failed.len(),
            left = report.left,
            "mailbox polled"
        );
        Ok(report)
    }

    /// Hands one message to [`receive`](Self::receive) and says what became
    /// of it.
    async fn take(
        &self,
        poller: &MailboxPoller,
        id: &str,
        raw: Vec<u8>,
        report: &mut PollReport,
    ) -> Taken {
        let mut incoming = Incoming::new(poller.source_name(), raw).provider_id(id);
        if let Some(address) = poller.delivered_to() {
            incoming = incoming.recipient(address);
        }
        match self.receive(incoming).await {
            Ok(received) => {
                report.received.push(received);
                Taken::Done
            }
            Err(error @ (InboundError::TooLarge { .. } | InboundError::Unparseable(_))) => {
                report.refused.push(PollIssue {
                    id: id.to_owned(),
                    reason: error.to_string(),
                });
                Taken::Refused
            }
            Err(error) => {
                report.failed.push(PollIssue {
                    id: id.to_owned(),
                    reason: error.to_string(),
                });
                Taken::Failed
            }
        }
    }

    async fn poll_imap(&self, poller: &MailboxPoller, max_size: usize) -> Step<PollReport> {
        let mut imap = Imap::open(poller, max_size).await?;
        imap.run(
            "LOGIN",
            &[
                Arg::Text(&poller.username),
                Arg::Text(poller.password.expose()),
            ],
        )
        .await
        .map_err(|reason| format!("the login was refused: {reason}"))?;
        imap.run("SELECT", &[Arg::Text(&poller.folder)]).await?;

        let found = imap
            .run("UID SEARCH", &[Arg::Atom("UNSEEN"), Arg::Atom("UNDELETED")])
            .await?;
        let mut uids: Vec<String> = found
            .lines
            .iter()
            .find_map(|line| line.strip_prefix("* SEARCH"))
            .map(|rest| {
                rest.split_whitespace()
                    .filter(|uid| uid.bytes().all(|byte| byte.is_ascii_digit()))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let mut report = PollReport::default();
        if uids.len() > poller.max_messages {
            report.left = uids.len() - poller.max_messages;
            uids.truncate(poller.max_messages);
        }

        let mut deleted = false;
        for uid in uids {
            let mut fetched = imap
                .run("UID FETCH", &[Arg::Atom(&uid), Arg::Atom("BODY.PEEK[]")])
                .await?;
            let flag = if let Some(size) = fetched.oversized.first() {
                report.refused.push(PollIssue {
                    id: uid.clone(),
                    reason: format!(
                        "the message is {size} bytes, over the {max_size}-byte limit (`mail.inbound.max_size`)"
                    ),
                });
                Some("(\\Seen)")
            } else if fetched.literals.is_empty() {
                // Gone since the search: another client took it.
                continue;
            } else {
                match self
                    .take(poller, &uid, fetched.literals.remove(0), &mut report)
                    .await
                {
                    Taken::Done => match poller.after {
                        PollAfter::MarkSeen => Some("(\\Seen)"),
                        PollAfter::Delete => {
                            deleted = true;
                            Some("(\\Deleted)")
                        }
                    },
                    // Never to be received: flagged, so it is not read again,
                    // but kept.
                    Taken::Refused => Some("(\\Seen)"),
                    Taken::Failed => None,
                }
            };
            if let Some(flag) = flag {
                imap.run(
                    "UID STORE",
                    &[Arg::Atom(&uid), Arg::Atom("+FLAGS.SILENT"), Arg::Atom(flag)],
                )
                .await?;
            }
        }
        if deleted {
            imap.run("EXPUNGE", &[]).await?;
        }
        // The server may close the connection on its own answer; the work is done.
        let _ = imap.run("LOGOUT", &[]).await;
        Ok(report)
    }

    async fn poll_pop3(&self, poller: &MailboxPoller, max_size: usize) -> Step<PollReport> {
        let mut pop = Pop3::open(poller, max_size).await?;
        pop.command(&format!("USER {}", poller.username))
            .await
            .map_err(|reason| format!("the login was refused: {reason}"))?;
        pop.command(&format!("PASS {}", poller.password.expose()))
            .await
            .map_err(|reason| format!("the login was refused: {reason}"))?;

        let mut waiting = pop.unique_ids().await?;
        let mut report = PollReport::default();
        if waiting.len() > poller.max_messages {
            report.left = waiting.len() - poller.max_messages;
            waiting.truncate(poller.max_messages);
        }
        for (number, id) in waiting {
            pop.command(&format!("RETR {number}")).await?;
            let (raw, oversized) = pop.message().await?;
            if let Some(size) = oversized {
                // Not read, so not deleted: it stays for someone to look at.
                report.refused.push(PollIssue {
                    id,
                    reason: format!(
                        "the message is over the {max_size}-byte limit (`mail.inbound.max_size`) at {size} bytes"
                    ),
                });
                continue;
            }
            if self.take(poller, &id, raw, &mut report).await == Taken::Done {
                pop.command(&format!("DELE {number}")).await?;
            }
        }
        // QUIT commits the deletions.
        pop.command("QUIT").await?;
        Ok(report)
    }
}

// ---------------------------------------------------------------------
// The connection
// ---------------------------------------------------------------------

/// A byte stream: plain TCP or TLS.
trait Io: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

/// A connection to a server, read by lines.
struct Conn {
    reader: BufReader<Box<dyn Io>>,
    timeout: Duration,
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn tls_connector(roots: &[CertificateDer<'static>]) -> Step<TlsConnector> {
    let failed = |error: &dyn fmt::Display| format!("TLS setup failed: {error}");
    let mut store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    for certificate in roots {
        store.add(certificate.clone()).map_err(|e| failed(&e))?;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| failed(&e))?
        .with_root_certificates(store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

impl Conn {
    /// Connects to the poller's server; TLS from the first byte when the
    /// poller says so.
    async fn open(poller: &MailboxPoller) -> Step<Conn> {
        let port = poller.port_or_default();
        let tcp = timeout(
            poller.timeout,
            TcpStream::connect((poller.host.as_str(), port)),
        )
        .await
        .map_err(|_| format!("the connection to port {port} timed out"))?
        .map_err(|error| format!("cannot connect to port {port}: {error}"))?;
        let stream: Box<dyn Io> = Box::new(tcp);
        let conn = Conn {
            reader: BufReader::new(stream),
            timeout: poller.timeout,
        };
        if poller.security == PollSecurity::Tls {
            conn.secure(poller).await
        } else {
            Ok(conn)
        }
    }

    /// The TLS handshake on this connection. The server's name must match
    /// its certificate.
    async fn secure(self, poller: &MailboxPoller) -> Step<Conn> {
        let connector = tls_connector(&poller.roots)?;
        let name = ServerName::try_from(poller.host.clone())
            .map_err(|_| "the host is not a valid server name".to_owned())?;
        let Conn {
            reader,
            timeout: wait,
        } = self;
        let tls = timeout(wait, connector.connect(name, reader.into_inner()))
            .await
            .map_err(|_| "the TLS handshake timed out".to_owned())?
            .map_err(|error| format!("the TLS handshake failed: {error}"))?;
        let stream: Box<dyn Io> = Box::new(tls);
        Ok(Conn {
            reader: BufReader::new(stream),
            timeout: wait,
        })
    }

    /// One line, without its line break. Fails when the server closes the
    /// connection, is silent for `timeout` or sends more than `cap` bytes.
    async fn line(&mut self, cap: u64) -> Step<Vec<u8>> {
        let mut line = Vec::new();
        let read = {
            let mut limited = (&mut self.reader).take(cap);
            timeout(self.timeout, limited.read_until(b'\n', &mut line))
                .await
                .map_err(|_| "the server did not answer in time".to_owned())?
                .map_err(|error| format!("reading from the server failed: {error}"))?
        };
        if read == 0 {
            return Err("the server closed the connection".to_owned());
        }
        if line.last() != Some(&b'\n') {
            return Err(if read as u64 >= cap {
                "the server sent a line that is too long".to_owned()
            } else {
                "the server closed the connection in the middle of a line".to_owned()
            });
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        Ok(line)
    }

    async fn send(&mut self, bytes: &[u8]) -> Step<()> {
        let writer = self.reader.get_mut();
        timeout(self.timeout, async {
            writer.write_all(bytes).await?;
            writer.flush().await
        })
        .await
        .map_err(|_| "the server did not take the command in time".to_owned())?
        .map_err(|error| format!("writing to the server failed: {error}"))
    }

    /// `size` bytes, which are dropped when `keep` is false.
    async fn bytes(&mut self, size: u64, keep: bool) -> Step<Vec<u8>> {
        let read = |error: std::io::Error| format!("reading from the server failed: {error}");
        let timed_out = || "the server did not send the message in time".to_owned();
        if keep {
            let mut buffer = vec![0; size as usize];
            timeout(self.timeout, self.reader.read_exact(&mut buffer))
                .await
                .map_err(|_| timed_out())?
                .map_err(read)?;
            Ok(buffer)
        } else {
            let copied = timeout(
                self.timeout,
                tokio::io::copy(&mut (&mut self.reader).take(size), &mut tokio::io::sink()),
            )
            .await
            .map_err(|_| timed_out())?
            .map_err(read)?;
            if copied < size {
                return Err(
                    "the server closed the connection in the middle of a message".to_owned(),
                );
            }
            Ok(Vec::new())
        }
    }
}

// ---------------------------------------------------------------------
// IMAP
// ---------------------------------------------------------------------

/// An argument of an IMAP command.
enum Arg<'a> {
    /// Sent as it is (a keyword, a number, a flag list).
    Atom(&'a str),
    /// A string: quoted when it is printable ASCII, else sent as a literal.
    Text(&'a str),
}

/// What a command answered: the untagged lines, and the literals in them.
#[derive(Default)]
struct Answer {
    lines: Vec<String>,
    literals: Vec<Vec<u8>>,
    /// The sizes of literals over the limit, which were dropped.
    oversized: Vec<u64>,
}

/// The size in an IMAP line that ends in a literal marker, `... {1234}`.
fn literal_size(line: &str) -> Option<u64> {
    line.strip_suffix('}')?
        .rsplit_once('{')?
        .1
        .parse::<u64>()
        .ok()
}

struct Imap {
    conn: Conn,
    tag: u32,
    max_size: u64,
}

impl Imap {
    /// Connects, reads the greeting and, for STARTTLS, secures the
    /// connection. The login comes after.
    async fn open(poller: &MailboxPoller, max_size: usize) -> Step<Imap> {
        let mut conn = Conn::open(poller).await?;
        let greeting = conn.line(LINE).await?;
        if !greeting.starts_with(b"* OK") {
            return Err(format!(
                "the server did not greet with OK: {}",
                lossy(&greeting)
            ));
        }
        if poller.security == PollSecurity::StartTls {
            conn.send(b"s0 STARTTLS\r\n").await?;
            loop {
                let line = conn.line(LINE).await?;
                if let Some(status) = line.strip_prefix(b"s0 ") {
                    if !status.starts_with(b"OK") {
                        return Err(format!(
                            "the server does not offer STARTTLS: {}",
                            lossy(status)
                        ));
                    }
                    break;
                }
            }
            conn = conn.secure(poller).await?;
        }
        Ok(Imap {
            conn,
            tag: 0,
            max_size: max_size as u64,
        })
    }

    /// Sends one command and reads up to its tagged answer, which must be OK.
    async fn run(&mut self, command: &str, args: &[Arg<'_>]) -> Step<Answer> {
        self.tag += 1;
        let tag = format!("a{}", self.tag);
        let mut out = format!("{tag} {command}").into_bytes();
        for arg in args {
            out.push(b' ');
            match arg {
                Arg::Atom(atom) => out.extend_from_slice(atom.as_bytes()),
                Arg::Text(text) if text.bytes().all(|byte| (0x20..0x7f).contains(&byte)) => {
                    out.push(b'"');
                    for byte in text.bytes() {
                        if byte == b'"' || byte == b'\\' {
                            out.push(b'\\');
                        }
                        out.push(byte);
                    }
                    out.push(b'"');
                }
                Arg::Text(text) => {
                    // A literal: the server asks for its bytes with a `+`.
                    out.extend_from_slice(format!("{{{}}}\r\n", text.len()).as_bytes());
                    self.conn.send(&out).await?;
                    let reply = self.conn.line(LINE).await?;
                    if !reply.starts_with(b"+") {
                        return Err(lossy(&reply));
                    }
                    out = text.as_bytes().to_vec();
                }
            }
        }
        out.extend_from_slice(b"\r\n");
        self.conn.send(&out).await?;
        self.answer(&tag).await
    }

    async fn answer(&mut self, tag: &str) -> Step<Answer> {
        let ours = format!("{tag} ");
        let mut answer = Answer::default();
        loop {
            let line = self.conn.line(LINE).await?;
            if let Some(status) = line.strip_prefix(ours.as_bytes()) {
                let status = lossy(status);
                return if status.starts_with("OK") {
                    Ok(answer)
                } else {
                    Err(status)
                };
            }
            let text = lossy(&line);
            if let Some(size) = literal_size(&text) {
                if size <= self.max_size {
                    answer.literals.push(self.conn.bytes(size, true).await?);
                } else {
                    self.conn.bytes(size, false).await?;
                    answer.oversized.push(size);
                }
            }
            answer.lines.push(text);
        }
    }
}

// ---------------------------------------------------------------------
// POP3
// ---------------------------------------------------------------------

struct Pop3 {
    conn: Conn,
    max_size: u64,
}

impl Pop3 {
    /// Connects, reads the greeting and, for STLS, secures the connection.
    /// The login comes after.
    async fn open(poller: &MailboxPoller, max_size: usize) -> Step<Pop3> {
        let mut conn = Conn::open(poller).await?;
        let greeting = conn.line(LINE).await?;
        if !greeting.starts_with(b"+OK") {
            return Err(format!(
                "the server did not greet with +OK: {}",
                lossy(&greeting)
            ));
        }
        if poller.security == PollSecurity::StartTls {
            conn.send(b"STLS\r\n").await?;
            let reply = conn.line(LINE).await?;
            if !reply.starts_with(b"+OK") {
                return Err(format!("the server does not offer STLS: {}", lossy(&reply)));
            }
            conn = conn.secure(poller).await?;
        }
        Ok(Pop3 {
            conn,
            max_size: max_size as u64,
        })
    }

    /// Sends one command and reads its status line, which must be `+OK`;
    /// the text after it is returned.
    async fn command(&mut self, command: &str) -> Step<String> {
        self.conn.send(format!("{command}\r\n").as_bytes()).await?;
        let reply = lossy(&self.conn.line(LINE).await?);
        match reply.strip_prefix("+OK") {
            Some(rest) => Ok(rest.trim().to_owned()),
            None => Err(reply),
        }
    }

    /// The lines of a multi-line reply, up to the line with a dot alone.
    async fn list(&mut self) -> Step<Vec<String>> {
        let mut lines = Vec::new();
        loop {
            let line = lossy(&self.conn.line(LINE).await?);
            if line == "." {
                return Ok(lines);
            }
            lines.push(line);
        }
    }

    /// The waiting messages: their numbers and unique ids (`UIDL`; the
    /// number when a server has no `UIDL`).
    async fn unique_ids(&mut self) -> Step<Vec<(u32, String)>> {
        let has_ids = self.command("UIDL").await.is_ok();
        if !has_ids {
            self.command("LIST").await?;
        }
        let mut waiting = Vec::new();
        for line in self.list().await? {
            let mut parts = line.split_whitespace();
            let (Some(number), id) = (parts.next(), parts.next()) else {
                continue;
            };
            let Ok(number) = number.parse::<u32>() else {
                continue;
            };
            // A UIDL line is `number id`; a LIST line is `number size`, and
            // the number then names the message.
            let id = match id {
                Some(id) if has_ids => id.to_owned(),
                _ => number.to_string(),
            };
            waiting.push((number, id));
        }
        Ok(waiting)
    }

    /// The message that follows a `RETR` answer, as the server stored it
    /// (dot-stuffing undone, lines ended with CR LF), and its size when it
    /// is over the limit and was dropped.
    async fn message(&mut self) -> Step<(Vec<u8>, Option<u64>)> {
        let mut message = Vec::new();
        let mut size = 0_u64;
        loop {
            // A line of a message is no longer than the message may be.
            let line = self.conn.line(self.max_size.max(LINE) + 1).await?;
            if line == b"." {
                break;
            }
            let line = line
                .strip_prefix(b".")
                .filter(|_| line.starts_with(b".."))
                .unwrap_or(&line);
            size += line.len() as u64 + 2;
            if size <= self.max_size {
                message.extend_from_slice(line);
                message.extend_from_slice(b"\r\n");
            }
        }
        if size > self.max_size {
            return Ok((Vec::new(), Some(size)));
        }
        Ok((message, None))
    }
}

//! Inbound mail (crate feature `inbound`): received messages parsed with
//! mail-parser, the provider sources, mailboxes routed by address, signed
//! reply tokens and the raw `.eml` stored on a disk.
//!
//! - [`InboundMail`]: a parsed message (headers, addresses, bodies,
//!   attachments, forwarded messages), built from an [`Incoming`] that a
//!   source hands over.
//! - [`Inbound`] (provided by `MailModule`): [`receive`](Inbound::receive)
//!   checks the size, parses, stores the raw message on `mail.inbound.disk`,
//!   routes it to the first matching mailbox and emits
//!   [`InboundMailReceived`].
//! - Mailboxes: `m.mailbox("reply+{token}@in.acme.test", handler)`
//!   ([`ModuleBuilderMailExt`], [`MailboxPattern`], [`MailboxContext`]).
//! - [`ReplyTokens`]: signed reply addresses.
//! - Sources ([`InboundController`]): `POST /api/mail/inbound/ses`,
//!   `/mailgun`, `/mailgun/mime`, `/postmark` and `/raw`; Gmail and
//!   Microsoft Graph mailboxes through navree-integrations.

mod compose;
mod controller;
mod error;
mod mail;
mod mailbox;
mod options;
mod sources;
mod tokens;

use std::fmt;
use std::sync::Arc;

use navree_config::Config;
use navree_contracts::{Clock, SystemClock};
use navree_crypto::{AppKey, Encrypter};
use navree_events::EventBus;
use navree_kernel::{BoxError, ModuleId, Resolver, WeakAppContext};
use navree_storage::Storage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub use crate::events::InboundMailReceived;
pub use compose::Compose;
pub use controller::InboundController;
pub use error::{INVALID_MAIL_CODE, INVALID_REPLY_TOKEN_CODE, InboundError, TOO_LARGE_CODE};
pub use mail::{
    Address, Attachment, Envelope, Header, InboundMail, Incoming, StoredMail, Verdict, Verdicts,
};
pub use mailbox::{MailboxContext, MailboxHandler, MailboxPattern, ModuleBuilderMailExt};
pub use options::{
    InboundOptions, MailgunInboundOptions, PostmarkInboundOptions, RawInboundOptions,
    SesInboundOptions,
};
pub use tokens::ReplyTokens;

pub(crate) use error::map_inbound_error;

use mailbox::{HandlerSource, MailboxRegistration};
use sources::SesContent;

/// What became of a received message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundStatus {
    /// A mailbox handled it.
    Handled,
    /// No mailbox matched its recipients.
    Unrouted,
    /// Its mailbox rejected it ([`MailboxContext::reject`], or a bad reply
    /// token).
    Rejected,
}

/// What [`Inbound::receive`] did with a message: the answer of the source
/// routes too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Received {
    /// The receipt's id ([`InboundMail::id`]).
    pub id: Uuid,
    /// What became of it.
    pub status: InboundStatus,
    /// The mailbox pattern that matched.
    pub mailbox: Option<String>,
    /// The recipient address that matched it.
    pub address: Option<String>,
    /// Why it was rejected.
    pub reason: Option<String>,
    /// Where the raw message is stored.
    pub stored: Option<StoredMail>,
}

/// One mailbox, ready to match.
struct Route {
    pattern: MailboxPattern,
    handler: HandlerSource,
    module: ModuleId,
}

/// Receives mail: provided by `MailModule` (crate feature `inbound`).
///
/// [`receive`](Inbound::receive) runs every message through the same
/// steps, whatever its source:
///
/// 1. a message over `mail.inbound.max_size` is refused
///    ([`InboundError::TooLarge`], a 413);
/// 2. it is parsed ([`InboundMail`]);
/// 3. the raw message is stored on `mail.inbound.disk` (when set) at
///    `{path}/{yyyy}/{mm}/{dd}/{sha-256}.eml`;
/// 4. the first mailbox whose pattern matches a recipient handles it:
///    exact addresses first, then patterns with `{name}`, then `*`; the
///    recipients are tried in the order of [`InboundMail::recipients`];
/// 5. [`InboundMailReceived`] is emitted (with `EventsModule`).
///
/// A handler's error is returned ([`InboundError::Handler`]) and the
/// source answers 500, so the provider delivers the message again; a
/// rejection is not an error.
pub struct Inbound {
    options: InboundOptions,
    routes: Vec<Route>,
    app: WeakAppContext,
    storage: Option<Arc<Storage>>,
    events: Option<Arc<EventBus>>,
    tokens: Arc<ReplyTokens>,
    clock: Arc<dyn Clock>,
}

impl Inbound {
    /// Builds the service for the app: the mailboxes every module
    /// registered, the storage disks and the event bus.
    pub(crate) async fn build(r: &Resolver, options: InboundOptions) -> Result<Self, BoxError> {
        let app = r.app();
        let mut routes = Vec::new();
        for contributed in app.contributions::<MailboxRegistration>() {
            let registration = &contributed.value;
            let pattern = MailboxPattern::parse(&registration.pattern)
                .map_err(|error| format!("module `{}`: {error}", contributed.module.name))?;
            routes.push(Route {
                pattern,
                handler: registration.handler.clone(),
                module: contributed.module.id,
            });
        }
        routes.sort_by_key(|route| route.pattern.rank());

        let needs_storage = options.disk.is_some() || options.ses.disk.is_some();
        let storage = app.try_get::<Storage>().await;
        if needs_storage {
            let Some(storage) = &storage else {
                return Err(
                    "`mail.inbound.disk` and `mail.inbound.ses.disk` need StorageModule".into(),
                );
            };
            for (key, disk) in [("disk", &options.disk), ("ses.disk", &options.ses.disk)] {
                if let Some(disk) = disk
                    && storage.disk(disk).is_err()
                {
                    return Err(format!(
                        "`mail.inbound.{key}` names disk `{disk}`, which `storage.disks` does not configure"
                    )
                    .into());
                }
            }
        }
        let clock: Arc<dyn Clock> = match app.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        Ok(Self {
            routes,
            app: app.downgrade(),
            storage,
            events: app.try_get::<EventBus>().await,
            tokens: r.get::<ReplyTokens>().await?,
            clock,
            options,
        })
    }

    /// The settings (`mail.inbound`).
    pub fn options(&self) -> &InboundOptions {
        &self.options
    }

    /// The reply tokens mailboxes verify with.
    pub fn tokens(&self) -> &ReplyTokens {
        &self.tokens
    }

    /// The mailbox patterns, in the order they are tried.
    pub fn mailboxes(&self) -> Vec<&str> {
        self.routes
            .iter()
            .map(|route| route.pattern.as_str())
            .collect()
    }

    /// The mailbox pattern that would take `mail`, and the recipient it
    /// matched.
    pub fn mailbox_for(&self, mail: &InboundMail) -> Option<(&str, String)> {
        self.find(mail)
            .map(|(route, address, _)| (route.pattern.as_str(), address))
    }

    fn find(&self, mail: &InboundMail) -> Option<(&Route, String, Vec<(String, String)>)> {
        let recipients = mail.recipients();
        for route in &self.routes {
            for address in &recipients {
                if let Some(params) = route.pattern.matches(address) {
                    return Some((route, address.clone(), params));
                }
            }
        }
        None
    }

    /// Receives one message: size check, parse, store, route, event (see
    /// [`Inbound`]).
    ///
    /// # Errors
    ///
    /// [`InboundError::TooLarge`], [`InboundError::Unparseable`],
    /// [`InboundError::Storage`] and [`InboundError::Handler`] (the
    /// provider should deliver the message again).
    pub async fn receive(&self, incoming: Incoming) -> Result<Received, InboundError> {
        let size = incoming.raw.len();
        if size > self.options.max_size {
            tracing::info!(target: "navree::mail", source = %incoming.source, size, max = self.options.max_size, "inbound message over the size limit refused");
            return Err(InboundError::TooLarge {
                size,
                max: self.options.max_size,
            });
        }
        let mut mail = incoming.parse_at(self.clock.now())?;
        if mail.stored.is_none() {
            mail.stored = self.store(&mail).await?;
        }

        let (status, mailbox, address, reason) = match self.find(&mail) {
            None => (InboundStatus::Unrouted, None, None, None),
            Some((route, address, params)) => {
                let pattern = route.pattern.as_str().to_owned();
                let app = self.app.upgrade().ok_or_else(|| {
                    InboundError::Config("the app is shutting down".to_owned())
                })?;
                let handler = match &route.handler {
                    HandlerSource::Value(handler) => handler.clone(),
                    HandlerSource::Provider(resolve) => resolve(app.clone(), route.module)
                        .await
                        .map_err(|error| InboundError::Handler {
                            mailbox: pattern.clone(),
                            error,
                        })?,
                };
                let ctx = MailboxContext::new(
                    pattern.clone(),
                    address.clone(),
                    params,
                    app,
                    self.tokens.clone(),
                );
                match handler.handle(mail.clone(), ctx).await {
                    Ok(()) => (InboundStatus::Handled, Some(pattern), Some(address), None),
                    Err(error) => match error.downcast::<InboundError>() {
                        Ok(error) if error.is_rejection() => (
                            InboundStatus::Rejected,
                            Some(pattern),
                            Some(address),
                            Some(error.to_string()),
                        ),
                        Ok(error) => {
                            return Err(InboundError::Handler {
                                mailbox: pattern,
                                error,
                            });
                        }
                        Err(error) => {
                            return Err(InboundError::Handler {
                                mailbox: pattern,
                                error,
                            });
                        }
                    },
                }
            }
        };

        tracing::info!(
            target: "navree::mail",
            id = %mail.id,
            source = %mail.source,
            status = ?status,
            mailbox = mailbox.as_deref().unwrap_or("-"),
            size = mail.size,
            "inbound message received"
        );
        if let Some(bus) = &self.events {
            let event = InboundMailReceived {
                id: mail.id,
                source: mail.source.clone(),
                provider_id: mail.provider_id.clone(),
                message_id: mail.message_id.clone(),
                from: mail.from.as_ref().map(|from| from.email.clone()),
                recipients: mail.recipients(),
                subject: mail.subject.clone(),
                size: mail.size,
                attachments: mail.attachments.len(),
                fingerprint: mail.fingerprint.clone(),
                status,
                mailbox: mailbox.clone(),
                address: address.clone(),
                stored: mail.stored.clone(),
                received_at: mail.received_at,
            };
            if let Err(error) = bus.emit(event).await {
                tracing::warn!(target: "navree::mail", %error, "a listener of mail.inbound.received failed");
            }
        }
        Ok(Received {
            id: mail.id,
            status,
            mailbox,
            address,
            reason,
            stored: mail.stored,
        })
    }

    /// Receives the message of an Amazon SES receipt notification (the SNS
    /// message's JSON, `notificationType: Received`): from the
    /// notification with an SNS action, or from `mail.inbound.ses.disk`
    /// with an S3 action. Use it from your own route when you verify SNS
    /// signatures yourself.
    ///
    /// # Errors
    ///
    /// [`InboundError::Malformed`] for another notification,
    /// [`InboundError::Config`] for an S3 action without
    /// `mail.inbound.ses.disk`, and the errors of
    /// [`receive`](Inbound::receive).
    pub async fn receive_ses(&self, notification: &Value) -> Result<Received, InboundError> {
        let receipt = sources::ses(notification)?;
        let incoming = match receipt.content {
            SesContent::Inline(raw) => Incoming::new("ses", raw),
            SesContent::S3 { bucket, key } => {
                let Some(name) = &self.options.ses.disk else {
                    return Err(InboundError::Config(format!(
                        "the SES receipt rule stored the message in bucket `{bucket}`: set `mail.inbound.ses.disk` to a disk on it"
                    )));
                };
                let disk = self
                    .storage
                    .as_ref()
                    .ok_or_else(|| {
                        InboundError::Config("`mail.inbound.ses.disk` needs StorageModule".into())
                    })?
                    .disk(name)?;
                if let Some(size) = disk.size(&key).await.ok().map(|size| {
                    usize::try_from(size).unwrap_or(usize::MAX)
                }) && size > self.options.max_size
                {
                    return Err(InboundError::TooLarge {
                        size,
                        max: self.options.max_size,
                    });
                }
                let raw = disk.get(&key).await?;
                Incoming::new("ses", raw).stored(StoredMail {
                    disk: name.clone(),
                    path: key,
                })
            }
        };
        self.receive(Incoming {
            provider_id: receipt.provider_id,
            envelope: receipt.envelope,
            verdicts: receipt.verdicts,
            ..incoming
        })
        .await
    }

    /// Reads a stored message back (the `stored` of a [`Received`] or an
    /// [`InboundMailReceived`]), parsed. Its `source` is `raw` and its
    /// envelope empty: the file holds the message only.
    ///
    /// # Errors
    ///
    /// [`InboundError::Config`] without `StorageModule`,
    /// [`InboundError::Storage`] when the file cannot be read, and
    /// [`InboundError::Unparseable`].
    pub async fn read(&self, stored: &StoredMail) -> Result<InboundMail, InboundError> {
        let storage = self
            .storage
            .as_ref()
            .ok_or_else(|| InboundError::Config("reading a stored message needs StorageModule".into()))?;
        let raw = storage.disk(&stored.disk)?.get(&stored.path).await?;
        let mut mail = InboundMail::parse(raw)?;
        mail.stored = Some(stored.clone());
        Ok(mail)
    }

    /// Stores the raw message on `mail.inbound.disk`, once per content.
    async fn store(&self, mail: &InboundMail) -> Result<Option<StoredMail>, InboundError> {
        let Some(name) = &self.options.disk else {
            return Ok(None);
        };
        let storage = self
            .storage
            .as_ref()
            .ok_or_else(|| InboundError::Config("`mail.inbound.disk` needs StorageModule".into()))?;
        let disk = storage.disk(name)?;
        let folder = self.options.path.trim().trim_matches('/');
        let file = format!(
            "{}/{}.eml",
            mail.received_at.format("%Y/%m/%d"),
            mail.fingerprint
        );
        let path = if folder.is_empty() {
            file
        } else {
            format!("{folder}/{file}")
        };
        if !disk.exists(&path).await? {
            disk.put(&path, mail.raw().clone()).await?;
        }
        Ok(Some(StoredMail {
            disk: name.clone(),
            path,
        }))
    }
}

impl fmt::Debug for Inbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inbound")
            .field("mailboxes", &self.mailboxes())
            .field("disk", &self.options.disk)
            .field("max_size", &self.options.max_size)
            .finish_non_exhaustive()
    }
}

/// The reply tokens of the app: a key derived from the application key
/// (`CryptoModule`'s, or `app.key` / `key` from configuration).
pub(crate) async fn reply_tokens(r: &Resolver, ttl_days: u32) -> ReplyTokens {
    let app = r.app();
    let key = match app.try_get::<Encrypter>().await {
        Some(encrypter) => encrypter.derive_key(tokens::KEY_PURPOSE).ok(),
        None => None,
    };
    let key = match key {
        Some(key) => Some(key),
        None => app.try_get::<Config>().await.and_then(|config| {
            ["app.key", "key"]
                .into_iter()
                .filter_map(|name| config.get_string(name))
                .find(|key| !key.trim().is_empty())
                .and_then(|raw| AppKey::parse(&raw).ok())
                .and_then(|key| Encrypter::new(key).derive_key(tokens::KEY_PURPOSE).ok())
        }),
    };
    let tokens = match key {
        Some(key) => ReplyTokens::new(key),
        None => ReplyTokens::without_key(),
    };
    let clock: Arc<dyn Clock> = match app.try_get::<dyn Clock>().await {
        Some(clock) => clock,
        None => Arc::new(SystemClock),
    };
    tokens.with_ttl_days(ttl_days).with_clock(clock)
}

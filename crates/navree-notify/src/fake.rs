//! Fakes of the channel drivers: [`SlackFake`], [`TeamsFake`], [`SmsFake`],
//! [`WhatsAppFake`] and [`WebPushFake`]. Each records what it is asked to
//! send instead of sending it, can be told to refuse, and the SMS and
//! WhatsApp fakes play the provider: they report statuses and replies
//! through the app's [`DeliveryLog`], as a provider's callbacks would.

use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_contracts::BoxFuture;
use navree_kernel::AppContext;

use crate::{
    Channel, ChannelCallback, ChannelError, DeliveryLog, DeliveryStatus, InboundMessage, Ingested,
    NotifyError, PushTarget, SentMessage, SlackDriver, SlackMessage, SlackRoute, SmsDriver,
    SmsMessage, StatusUpdate, TeamsDriver, TeamsMessage, TeamsRoute, WebPushDriver, WebPushMessage,
    WhatsAppDriver, WhatsAppMessage,
};

/// The provider name the fakes record and report under.
pub const FAKE_PROVIDER: &str = "fake";

/// One message a fake was asked to send.
#[derive(Clone, Debug, PartialEq)]
pub struct Sent<R, M> {
    /// Where it was going.
    pub to: R,
    /// The message.
    pub message: M,
    /// The id the fake gave it (`fake-sms-1`, ...).
    pub id: String,
}

/// What every fake keeps.
struct Recorder<R, M> {
    kind: &'static str,
    sent: Vec<Sent<R, M>>,
    refusal: Option<String>,
    opted_out: HashSet<String>,
    gone: HashSet<String>,
}

impl<R, M> Recorder<R, M> {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            sent: Vec::new(),
            refusal: None,
            opted_out: HashSet::new(),
            gone: HashSet::new(),
        }
    }
}

/// The shared state of a fake.
struct Shared<R, M>(Arc<Mutex<Recorder<R, M>>>);

impl<R, M> Clone for Shared<R, M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: Clone + Debug, M: Clone> Shared<R, M> {
    fn new(kind: &'static str) -> Self {
        Self(Arc::new(Mutex::new(Recorder::new(kind))))
    }

    fn state(&self) -> MutexGuard<'_, Recorder<R, M>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records `message` to `to`, or refuses it.
    fn send(&self, to: &R, message: &M, key: &str) -> Result<SentMessage, ChannelError> {
        let mut state = self.state();
        if let Some(reason) = &state.refusal {
            return Err(ChannelError::Refused {
                provider: FAKE_PROVIDER.to_owned(),
                status: 400,
                code: None,
                message: reason.clone(),
            });
        }
        if state.opted_out.contains(key) {
            return Err(ChannelError::OptedOut {
                provider: FAKE_PROVIDER.to_owned(),
            });
        }
        if state.gone.contains(key) {
            return Err(ChannelError::Gone);
        }
        let id = format!("fake-{}-{}", state.kind, state.sent.len() + 1);
        state.sent.push(Sent {
            to: to.clone(),
            message: message.clone(),
            id: id.clone(),
        });
        Ok(SentMessage::new(FAKE_PROVIDER).id(id))
    }

    fn sent(&self) -> Vec<Sent<R, M>> {
        self.state().sent.clone()
    }

    fn refuse(&self, reason: impl Into<String>) {
        self.state().refusal = Some(reason.into());
    }

    fn accept(&self) {
        self.state().refusal = None;
    }

    #[track_caller]
    fn assert_sent(&self, predicate: impl Fn(&Sent<R, M>) -> bool) {
        let state = self.state();
        if !state.sent.iter().any(predicate) {
            let to: Vec<String> = state.sent.iter().map(|s| format!("{:?}", s.to)).collect();
            panic!(
                "no {} message matches the predicate; sent to: [{}]",
                state.kind,
                to.join(", ")
            );
        }
    }

    #[track_caller]
    fn assert_nothing_sent(&self) {
        let state = self.state();
        assert!(
            state.sent.is_empty(),
            "expected no {} message, but {} were sent",
            state.kind,
            state.sent.len()
        );
    }

    /// The id of the last message to `to`.
    fn last_id(&self, matches: impl Fn(&R) -> bool) -> Option<String> {
        self.state()
            .sent
            .iter()
            .rev()
            .find(|s| matches(&s.to))
            .map(|s| s.id.clone())
    }
}

/// Generates the shared methods of a fake.
macro_rules! common {
    ($fake:ident, $route:ty, $message:ty, $driver:ident) => {
        impl $fake {
            /// This fake as a driver, to install with
            #[doc = concat!("`override_value::<dyn ", stringify!($driver), ">(fake.driver())`.")]
            pub fn driver(&self) -> Arc<dyn $driver> {
                Arc::new(self.clone())
            }

            /// Every message the fake was asked to send, oldest first.
            pub fn sent(&self) -> Vec<Sent<$route, $message>> {
                self.shared.sent()
            }

            /// Refuses every message from now on (a `Refused` error with
            /// `reason`), until [`accept`](Self::accept).
            pub fn refuse(&self, reason: impl Into<String>) {
                self.shared.refuse(reason);
            }

            /// Takes messages again.
            pub fn accept(&self) {
                self.shared.accept();
            }

            /// Panics unless some sent message satisfies `predicate`.
            #[track_caller]
            pub fn assert_sent(&self, predicate: impl Fn(&Sent<$route, $message>) -> bool) {
                self.shared.assert_sent(predicate);
            }

            /// Panics if any message was sent.
            #[track_caller]
            pub fn assert_nothing_sent(&self) {
                self.shared.assert_nothing_sent();
            }
        }

        impl Default for $fake {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Debug for $fake {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($fake))
                    .field("sent", &self.shared.state().sent.len())
                    .finish_non_exhaustive()
            }
        }
    };
}

/// Records Slack messages instead of sending them. Install it with
/// `override_value::<dyn SlackDriver>(fake.driver())`.
///
/// ```
/// use navree_notify::{SlackDriver, SlackFake, SlackMessage, SlackRoute};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let fake = SlackFake::new();
/// let route = SlackRoute::channel("C0123");
/// fake.driver().send(&route, &SlackMessage::new("Deal won")).await.unwrap();
/// fake.assert_sent(|sent| sent.message.text == "Deal won");
/// # }
/// ```
#[derive(Clone)]
pub struct SlackFake {
    shared: Shared<SlackRoute, SlackMessage>,
}

impl SlackFake {
    /// An empty fake.
    pub fn new() -> Self {
        Self {
            shared: Shared::new("slack"),
        }
    }
}

common!(SlackFake, SlackRoute, SlackMessage, SlackDriver);

impl SlackDriver for SlackFake {
    fn send<'a>(
        &'a self,
        route: &'a SlackRoute,
        message: &'a SlackMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let result = self.shared.send(route, message, "");
        Box::pin(async move { result })
    }
}

/// Records Microsoft Teams messages instead of sending them. Install it
/// with `override_value::<dyn TeamsDriver>(fake.driver())`.
#[derive(Clone)]
pub struct TeamsFake {
    shared: Shared<TeamsRoute, TeamsMessage>,
}

impl TeamsFake {
    /// An empty fake.
    pub fn new() -> Self {
        Self {
            shared: Shared::new("teams"),
        }
    }
}

common!(TeamsFake, TeamsRoute, TeamsMessage, TeamsDriver);

impl TeamsDriver for TeamsFake {
    fn send<'a>(
        &'a self,
        route: &'a TeamsRoute,
        message: &'a TeamsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let result = self.shared.send(route, message, "");
        Box::pin(async move { result })
    }
}

/// Records text messages instead of sending them, and plays the provider:
/// [`deliver`](SmsFake::deliver) and [`fail`](SmsFake::fail) report a
/// status for the last message to a number, [`reply`](SmsFake::reply)
/// sends the app a message from one (`"STOP"` opts it out), and
/// [`opt_out_at_provider`](SmsFake::opt_out_at_provider) makes the fake refuse a number
/// the way Twilio refuses one that replied STOP. Install it with
/// `override_value::<dyn SmsDriver>(fake.driver())`.
///
/// ```no_run
/// use navree_notify::SmsFake;
///
/// # async fn demo(app: navree_kernel::AppContext) -> Result<(), navree_notify::NotifyError> {
/// let fake = SmsFake::new();
/// // TestApp::builder().override_value::<dyn SmsDriver>(fake.driver()) ...
/// fake.deliver(&app, "+14155550100").await?;
/// fake.reply(&app, "+14155550100", "STOP").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SmsFake {
    shared: Shared<String, SmsMessage>,
}

impl SmsFake {
    /// An empty fake.
    pub fn new() -> Self {
        Self {
            shared: Shared::new("sms"),
        }
    }

    /// The messages sent to `number` (E.164).
    pub fn sent_to(&self, number: &str) -> Vec<SmsMessage> {
        self.sent()
            .into_iter()
            .filter(|sent| sent.to == number)
            .map(|sent| sent.message)
            .collect()
    }

    /// Refuses `number` from now on as opted out at the provider
    /// ([`ChannelError::OptedOut`]).
    pub fn opt_out_at_provider(&self, number: &str) {
        self.shared.state().opted_out.insert(number.to_owned());
    }

    /// Reports the last message to `number` delivered.
    pub async fn deliver(&self, app: &AppContext, number: &str) -> Result<Ingested, NotifyError> {
        self.report(app, number, DeliveryStatus::Delivered).await
    }

    /// Reports the last message to `number` failed.
    pub async fn fail(&self, app: &AppContext, number: &str) -> Result<Ingested, NotifyError> {
        self.report(app, number, DeliveryStatus::Failed).await
    }

    async fn report(
        &self,
        app: &AppContext,
        number: &str,
        status: DeliveryStatus,
    ) -> Result<Ingested, NotifyError> {
        let id = self.shared.last_id(|to| to == number).ok_or_else(|| {
            NotifyError::InvalidRecipient(format!("the fake sent nothing to {number}"))
        })?;
        ingest(
            app,
            ChannelCallback::Status(StatusUpdate::new(FAKE_PROVIDER, id, status)),
        )
        .await
    }

    /// Sends the app a message from `number`, as an inbound SMS.
    pub async fn reply(
        &self,
        app: &AppContext,
        number: &str,
        body: &str,
    ) -> Result<Ingested, NotifyError> {
        ingest(app, inbound(Channel::Sms, number, body)).await
    }
}

common!(SmsFake, String, SmsMessage, SmsDriver);

impl SmsDriver for SmsFake {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a SmsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let result = self.shared.send(&to.to_owned(), message, to);
        Box::pin(async move { result })
    }
}

/// Records WhatsApp messages instead of sending them, and plays the
/// provider like [`SmsFake`] (plus [`read`](WhatsAppFake::read)). Install
/// it with `override_value::<dyn WhatsAppDriver>(fake.driver())`.
#[derive(Clone)]
pub struct WhatsAppFake {
    shared: Shared<String, WhatsAppMessage>,
}

impl WhatsAppFake {
    /// An empty fake.
    pub fn new() -> Self {
        Self {
            shared: Shared::new("whatsapp"),
        }
    }

    /// The messages sent to `number` (E.164).
    pub fn sent_to(&self, number: &str) -> Vec<WhatsAppMessage> {
        self.sent()
            .into_iter()
            .filter(|sent| sent.to == number)
            .map(|sent| sent.message)
            .collect()
    }

    /// Refuses `number` from now on as opted out at the provider.
    pub fn opt_out_at_provider(&self, number: &str) {
        self.shared.state().opted_out.insert(number.to_owned());
    }

    /// Reports the last message to `number` delivered.
    pub async fn deliver(&self, app: &AppContext, number: &str) -> Result<Ingested, NotifyError> {
        self.report(app, number, DeliveryStatus::Delivered).await
    }

    /// Reports the last message to `number` read.
    pub async fn read(&self, app: &AppContext, number: &str) -> Result<Ingested, NotifyError> {
        self.report(app, number, DeliveryStatus::Read).await
    }

    /// Reports the last message to `number` failed.
    pub async fn fail(&self, app: &AppContext, number: &str) -> Result<Ingested, NotifyError> {
        self.report(app, number, DeliveryStatus::Failed).await
    }

    async fn report(
        &self,
        app: &AppContext,
        number: &str,
        status: DeliveryStatus,
    ) -> Result<Ingested, NotifyError> {
        let id = self.shared.last_id(|to| to == number).ok_or_else(|| {
            NotifyError::InvalidRecipient(format!("the fake sent nothing to {number}"))
        })?;
        ingest(
            app,
            ChannelCallback::Status(StatusUpdate::new(FAKE_PROVIDER, id, status)),
        )
        .await
    }

    /// Sends the app a WhatsApp message from `number`.
    pub async fn reply(
        &self,
        app: &AppContext,
        number: &str,
        body: &str,
    ) -> Result<Ingested, NotifyError> {
        ingest(app, inbound(Channel::WhatsApp, number, body)).await
    }
}

common!(WhatsAppFake, String, WhatsAppMessage, WhatsAppDriver);

impl WhatsAppDriver for WhatsAppFake {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a WhatsAppMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let result = self.shared.send(&to.to_owned(), message, to);
        Box::pin(async move { result })
    }
}

/// Records browser push notifications instead of sending them;
/// [`expire`](WebPushFake::expire) makes an endpoint answer like a push
/// service whose subscription is gone. Install it with
/// `override_value::<dyn WebPushDriver>(fake.driver())`.
#[derive(Clone)]
pub struct WebPushFake {
    shared: Shared<PushTarget, WebPushMessage>,
}

impl WebPushFake {
    /// An empty fake.
    pub fn new() -> Self {
        Self {
            shared: Shared::new("web-push"),
        }
    }

    /// The notifications pushed to `endpoint`.
    pub fn sent_to(&self, endpoint: &str) -> Vec<WebPushMessage> {
        self.sent()
            .into_iter()
            .filter(|sent| sent.to.endpoint == endpoint)
            .map(|sent| sent.message)
            .collect()
    }

    /// Answers pushes to `endpoint` from now on with
    /// [`ChannelError::Gone`], so the channel deletes its subscription.
    pub fn expire(&self, endpoint: &str) {
        self.shared.state().gone.insert(endpoint.to_owned());
    }
}

common!(WebPushFake, PushTarget, WebPushMessage, WebPushDriver);

impl WebPushDriver for WebPushFake {
    fn send<'a>(
        &'a self,
        target: &'a PushTarget,
        message: &'a WebPushMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        let result = self.shared.send(target, message, &target.endpoint);
        Box::pin(async move { result })
    }
}

/// An inbound message from `number` over `channel`.
fn inbound(channel: Channel, number: &str, body: &str) -> ChannelCallback {
    ChannelCallback::Inbound(InboundMessage {
        provider: FAKE_PROVIDER.to_owned(),
        channel,
        from: number.to_owned(),
        to: None,
        body: body.to_owned(),
        opt_out_type: None,
    })
}

/// Applies `callback` through the app's delivery log.
async fn ingest(app: &AppContext, callback: ChannelCallback) -> Result<Ingested, NotifyError> {
    let log = app
        .get::<DeliveryLog>()
        .await
        .map_err(|error| NotifyError::Unavailable {
            channel: None,
            reason: format!("the delivery log needs NotifyModule: {error}"),
        })?;
    log.ingest([callback]).await
}

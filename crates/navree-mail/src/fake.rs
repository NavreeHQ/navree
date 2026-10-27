//! [`MailFake`]: a transport for tests that also plays the provider.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_kernel::{AppContext, BoxFuture};

use crate::message::bare_address;
use crate::{
    Deliveries, Delivery, MailError, MailEventKind, MailTransport, Message, ProviderEvent, Sent,
};

/// A transport for tests that records every message, gives each a provider
/// id (`fake-1`, `fake-2`, ...), and plays the provider: [`bounce`](MailFake::bounce),
/// [`complain`](MailFake::complain), [`deliver`](MailFake::deliver),
/// [`open`](MailFake::open) and [`click`](MailFake::click) record the event
/// for the last message sent to a recipient, as a provider webhook would,
/// through the app's [`Deliveries`] (so the delivery log, suppressions and
/// the `Mail*` events all follow). The events it reported are kept for
/// assertions.
///
/// ```no_run
/// use std::sync::Arc;
/// use navree_mail::{MailFake, MailTransport};
///
/// # async fn example(app: navree_testing::TestApp) -> Result<(), navree_mail::MailError> {
/// let fake = MailFake::new();
/// // TestApp::builder().override_value::<dyn MailTransport>(fake.transport()) ...
/// fake.bounce(app.ctx(), "ada@example.com").await?;
/// assert_eq!(fake.events().len(), 1);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct MailFake {
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    sent: Vec<(String, Message)>,
    events: Vec<ProviderEvent>,
}

impl MailFake {
    /// An empty fake.
    pub fn new() -> Self {
        Self::default()
    }

    /// This fake as a transport, to install with
    /// `override_value::<dyn MailTransport>(fake.transport())`.
    pub fn transport(&self) -> Arc<dyn MailTransport> {
        Arc::new(self.clone())
    }

    /// Every message sent, oldest first.
    pub fn sent(&self) -> Vec<Message> {
        self.state().sent.iter().map(|(_, m)| m.clone()).collect()
    }

    /// The messages sent to `recipient` (`to`, `cc` or `bcc`).
    pub fn sent_to(&self, recipient: &str) -> Vec<Message> {
        let recipient = bare_address(recipient);
        self.state()
            .sent
            .iter()
            .filter(|(_, m)| m.recipients().any(|r| bare_address(r) == recipient))
            .map(|(_, m)| m.clone())
            .collect()
    }

    /// Every event the fake reported, oldest first.
    pub fn events(&self) -> Vec<ProviderEvent> {
        self.state().events.clone()
    }

    /// Panics unless some sent message satisfies `predicate` (the message
    /// lists recipients and subjects, not bodies).
    #[track_caller]
    pub fn assert_sent(&self, predicate: impl Fn(&Message) -> bool) {
        let state = self.state();
        if !state.sent.iter().any(|(_, m)| predicate(m)) {
            let sent: Vec<String> = state
                .sent
                .iter()
                .map(|(_, m)| format!("[{}: {:?}]", m.to.join(", "), m.subject))
                .collect();
            panic!("no sent message matches the predicate; sent: {}", sent.join(", "));
        }
    }

    /// Panics if any message was sent.
    #[track_caller]
    pub fn assert_nothing_sent(&self) {
        let state = self.state();
        if !state.sent.is_empty() {
            panic!("expected no mail, but {} were sent", state.sent.len());
        }
    }

    /// Reports the last message to `recipient` delivered.
    pub async fn deliver(&self, app: &AppContext, recipient: &str) -> Result<Option<Delivery>, MailError> {
        self.report(app, recipient, MailEventKind::Delivered, |e| e).await
    }

    /// Reports a hard bounce for the last message to `recipient` (which
    /// suppresses the address with `mail.delivery.auto_suppress`).
    pub async fn bounce(&self, app: &AppContext, recipient: &str) -> Result<Option<Delivery>, MailError> {
        self.report(app, recipient, MailEventKind::Bounced, |e| {
            e.permanent().detail("550 5.1.1 user unknown")
        })
        .await
    }

    /// Reports a soft bounce for the last message to `recipient`.
    pub async fn soft_bounce(
        &self,
        app: &AppContext,
        recipient: &str,
    ) -> Result<Option<Delivery>, MailError> {
        self.report(app, recipient, MailEventKind::Bounced, |e| {
            e.detail("452 4.2.2 mailbox full")
        })
        .await
    }

    /// Reports a spam complaint about the last message to `recipient`.
    pub async fn complain(&self, app: &AppContext, recipient: &str) -> Result<Option<Delivery>, MailError> {
        self.report(app, recipient, MailEventKind::Complained, |e| e).await
    }

    /// Reports that `recipient` opened the last message sent to them.
    pub async fn open(&self, app: &AppContext, recipient: &str) -> Result<Option<Delivery>, MailError> {
        self.report(app, recipient, MailEventKind::Opened, |e| e).await
    }

    /// Reports that `recipient` clicked `url` in the last message sent to
    /// them.
    pub async fn click(
        &self,
        app: &AppContext,
        recipient: &str,
        url: &str,
    ) -> Result<Option<Delivery>, MailError> {
        let url = url.to_owned();
        self.report(app, recipient, MailEventKind::Clicked, move |e| e.url(url))
            .await
    }

    async fn report(
        &self,
        app: &AppContext,
        recipient: &str,
        kind: MailEventKind,
        build: impl FnOnce(ProviderEvent) -> ProviderEvent,
    ) -> Result<Option<Delivery>, MailError> {
        let wanted = bare_address(recipient);
        let provider_id = self
            .state()
            .sent
            .iter()
            .rev()
            .find(|(_, m)| m.recipients().any(|r| bare_address(r) == wanted))
            .map(|(id, _)| id.clone())
            .ok_or(MailError::NotFound("message sent to this recipient"))?;
        let event = build(ProviderEvent::new("fake", provider_id, kind).recipient(wanted));
        self.state().events.push(event.clone());
        let deliveries = app.get::<Deliveries>().await.map_err(|error| MailError::Store {
            source: Box::new(error),
        })?;
        deliveries.record(event).await
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl MailTransport for MailFake {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move { self.deliver_message(message).map(drop) })
    }

    fn deliver<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<Sent, MailError>> {
        Box::pin(async move { self.deliver_message(message) })
    }

    fn name(&self) -> &'static str {
        "fake"
    }
}

impl MailFake {
    fn deliver_message(&self, message: &Message) -> Result<Sent, MailError> {
        let mut state = self.state();
        let id = format!("fake-{}", state.sent.len() + 1);
        state.sent.push((id.clone(), message.clone()));
        Ok(Sent::new("fake").with_provider_id(id))
    }
}

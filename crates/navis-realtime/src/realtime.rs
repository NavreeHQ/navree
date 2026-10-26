//! [`Realtime`]: the in-process hub of connections and channels.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use axum::extract::ws::Utf8Bytes;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use navis_contracts::{Broadcaster, Principal, Secret};
use navis_http::{AppError, HttpServer};
use navis_kernel::{AppContext, BoxError, BoxFuture, Contributed};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use uuid::Uuid;

use crate::channel::{ChannelKind, ChannelPattern, is_valid_name, unprefixed};
use crate::module::{ChannelAuthorizer, ChannelRegistration};
use crate::protocol::{self, ClientMessage, frame};
use crate::relay::{self, RelayClient, RelayMessage};
use crate::{ChannelDecision, RealtimeOptions};

/// Frames queued for one connection before it counts as too slow.
const OUTBOX_CAPACITY: usize = 512;

/// Channels one connection may subscribe to.
pub(crate) const MAX_SUBSCRIPTIONS: usize = 100;

/// The longest event name accepted (Pusher's limit).
const MAX_EVENT_NAME: usize = 200;

/// Errors of [`Realtime::broadcast`].
#[derive(Debug, thiserror::Error)]
pub enum RealtimeError {
    /// The event name is empty, longer than 200 characters, or reserved
    /// (`pusher:…`, `pusher_internal:…`).
    #[error(
        "invalid event name `{0}`: use 1 to 200 characters, not starting with `pusher:` or `pusher_internal:`"
    )]
    InvalidEvent(String),
    /// The data could not be serialized to JSON.
    #[error("the broadcast data cannot be serialized to JSON: {0}")]
    Data(#[from] serde_json::Error),
}

/// The realtime server: every open connection and channel of this process.
///
/// Resolve it (`Arc<Realtime>`, exported by `RealtimeModule`) to push events
/// to subscribers ([`broadcast`](Realtime::broadcast)) or to close a user's
/// connections ([`disconnect_user`](Realtime::disconnect_user)). Modules that
/// should not depend on navis-realtime resolve `Arc<dyn Broadcaster>`
/// instead: it is the same instance.
///
/// Fan-out is in-process and never waits for a client: every connection has
/// a bounded outbox, and a connection that falls behind is closed (code 4100,
/// reconnect after a back-off) rather than slowing the broadcaster down.
///
/// Processes that do not serve the sockets (`queue:work`, `schedule:work`,
/// commands) reach them through the relay (`realtime.relay_url`): their
/// `dyn Broadcaster` posts each broadcast to the serving process's
/// `POST /apps/{app_id}/events`.
pub struct Realtime {
    options: RealtimeOptions,
    secret: Secret<Vec<u8>>,
    allowed_origins: Vec<String>,
    authorizers: Vec<CompiledChannel>,
    connections: DashMap<String, Arc<Connection>>,
    channels: DashMap<String, ChannelState>,
    shutdown: CancellationToken,
    tracker: TaskTracker,
    relay: Relay,
}

/// The relay settings of a process (see [`crate::relay`]).
#[derive(Default)]
pub(crate) struct Relay {
    /// Verifies relayed messages; `None`: the endpoint answers 404.
    pub(crate) key: Option<Secret<Vec<u8>>>,
    /// Sends this process's broadcasts to the serving process.
    pub(crate) client: Option<RelayClient>,
    /// The app's HTTP server: the process serving it delivers itself.
    pub(crate) server: Option<Arc<HttpServer>>,
}

/// A registered channel authorizer with its parsed pattern.
struct CompiledChannel {
    pattern: ChannelPattern,
    authorize: ChannelAuthorizer,
}

impl Realtime {
    /// The hub for `options`. `cors_origins` are used when
    /// `options.allowed_origins` is empty; `channels` are the registered
    /// authorizers. Fails on invalid options, malformed patterns and
    /// patterns registered twice.
    pub(crate) fn new(
        options: RealtimeOptions,
        cors_origins: Vec<String>,
        channels: Vec<Contributed<ChannelRegistration>>,
        shutdown: CancellationToken,
        tracker: TaskTracker,
    ) -> Result<Realtime, BoxError> {
        let mut errors = options.check().err().unwrap_or_default();
        let mut authorizers: Vec<CompiledChannel> = Vec::new();
        let mut owners: HashMap<&'static str, &'static str> = HashMap::new();
        for registration in channels {
            let module = registration.module.name;
            let pattern = registration.value.pattern;
            match ChannelPattern::parse(pattern) {
                Err(problem) => errors.push(format!(
                    "channel pattern `{pattern}` of module `{module}`: {problem}"
                )),
                Ok(parsed) => {
                    if let Some(first) = owners.insert(pattern, module) {
                        errors.push(format!(
                            "channel pattern `{pattern}` is registered twice (modules `{first}` and `{module}`)"
                        ));
                        continue;
                    }
                    authorizers.push(CompiledChannel {
                        pattern: parsed,
                        authorize: registration.value.authorize.clone(),
                    });
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors.join("; ").into());
        }

        let secret = if options.secret.expose().is_empty() {
            tracing::debug!(
                target: "navis::realtime",
                "realtime.secret is not set; channel signatures use a random per-process secret"
            );
            rand::random::<[u8; 32]>().to_vec()
        } else {
            options.secret.expose().as_bytes().to_vec()
        };
        let allowed_origins = if options.allowed_origins.is_empty() {
            cors_origins
        } else {
            options.allowed_origins.clone()
        };
        Ok(Realtime {
            options,
            secret: Secret::new(secret),
            allowed_origins,
            authorizers,
            connections: DashMap::new(),
            channels: DashMap::new(),
            shutdown,
            tracker,
            relay: Relay::default(),
        })
    }

    /// Sets the relay of this process.
    pub(crate) fn with_relay(mut self, relay: Relay) -> Self {
        self.relay = relay;
        self
    }

    /// Where this process's broadcasts go when it does not deliver them
    /// itself: the relay, unless the process serves HTTP (or holds
    /// connections), in which case its own subscribers are the audience.
    fn relay_target(&self) -> Option<&RelayClient> {
        let client = self.relay.client.as_ref()?;
        let serving = self
            .relay
            .server
            .as_ref()
            .is_some_and(|server| server.local_addr().is_some())
            || !self.connections.is_empty();
        (!serving).then_some(client)
    }

    /// The options the server runs with.
    pub fn options(&self) -> &RealtimeOptions {
        &self.options
    }

    /// The number of open connections.
    pub fn connections(&self) -> usize {
        self.connections.len()
    }

    /// The number of connections subscribed to `channel`.
    pub fn subscribers(&self, channel: &str) -> usize {
        self.channels
            .get(channel)
            .map_or(0, |state| state.subscribers.len())
    }

    /// Sends `event` with `data` (any serializable value) to every
    /// connection subscribed to each of `channels`. Returns the number of
    /// deliveries queued (a connection on two of the channels counts twice).
    ///
    /// ```no_run
    /// # async fn example(realtime: &navis_realtime::Realtime) -> Result<(), navis_realtime::RealtimeError> {
    /// realtime.broadcast(
    ///     ["private-crm.deals"],
    ///     "deal.updated",
    ///     serde_json::json!({ "deal": { "id": 7 } }),
    /// )?;
    /// # Ok(()) }
    /// ```
    pub fn broadcast<I>(
        &self,
        channels: I,
        event: &str,
        data: impl Serialize,
    ) -> Result<usize, RealtimeError>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        check_event(event)?;
        let data = serde_json::to_string(&data)?;
        let mut delivered = 0;
        for channel in channels {
            let channel = channel.as_ref();
            let Some(state) = self.channels.get(channel) else {
                continue;
            };
            let message = Utf8Bytes::from(frame(event, Some(channel), &data));
            for subscriber in state.subscribers.values() {
                if subscriber.connection.push(message.clone()) {
                    delivered += 1;
                }
            }
        }
        Ok(delivered)
    }

    /// Closes every connection of the user `user_id` (code 4009, "You got
    /// disconnected by the app."; clients do not reconnect on their own).
    /// Returns the number of connections closed.
    ///
    /// A connection belongs to a user once the auth endpoint signed a
    /// channel for it on their behalf, it joined a presence channel as them,
    /// or its handshake request was authenticated.
    pub fn disconnect_user(&self, user_id: impl fmt::Display) -> usize {
        self.close_user(&user_id.to_string(), CloseReason::DISCONNECTED)
    }

    /// Ends every subscription of the user `user_id` in this process: their
    /// connections close with code 4200 ("reconnect"), so clients connect
    /// again and authorize each private channel with their current
    /// credentials; channels they may no longer join stay closed. Used
    /// when a user's roles change or they sign out
    /// (`Broadcaster::end_user_subscriptions`, which also reaches the
    /// serving process from workers). Returns the number of connections
    /// closed.
    pub fn end_user_subscriptions(&self, user_id: impl fmt::Display) -> usize {
        self.close_user(&user_id.to_string(), CloseReason::RESUBSCRIBE)
    }

    fn close_user(&self, user_id: &str, reason: CloseReason) -> usize {
        let targets: Vec<Arc<Connection>> = self
            .connections
            .iter()
            .filter(|entry| entry.value().user().as_deref() == Some(user_id))
            .map(|entry| entry.value().clone())
            .collect();
        for connection in &targets {
            connection.close(reason);
        }
        if !targets.is_empty() {
            tracing::info!(
                target: "navis::realtime",
                user_id = %user_id,
                connections = targets.len(),
                code = reason.code,
                "closed the user's realtime connections"
            );
        }
        targets.len()
    }

    /// Handles a message relayed by another process of the app
    /// (`POST /apps/{app_id}/events`): 404 for another app or when this
    /// process has no relay key, 401 for a missing, wrong or stale
    /// signature, 400 for a malformed message; otherwise it is applied to
    /// this process's connections.
    pub(crate) fn receive_relayed(
        &self,
        app_id: &str,
        signature: Option<&str>,
        body: &[u8],
    ) -> Result<Value, AppError> {
        let Some(key) = self
            .relay
            .key
            .as_ref()
            .filter(|_| app_id == self.options.app_id)
        else {
            return Err(AppError::not_found());
        };
        let signed = signature.is_some_and(|signature| {
            relay::verify(key.expose(), signature, body, relay::unix_now())
        });
        if !signed {
            tracing::warn!(target: "navis::security", "relayed realtime message with a bad signature");
            return Err(AppError::unauthenticated().with_detail("Invalid relay signature"));
        }
        let message: RelayMessage = serde_json::from_slice(body)
            .map_err(|_| AppError::bad_request("The relayed message is malformed"))?;
        match message {
            RelayMessage::Broadcast {
                channels,
                event,
                data,
            } => {
                let delivered = self
                    .broadcast(channels, &event, data)
                    .map_err(|error| AppError::bad_request(error.to_string()))?;
                Ok(json!({ "delivered": delivered }))
            }
            RelayMessage::EndUserSubscriptions { user_id } => {
                Ok(json!({ "closed": self.end_user_subscriptions(user_id) }))
            }
        }
    }

    /// The `auth` value for subscribing socket `socket_id` to `channel`
    /// (with `channel_data` for presence channels):
    /// `<key>:<hex HMAC-SHA256(secret, socket_id:channel[:channel_data])>`.
    /// The auth endpoint uses it; custom auth endpoints may too.
    pub fn sign(&self, socket_id: &str, channel: &str, channel_data: Option<&str>) -> String {
        protocol::sign(
            &self.options.key,
            self.secret.expose(),
            socket_id,
            channel,
            channel_data,
        )
    }

    /// The app's shutdown token (connections close when it is cancelled).
    pub(crate) fn shutdown_token(&self) -> &CancellationToken {
        &self.shutdown
    }

    /// The app's task tracker (shutdown waits for connections).
    pub(crate) fn tracker(&self) -> &TaskTracker {
        &self.tracker
    }

    /// Whether a page from `origin` may connect: no `Origin` (not a
    /// browser), an allowed origin (or `*`), or the server's own origin
    /// (`host` is the request's `Host` header).
    pub(crate) fn origin_allowed(&self, origin: Option<&str>, host: Option<&str>) -> bool {
        let Some(origin) = origin else {
            return true;
        };
        let origin = origin.trim().trim_end_matches('/');
        if self.allowed_origins.iter().any(|allowed| {
            allowed == "*"
                || allowed
                    .trim()
                    .trim_end_matches('/')
                    .eq_ignore_ascii_case(origin)
        }) {
            return true;
        }
        match (origin.split_once("://"), host) {
            (Some((_, authority)), Some(host)) => authority.eq_ignore_ascii_case(host.trim()),
            _ => false,
        }
    }

    /// Registers a new connection with a fresh socket id; returns it and the
    /// receiving end of its outbox.
    pub(crate) fn connect(
        &self,
        user: Option<String>,
    ) -> (Arc<Connection>, mpsc::Receiver<Utf8Bytes>) {
        let (sender, receiver) = mpsc::channel(OUTBOX_CAPACITY);
        loop {
            let socket_id = format!("{}.{}", rand::random::<u32>(), rand::random::<u32>());
            if let Entry::Vacant(vacant) = self.connections.entry(socket_id.clone()) {
                let connection = Arc::new(Connection {
                    socket_id,
                    outbox: sender,
                    closing: CancellationToken::new(),
                    close_reason: OnceLock::new(),
                    user: Mutex::new(user),
                    channels: Mutex::new(HashSet::new()),
                });
                vacant.insert(connection.clone());
                return (connection, receiver);
            }
        }
    }

    /// Removes a closed connection: leaves its channels (presence members
    /// are announced as removed) and forgets it.
    pub(crate) fn disconnect(&self, connection: &Connection) {
        let channels = std::mem::take(&mut *connection.channels());
        for channel in channels {
            self.leave(&connection.socket_id, &channel);
        }
        self.connections.remove(&connection.socket_id);
    }

    /// Handles a text frame from `connection`.
    pub(crate) fn handle(&self, connection: &Arc<Connection>, text: &str) {
        let Some(message) = ClientMessage::parse(text) else {
            connection.push(protocol::error_frame(None, "Invalid message format").into());
            return;
        };
        match message.event.as_str() {
            "pusher:ping" => {
                connection.push(frame("pusher:pong", None, "{}").into());
            }
            "pusher:pong" => {}
            "pusher:subscribe" => {
                if let Err(error) = self.subscribe(connection, &message) {
                    connection.push(protocol::error_frame(error.code, &error.message).into());
                }
            }
            "pusher:unsubscribe" => {
                if let Some(channel) = message.str("channel") {
                    connection.channels().remove(channel);
                    self.leave(&connection.socket_id, channel);
                }
            }
            event => {
                tracing::debug!(
                    target: "navis::realtime",
                    socket_id = %connection.socket_id,
                    event,
                    "ignoring an unsupported client event"
                );
            }
        }
    }

    /// `pusher:subscribe`: checks the name and, for private and presence
    /// channels, the signature; then joins.
    fn subscribe(
        &self,
        connection: &Arc<Connection>,
        message: &ClientMessage,
    ) -> Result<(), ProtocolError> {
        let channel = message
            .str("channel")
            .filter(|channel| is_valid_name(channel))
            .ok_or_else(|| ProtocolError::new(None, "Invalid channel name"))?;
        let auth = message.str("auth").unwrap_or_default();
        let member =
            match ChannelKind::of(channel) {
                ChannelKind::Public => None,
                ChannelKind::Private => {
                    self.verify(&connection.socket_id, channel, None, auth)?;
                    None
                }
                ChannelKind::Presence => {
                    let data = message.str("channel_data").ok_or_else(|| {
                        ProtocolError::unauthorized("Presence channels need channel_data")
                    })?;
                    self.verify(&connection.socket_id, channel, Some(data), auth)?;
                    Some(Member::parse(data).ok_or_else(|| {
                        ProtocolError::unauthorized("channel_data needs a user_id")
                    })?)
                }
            };
        {
            let mut channels = connection.channels();
            if !channels.contains(channel) && channels.len() >= MAX_SUBSCRIPTIONS {
                return Err(ProtocolError::new(
                    None,
                    &format!("A connection may subscribe to at most {MAX_SUBSCRIPTIONS} channels"),
                ));
            }
            channels.insert(channel.to_owned());
        }
        if let Some(member) = &member {
            connection.bind_user(&member.user_id);
        }
        self.join(connection, channel, member);
        Ok(())
    }

    /// Checks a subscription signature.
    fn verify(
        &self,
        socket_id: &str,
        channel: &str,
        channel_data: Option<&str>,
        auth: &str,
    ) -> Result<(), ProtocolError> {
        if protocol::verify(
            &self.options.key,
            self.secret.expose(),
            socket_id,
            channel,
            channel_data,
            auth,
        ) {
            Ok(())
        } else {
            tracing::debug!(
                target: "navis::realtime",
                socket_id,
                channel,
                "rejected a subscription with an invalid signature"
            );
            Err(ProtocolError::unauthorized("Connection is unauthorized"))
        }
    }

    /// Adds `connection` to `channel`, confirms the subscription and
    /// announces a new presence member to the others.
    fn join(&self, connection: &Arc<Connection>, channel: &str, member: Option<Member>) {
        let mut state = self.channels.entry(channel.to_owned()).or_default();
        let rejoin = state.subscribers.contains_key(&connection.socket_id);
        let added = match &member {
            Some(member) if !rejoin && !state.has_user(&member.user_id) => Some(member.clone()),
            _ => None,
        };
        if !rejoin {
            state.subscribers.insert(
                connection.socket_id.clone(),
                Subscriber {
                    connection: connection.clone(),
                    member,
                },
            );
        }
        let data = match ChannelKind::of(channel) {
            ChannelKind::Presence => state.presence().to_string(),
            _ => "{}".to_owned(),
        };
        connection.push(
            frame(
                "pusher_internal:subscription_succeeded",
                Some(channel),
                &data,
            )
            .into(),
        );
        if let Some(member) = added {
            let data = json!({ "user_id": member.user_id, "user_info": member.user_info });
            let message = Utf8Bytes::from(frame(
                "pusher_internal:member_added",
                Some(channel),
                &data.to_string(),
            ));
            for (socket_id, subscriber) in &state.subscribers {
                if *socket_id != connection.socket_id {
                    subscriber.connection.push(message.clone());
                }
            }
        }
    }

    /// Removes `socket_id` from `channel`, announcing a presence member
    /// whose last connection left.
    fn leave(&self, socket_id: &str, channel: &str) {
        let Entry::Occupied(mut entry) = self.channels.entry(channel.to_owned()) else {
            return;
        };
        let Some(removed) = entry.get_mut().subscribers.remove(socket_id) else {
            return;
        };
        if let Some(member) = removed.member
            && !entry.get().has_user(&member.user_id)
        {
            let data = json!({ "user_id": member.user_id });
            let message = Utf8Bytes::from(frame(
                "pusher_internal:member_removed",
                Some(channel),
                &data.to_string(),
            ));
            for subscriber in entry.get().subscribers.values() {
                subscriber.connection.push(message.clone());
            }
        }
        if entry.get().subscribers.is_empty() {
            entry.remove();
        }
    }

    /// Records that `user_id` owns `socket_id`, if that connection is here.
    pub(crate) fn bind_user(&self, socket_id: &str, user_id: &str) {
        let connection = self
            .connections
            .get(socket_id)
            .map(|entry| entry.value().clone());
        if let Some(connection) = connection {
            connection.bind_user(user_id);
        }
    }

    /// The auth endpoint's work: asks the channel's authorizer and signs.
    /// Returns `{"auth"}` or, for presence channels, `{"auth", "channel_data"}`.
    pub(crate) async fn authorize(
        &self,
        principal: &Principal,
        socket_id: &str,
        channel: &str,
        ctx: &AppContext,
    ) -> Result<Value, AuthFailure> {
        if !protocol::is_valid_socket_id(socket_id) {
            return Err(AuthFailure::Invalid {
                field: "socket_id",
                message: "socket_id must look like 1234.5678",
            });
        }
        if !is_valid_name(channel) {
            return Err(AuthFailure::Invalid {
                field: "channel_name",
                message: "channel_name must be a valid channel name",
            });
        }
        let kind = ChannelKind::of(channel);
        if kind == ChannelKind::Public {
            return Err(AuthFailure::Forbidden);
        }
        let name = unprefixed(channel);
        let Some((authorizer, params)) = self.authorizers.iter().find_map(|authorizer| {
            authorizer
                .pattern
                .matches(name)
                .map(|params| (authorizer, params))
        }) else {
            tracing::debug!(target: "navis::realtime", channel, "no channel authorizer matches");
            return Err(AuthFailure::Forbidden);
        };
        let decision = (authorizer.authorize)(principal, &params, ctx)
            .await
            .map_err(|source| {
                AuthFailure::Failed(
                    format!(
                        "the authorizer of channel pattern `{}` failed: {source}",
                        authorizer.pattern.source()
                    )
                    .into(),
                )
            })?;
        let user_id = principal.user_id.to_string();
        let response = match (kind, decision) {
            (_, ChannelDecision::Deny) => {
                tracing::debug!(
                    target: "navis::realtime",
                    channel,
                    user_id = %user_id,
                    "channel authorization denied"
                );
                return Err(AuthFailure::Forbidden);
            }
            (ChannelKind::Presence, decision) => {
                let mut data = json!({ "user_id": user_id });
                if let ChannelDecision::Presence(user_info) = decision {
                    data["user_info"] = user_info;
                }
                let channel_data = data.to_string();
                let auth = self.sign(socket_id, channel, Some(&channel_data));
                json!({ "auth": auth, "channel_data": channel_data })
            }
            _ => json!({ "auth": self.sign(socket_id, channel, None) }),
        };
        self.bind_user(socket_id, &user_id);
        Ok(response)
    }
}

/// In the process serving the sockets (or without a relay), events go to
/// this process's subscribers; elsewhere (`queue:work`, `schedule:work`,
/// commands) they are posted to the serving process through the relay.
impl Broadcaster for Realtime {
    fn broadcast(
        &self,
        channels: Vec<String>,
        event: String,
        data: Value,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            check_event(&event)?;
            match self.relay_target() {
                Some(relay) => {
                    relay
                        .send(&RelayMessage::Broadcast {
                            channels,
                            event,
                            data,
                        })
                        .await
                }
                None => Realtime::broadcast(self, channels, &event, data)
                    .map(drop)
                    .map_err(BoxError::from),
            }
        })
    }

    fn end_user_subscriptions(&self, user_id: Uuid) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            match self.relay_target() {
                Some(relay) => {
                    relay
                        .send(&RelayMessage::EndUserSubscriptions { user_id })
                        .await
                }
                None => {
                    Realtime::end_user_subscriptions(self, user_id);
                    Ok(())
                }
            }
        })
    }
}

/// [`RealtimeError::InvalidEvent`] unless `event` is a name apps may send.
fn check_event(event: &str) -> Result<(), RealtimeError> {
    if event.is_empty()
        || event.len() > MAX_EVENT_NAME
        || event.starts_with("pusher:")
        || event.starts_with("pusher_internal:")
    {
        return Err(RealtimeError::InvalidEvent(event.to_owned()));
    }
    Ok(())
}

impl fmt::Debug for Realtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Realtime")
            .field("options", &self.options)
            .field("allowed_origins", &self.allowed_origins)
            .field("channel_patterns", &self.authorizers.len())
            .field("connections", &self.connections.len())
            .field("channels", &self.channels.len())
            .finish_non_exhaustive()
    }
}

/// Why the auth endpoint refuses.
#[derive(Debug)]
pub(crate) enum AuthFailure {
    /// A malformed request field (400).
    Invalid {
        field: &'static str,
        message: &'static str,
    },
    /// Not allowed (403).
    Forbidden,
    /// The authorizer failed (500).
    Failed(BoxError),
}

/// A `pusher:error` to send back to a client.
#[derive(Debug)]
pub(crate) struct ProtocolError {
    pub(crate) code: Option<u16>,
    pub(crate) message: String,
}

impl ProtocolError {
    fn new(code: Option<u16>, message: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
        }
    }

    fn unauthorized(message: &str) -> Self {
        Self::new(Some(protocol::UNAUTHORIZED), message)
    }
}

/// Why the server closes a connection: a Pusher close code and message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CloseReason {
    pub(crate) code: u16,
    pub(crate) message: &'static str,
}

impl CloseReason {
    /// [`Realtime::disconnect_user`].
    pub(crate) const DISCONNECTED: CloseReason = CloseReason {
        code: protocol::UNAUTHORIZED,
        message: "You got disconnected by the app.",
    };
    /// [`Realtime::end_user_subscriptions`].
    pub(crate) const RESUBSCRIBE: CloseReason = CloseReason {
        code: protocol::RECONNECT,
        message: "Your access changed; reconnect and subscribe again",
    };
    /// The outbox overflowed.
    pub(crate) const OVER_CAPACITY: CloseReason = CloseReason {
        code: protocol::OVER_CAPACITY,
        message: "Over capacity: the connection does not read its messages",
    };
    /// The app shuts down.
    pub(crate) const SHUTTING_DOWN: CloseReason = CloseReason {
        code: protocol::RECONNECT,
        message: "The server is shutting down; reconnect",
    };
    /// No answer to the server's ping.
    pub(crate) const PONG_TIMEOUT: CloseReason = CloseReason {
        code: protocol::PONG_TIMEOUT,
        message: "Pong reply not received",
    };
}

/// One open WebSocket connection.
pub(crate) struct Connection {
    pub(crate) socket_id: String,
    outbox: mpsc::Sender<Utf8Bytes>,
    closing: CancellationToken,
    close_reason: OnceLock<CloseReason>,
    user: Mutex<Option<String>>,
    channels: Mutex<HashSet<String>>,
}

impl Connection {
    /// Queues a frame; a full outbox closes the connection. Returns whether
    /// the frame was queued.
    pub(crate) fn push(&self, message: Utf8Bytes) -> bool {
        match self.outbox.try_send(message) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.close(CloseReason::OVER_CAPACITY);
                false
            }
            Err(TrySendError::Closed(_)) => false,
        }
    }

    /// Asks the connection's task to close it with `reason` (the first
    /// reason wins).
    pub(crate) fn close(&self, reason: CloseReason) {
        let _ = self.close_reason.set(reason);
        self.closing.cancel();
    }

    /// Cancelled when the connection must close.
    pub(crate) fn closing(&self) -> &CancellationToken {
        &self.closing
    }

    /// Why the connection must close.
    pub(crate) fn close_reason(&self) -> Option<CloseReason> {
        self.close_reason.get().copied()
    }

    /// The user owning the connection, if known.
    fn user(&self) -> Option<String> {
        self.user
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Records the owner, unless one is already known.
    fn bind_user(&self, user_id: &str) {
        let mut user = self.user.lock().unwrap_or_else(PoisonError::into_inner);
        if user.is_none() {
            *user = Some(user_id.to_owned());
        }
    }

    fn channels(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.channels.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A channel's subscribers, by socket id.
#[derive(Default)]
struct ChannelState {
    subscribers: HashMap<String, Subscriber>,
}

impl ChannelState {
    /// Whether any subscriber is the presence member `user_id`.
    fn has_user(&self, user_id: &str) -> bool {
        self.subscribers
            .values()
            .any(|s| s.member.as_ref().is_some_and(|m| m.user_id == user_id))
    }

    /// `{"presence": {"ids", "hash", "count"}}`: one entry per user.
    fn presence(&self) -> Value {
        let hash: BTreeMap<&str, &Value> = self
            .subscribers
            .values()
            .filter_map(|s| s.member.as_ref())
            .map(|m| (m.user_id.as_str(), &m.user_info))
            .collect();
        let ids: Vec<&str> = hash.keys().copied().collect();
        json!({ "presence": { "ids": ids, "hash": hash, "count": ids.len() } })
    }
}

struct Subscriber {
    connection: Arc<Connection>,
    member: Option<Member>,
}

/// A presence channel member, from the signed `channel_data`.
#[derive(Clone, Debug)]
struct Member {
    user_id: String,
    user_info: Value,
}

impl Member {
    /// `{"user_id": "…" | 42, "user_info"?: …}`.
    fn parse(channel_data: &str) -> Option<Member> {
        let mut data: serde_json::Map<String, Value> = serde_json::from_str(channel_data).ok()?;
        let user_id = match data.remove("user_id")? {
            Value::String(id) if !id.is_empty() => id,
            Value::Number(id) => id.to_string(),
            _ => return None,
        };
        Some(Member {
            user_id,
            user_info: data.remove("user_info").unwrap_or(Value::Null),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn realtime(allowed_origins: &[&str]) -> Realtime {
        Realtime::new(
            RealtimeOptions {
                allowed_origins: allowed_origins.iter().map(|o| (*o).to_owned()).collect(),
                ..RealtimeOptions::default()
            },
            vec!["http://cors.example".to_owned()],
            Vec::new(),
            CancellationToken::new(),
            TaskTracker::new(),
        )
        .expect("valid options")
    }

    #[test]
    fn origins_match_exactly_ignoring_case_and_a_trailing_slash() {
        let hub = realtime(&["https://App.example.com/"]);
        let allowed = |origin: &str| hub.origin_allowed(Some(origin), Some("api.example.com"));
        assert!(allowed("https://app.example.com"));
        assert!(allowed("HTTPS://APP.EXAMPLE.COM/"));
        assert!(!allowed("http://app.example.com"), "scheme matters");
        assert!(!allowed("https://app.example.com.evil.net"));
        assert!(!allowed("null"));
        assert!(
            !allowed("http://cors.example"),
            "own list replaces the CORS list"
        );
        assert!(allowed("https://api.example.com"), "same origin");
        assert!(hub.origin_allowed(None, None), "not a browser");

        let any = realtime(&["*"]);
        assert!(any.origin_allowed(Some("https://anything.example"), None));
    }

    #[test]
    fn empty_allowed_origins_fall_back_to_cors() {
        let realtime = realtime(&[]);
        assert!(realtime.origin_allowed(Some("http://cors.example"), None));
        assert!(!realtime.origin_allowed(Some("http://other.example"), None));
    }

    #[test]
    fn a_full_outbox_closes_the_connection() {
        let realtime = realtime(&[]);
        let (connection, mut outbox) = realtime.connect(None);
        for _ in 0..OUTBOX_CAPACITY {
            assert!(connection.push(Utf8Bytes::from_static("{}")));
        }
        assert!(!connection.closing().is_cancelled());
        assert!(
            !connection.push(Utf8Bytes::from_static("{}")),
            "the outbox is full"
        );
        assert!(connection.closing().is_cancelled());
        assert_eq!(connection.close_reason(), Some(CloseReason::OVER_CAPACITY));
        assert_eq!(protocol::OVER_CAPACITY, 4100);

        // A later reason does not replace the first one.
        connection.close(CloseReason::DISCONNECTED);
        assert_eq!(connection.close_reason(), Some(CloseReason::OVER_CAPACITY));
        assert!(outbox.try_recv().is_ok());
    }

    #[test]
    fn disconnect_forgets_the_connection_and_its_channels() {
        let realtime = realtime(&[]);
        let (connection, _outbox) = realtime.connect(Some("u1".to_owned()));
        let subscribe =
            ClientMessage::parse(r#"{"event":"pusher:subscribe","data":{"channel":"news"}}"#)
                .unwrap();
        realtime.subscribe(&connection, &subscribe).unwrap();
        assert_eq!(
            (realtime.connections(), realtime.subscribers("news")),
            (1, 1)
        );

        realtime.disconnect(&connection);
        assert_eq!(
            (realtime.connections(), realtime.subscribers("news")),
            (0, 0)
        );
        assert!(realtime.channels.is_empty(), "empty channels are dropped");
    }

    #[test]
    fn members_parse_string_or_number_ids() {
        let member = Member::parse(r#"{"user_id":10,"user_info":{"name":"Ada"}}"#).unwrap();
        assert_eq!(member.user_id, "10");
        assert_eq!(member.user_info["name"], "Ada");
        let member = Member::parse(r#"{"user_id":"u1"}"#).unwrap();
        assert_eq!(
            (member.user_id.as_str(), member.user_info),
            ("u1", Value::Null)
        );
        for bad in [
            "",
            "[]",
            "{}",
            r#"{"user_id":""}"#,
            r#"{"user_id":null}"#,
            r#"{"user_id":[1]}"#,
        ] {
            assert!(Member::parse(bad).is_none(), "{bad}");
        }
    }
}

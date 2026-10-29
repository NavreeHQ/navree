//! # navree-webhooks
//!
//! `WebhooksModule`: signed outgoing webhooks with queue-backed retries, a
//! delivery log and endpoint management, and verified incoming webhooks.
//!
//! **Outgoing.** Endpoints (a URL, the event types it receives, an optional
//! workspace) are managed with [`Webhooks`] or the `/api/webhooks` routes.
//! An event registered with [`WebhooksModule::for_feature`] (it implements
//! [`WebhookEvent`]), or a payload sent with [`Webhooks::dispatch_json`],
//! is fanned out on the queue ([`FanOut`]): one delivery per subscribed
//! endpoint, each posted by a [`Deliver`] job and signed per
//! [Standard Webhooks](https://www.standardwebhooks.com) (`webhook-id`,
//! `webhook-timestamp`, `webhook-signature: v1,<base64 HMAC-SHA256>`, keyed
//! with the endpoint's `whsec_` secret). A failed attempt is retried on
//! [`RETRY_SCHEDULE`] (eight attempts over about 27 hours); every attempt
//! is logged; an endpoint that keeps failing for
//! `webhooks.disable_after_days`, or answers `410 Gone`, is disabled.
//!
//! **Incoming.** [`VerifiedWebhook<S>`] reads the raw body and verifies it
//! with the source's verifier and secret before the handler runs:
//! [`Stripe`], [`GitHub`], [`Paddle`], [`StandardWebhooks`], [`AwsSns`],
//! [`GooglePubSub`], [`MicrosoftGraph`], [`Twilio`], [`Slack`],
//! [`Mailgun`], [`Postmark`], [`Vonage`], [`Meta`] or the configurable
//! [`Hmac`]; handshakes (Graph's `validationToken`, Slack's
//! `url_verification`, SNS subscription confirmations, Meta's
//! `hub.challenge`) are answered without running the handler;
//! [`VerifiedWebhook::mark_processed`] drops redeliveries.
//!
//! **Notification callbacks** (crate feature `notify`): with navree-notify,
//! `WebhooksModule` mounts `POST {api_prefix}/webhooks/notify/twilio`,
//! `/vonage` and `GET`/`POST /meta` for each of those sources that is
//! configured, verified like any route of the source, which apply the
//! providers' status callbacks and STOP replies to the notification
//! delivery log and opt-outs ([`NotifyCallbacksController`]).
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_db::DatabaseModule;
//! use navree_events::EventsModule;
//! use navree_http::HttpModule;
//! use navree_http_client::HttpClientModule;
//! use navree_kernel::Navree;
//! use navree_queue::QueueModule;
//! use navree_webhooks::WebhooksModule;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(HttpModule::from_config())
//!     .module(DatabaseModule::from_config())
//!     .module(EventsModule::for_root())
//!     .module(QueueModule::from_config())
//!     .module(HttpClientModule::from_config())
//!     .module(WebhooksModule::from_config()) // section `webhooks`
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod entity;
pub mod events;

mod changes;
mod commands;
mod controller;
mod error;
mod event;
mod extract;
mod jobs;
mod keys;
mod migration;
mod module;
#[cfg(feature = "notify")]
mod notify;
mod options;
mod request;
mod resources;
mod service;
mod signing;
mod sources;
mod template;
mod verify;

pub use controller::{
    CreateEndpointInput, MANAGE_PERMISSION, RotatedSecret, UpdateEndpointInput, WebhooksController,
};
pub use error::WebhooksError;
pub use event::{WebhookContext, WebhookEvent};
pub use events::{WebhookDeliveryFailed, WebhookEndpointDisabled};
pub use extract::{INVALID_SIGNATURE, INVALID_SIGNATURE_CODE, VerifiedWebhook};
pub use jobs::{Deliver, FanOut, MAX_ATTEMPTS, RETRY_SCHEDULE, retry_delay};
pub use keys::SenderKeys;
pub use migration::WEBHOOKS_MIGRATIONS;
pub use module::{ModuleBuilderWebhooksExt, WebhooksModule};
#[cfg(feature = "notify")]
pub use notify::{NOTIFY_CALLBACKS_PREFIX, NotifyCallbacksController};
pub use options::{
    HmacAlgorithm, IncomingOptions, SignatureEncoding, SourceOptions, WebhooksOptions,
};
pub use request::{Handshake, IncomingRequest, Verdict};
pub use resources::{
    Attempt, CreatedEndpoint, Delivery, DeliveryStatus, Endpoint, EndpointChanges, EventType,
    NewEndpoint,
};
pub use service::{
    Dispatch, Pruned, ROTATION_OVERLAP, TEST_EVENT_TYPE, USER_AGENT_VALUE, Webhooks,
};
pub use signing::{SECRET_PREFIX, generate_secret};
pub use sources::{
    AwsSns, GooglePubSub, GraphNotification, GraphNotifications, Mailgun, Meta, MicrosoftGraph,
    Postmark, PubSubMessage, PubSubPush, Slack, SnsMessage, Twilio, Vonage,
};
pub use verify::{
    GitHub, Hmac, Paddle, StandardWebhooks, Stripe, Tolerance, Verified, VerifyError,
    WebhookSource, WebhookVerifier,
};

//! # navree-events
//!
//! `EventsModule`: a typed, in-process event bus with synchronous and queued
//! listeners.
//!
//! - An [`Event`] is a serializable value with a stable [`NAME`](Event::NAME).
//! - A [`Listener`] is a provider that handles one or more event types. Modules
//!   subscribe listeners with [`ModuleBuilderEventsExt::listen`] and
//!   [`on`](ModuleBuilderEventsExt::on); the boot check verifies that every
//!   subscribed listener is visible to its module.
//! - [`EventBus::emit`] runs sync listeners in subscription order and hands
//!   queued ones to the queue as jobs (through `dyn Dispatcher` from
//!   navree-contracts, so this crate does not depend on the queue). A failing
//!   listener never stops the others; the emitter gets every failure in one
//!   [`EventsError::ListenersFailed`].
//! - The bus also implements [`EventSink`](navree_contracts::EventSink): crates
//!   that only know the contracts publish `(name, JSON)` pairs, which reach
//!   the typed listeners of that name and [`EventBus::subscribe_json`]
//!   subscribers.
//!
//! ```
//! use navree_events::{Event, EventBus, EventsModule, Listener, ModuleBuilderEventsExt};
//! use navree_kernel::{
//!     AppContext, BoxError, BoxFuture, Injectable, Navree, KernelError, Module, ModuleBuilder,
//!     Resolver,
//! };
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Clone, Debug, Serialize, Deserialize)]
//! struct UserRegistered {
//!     email: String,
//! }
//!
//! impl Event for UserRegistered {
//!     const NAME: &'static str = "user.registered";
//! }
//!
//! /// Sends the welcome mail from a queue worker.
//! struct SendWelcomeMail;
//!
//! impl Injectable for SendWelcomeMail {
//!     fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
//!         Box::pin(async { Ok(SendWelcomeMail) })
//!     }
//! }
//!
//! impl Listener<UserRegistered> for SendWelcomeMail {
//!     fn handle<'a>(
//!         &'a self,
//!         event: &'a UserRegistered,
//!         _ctx: &'a AppContext,
//!     ) -> BoxFuture<'a, Result<(), BoxError>> {
//!         Box::pin(async move {
//!             println!("welcome, {}", event.email);
//!             Ok(())
//!         })
//!     }
//!
//!     // Runs as job `events.listener.user.registered.<type name>`; an app
//!     // without a queue module (like this one) runs it inline.
//!     fn queued(&self) -> bool {
//!         true
//!     }
//! }
//!
//! struct UsersModule;
//!
//! impl Module for UsersModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.listen::<UserRegistered, SendWelcomeMail>();
//!     }
//! }
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), BoxError> {
//! let app = Navree::app()
//!     .module(EventsModule::for_root())
//!     .module(UsersModule)
//!     .boot()
//!     .await?;
//!
//! let bus = app.ctx().get::<EventBus>().await?;
//! bus.emit(UserRegistered { email: "ada@example.com".into() }).await?;
//!
//! app.shutdown().await?;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod bus;
mod error;
mod event;
mod module;
mod registry;

pub use bus::EventBus;
pub use error::{EventsError, ListenerFailure};
pub use event::{Event, Listener};
pub use module::EventsModule;
pub use registry::{ModuleBuilderEventsExt, listener_job_name};

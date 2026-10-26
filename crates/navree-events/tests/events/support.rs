//! Helpers shared by the navree-events integration tests.

#![allow(dead_code, unused_macros, unused_imports)]

use std::sync::{Arc, Mutex};

use navree_contracts::{BoxError, BoxFuture, Dispatcher, JobEnvelope};
use navree_events::Event;
use serde::{Deserialize, Serialize};

/// The event most tests emit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserRegistered {
    pub email: String,
}

impl Event for UserRegistered {
    const NAME: &'static str = "user.registered";
}

impl UserRegistered {
    pub fn new(email: &str) -> Self {
        Self {
            email: email.to_owned(),
        }
    }
}

/// A second event type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserDeleted {
    pub id: u64,
}

impl Event for UserDeleted {
    const NAME: &'static str = "user.deleted";
}

/// Records what listeners saw, in order. Tests register it with
/// `override_value`, which makes it visible to every module.
#[derive(Default, Debug)]
pub struct Journal(Mutex<Vec<String>>);

impl Journal {
    pub fn push(&self, entry: impl Into<String>) {
        self.0.lock().unwrap().push(entry.into());
    }

    pub fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// A dispatcher that captures envelopes instead of queueing them.
#[derive(Default)]
pub struct CaptureDispatcher(Mutex<Vec<JobEnvelope>>);

impl CaptureDispatcher {
    pub fn envelopes(&self) -> Vec<JobEnvelope> {
        self.0.lock().unwrap().clone()
    }
}

impl Dispatcher for CaptureDispatcher {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        Box::pin(async move {
            let mut captured = self.0.lock().unwrap();
            captured.push(env);
            Ok(format!("job-{}", captured.len()))
        })
    }
}

/// Declares a listener struct that depends on [`Journal`] and, for each
/// listed event, records `"<label>:<entry>"` where `entry` is computed from
/// the event by the given closure-like expression.
///
/// ```ignore
/// listener!(Welcome, "welcome", [UserRegistered => |e| e.email.clone()]);
/// listener!(Mailer, "mailer", queued, [UserRegistered => |e| e.email.clone()]);
/// ```
macro_rules! listener {
    ($name:ident, $label:literal, [$($event:ty => |$e:ident| $entry:expr),+ $(,)?]) => {
        listener!(@define $name, $label, false, [$($event => |$e| $entry),+]);
    };
    ($name:ident, $label:literal, queued, [$($event:ty => |$e:ident| $entry:expr),+ $(,)?]) => {
        listener!(@define $name, $label, true, [$($event => |$e| $entry),+]);
    };
    (@define $name:ident, $label:literal, $queued:expr, [$($event:ty => |$e:ident| $entry:expr),+]) => {
        pub struct $name {
            pub journal: ::std::sync::Arc<$crate::support::Journal>,
        }

        impl ::navree_kernel::Injectable for $name {
            fn dependencies() -> Vec<::navree_kernel::ProviderKey> {
                vec![::navree_kernel::ProviderKey::of::<$crate::support::Journal>()]
            }

            fn construct(
                r: ::navree_kernel::Resolver,
            ) -> ::navree_kernel::BoxFuture<'static, Result<Self, ::navree_kernel::KernelError>> {
                Box::pin(async move {
                    Ok(Self {
                        journal: r.get::<$crate::support::Journal>().await?,
                    })
                })
            }
        }

        $(
            impl ::navree_events::Listener<$event> for $name {
                fn handle<'a>(
                    &'a self,
                    event: &'a $event,
                    _ctx: &'a ::navree_kernel::AppContext,
                ) -> ::navree_kernel::BoxFuture<'a, Result<(), ::navree_kernel::BoxError>> {
                    Box::pin(async move {
                        let $e = event;
                        self.journal.push(format!("{}:{}", $label, $entry));
                        Ok(())
                    })
                }

                fn queued(&self) -> bool {
                    $queued
                }
            }
        )+
    };
}

pub(crate) use listener;

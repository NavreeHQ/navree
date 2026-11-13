//! The device's push token, and signing out on purpose.
//!
//! - The token reaches the server by one route, the one the shell names, and
//!   the core posts it there: when the client hands it a token and somebody
//!   is signed in, again when the token changes, and again for the next
//!   person who signs in. A client makes no such request itself.
//! - A token the server would not take is not posted again until it changes
//!   or another person signs in. One that could not be sent is tried again
//!   when the app comes to the front or the shell arrives.
//! - When the person signs out on purpose (the reply says so), the token is
//!   taken away at the same route first. Only when that is answered, or has
//!   failed, is the client told to forget the session's cookies: a request
//!   takes its cookies when it is sent, not when it is asked for. Then the
//!   app starts again from the shell, which says nobody is signed in.
//! - A session that ran out is not a sign-out: the way in is put up over the
//!   work, and the work stays.

use navree_native::Platform;
use navree_native::device::{PushDevice, PushPlatform};

use crate::effect::{Body, Change, Effect};
use crate::event::Response;
use crate::origin::Target;
use crate::state::{Asked, Pending, Stage};
use crate::Core;

/// How long the removal of a token may take, in milliseconds. The server
/// ends a device with its session whether or not the removal arrives.
const REMOVAL_LIMIT: u32 = 5_000;
/// How long a registration may take.
const REGISTER_LIMIT: u32 = 30_000;

impl Core {
    fn push_platform(&self) -> Option<PushPlatform> {
        match self.config.client.platform {
            Platform::Ios | Platform::Ipados | Platform::Macos | Platform::Watchos | Platform::Tvos | Platform::Visionos => Some(PushPlatform::Ios),
            Platform::Android | Platform::Wearos => Some(PushPlatform::Android),
            _ => None,
        }
    }

    /// The system gave the device a token, or changed it.
    pub(crate) fn push_token(&mut self, token: String, environment: Option<navree_native::device::PushEnvironment>) {
        if token.is_empty() {
            return;
        }
        self.inner.push.token = Some(token);
        self.inner.push.environment = environment.unwrap_or_default();
        self.push_sync();
    }

    /// Posts the device's token for the session that is signed in, when the
    /// server does not hold it yet.
    pub(crate) fn push_sync(&mut self) {
        let push = &self.inner.push;
        let (Some(token), Some(route), Some(platform)) = (push.token.clone(), self.inner.push_route.clone(), self.push_platform()) else {
            return;
        };
        let known = [&push.registered, &push.posting, &push.refused].into_iter().any(|held| held.as_ref() == Some(&token));
        if known || self.inner.user.is_none() || self.inner.signing_out || self.config.app_id.is_empty() {
            return;
        }
        let device = PushDevice {
            platform,
            token: token.clone(),
            app_id: self.config.app_id.clone(),
            environment: push.environment,
        };
        let (Ok(text), Target::Inside(url)) = (serde_json::to_string(&device), self.inner.origin.resolve(&route, "/")) else {
            return;
        };
        self.inner.push.posting = Some(token.clone());
        self.ask(Asked {
            purpose: Pending::PushPut { token },
            first: url.clone(),
            url,
            method: navree_native::node::Method::Post,
            body: Some(Body::Json { text }),
            limit: REGISTER_LIMIT,
            delete: false,
            hops: 0,
            region: None,
        });
    }

    pub(crate) fn push_put_answered(&mut self, token: String, response: &Response) {
        let push = &mut self.inner.push;
        if push.posting.as_ref() == Some(&token) {
            push.posting = None;
        }
        let status = response.status;
        if response.error.is_some() || response.left.is_some() || status >= 500 {
            // It did not get there: tried again when the app is next in front.
            return;
        }
        match status {
            200..=299 => push.registered = Some(token),
            // Nobody is signed in after all: it goes with the next sign-in.
            401 | 403 => {}
            _ => {
                push.refused = Some(token);
                self.report("push", format!("the push route answered {status}: the device is not registered"));
            }
        }
        // The system changed the token while this one was on its way.
        self.push_sync();
    }

    /// Another person is signed in, or nobody: what the server held of the
    /// token belonged to the session before.
    pub(crate) fn push_forget(&mut self) {
        self.inner.push.registered = None;
        self.inner.push.refused = None;
    }

    // Signing out on purpose.

    /// The reply said the person signed out. Everything of theirs goes at
    /// once; the session's cookies go when the push token's removal has been
    /// answered, and then the app starts again from the shell.
    pub(crate) fn signed_out_on_purpose(&mut self) {
        if self.inner.signing_out {
            return;
        }
        let token = self.inner.push.registered.take().or(self.inner.push.posting.take());
        let route = self.inner.push_route.clone();
        // Nothing asked for the person who left is wanted any more.
        self.drop_pending(|_| true);
        self.wipe();
        self.forget_all();
        self.inner.hand_over = None;
        self.inner.signing_in = false;
        self.inner.has_shell = false;
        self.inner.user = None;
        self.inner.restore = None;
        self.push_forget();
        self.view.app = None;
        self.view.destinations.clear();
        self.view.stage = Stage::Starting;
        self.touch(Change::App);
        self.touch(Change::Nav);
        let removal = token.zip(route).and_then(|(token, route)| match self.inner.origin.resolve(&route, "/") {
            Target::Inside(url) => Some((serde_json::json!({ "token": token }).to_string(), url)),
            _ => None,
        });
        match removal {
            Some((text, url)) => {
                self.inner.signing_out = true;
                self.ask(Asked {
                    purpose: Pending::PushGone,
                    first: url.clone(),
                    url,
                    method: navree_native::node::Method::Post,
                    body: Some(Body::Json { text }),
                    limit: REMOVAL_LIMIT,
                    delete: true,
                    hops: 0,
                    region: None,
                });
            }
            None => self.session_forgotten(),
        }
    }

    /// The token's removal was answered, or failed: either way the session
    /// is forgotten now.
    pub(crate) fn push_gone_answered(&mut self) {
        if std::mem::take(&mut self.inner.signing_out) {
            self.session_forgotten();
        }
    }

    /// Tells the client to forget the session, and starts again from the shell.
    fn session_forgotten(&mut self) {
        self.effects.push(Effect::ForgetSession);
        self.start();
    }
}

//! The reducer: one event in, the next state and what to do about it out.
//!
//! The rules, in short:
//! - One origin. An address is resolved before anything is done with it.
//! - Every request has a number and a purpose. An answer whose purpose is gone
//!   (the person left the screen, asked again, a newer fetch of the region
//!   began) is dropped, and changes nothing on screen.
//! - A new screen shows the kept tree of its address at once and fetches a
//!   fresh one. A build the core has not seen empties what is kept.
//! - A write makes every other screen out of date; each is fetched again when
//!   it comes to the front.
//! - What can't be read is never a crash: the web view for a page, a fresh
//!   fetch for a change to one.

use navree_native::action::Present;
use navree_native::device::{
    Call, DeviceCall, Feature, HapticKind, OUTCOME_FIELD, OUTCOME_OK, ReplyCall, TICKET_FIELD,
};
use navree_native::node::Method;
use navree_native::shell::{NavItem, Route, Shell};
use navree_native::{
    CLIENT_HEADER, CONTENT_TYPE, MAJOR, NATIVE_HEADER, Op, SHELL_PATH, Screen, Tone,
};
use serde_json::Value;

use crate::Core;
use crate::decode::{self, Answer, Read};
use crate::effect::{Body, Change, Effect, Request, Update, Verb};
use crate::event::{Event, Response, StreamState, TransportError};
use crate::origin::{Target, https_address, openable, path_of, route_matches};
use crate::state::{
    App, Asked, Content, Destination, Failure, Goes, Held, Message, MessageInner, Modal, Notice,
    Pending, ScreenId, ScreenInner, ScreenView, Stage, TimerFor,
};
use crate::tree::{Names, Survey, has_id, opens, roots, walk};

/// What the framework's own routes speak.
const JSON: &str = "application/json";
/// How long a page may take, in milliseconds.
const PAGE_LIMIT: u32 = 30_000;
/// The request header that names the part of a screen a request is for.
const REGION_HEADER: &str = "x-navree-region";
/// The most redirects followed for one request.
const MOST_REDIRECTS: u8 = 5;
/// How long the answer to a form or a button may take.
pub(crate) const SEND_LIMIT: u32 = 60_000;

/// A value a server chose, written so it can go in a header: every byte of its
/// UTF-8 outside `!` to `~`, and `%`, as `%` and two hex digits. A plain ASCII
/// id without `%` goes out as it is.
pub(crate) fn header_safe(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if (0x21..=0x7e).contains(&byte) && byte != b'%' {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// What an answer that asks for a newer app gives: its address only when it
/// is a whole `https` address that reads. Any other is dropped, as if none had
/// been given, and the app offers nothing to open.
fn newer_app(mut upgrade: navree_native::envelope::Upgrade) -> navree_native::envelope::Upgrade {
    upgrade.url = https_address(&upgrade.url).unwrap_or_default();
    upgrade
}

/// What an answer gives a screen to show.
pub(crate) enum Shown {
    /// A tree, with the status and the body it came in.
    Native {
        screen: Box<Screen>,
        status: u16,
        body: String,
        /// The answer said the page may be kept on the device.
        keep: bool,
    },
    /// The page in the web view.
    Web {
        url: String,
        title: Option<String>,
        why: Vec<String>,
    },
}

impl Core {
    /// Tells the core that something happened, and gets back what changed and
    /// what to do about it.
    pub fn send(&mut self, event: Event) -> Update {
        if !self.inner.usable {
            return Update::default();
        }
        match event {
            Event::Start => self.start(),
            Event::Response(response) => self.answered(response),
            Event::Select { destination } => self.select(destination),
            Event::Act {
                screen,
                action,
                form,
                overlay,
                text,
                now,
                ask,
                from,
            } => {
                if let Some(screen) = screen.or(self.front()) {
                    match ask {
                        // The control asks first: what it does waits for the answer.
                        Some(ask) => self.ask_first(
                            screen,
                            *ask,
                            Held {
                                question: 0,
                                action,
                                form,
                                overlay,
                                text,
                                now,
                                from,
                            },
                        ),
                        None => self.act(screen, action, form, overlay, text, now, from),
                    }
                }
            }
            Event::Answer {
                screen,
                question,
                yes,
            } => self.answer(screen, question, yes),
            Event::Choose {
                screen,
                table,
                row,
                on,
                across,
            } => self.choose(screen, &table, row.as_deref(), on, across),
            Event::Tab { screen, id, index } => self.tab(screen, &id, index),
            Event::Random { request, bytes } => self.random(request, &bytes),
            Event::PushToken { token, environment } => self.push_token(token, environment),
            Event::Restore { snapshot } => self.restore(snapshot),
            Event::Move {
                screen,
                item,
                from,
                to,
                before,
            } => self.moved(screen, &item, &from, &to, before.as_deref()),
            Event::Options {
                screen,
                form,
                field,
                text,
                more,
            } => self.options(screen, form.as_deref(), &field, text, more),
            Event::Field {
                screen,
                form,
                field,
                value,
            } => self.field_changed(screen, form.as_deref(), &field, value),
            Event::Search { screen, text } => self.search(screen, &text),
            Event::Back => self.back(),
            Event::PopTo { screen } => self.pop_to(screen),
            Event::Dismissed { screen } => {
                // The sign-in a session's end put up is not the person's to take away.
                if !self
                    .view
                    .modals
                    .iter()
                    .any(|modal| modal.locked && modal.stack.contains(&screen))
                {
                    self.dismissed(screen);
                }
            }
            Event::Overlay { screen, id, open } => self.set_open(screen, &id, open),
            Event::Refresh { screen } => self.refresh(screen),
            Event::RegionRetry { screen, region } => self.fetch_region(screen, &region, None),
            Event::More {
                screen,
                id,
                before: false,
            } => self.more(screen, &id),
            Event::More {
                screen,
                id,
                before: true,
            } => self.rows_before(screen, &id),
            Event::RegionGo {
                screen,
                region,
                href,
            } => {
                if let Target::Inside(url) = self.resolve(&href, screen) {
                    self.region_go(screen, &region, url);
                }
            }
            Event::MessageShown { id } => self.message_gone(id),
            Event::MessageAction { id } => self.message_action(id),
            Event::WebLink { screen, url } => self.web_link(screen, &url),
            Event::OpenLink { url } => self.opened(&url),
            Event::Notification { href } => self.opened(&href),
            Event::Window { width } => {
                if self.view.window.width != width {
                    self.view.window.width = width;
                    self.touch(Change::Nav);
                }
            }
            Event::Environment {
                scheme,
                contrast,
                text_percent,
                locale,
                tz,
            } => {
                let client = &mut self.config.client;
                client.scheme = scheme.unwrap_or(client.scheme);
                client.contrast = contrast.unwrap_or(client.contrast);
                client.text_percent = text_percent.unwrap_or(client.text_percent);
                if let Some(locale) = locale {
                    client.locale = locale;
                }
                if let Some(tz) = tz {
                    client.tz = tz;
                }
            }
            Event::Foreground => {
                self.inner.background = false;
                // A token that could not be sent before is tried again.
                self.push_sync();
            }
            Event::Background => {
                self.inner.background = true;
                // A hidden app holds no stream; what its regions miss is fetched when it is back.
                if let Some(inner) = self
                    .front()
                    .and_then(|front| self.inner.screens.get_mut(&front))
                {
                    inner.missed = true;
                }
            }
            Event::SignedOut => {
                // A shell asked for before now would be the old session's.
                self.drop_pending(|_| true);
                self.wipe();
                self.inner.hand_over = None;
                self.inner.signing_out = false;
                self.push_forget();
                self.inner.has_shell = false;
                self.inner.user = None;
                self.view.app = None;
                self.view.destinations.clear();
                self.view.stage = Stage::Starting;
                self.touch(Change::App);
                self.touch(Change::Nav);
                self.forget_all();
                self.start();
            }
            Event::Timer { timer } => match self.inner.timers.remove(&timer) {
                Some(TimerFor::Query { screen, form }) => {
                    if let Some(inner) = self.inner.screens.get_mut(&screen) {
                        inner.query_timers.remove(&form);
                    }
                    self.query(screen, &form);
                }
                Some(TimerFor::Live { screen, region }) => self.live_fire(screen, &region),
                Some(TimerFor::Changes { screen, list }) => self.ask_changes(screen, &list),
                Some(TimerFor::Changed {
                    screen,
                    form,
                    action,
                }) => {
                    if let Some(inner) = self.inner.screens.get_mut(&screen) {
                        inner.query_timers.remove(&form);
                    }
                    self.changed_fire(screen, &form, *action);
                }
                Some(TimerFor::Options {
                    screen,
                    form,
                    field,
                }) => self.options_fire(screen, &form, &field),
                Some(TimerFor::Reopen { stream }) => self.reopen(stream),
                None => {}
            },
            Event::Stream {
                stream,
                state,
                name,
                status,
            } => match state {
                StreamState::Open => self.stream_open(stream),
                StreamState::Event => self.stream_spoke(stream, name.as_deref()),
                StreamState::Closed => self.stream_closed(stream, status),
            },
            Event::Cache { key, value } => self.cache_loaded(&key, value),
            Event::DeviceResult {
                call,
                ok,
                refusal,
                fields,
            } => self.device_result(call, ok, refusal, fields),
            Event::SignInDone { url } => self.sign_in_done(url.as_deref()),
            Event::Focused { screen, serial } => {
                if let Some(view) = self.view.screens.get_mut(&screen)
                    && view
                        .focus
                        .as_ref()
                        .is_some_and(|focus| focus.serial == serial)
                {
                    view.focus = None;
                    self.touch(Change::Screen { id: screen });
                }
            }
            Event::OpenUpgrade => {
                if let Some(url) = self
                    .view
                    .upgrade
                    .as_ref()
                    .map(|upgrade| upgrade.url.clone())
                    .filter(|url| !url.is_empty())
                {
                    self.open_outside(&url, false);
                }
            }
        }
        self.settle()
    }

    // What every part of the reducer uses.

    pub(crate) fn touch(&mut self, change: Change) {
        self.changed.insert(change);
    }

    pub(crate) fn touch_screen(&mut self, id: ScreenId) {
        self.changed.insert(Change::Screen { id });
    }

    /// The screen the person is looking at.
    pub(crate) fn front(&self) -> Option<ScreenId> {
        self.view.front_stack().last().copied()
    }

    /// Reads `href` as found on `screen`.
    pub(crate) fn resolve(&self, href: &str, screen: ScreenId) -> Target {
        let from = self
            .view
            .screens
            .get(&screen)
            .map_or("/", |screen| screen.url.as_str());
        self.inner.origin.resolve(href, from)
    }

    /// The first route of the shell that names the address.
    pub(crate) fn route(&self, url: &str) -> Option<&Route> {
        self.inner
            .routes
            .iter()
            .find(|route| route_matches(&route.matches, url))
    }

    fn headers(&self, body: Option<&Body>, region: Option<&str>) -> Vec<(String, String)> {
        let client = self.config.client.to_header();
        let (form, json) = (
            matches!(body, Some(Body::Form { .. })),
            matches!(body, Some(Body::Json { .. })),
        );
        let mut headers = vec![
            (NATIVE_HEADER.to_owned(), MAJOR.to_string()),
            (CLIENT_HEADER.to_owned(), client),
            // The framework's own routes speak JSON; every page speaks the protocol.
            (
                "accept".to_owned(),
                if json {
                    JSON.to_owned()
                } else {
                    CONTENT_TYPE.to_owned()
                },
            ),
        ];
        if json {
            headers.push(("content-type".to_owned(), JSON.to_owned()));
        }
        if let Some(region) = region {
            // The part of the screen this asks for, so the server answers that
            // part. The id is the server's own, so it is written header-safe.
            headers.push((REGION_HEADER.to_owned(), header_safe(region)));
        }
        if form {
            headers.push((
                "content-type".to_owned(),
                "application/x-www-form-urlencoded".to_owned(),
            ));
        }
        headers
    }

    /// Asks for a part of a screen by its id: a region, or a list's next rows.
    pub(crate) fn fetch_part(&mut self, purpose: Pending, url: &str, part: &str, limit: u32) {
        self.ask(Asked {
            purpose,
            url: url.to_owned(),
            first: url.to_owned(),
            method: Method::Get,
            body: None,
            limit,
            delete: false,
            hops: 0,
            region: Some(part.to_owned()),
        });
    }

    /// Asks for `url`, an address inside. A post always says it is a form,
    /// even one with nothing in it: without that the server refuses it.
    pub(crate) fn fetch(
        &mut self,
        purpose: Pending,
        method: Method,
        url: &str,
        body: Option<Body>,
        limit: u32,
    ) {
        let body = match body {
            None if method == Method::Post => Some(Body::Form {
                text: String::new(),
            }),
            body => body,
        };
        self.ask(Asked {
            purpose,
            url: url.to_owned(),
            first: url.to_owned(),
            method,
            body,
            limit,
            delete: false,
            hops: 0,
            region: None,
        });
    }

    /// Posts JSON to one of the framework's own routes, by its path on the
    /// app's origin. False when the path is not one.
    pub(crate) fn post_json(&mut self, purpose: Pending, path: &str, text: String) -> bool {
        let Target::Inside(url) = self.inner.origin.resolve(path, "/") else {
            return false;
        };
        self.ask(Asked {
            purpose,
            first: url.clone(),
            url,
            method: Method::Post,
            body: Some(Body::Json { text }),
            limit: SEND_LIMIT,
            delete: false,
            hops: 0,
            region: None,
        });
        true
    }

    pub(crate) fn ask(&mut self, asked: Asked) {
        self.inner.next.request += 1;
        let request = self.inner.next.request;
        match self.inner.origin.absolute(&asked.url) {
            Some(url) => {
                let headers = self.headers(asked.body.as_ref(), asked.region.as_deref());
                self.effects.push(Effect::Fetch(Request {
                    request,
                    method: if asked.delete {
                        Verb::Delete
                    } else {
                        asked.method.into()
                    },
                    url,
                    headers,
                    body: asked.body.clone(),
                    timeout_ms: asked.limit,
                    xsrf: asked.method == Method::Post,
                }));
            }
            // An address that is not on one of the app's origins is never
            // asked: the request fails as one whose redirect led nowhere.
            None => self.inner.unsent.push(request),
        }
        self.inner.pending.insert(request, asked);
    }

    /// Fails the requests that were never sent, each as an answer of 502.
    /// What their failure asks for in turn is failed the same way, a few
    /// rounds at most; after them, what is left is failed once more and what
    /// that asks for is failed with the next event.
    fn fail_unsent(&mut self) {
        self.fail_unsent_within(MOST_REDIRECTS);
    }

    fn fail_unsent_within(&mut self, rounds: u8) {
        for _ in 0..rounds {
            let unsent = std::mem::take(&mut self.inner.unsent);
            if unsent.is_empty() {
                return;
            }
            for request in unsent {
                self.answered(Response {
                    request,
                    status: 502,
                    ..Response::default()
                });
            }
        }
        // What is left is failed all the same. What failing it asks for in
        // turn waits for the next event's update, so no asker is left waiting.
        for request in std::mem::take(&mut self.inner.unsent) {
            self.answered(Response {
                request,
                status: 502,
                ..Response::default()
            });
        }
    }

    /// Says, once for the event, that an address an answer gave does not
    /// read and was not followed: of what kind, and its text up to its query.
    pub(crate) fn said_refused(&mut self, kind: &str, address: &str) {
        if std::mem::replace(&mut self.inner.refusal_said, true) {
            return;
        }
        let text = address.split(['?', '#']).next().unwrap_or("");
        self.report("address", format!("{kind}: {text}"));
    }

    /// Asks the client to open an address outside the app: the one way an
    /// `open_url` effect is made. Only a whole `http` or `https` address that
    /// reads, written again from its parts, or an address of a scheme the
    /// system handles, is ever opened; anything else is dropped.
    pub(crate) fn open_outside(&mut self, url: &str, inside: bool) {
        if let Some(url) = openable(url) {
            self.effects.push(Effect::OpenUrl { url, inside });
        }
    }

    /// Opens an address of the app's own outside it, as a whole address.
    pub(crate) fn open_ours_outside(&mut self, url: &str) {
        if let Some(url) = self.inner.origin.absolute(url) {
            self.open_outside(&url, false);
        }
    }

    /// Follows a redirect the transport handed over: asks again where it
    /// leads, for the same purpose, when that is inside. A 307 or a 308 asks
    /// the same way with the same body; any other becomes a plain `get`.
    /// Gives back the response to read when there is nothing to follow.
    fn follow(&mut self, asked: &Asked, mut response: Response) -> Option<Response> {
        let Some(location) = response
            .location
            .take()
            .filter(|_| matches!(response.status, 301 | 302 | 303 | 307 | 308))
        else {
            return Some(response);
        };
        match self.inner.origin.resolve(&location, &asked.url) {
            Target::Inside(url) if asked.hops < MOST_REDIRECTS => {
                let same = matches!(response.status, 307 | 308);
                self.ask(Asked {
                    purpose: asked.purpose.clone(),
                    url,
                    first: asked.first.clone(),
                    method: if same { asked.method } else { Method::Get },
                    body: asked.body.clone().filter(|_| same),
                    limit: asked.limit,
                    delete: asked.delete && same,
                    hops: asked.hops + 1,
                    region: asked.region.clone(),
                });
                None
            }
            // Another site is never asked: where the redirect leads opens outside.
            Target::Outside(url) => Some(Response {
                request: response.request,
                left: Some(url),
                ..Response::default()
            }),
            // A redirect that leads nowhere a client goes, or one too many.
            target => {
                if target == Target::Refused {
                    self.said_refused("redirect", &location);
                }
                Some(Response {
                    request: response.request,
                    status: 502,
                    ..Response::default()
                })
            }
        }
    }

    /// Forgets the requests on their way that `is` accepts: their answers are
    /// no longer wanted.
    pub(crate) fn drop_pending(&mut self, is: impl Fn(&Pending) -> bool) {
        let gone: Vec<u64> = self
            .inner
            .pending
            .iter()
            .filter(|(_, asked)| is(&asked.purpose))
            .map(|(request, _)| *request)
            .collect();
        for request in gone {
            self.inner.pending.remove(&request);
            self.effects.push(Effect::Cancel { request });
        }
    }

    pub(crate) fn timer(&mut self, ms: u32, what: TimerFor) -> u64 {
        self.inner.next.timer += 1;
        let timer = self.inner.next.timer;
        self.inner.timers.insert(timer, what);
        self.effects.push(Effect::Timer { timer, ms });
        timer
    }

    pub(crate) fn cancel_timer(&mut self, timer: u64) {
        if self.inner.timers.remove(&timer).is_some() {
            self.effects.push(Effect::CancelTimer { timer });
        }
    }

    /// Shows a message. A later one of the same name takes an earlier one's place.
    pub(crate) fn say(&mut self, mut message: Message, about: MessageInner) {
        if let Some(name) = &about.name {
            let same: Vec<u64> = self
                .inner
                .messages
                .iter()
                .filter(|(_, other)| other.name.as_ref() == Some(name))
                .map(|(id, _)| *id)
                .collect();
            for id in same {
                self.inner.messages.remove(&id);
                self.view.messages.retain(|message| message.id != id);
            }
        }
        self.inner.next.message += 1;
        message.id = self.inner.next.message;
        self.inner.messages.insert(message.id, about);
        self.view.messages.push(message.clone());
        self.effects.push(Effect::Message(message));
        self.touch(Change::Messages);
    }

    /// Shows a message of the core's own.
    pub(crate) fn notice(&mut self, notice: Notice, screen: Option<ScreenId>) {
        let tone = match notice {
            Notice::Unsaved | Notice::SecondStep => Tone::Warning,
            Notice::Loading | Notice::Copied | Notice::Loaded => Tone::Neutral,
            _ => Tone::Danger,
        };
        let message = Message {
            id: 0,
            tone,
            text: None,
            title: None,
            notice: Some(notice),
            action: None,
        };
        self.say(
            message,
            MessageInner {
                screen,
                ..MessageInner::default()
            },
        );
    }

    pub(crate) fn failed(&mut self, failure: Failure, screen: Option<ScreenId>) {
        let notice = match failure {
            Failure::Offline => Notice::Offline,
            Failure::Timeout => Notice::Timeout,
            _ => Notice::Failed,
        };
        self.notice(notice, screen);
    }

    pub(crate) fn report(&mut self, what: &str, detail: String) {
        self.effects.push(Effect::Report {
            what: what.to_owned(),
            detail,
        });
    }

    fn message_gone(&mut self, id: u64) {
        self.inner.messages.remove(&id);
        let before = self.view.messages.len();
        self.view.messages.retain(|message| message.id != id);
        if self.view.messages.len() != before {
            self.touch(Change::Messages);
        }
    }

    fn message_action(&mut self, id: u64) {
        let Some(about) = self.inner.messages.get(&id).cloned() else {
            return;
        };
        self.message_gone(id);
        let Some(action) = about.action else {
            return;
        };
        let from = about
            .screen
            .filter(|screen| self.view.holds(*screen))
            .or(self.front());
        if !action.post {
            self.navigate(from, &action.href, Present::Auto);
        } else if let Some(screen) = from {
            self.post(screen, &action.href, action.fields, None);
        }
    }

    // Starting, and the shell.

    pub(crate) fn start(&mut self) {
        let asking = self
            .inner
            .pending
            .values()
            .any(|asked| asked.purpose == Pending::Shell);
        if asking {
            return;
        }
        if self.view.stage == Stage::Failed {
            self.view.stage = Stage::Starting;
            self.view.failure = None;
            self.touch(Change::Nav);
        }
        if self.config.cache.persist && !self.inner.has_shell {
            self.effects.push(Effect::CacheRead {
                key: "boot".to_owned(),
            });
        }
        self.fetch(Pending::Shell, Method::Get, SHELL_PATH, None, PAGE_LIMIT);
    }

    pub(crate) fn fetch_shell(&mut self) {
        let asking = self
            .inner
            .pending
            .values()
            .any(|asked| asked.purpose == Pending::Shell);
        if !asking {
            self.fetch(Pending::Shell, Method::Get, SHELL_PATH, None, PAGE_LIMIT);
        }
    }

    fn shell_failed(&mut self, failure: Failure) {
        // A shell that is already drawn stays; only a first load can fail the app.
        if !self.inner.has_shell {
            self.view.stage = Stage::Failed;
            self.view.failure = Some(failure);
            self.touch(Change::Nav);
        }
    }

    fn shell_answered(&mut self, response: Response) {
        if let Some(error) = response.error {
            return self.shell_failed(match error {
                TransportError::Timeout => Failure::Timeout,
                _ => Failure::Offline,
            });
        }
        if response.left.is_some() {
            return self.shell_failed(Failure::Protocol);
        }
        if response.status >= 500 {
            return self.shell_failed(Failure::Server);
        }
        let media = response
            .content_type
            .as_deref()
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !(200..300).contains(&response.status) || media != CONTENT_TYPE {
            // A server with no shell still has pages: one destination, its first page.
            if !self.inner.has_shell {
                self.install(Shell::new("", ""));
            }
            return;
        }
        let header = decode::shell_stamp_of(&response);
        // Read where there is room for it, like any other answer.
        let body = response.body.as_str();
        let value: Value = decode::with_room(|| serde_json::from_str(body).ok())
            .flatten()
            .unwrap_or(Value::Null);
        if let Some(upgrade) = value
            .get("upgrade")
            .cloned()
            .and_then(|upgrade| serde_json::from_value(upgrade).ok())
        {
            self.view.upgrade = Some(newer_app(upgrade));
            self.view.stage = Stage::Upgrade;
            return self.touch(Change::Nav);
        }
        match decode::with_room(move || serde_json::from_value::<Shell>(value))
            .unwrap_or_else(|| Err(serde::de::Error::custom("the shell could not be read")))
        {
            Ok(shell) if shell.navree == MAJOR => {
                // The stamp of the shell now held: its own, or the one its answer named.
                let stamp = (!shell.stamp.is_empty())
                    .then(|| shell.stamp.clone())
                    .or(header);
                self.install(shell);
                // The person is the server's word now: what is kept is read
                // only when it is theirs, and emptied when it is not.
                let person = self.inner.user.clone();
                self.shell_named(person);
                self.remember_shell(response.body);
                self.inner.shell_stamp = stamp;
                self.inner.seen_stamp = None;
            }
            Ok(_) => self.shell_failed(Failure::Protocol),
            Err(error) => {
                self.report("decode", format!("shell: {error}"));
                if !self.inner.has_shell {
                    self.install(Shell::new("", ""));
                }
            }
        }
    }

    /// Draws the app from the shell kept at the last launch. False when it
    /// does not read.
    pub(crate) fn shell_from_cache(&mut self, body: &str) -> bool {
        if let Some(Ok(shell)) = decode::with_room(|| serde_json::from_str::<Shell>(body))
            && shell.navree == MAJOR
        {
            self.inner.cache.shell = Some(body.to_owned());
            self.install(shell);
            return true;
        }
        false
    }

    /// The address a destination opens at.
    pub(crate) fn destination_url(&self, href: &str) -> String {
        match self.inner.origin.resolve(href, "/") {
            Target::Inside(url) => url,
            _ => "/".to_owned(),
        }
    }

    fn install(&mut self, shell: Shell) {
        let user = shell
            .user
            .as_ref()
            .map(|user| (user.name.clone(), user.email.clone()));
        self.inner.sign_in = shell.sign_in;
        self.inner.push_route = shell
            .device
            .and_then(|device| device.push)
            .map(|push| push.register.as_str().to_owned());
        if self.inner.user != user {
            // What the server held of the device's token was the session's before.
            self.push_forget();
        }
        if self.inner.has_shell && self.inner.user != user {
            match (&user, &self.inner.person) {
                // Another person: nothing of the one before stays, not even where they were.
                (Some(new), Some(last)) if new != last => {
                    self.wipe();
                    self.forget_all();
                }
                // The same person again, or the first to sign in.
                (Some(_), _) => {}
                // Nobody is signed in any more: what the screens showed is
                // thrown away. Their addresses stay, so whoever signs in as
                // the same person is back where they were.
                (None, _) => {
                    self.blank_all();
                    self.forget_all();
                }
            }
        }
        if user.is_some() {
            self.inner.person.clone_from(&user);
        }
        self.inner.user = user;
        self.inner.has_shell = true;
        if !shell.build.is_empty() && self.inner.build.as_deref() != Some(&shell.build) {
            let first = self.inner.build.is_none();
            if !first {
                self.forget_pages();
            }
            self.inner.build = Some(shell.build.clone());
            if first {
                let _ = self.build_known();
            }
        }
        self.inner.routes = shell.routes;
        let mut items: Vec<(Option<String>, NavItem)> = Vec::new();
        for group in shell.nav.groups {
            for item in group.items {
                match self.inner.origin.resolve(&item.href, "/") {
                    Target::Inside(_) => items.push((group.label.clone(), item)),
                    Target::Refused => self.said_refused("shell", &item.href),
                    Target::Outside(_) => {}
                }
            }
        }
        if items.is_empty() {
            let home = NavItem {
                label: shell.name.clone(),
                href: shell.home.clone(),
                ..NavItem::default()
            };
            items.push((None, home));
        }
        let was = self
            .view
            .destinations
            .get(self.view.selected)
            .map(|destination| destination.href.clone());
        let mut old: Vec<Destination> = std::mem::take(&mut self.view.destinations);
        for (group, item) in items {
            let kept = old
                .iter()
                .position(|destination| destination.href == item.href);
            let stack = kept.map(|at| old.remove(at).stack).unwrap_or_default();
            self.view.destinations.push(Destination {
                label: item.label,
                icon: item.icon,
                href: item.href,
                badge: item.badge,
                group,
                items: item.items,
                stack,
            });
        }
        // The screens of a destination that is no longer there go with it.
        for id in old.into_iter().flat_map(|destination| destination.stack) {
            self.close(id);
        }
        self.view.selected = self
            .view
            .destinations
            .iter()
            .position(|destination| Some(&destination.href) == was.as_ref())
            .unwrap_or(0);
        self.view.app = Some(App {
            name: shell.name,
            context: shell.context,
            home: shell.home,
            theme: shell.theme,
            user: shell.user,
        });
        if self.view.stage != Stage::Upgrade {
            self.view.stage = Stage::Ready;
            self.view.failure = None;
        }
        self.touch(Change::App);
        self.touch(Change::Nav);
        // Where the person was when the system ended the app, if a snapshot says.
        self.restore_stacks();
        if let Some(launch) = self.inner.launch.take() {
            self.opened(&launch);
        }
        // Where a reply said to land, whatever the rules for links from outside say.
        if let Some(land) = self.inner.land.take() {
            self.go(None, &land, Present::Auto);
        }
        self.push_sync();
    }

    /// Empties every screen but the sign-in a session's end put up: what they
    /// showed and what was typed in them belonged to someone who is gone.
    fn blank_all(&mut self) {
        let covers: Vec<ScreenId> = self
            .view
            .modals
            .iter()
            .filter(|modal| modal.locked)
            .flat_map(|modal| modal.stack.iter().copied())
            .collect();
        let ids: Vec<ScreenId> = self
            .view
            .screens
            .keys()
            .copied()
            .filter(|id| !covers.contains(id))
            .collect();
        for id in ids {
            let (Some(view), Some(inner)) = (
                self.view.screens.get_mut(&id),
                self.inner.screens.get_mut(&id),
            ) else {
                continue;
            };
            let fresh = ScreenView::new(id, view.url.clone(), view.chrome);
            *view = fresh;
            let visit = inner.visit + 1;
            *inner = ScreenInner {
                visit,
                stale: true,
                ..ScreenInner::default()
            };
            self.drop_pending(|purpose| purpose.screen() == Some(id));
            self.touch_screen(id);
        }
        self.view.messages.clear();
        self.inner.messages.clear();
        self.touch(Change::Messages);
    }

    /// Takes every screen, message and request away. The shell stays until a
    /// new one says who is here now.
    pub(crate) fn wipe(&mut self) {
        let ids: Vec<ScreenId> = self.view.screens.keys().copied().collect();
        for id in ids {
            self.close(id);
        }
        for destination in &mut self.view.destinations {
            destination.stack.clear();
        }
        self.view.modals.clear();
        self.view.selected = 0;
        self.view.messages.clear();
        self.inner.messages.clear();
        self.inner.calls.clear();
        self.inner.launch = None;
        self.drop_pending(|purpose| *purpose != Pending::Shell);
        self.touch(Change::Nav);
        self.touch(Change::Messages);
    }

    /// Takes note of the app's stamp an answer carries: its `app`, or its
    /// `build` from a server that writes no other. A new one empties what is
    /// kept. Nothing on screen is thrown away: what was typed stays.
    fn note_app(&mut self, stamp: &str) {
        if stamp.is_empty() || self.inner.build.as_deref() == Some(stamp) {
            return;
        }
        let first = self.inner.build.is_none();
        self.inner.build = Some(stamp.to_owned());
        if first {
            let _ = self.build_known();
        } else {
            self.forget_pages();
            // A server that stamps no shell: a new app may bring a new one.
            if self.inner.shell_stamp.is_none() && self.inner.seen_stamp.is_none() {
                self.fetch_shell();
            }
        }
    }

    /// Takes note of the shell's stamp an answer names. One that differs from
    /// the stamp of the shell held says the person, the destinations or a
    /// count changed: the shell is fetched again, once for each new stamp.
    fn note_shell(&mut self, stamp: Option<String>) {
        let Some(stamp) = stamp else {
            return;
        };
        let held = self
            .inner
            .shell_stamp
            .as_ref()
            .or(self.inner.seen_stamp.as_ref());
        if held.is_some_and(|held| *held != stamp)
            && self.inner.asked_stamp.as_ref() != Some(&stamp)
        {
            self.inner.asked_stamp = Some(stamp.clone());
            self.fetch_shell();
        }
        self.inner.seen_stamp = Some(stamp);
    }

    /// What every envelope is read for, whatever it answers. `shell` is the
    /// stamp of the shell the answer stands in. True when the answer asks for
    /// a newer app, and holds nothing else.
    pub(crate) fn note(&mut self, read: &mut Read, shell: Option<String>) -> bool {
        if !read.missing.is_empty() {
            let what = match read.missing.first().map(String::as_str) {
                Some("client:decode") => "decode",
                Some("client:depth") => "depth",
                _ => "kind",
            };
            self.report(what, read.missing.join(" "));
        }
        if let Some(upgrade) = read.envelope.upgrade.take() {
            self.view.upgrade = Some(newer_app(upgrade));
            self.view.stage = Stage::Upgrade;
            self.touch(Change::Nav);
            return true;
        }
        let app = if read.envelope.app.is_empty() {
            &read.envelope.build
        } else {
            &read.envelope.app
        };
        let app = app.clone();
        self.note_app(&app);
        self.note_shell(shell);
        false
    }

    /// The way in that an answer names when the session has ended: the server
    /// answers 401 with a `navigate` op.
    pub(crate) fn way_in(status: u16, ops: &[Op]) -> Option<String> {
        if status != 401 {
            return None;
        }
        ops.iter().find_map(|op| match op {
            Op::Navigate { href, .. } => Some(href.clone()),
            _ => None,
        })
    }

    /// The session ended. The way in is presented as the shell's routes say,
    /// and over everything when they say nothing; the person can't take it
    /// away; and the screen that asked stays in its stack, to be fetched again
    /// once they are back.
    pub(crate) fn session_ended(&mut self, under: ScreenId, href: &str) {
        let url = match self.resolve(href, under) {
            Target::Inside(url) => url,
            Target::Outside(url) => return self.open_outside(&url, false),
            Target::Refused => return self.said_refused("navigate", href),
        };
        if let (Some(view), Some(inner)) = (
            self.view.screens.get_mut(&under),
            self.inner.screens.get_mut(&under),
        ) {
            view.loading = false;
            view.refreshing = false;
            inner.stale = true;
            self.touch_screen(under);
        }
        let route = self.route(&url);
        let present = match route.and_then(|route| route.present) {
            Some(present @ (Present::Sheet | Present::Full | Present::Task)) => present,
            _ => Present::Full,
        };
        let chrome = route.is_some_and(|route| route.chrome);
        let name = route.map_or_else(|| path_of(&url), |route| route.matches.clone());
        // One way in for one session's end, however many requests it failed.
        let up = self.view.modals.iter().any(|modal| {
            modal
                .stack
                .first()
                .and_then(|first| self.inner.screens.get(first))
                .is_some_and(|inner| inner.cover.as_ref() == Some(&name))
        });
        if up {
            return;
        }
        let id = self.open(&url);
        if let (Some(view), Some(inner)) = (
            self.view.screens.get_mut(&id),
            self.inner.screens.get_mut(&id),
        ) {
            view.chrome = chrome;
            inner.cover = Some(name);
        }
        self.view.modals.push(Modal {
            present,
            locked: true,
            stack: vec![id],
        });
        self.touch(Change::Nav);
    }

    // Device calls.

    fn built_with(&self, feature: Option<Feature>) -> bool {
        match feature {
            Some(Feature::Haptic) => true,
            Some(feature) => self
                .config
                .client
                .caps
                .iter()
                .any(|cap| cap == feature.as_str()),
            None => false,
        }
    }

    fn haptic(&mut self, kind: HapticKind) {
        let kind = serde_json::to_value(kind)
            .ok()
            .and_then(|kind| kind.as_str().map(str::to_owned));
        self.effects.push(Effect::Haptic {
            kind: kind.unwrap_or_else(|| "success".to_owned()),
        });
    }

    fn run(&mut self, device: serde_json::Result<Value>, posts: bool) -> u64 {
        self.inner.next.call += 1;
        let call = self.inner.next.call;
        self.run_as(call, device, posts, None);
        call
    }

    /// Tells the client to run the call numbered `call`, with what the server
    /// gave for it to start with.
    fn run_as(
        &mut self,
        call: u64,
        device: serde_json::Result<Value>,
        posts: bool,
        options: Option<Value>,
    ) {
        if let Ok(device) = device {
            self.effects.push(Effect::Device {
                call,
                device,
                posts,
                options,
            });
        }
    }

    /// A device call a server's reply started: one of the few that need no
    /// tap. The protocol's types let no other be written into a reply.
    pub(crate) fn reply_call(&mut self, call: ReplyCall) {
        match call {
            ReplyCall::Haptic(haptic) => self.haptic(haptic.kind),
            ReplyCall::Unknown => self.report(
                "device",
                "a reply started a call this build does not know".to_owned(),
            ),
            call if self.built_with(call.feature()) => {
                self.run(serde_json::to_value(&call), false);
            }
            // A feature the app was not built with is passed over.
            _ => {}
        }
    }

    /// A device call the person's tap started. `now` is the time, in seconds
    /// since 1970: a call whose ticket has run out is not started, and its
    /// screen is fetched again for a fresh one.
    pub(crate) fn device_call(&mut self, id: ScreenId, device: DeviceCall, now: Option<i64>) {
        if let (Some(then), Some(now)) = (&device.then, now)
            && then.exp <= now
        {
            self.notice(Notice::Loading, Some(id));
            return self.fetch_page(id, false);
        }
        if !decode::can_start(&self.config, &device.call) {
            // Such a control is not drawn: see `decode`.
            return self.report(
                "device",
                "a call this build was not built with was tapped".to_owned(),
            );
        }
        match device.call {
            Call::Haptic(haptic) => self.haptic(haptic.kind),
            Call::Copy(copy) => {
                self.effects.push(Effect::Copy {
                    text: copy.text,
                    sensitive: copy.sensitive,
                });
                self.effects.push(Effect::Announce {
                    text: None,
                    notice: Some(Notice::Copied),
                    count: None,
                });
            }
            Call::OpenUrl(open) => self.open_outside(open.url.as_str(), open.inside),
            Call::AuthSession(session) => {
                self.sign_in_session(session.start.as_str(), session.ephemeral);
            }
            Call::Confirm(confirm) => {
                // The ticket is spent at the challenge route, which answers
                // what the person signs: the core asks, the client only prompts.
                let Some(then) = device.then else {
                    return self.report(
                        "device",
                        "a confirmation with nowhere to post its result".to_owned(),
                    );
                };
                if !self.may_send(id) {
                    return;
                }
                self.inner.next.call += 1;
                let number = self.inner.next.call;
                let text = serde_json::json!({ "ticket": then.ticket.as_str() }).to_string();
                let challenge = confirm.challenge.as_str().to_owned();
                self.inner.calls.insert(
                    number,
                    crate::state::DeviceCall {
                        screen: id,
                        call: Call::Confirm(confirm),
                        goes: Goes::Then { then, spent: true },
                    },
                );
                if !self.post_json(Pending::BeforeCall { call: number }, &challenge, text) {
                    self.inner.calls.remove(&number);
                }
            }
            Call::PlatformSignIn(sign_in) => {
                let provider = serde_json::to_value(sign_in.provider)
                    .ok()
                    .and_then(|provider| provider.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let web = sign_in.web.as_ref().map(|web| web.as_str().to_owned());
                let sheet = self.built_with(Some(Feature::PlatformSignIn))
                    && self.config.sign_in.native.contains(&provider);
                if !sheet {
                    // This system has no sheet for the provider: the web's way.
                    return match web {
                        Some(web) => self.sign_in_on_the_web(&web),
                        None => self.report(
                            "device",
                            format!("no way to sign in with `{provider}` on this system"),
                        ),
                    };
                }
                if !self.may_send(id) {
                    return;
                }
                self.inner.next.call += 1;
                let number = self.inner.next.call;
                let (nonce, route) = (
                    sign_in.nonce.as_str().to_owned(),
                    sign_in.sign_in.as_str().to_owned(),
                );
                let second_step = sign_in
                    .second_step
                    .as_ref()
                    .map(|page| page.as_str().to_owned());
                self.inner.calls.insert(
                    number,
                    crate::state::DeviceCall {
                        screen: id,
                        call: Call::PlatformSignIn(sign_in),
                        goes: Goes::SignIn {
                            sign_in: route,
                            state: String::new(),
                            web,
                            second_step,
                        },
                    },
                );
                if !self.post_json(Pending::BeforeCall { call: number }, &nonce, String::new()) {
                    self.inner.calls.remove(&number);
                }
            }
            call => {
                // Nothing is posted from a kept tree: its ticket may be an old one.
                let then = device.then.filter(|_| call.posts());
                if then.is_some() && !self.may_send(id) {
                    return;
                }
                let number = self.run(serde_json::to_value(&call), then.is_some());
                if let Some(then) = then {
                    self.inner.calls.insert(
                        number,
                        crate::state::DeviceCall {
                            screen: id,
                            call,
                            goes: Goes::Then { then, spent: false },
                        },
                    );
                }
            }
        }
    }

    /// A device call finished: its result is posted where the call said, with
    /// the server's ticket and how it went.
    fn device_result(
        &mut self,
        call: u64,
        ok: bool,
        refusal: Option<String>,
        mut fields: Vec<navree_native::Pair>,
    ) {
        let Some(known) = self.inner.calls.remove(&call) else {
            return;
        };
        let Some(screen) = Some(known.screen)
            .filter(|screen| self.view.holds(*screen))
            .or(self.front())
        else {
            return;
        };
        let pair = |name: &str, value: &str| navree_native::Pair {
            name: name.to_owned(),
            value: value.to_owned(),
        };
        let (then, spent) = match known.goes {
            Goes::Then { then, spent } => (then, spent),
            Goes::SignIn {
                sign_in,
                state,
                web,
                second_step,
            } => {
                return self.signed_with_provider(
                    screen,
                    ok,
                    refusal.as_deref(),
                    &fields,
                    (&sign_in, &state),
                    web.as_deref(),
                    second_step,
                );
            }
        };
        if !ok {
            // A refusal has no result to post.
            fields.clear();
        }
        let outcome = if ok {
            OUTCOME_OK.to_owned()
        } else {
            refusal.unwrap_or_else(|| "failed".to_owned())
        };
        if !spent {
            fields.push(pair(TICKET_FIELD, then.ticket.as_str()));
        }
        fields.push(pair(OUTCOME_FIELD, &outcome));
        self.post(screen, then.href.as_str(), fields, None);
    }

    /// Starts the web's sign-in with a provider, in the system's sign-in session.
    fn sign_in_on_the_web(&mut self, web: &str) {
        if !self.built_with(Some(Feature::AuthSession)) {
            return self.report(
                "device",
                "no sign-in session to sign in on the web with".to_owned(),
            );
        }
        self.sign_in_session(web, false);
    }

    /// What the server gave for a device call to start with came: the
    /// challenge a confirmation signs, or the nonce a sign-in is made with.
    /// The client is told to run the call with it.
    fn before_call_answered(&mut self, call: u64, response: &Response) {
        let Some(known) = self.inner.calls.get_mut(&call) else {
            return;
        };
        let screen = known.screen;
        let good = response.error.is_none()
            && response.left.is_none()
            && (200..300).contains(&response.status);
        let answer: Option<Value> = good
            .then(|| serde_json::from_str(&response.body).ok())
            .flatten();
        let options = match (&mut known.goes, answer) {
            (Goes::Then { .. }, Some(answer)) if answer.get("publicKey").is_some() => Some(answer),
            (Goes::SignIn { state, .. }, Some(answer)) => match (
                answer.get("nonce").and_then(Value::as_str),
                answer.get("state").and_then(Value::as_str),
            ) {
                (Some(nonce), Some(sealed)) => {
                    // The state goes back with the token, from here: the client needs only the nonce.
                    sealed.clone_into(state);
                    Some(serde_json::json!({ "nonce": nonce }))
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(options) = options {
            let device = serde_json::to_value(&known.call);
            return self.run_as(call, device, true, Some(options));
        }
        let confirms = matches!(known.goes, Goes::Then { .. });
        self.inner.calls.remove(&call);
        match response.error {
            Some(TransportError::Timeout) => self.failed(Failure::Timeout, Some(screen)),
            Some(_) => self.failed(Failure::Offline, Some(screen)),
            // The session ended, or the ticket is no longer good: the screen
            // is fetched again, which says the first and mends the second.
            None if response.status == 401 || (confirms && response.status == 422) => {
                self.notice(Notice::Loading, Some(screen));
                self.fetch_page(screen, false);
            }
            None => {
                self.notice(Notice::Failed, Some(screen));
                self.report(
                    "device",
                    format!(
                        "the server answered {} before a device call could start",
                        response.status
                    ),
                );
            }
        }
    }

    /// The provider's sheet is done. Its token goes to the app's own sign-in
    /// route as JSON, with the state the nonce came with; a system with no
    /// sheet for the provider goes the web's way.
    #[allow(clippy::too_many_arguments)]
    fn signed_with_provider(
        &mut self,
        screen: ScreenId,
        ok: bool,
        refusal: Option<&str>,
        fields: &[navree_native::Pair],
        route: (&str, &str),
        web: Option<&str>,
        second_step: Option<String>,
    ) {
        let (sign_in, state) = route;
        let held = |name: &str| {
            fields
                .iter()
                .find(|pair| pair.name == name)
                .map(|pair| pair.value.as_str())
                .filter(|value| !value.is_empty())
        };
        let Some(token) = held("id_token").filter(|_| ok) else {
            // The person closed the sheet or said no: nothing changed.
            if let (Some("unsupported" | "not_built" | "unavailable"), Some(web)) = (refusal, web) {
                self.sign_in_on_the_web(web);
            }
            return;
        };
        let mut body = serde_json::json!({ "id_token": token, "state": state });
        if let Some(code) = held("authorization_code") {
            body["authorization_code"] = Value::from(code);
        }
        let (first, last) = (held("first_name"), held("last_name"));
        if first.is_some() || last.is_some() {
            // As the provider words a name, the first time it tells it.
            let mut name = serde_json::Map::new();
            if let Some(first) = first {
                name.insert("firstName".to_owned(), Value::from(first));
            }
            if let Some(last) = last {
                name.insert("lastName".to_owned(), Value::from(last));
            }
            body["user"] = serde_json::json!({ "name": name });
        }
        self.post_json(
            Pending::SignIn {
                screen,
                second_step,
            },
            sign_in,
            body.to_string(),
        );
    }

    /// The app's sign-in route answered the provider's token.
    fn sign_in_answered(
        &mut self,
        screen: ScreenId,
        second_step: Option<&str>,
        response: &Response,
    ) {
        let screen = Some(screen)
            .filter(|screen| self.view.holds(*screen))
            .or(self.front());
        let good = response.error.is_none()
            && response.left.is_none()
            && (200..300).contains(&response.status);
        let answer: Option<Value> = serde_json::from_str(&response.body).ok();
        // Anything but a plain yes is not a sign-in. The route says when a
        // second step is still to come, and until it is done nobody is signed in.
        let second = answer
            .as_ref()
            .and_then(|answer| answer.get("two_factor_required"))
            .is_some_and(|needed| needed != &Value::Bool(false) && !needed.is_null());
        if good && !second && answer.is_some_and(|answer| answer.is_object()) {
            // Who is signed in changed, and with it everything on screen.
            return self.signed_in();
        }
        match response.error {
            Some(TransportError::Timeout) => self.failed(Failure::Timeout, screen),
            Some(_) => self.failed(Failure::Offline, screen),
            // Nobody is signed in yet. The step is finished on a page of the
            // app, opened here with the app's own cookies.
            None if good && second && self.second_step(second_step) => {}
            None if good && second => {
                // Neither the call nor the shell names a page for it: the
                // person is told, and the page is asked again for what it shows now.
                self.notice(Notice::SecondStep, screen);
                self.report("sign_in", "the sign-in route asks for a second step, and no page is named to finish it on".to_owned());
                if let Some(screen) = screen {
                    self.fetch_page(screen, false);
                }
            }
            None => {
                self.notice(Notice::Failed, screen);
                self.report(
                    "sign_in",
                    format!("the sign-in route answered {}", response.status),
                );
            }
        }
    }

    // Screens.

    pub(crate) fn chrome(&self, url: &str) -> bool {
        self.route(url).is_none_or(|route| route.chrome)
    }

    /// A new screen for `url`, in no stack yet and with nothing asked for.
    pub(crate) fn new_screen(&mut self, url: &str) -> ScreenId {
        self.inner.next.screen += 1;
        let id = self.inner.next.screen;
        let chrome = self.chrome(url);
        self.view
            .screens
            .insert(id, ScreenView::new(id, url.to_owned(), chrome));
        self.inner.screens.insert(id, ScreenInner::default());
        self.touch_screen(id);
        id
    }

    /// A new screen for `url`: the kept tree at once, if there is one, and a
    /// fresh one on its way.
    pub(crate) fn open(&mut self, url: &str) -> ScreenId {
        let id = self.new_screen(url);
        if let Some(view) = self.view.screens.get_mut(&id) {
            view.loading = true;
        }
        self.head_start(id, url);
        self.fetch_page(id, false);
        id
    }

    /// Fetches the screen's page again. An answer still on its way for it is
    /// no longer wanted.
    pub(crate) fn fetch_page(&mut self, id: ScreenId, by_user: bool) {
        let (Some(view), Some(inner)) = (
            self.view.screens.get_mut(&id),
            self.inner.screens.get_mut(&id),
        ) else {
            return;
        };
        inner.visit += 1;
        inner.stale = false;
        let visit = inner.visit;
        view.loading = true;
        view.refreshing = by_user;
        view.failure = None;
        if let Content::Web { serial, .. } = &mut view.content {
            // The web view fetches its own page: it is told to load it again.
            self.inner.next.web += 1;
            *serial = self.inner.next.web;
        }
        let url = view.url.clone();
        self.drop_pending(
            |purpose| matches!(purpose, Pending::Page { screen, .. } if *screen == id),
        );
        self.fetch(
            Pending::Page { screen: id, visit },
            Method::Get,
            &url,
            None,
            PAGE_LIMIT,
        );
        self.touch_screen(id);
    }

    fn refresh(&mut self, id: ScreenId) {
        self.fetch_page(id, true);
    }

    /// Forgets a screen that no stack holds any more.
    pub(crate) fn close(&mut self, id: ScreenId) {
        self.view.screens.remove(&id);
        if let Some(inner) = self.inner.screens.remove(&id) {
            let timers = inner
                .query_timers
                .into_values()
                .chain(inner.live.into_values().filter_map(|sub| sub.timer));
            let timers = timers
                .chain(inner.live_lists.into_values().filter_map(|sub| sub.timer))
                .chain(inner.options.into_values().filter_map(|ask| ask.timer));
            for timer in timers {
                self.cancel_timer(timer);
            }
        }
        // The answer to a write is still wanted: it says whether it worked.
        self.drop_pending(|purpose| purpose.screen() == Some(id) && !purpose.writes());
        self.touch(Change::Nav);
    }

    /// What is open in a tree, by id: the sheets, dialogs and disclosures.
    fn open_in(screen: &Screen) -> Vec<(String, bool)> {
        let mut open = Vec::new();
        for root in roots(screen) {
            walk(root, &mut |node| {
                let state = match node {
                    navree_native::Node::Sheet(sheet) => sheet.open,
                    navree_native::Node::Dialog(dialog) => dialog.open,
                    navree_native::Node::Disclosure(disclosure) => disclosure.open,
                    _ => return,
                };
                if let Some(id) = node.id() {
                    open.push((id.to_owned(), state));
                }
            });
        }
        open
    }

    /// Puts a tree on a screen. The same page again keeps what is open and
    /// what was typed, and keeps showing what its regions held until they
    /// have fetched themselves again; another page starts afresh.
    pub(crate) fn show_native(
        &mut self,
        id: ScreenId,
        url: &str,
        mut screen: Screen,
        status: u16,
        kept: bool,
    ) {
        let (Some(view), Some(inner)) = (
            self.view.screens.get_mut(&id),
            self.inner.screens.get_mut(&id),
        ) else {
            return;
        };
        let same = path_of(&view.url) == path_of(url) && view.screen().is_some();
        // A question the screen asked was about what it showed before.
        view.question = None;
        inner.held = None;
        inner.form_ids.clear();
        inner.field_ids.clear();
        inner.server_ids.clear();
        let mut names = Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
        };
        names.name(&mut screen.actions, None);
        if let Some(scores) = screen
            .pager
            .as_mut()
            .and_then(|pager| pager.scores.as_deref_mut())
        {
            names.name(std::slice::from_mut(scores), None);
        }
        names.name(std::slice::from_mut(&mut screen.body), None);
        let mut again: Vec<String> = Vec::new();
        if let (true, Some(old)) = (same, view.screen_mut()) {
            for (target, open) in Self::open_in(old) {
                let is = |node: &navree_native::Node| has_id(node, &target) && opens(node);
                if let Some(node) = crate::tree::find_in_mut(&mut screen, &is) {
                    crate::ops::set_open_flag(node, open);
                }
            }
            // A region that comes empty keeps what it held until its own fetch lands.
            let wanting: Vec<String> = Survey::of(roots(&screen))
                .regions
                .iter()
                .filter(|region| region.wants)
                .map(|region| region.id.to_owned())
                .collect();
            for region in wanting {
                let is = |node: &navree_native::Node| matches!(node, navree_native::Node::Region(inner) if inner.id == region);
                let held = match crate::tree::find_in_mut(old, &is) {
                    Some(navree_native::Node::Region(old)) if !old.children.is_empty() => {
                        std::mem::take(&mut old.children)
                    }
                    _ => continue,
                };
                if let Some(navree_native::Node::Region(new)) =
                    crate::tree::find_in_mut(&mut screen, &is)
                {
                    new.children = held;
                    again.push(region);
                }
            }
        } else {
            inner.dirty.clear();
            view.forms.clear();
            view.overlays.clear();
        }
        view.states.clear();
        view.regions.clear();
        view.lists.clear();
        inner.more.clear();
        inner.turns.clear();
        if view.url != url {
            view.url = url.to_owned();
            view.chrome = self
                .inner
                .routes
                .iter()
                .find(|route| route_matches(&route.matches, url))
                .is_none_or(|route| route.chrome);
        }
        view.title = Some(screen.title.clone()).filter(|title| !title.is_empty());
        view.content = Content::Native {
            screen: Box::new(screen),
        };
        view.status = status;
        view.cached = kept;
        if !kept {
            view.loading = false;
            view.refreshing = false;
            view.failure = None;
        }
        self.drop_pending(|purpose| matches!(purpose, Pending::Region { screen, .. } | Pending::More { screen, .. } if *screen == id));
        self.reconcile(id, None);
        if !kept {
            for region in again {
                self.fetch_region(id, &region, None);
            }
            // What the person had typed before the system ended the app.
            self.restore_typed(id);
        }
        self.touch_screen(id);
    }

    /// Puts the page in the web view on a screen.
    pub(crate) fn show_web(
        &mut self,
        id: ScreenId,
        url: &str,
        page: &str,
        title: Option<String>,
        why: Vec<String>,
    ) {
        let whole = self.inner.origin.absolute(page);
        let Some(view) = self.view.screens.get_mut(&id) else {
            return;
        };
        let Some(whole) = whole else {
            // A page that is not on one of the app's origins is never shown:
            // the screen failed, as one whose answer could not be read.
            view.loading = false;
            view.refreshing = false;
            view.failure = Some(Failure::Protocol);
            return self.touch_screen(id);
        };
        self.inner.next.web += 1;
        view.url = url.to_owned();
        if title.is_some() {
            view.title.clone_from(&title);
        }
        view.content = Content::Web {
            url: whole,
            title,
            why,
            serial: self.inner.next.web,
        };
        view.loading = false;
        view.refreshing = false;
        view.cached = false;
        view.failure = None;
        view.forms.clear();
        view.regions.clear();
        view.lists.clear();
        view.overlays.clear();
        view.states.clear();
        view.chrome = self
            .inner
            .routes
            .iter()
            .find(|route| route_matches(&route.matches, url))
            .is_none_or(|route| route.chrome);
        self.drop_pending(|purpose| matches!(purpose, Pending::Region { screen, .. } | Pending::More { screen, .. } if *screen == id));
        self.touch_screen(id);
    }

    pub(crate) fn show(&mut self, id: ScreenId, url: &str, shown: Shown) {
        match shown {
            Shown::Native {
                screen,
                status,
                body,
                keep,
            } => {
                self.remember(url, status, &body, keep);
                self.show_native(id, url, *screen, status, false);
            }
            Shown::Web {
                url: page,
                title,
                why,
            } => self.show_web(id, url, &page, title, why),
        }
    }

    /// What an envelope gives a screen to show, if it gives anything.
    pub(crate) fn shown(
        &self,
        url: &str,
        status: u16,
        read: &mut Read,
        body: String,
    ) -> Option<Shown> {
        if let Some(web) = read.envelope.web.take() {
            let page = match self.inner.origin.resolve(&web.url, url) {
                Target::Inside(page) => page,
                _ => url.to_owned(),
            };
            return Some(Shown::Web {
                url: page,
                title: web.title,
                why: web.why,
            });
        }
        // A page is never drawn with a hole in it: what can't be drawn sends it to the web view.
        if !read.missing.is_empty()
            && (read.envelope.screen.is_some() || read.envelope.ops.is_empty())
        {
            read.envelope.screen = None;
            return Some(Shown::Web {
                url: url.to_owned(),
                title: None,
                why: read.missing.clone(),
            });
        }
        let screen = read.envelope.screen.take()?;
        Some(Shown::Native {
            screen: Box::new(screen),
            status,
            body,
            keep: read.keep,
        })
    }

    // Answers.

    fn answered(&mut self, response: Response) {
        // An answer nobody waits for any more changes nothing.
        let Some(asked) = self.inner.pending.remove(&response.request) else {
            return;
        };
        let response = self.keeping(&asked, response);
        let Some(response) = self.follow(&asked, response) else {
            return;
        };
        match asked.purpose {
            Pending::Shell => return self.shell_answered(response),
            Pending::BeforeCall { call } => return self.before_call_answered(call, &response),
            Pending::SignIn {
                screen,
                second_step,
            } => return self.sign_in_answered(screen, second_step.as_deref(), &response),
            Pending::Exchange => return self.exchange_answered(&response),
            Pending::PushPut { token } => return self.push_put_answered(token, &response),
            Pending::PushGone => return self.push_gone_answered(),
            _ => {}
        }
        let answer = decode::read(response, &asked.url, &self.inner.origin, &self.config);
        // A redirect is measured against the address that was asked for first.
        let Asked {
            purpose,
            first: asked,
            ..
        } = asked;
        match purpose {
            Pending::Shell
            | Pending::BeforeCall { .. }
            | Pending::SignIn { .. }
            | Pending::Exchange
            | Pending::PushPut { .. }
            | Pending::PushGone => {}
            Pending::Before {
                screen,
                visit,
                id,
                was,
            } => self.before_answered(screen, visit, &id, &was, answer),
            Pending::Changes {
                screen,
                visit,
                list,
            } => self.changes_answered(screen, visit, &list, answer),
            Pending::Move {
                screen,
                visit,
                item,
                from,
                at,
            } => self.move_answered(screen, visit, &item, &from, at, answer),
            Pending::Options {
                screen,
                visit,
                form,
                field,
                turn,
                more,
            } => self.options_answered(screen, visit, &form, &field, turn, more, answer),
            Pending::Page { screen, visit } => self.page_answered(screen, visit, &asked, answer),
            Pending::Send {
                screen,
                visit,
                key,
                write,
                in_place,
            } => self.send_answered(screen, visit, &key, write, in_place, &asked, answer),
            Pending::Region {
                screen,
                visit,
                region,
                turn,
                epoch,
                go,
                url,
            } => self.region_answered(screen, visit, &region, turn, epoch, go, &url, answer),
            Pending::More {
                screen,
                visit,
                id,
                was,
            } => self.more_answered(screen, visit, &id, &was, answer),
        }
    }

    /// Whether an answer that came from `url`, when `asked` was asked for, is
    /// a screen the shell presents over the app: the sign-in a session that
    /// ended is sent to.
    pub(crate) fn covers(&self, asked: &str, url: &str) -> Option<(String, Present)> {
        if path_of(asked) == path_of(url) {
            return None;
        }
        let route = self.route(url)?;
        match route.present {
            // A page that moved to a sheet's address has only moved. A screen
            // with no chrome, presented over the app, is the way in.
            Some(present @ (Present::Full | Present::Sheet | Present::Task)) if !route.chrome => {
                Some((route.matches.clone(), present))
            }
            _ => None,
        }
    }

    /// Presents what a redirect led to over the app, and leaves the screen
    /// that asked as it was: the person comes back to it.
    pub(crate) fn cover(
        &mut self,
        under: ScreenId,
        route: (String, Present),
        url: &str,
        shown: Option<Shown>,
    ) {
        if let (Some(view), Some(inner)) = (
            self.view.screens.get_mut(&under),
            self.inner.screens.get_mut(&under),
        ) {
            view.loading = false;
            view.refreshing = false;
            inner.stale = true;
            self.touch_screen(under);
        }
        // One cover for one sign-in, however many requests were sent there.
        let up = self.view.modals.iter().any(|modal| {
            modal
                .stack
                .first()
                .and_then(|first| self.inner.screens.get(first))
                .is_some_and(|inner| inner.cover.as_ref() == Some(&route.0))
        });
        if up {
            return;
        }
        let id = self.new_screen(url);
        if let Some(inner) = self.inner.screens.get_mut(&id) {
            inner.cover = Some(route.0);
        }
        self.view.modals.push(Modal {
            present: route.1,
            locked: true,
            stack: vec![id],
        });
        self.touch(Change::Nav);
        match shown {
            // The sign-in is nobody's page to come back to: it is shown and not kept.
            Some(Shown::Native { screen, status, .. }) => {
                self.show_native(id, url, *screen, status, false)
            }
            Some(shown) => self.show(id, url, shown),
            None => self.fetch_page(id, false),
        }
    }

    fn page_answered(&mut self, id: ScreenId, visit: u64, asked: &str, answer: Answer) {
        // The person asked for something else on this screen, or left it.
        if self.inner.screens.get(&id).map(|inner| inner.visit) != Some(visit) {
            return;
        }
        let blank = self
            .view
            .screens
            .get(&id)
            .is_some_and(|view| matches!(view.content, Content::Blank));
        let settle = |core: &mut Core, failure: Option<Failure>| {
            if let Some(view) = core.view.screens.get_mut(&id) {
                view.loading = false;
                view.refreshing = false;
                view.cached = false;
                view.failure = failure;
            }
            core.touch_screen(id);
        };
        match answer {
            Answer::Failed(failure) => {
                settle(self, Some(failure));
                // With something on screen the failure is a passing message; without, it is the screen.
                if !blank {
                    self.failed(failure, Some(id));
                }
            }
            Answer::Left(url) => {
                // The page moved to another site: it opens there.
                self.open_outside(&url, false);
                if blank {
                    self.remove(id);
                } else {
                    settle(self, None);
                }
            }
            Answer::Html { url } => self.show_web(id, &url, &url, None, vec!["html".to_owned()]),
            Answer::Empty => settle(self, blank.then_some(Failure::Protocol)),
            Answer::Tree {
                url,
                status,
                mut read,
                body,
                shell,
            } => {
                if self.note(&mut read, shell) {
                    return settle(self, None);
                }
                if let Some(way_in) = Self::way_in(status, &read.envelope.ops) {
                    return self.session_ended(id, &way_in);
                }
                let shown = self.shown(&url, status, &mut read, body);
                if let Some(route) = self.covers(asked, &url) {
                    return self.cover(id, route, &url, shown);
                }
                let nothing = shown.is_none();
                match shown {
                    Some(shown) => self.show(id, &url, shown),
                    None => settle(self, None),
                }
                let left = self.apply_ops(id, std::mem::take(&mut read.envelope.ops), false);
                if nothing && !left && blank {
                    settle(self, Some(Failure::Protocol));
                }
            }
        }
    }

    /// Takes a screen out of its stack. A sheet or cover with nothing left goes with it.
    pub(crate) fn remove(&mut self, id: ScreenId) {
        if let Some(stack) = self.view.stack_of(id) {
            stack.retain(|other| *other != id);
        }
        self.view.modals.retain(|modal| !modal.stack.is_empty());
        self.close(id);
    }

    // The end of every event.

    /// Makes sure the screen in front exists and is not out of date.
    fn ensure_front(&mut self) {
        if self.view.stage != Stage::Ready {
            return;
        }
        match self.front() {
            Some(front) => {
                let stale = self
                    .inner
                    .screens
                    .get(&front)
                    .is_some_and(|inner| inner.stale);
                let loading = self
                    .view
                    .screens
                    .get(&front)
                    .is_some_and(|view| view.loading);
                if stale && !loading {
                    self.fetch_page(front, false);
                }
            }
            None => {
                let Some(href) = self
                    .view
                    .destinations
                    .get(self.view.selected)
                    .map(|destination| destination.href.clone())
                else {
                    return;
                };
                let url = self.destination_url(&href);
                let id = self.open(&url);
                self.view.destinations[self.view.selected].stack.push(id);
                self.touch(Change::Nav);
            }
        }
    }

    fn settle(&mut self) -> Update {
        self.fail_unsent();
        self.inner.refusal_said = false;
        self.ensure_front();
        let front = self.front();
        let stacked = self.view.front_stack().len() > 1;
        let covered = front
            .and_then(|front| self.view.screens.get(&front))
            .is_some_and(|view| !view.overlays.is_empty());
        let first = self.view.selected == 0 || !self.config.back_to_first;
        let can_go_back = !self.view.modals.is_empty() || stacked || covered || !first;
        if self.view.front != front || self.view.can_go_back != can_go_back {
            if let Some(inner) = self
                .view
                .front
                .and_then(|was| self.inner.screens.get_mut(&was))
            {
                inner.missed = true;
            }
            self.view.front = front;
            self.view.can_go_back = can_go_back;
            self.touch(Change::Nav);
        }
        self.sync_streams();
        let mut changed = std::mem::take(&mut self.changed);
        // Two changes to the rows of one list can't be told apart by their
        // order: its screen is read again instead.
        let mut lists: Vec<(ScreenId, &str)> = changed
            .iter()
            .filter_map(|change| match change {
                Change::Rows {
                    screen, id, count, ..
                } if *count > 0 => Some((*screen, id.as_str())),
                Change::Redraw { screen, id, .. } => Some((*screen, id.as_str())),
                _ => None,
            })
            .collect();
        lists.sort_unstable();
        let twice: Vec<ScreenId> = lists
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .map(|pair| pair[0].0)
            .collect();
        changed.extend(twice.into_iter().map(|id| Change::Screen { id }));
        let whole = |screen: &ScreenId| changed.contains(&Change::Screen { id: *screen });
        // A list whose rows were added says so once: the rows say its state too.
        let grew = |screen: &ScreenId, list: &str| {
            changed.iter().any(|change| match change {
                Change::Rows {
                    screen: other,
                    id,
                    count,
                    ..
                } => other == screen && id == list && *count > 0,
                // Rows drawn again say the list's state with them.
                Change::Redraw {
                    screen: other, id, ..
                } => other == screen && id == list,
                _ => false,
            })
        };
        let changed = changed
            .iter()
            .filter(|change| match change {
                Change::Screen { id } => self.view.holds(*id),
                Change::Form { screen, .. } => self.view.holds(*screen) && !whole(screen),
                Change::Rows {
                    screen, id, count, ..
                } => {
                    self.view.holds(*screen) && !whole(screen) && (*count > 0 || !grew(screen, id))
                }
                Change::Redraw { screen, .. } => self.view.holds(*screen) && !whole(screen),
                _ => true,
            })
            .cloned()
            .collect();
        Update {
            changed,
            effects: std::mem::take(&mut self.effects),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use navree_native::{Client, FormFactor, Platform};

    #[test]
    fn every_request_says_who_asks_and_a_post_says_it_is_a_form() {
        let mut client = Client::new(Platform::Ios, FormFactor::Phone);
        client.app = "1.4.0".to_owned();
        client.lib = "0.1.0".to_owned();
        let mut core = Core::new(Config::new("https://shop.example", client));
        let update = core.send(Event::Start);
        let Effect::Fetch(request) = &update.effects[0] else {
            panic!("{update:?}");
        };
        assert_eq!(request.method, Verb::Get);
        assert_eq!(
            request.headers[0],
            ("x-navree-native".to_owned(), "1".to_owned())
        );
        assert!(
            request.headers[1].1.starts_with("platform=ios; "),
            "{:?}",
            request.headers[1]
        );
        assert!(
            request.headers[1].1.contains("lib=0.1.0"),
            "{:?}",
            request.headers[1]
        );
        assert_eq!(
            request.headers[2],
            ("accept".to_owned(), CONTENT_TYPE.to_owned())
        );
        assert_eq!(request.headers.len(), 3);
        assert!(request.body.is_none() && !request.xsrf);

        core.fetch(Pending::Shell, Method::Post, "/sign-out", None, SEND_LIMIT);
        let Some(Effect::Fetch(post)) = core.effects.last() else {
            panic!();
        };
        assert_eq!(
            post.body,
            Some(Body::Form {
                text: String::new()
            })
        );
        assert_eq!(
            post.headers[3],
            (
                "content-type".to_owned(),
                "application/x-www-form-urlencoded".to_owned()
            )
        );
        assert!(post.xsrf);
    }

    #[test]
    fn a_value_a_server_chose_goes_in_a_header_as_visible_ascii() {
        assert_eq!(header_safe("figures"), "figures");
        assert_eq!(header_safe("orders-items_2.x~!"), "orders-items_2.x~!");
        assert_eq!(header_safe("a b%c\r\nd"), "a%20b%25c%0D%0Ad");
        assert_eq!(header_safe("é"), "%C3%A9");
        let id = "आंकड़े";
        let safe = header_safe(id);
        assert!(safe.bytes().all(|byte| (0x21..=0x7e).contains(&byte)));
        // Read back byte by byte, it is the id it was.
        let bytes: Vec<u8> = safe
            .split('%')
            .skip(1)
            .map(|hex| u8::from_str_radix(hex, 16).unwrap())
            .collect();
        assert_eq!(String::from_utf8(bytes).unwrap(), id);
    }

    #[test]
    fn the_newer_app_is_offered_only_at_an_https_address() {
        let upgrade = |url: &str| {
            newer_app(navree_native::envelope::Upgrade {
                min_app: "2.0.0".to_owned(),
                url: url.to_owned(),
            })
            .url
        };
        assert_eq!(
            upgrade("https://store.example/app?id=com.charkha.supply"),
            "https://store.example/app?id=com.charkha.supply"
        );
        for dropped in [
            "javascript:alert(1)",
            "http://store.example/app",
            "market://details?id=com.charkha.supply",
            "intent://x",
            "/app",
            " https://store.example/app",
            "https://shop.example@store.example/",
        ] {
            assert_eq!(upgrade(dropped), "", "{dropped:?}");
        }
    }

    /// A core that started against a server with no shell: one destination,
    /// its first page asked for.
    fn started() -> Core {
        let mut core = Core::new(Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        ));
        core.send(Event::Start);
        core.send(Event::Response(Response {
            request: 1,
            status: 404,
            ..Response::default()
        }));
        core
    }

    #[test]
    fn a_request_for_an_address_not_on_the_app_s_origins_is_never_sent_and_fails() {
        let mut core = Core::new(Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        ));
        for address in [
            "https://evil.example/x",
            "//evil.example/x",
            "https://shop.example//evil.example/x",
        ] {
            core.fetch(Pending::Shell, Method::Get, address, None, PAGE_LIMIT);
        }
        let update = core.settle();
        assert!(
            !update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Fetch(_))),
            "{update:?}"
        );
        assert!(core.inner.pending.is_empty() && core.inner.unsent.is_empty());
        // The shell failed as one whose answer was a 502.
        assert_eq!(core.view.stage, Stage::Failed);
        assert_eq!(core.view.failure, Some(Failure::Server));
    }

    #[test]
    fn a_page_not_on_the_app_s_origins_is_never_shown_in_the_web_view() {
        let mut core = started();
        core.show_web(1, "/", "https://evil.example/x", None, Vec::new());
        let view = &core.view.screens[&1];
        assert!(!matches!(view.content, Content::Web { .. }), "{view:?}");
        assert_eq!(view.failure, Some(Failure::Protocol));
        core.show_web(1, "/", "/", None, Vec::new());
        assert!(matches!(
            &core.view.screens[&1].content,
            Content::Web { url, .. } if url == "https://shop.example/"
        ));
    }

    #[test]
    fn a_stream_not_on_the_app_s_origins_is_never_opened() {
        let mut core = started();
        let stream = crate::state::Stream {
            id: 1,
            url: "//evil.example/stream".to_owned(),
            open: false,
            failures: 0,
            reopen: None,
            stopped: false,
            missed: false,
        };
        assert!(!core.open_stream(&stream));
        assert!(core.open_stream(&crate::state::Stream {
            url: "/stream?topics=stats".to_owned(),
            ..stream
        }));
        let opened: Vec<&Effect> = core
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::StreamOpen { .. }))
            .collect();
        assert!(
            matches!(opened[..], [Effect::StreamOpen { url, .. }] if url == "https://shop.example/stream?topics=stats"),
            "{opened:?}"
        );
    }

    #[test]
    fn a_page_whose_address_is_not_ours_fails_once_and_stops_loading() {
        let mut core = started();
        core.view.screens.get_mut(&1).unwrap().url = "https://evil.example/x".to_owned();
        core.fetch_page(1, false);
        let update = core.settle();
        assert!(
            !update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Fetch(_))),
            "{update:?}"
        );
        let view = &core.view.screens[&1];
        assert!(!view.loading);
        assert_eq!(view.failure, Some(Failure::Server));
        assert!(core.inner.pending.is_empty() && core.inner.unsent.is_empty());
    }

    #[test]
    fn a_request_left_after_the_rounds_is_failed_not_dropped() {
        let mut core = started();
        core.view.screens.get_mut(&1).unwrap().url = "https://evil.example/x".to_owned();
        core.fetch_page(1, false);
        core.fail_unsent_within(0);
        let view = &core.view.screens[&1];
        assert!(!view.loading);
        assert_eq!(view.failure, Some(Failure::Server));
        assert!(core.inner.pending.is_empty());
    }

    fn reports(update: &Update) -> Vec<String> {
        update
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Report { what, detail } if what == "address" => Some(detail.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_address_an_answer_gave_that_does_not_read_is_said_once() {
        let mut core = started();
        let tap = |href: &str| Event::Act {
            screen: Some(1),
            action: navree_native::Action::Navigate {
                href: href.to_owned(),
                present: Present::Auto,
            },
            form: None,
            overlay: None,
            text: None,
            now: None,
            ask: None,
            from: None,
        };
        let update = core.send(tap("/a/..//evil.example/x?token=s3cret#top"));
        assert_eq!(reports(&update), ["link: /a/..//evil.example/x"]);
        // A redirect that does not read.
        let update = core.send(tap("/orders"));
        let request = update
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Fetch(request) => Some(request.request),
                _ => None,
            })
            .unwrap();
        let update = core.send(Event::Response(Response {
            request,
            status: 302,
            location: Some(" /sign-in?next=/orders".to_owned()),
            ..Response::default()
        }));
        assert_eq!(reports(&update), ["redirect:  /sign-in"]);
        // Another site is not refused, and is not said.
        assert!(reports(&core.send(tap("https://evil.example/x"))).is_empty());
        // A link from outside the app is the system's, and is not said.
        assert!(
            reports(&core.send(Event::OpenLink {
                url: " https://shop.example/x".to_owned()
            }))
            .is_empty()
        );
    }

    #[test]
    fn a_shell_with_many_entries_that_do_not_read_is_said_once() {
        let mut core = Core::new(Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        ));
        core.send(Event::Start);
        let shell = include_str!("../transcripts/fixtures/made/shell-routes.json")
            .replace(r#""href": "/orders""#, r#""href": " /orders""#)
            .replace(r#""href": "/inventory""#, r#""href": "/a\\b""#);
        let update = core.send(Event::Response(Response {
            request: 1,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            body: shell,
            ..Response::default()
        }));
        assert_eq!(reports(&update), ["shell:  /orders"]);
        assert_eq!(core.view.destinations.len(), 1);
    }

    /// Every `open_url` effect is made in one place, and no address is
    /// judged by whether `://` appears in it.
    #[test]
    fn an_address_opens_outside_one_way() {
        let mut made = 0;
        let search = ["contains(\"", "://\")"].concat();
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            made += code.matches("Effect::OpenUrl {").count();
            assert!(!code.contains(&search), "{}", path.display());
        }
        assert_eq!(made, 1);
    }
}

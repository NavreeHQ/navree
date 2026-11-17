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
//!   it comes to the front. So does a write whose answer failed or was too
//!   large to read: the server may have written before it did. A write
//!   counts as never made only when the server cannot have run it (see
//!   `Heard::write_may_have_run`).
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
use crate::cache::Who;
use crate::decode::{self, Answer, Read};
use crate::effect::{Body, Change, Effect, Request, Update, Verb};
use crate::event::{Event, Response, StreamState, TransportError};
use crate::origin::{Target, https_address, openable};
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

/// What stands behind an address opened outside the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum By {
    /// The person tapped something: a link, a button, a message's button.
    Tap,
    /// An answer, by itself: a redirect, an op. The kind is what is said
    /// when what it asked is refused.
    Answer(&'static str),
}

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

/// Whether an answer got through to the app: one came, and it is not a 5xx
/// or a 429, which are what a proxy or a gateway in front of the app answers
/// while the app restarts or sheds load. The shell is asked for again after
/// one that did not, and one that did not and names nobody says nothing of
/// who is signed in.
pub(crate) fn got_through(response: &Response) -> bool {
    response.error.is_none() && response.status < 500 && response.status != 429
}

/// What an answer says of itself, read from its status, its headers and the
/// request it answers before its body is: the one place two questions about
/// it are decided, so the two answers cannot drift apart.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Heard {
    /// No answer came: the transport's error.
    pub(crate) error: Option<TransportError>,
    /// The answer's status.
    pub(crate) status: u16,
    /// It names a person (`x-navree-person`, not empty).
    pub(crate) names_someone: bool,
    /// It answers the request that was sent first: no redirect was followed
    /// to it, by the core or by the client's transport.
    pub(crate) first_hop: bool,
    /// The request carried a write.
    pub(crate) carried_write: bool,
    /// The core made it: the request was never sent.
    pub(crate) made_here: bool,
}

impl Heard {
    /// Whether this answer is the app's word on who is signed in. Not when
    /// it names nobody and did not get through to the app or says its
    /// request could not be handled (5xx, 408, 413, 414, 429, 431): what a
    /// proxy, a gateway or a layer in front of the app's own code answers
    /// while the app restarts, sheds load, or refuses a request too slow, too
    /// large or too long. Any answer that names a person is, and so is every
    /// other answer: a 401, a 403 or a 419 that names nobody is the news that
    /// the session ended or changed.
    pub(crate) fn speaks_for_the_session(&self) -> bool {
        self.names_someone
            || !(self.status >= 500 || matches!(self.status, 408 | 413 | 414 | 429 | 431))
    }

    /// Whether the server can have run the write this request carried. It
    /// cannot only on the first hop (the answer to the request that carried
    /// it, no redirect followed), and only when:
    /// 1. nothing of the request was sent (no network, or never sent);
    /// 2. the answer names nobody (a layer in front of the handlers made it)
    ///    and refuses the request for its size before any handler ran: 413,
    ///    414, 431;
    /// 3. the answer names the person (the app's own handler made it) with a
    ///    status by which a handler says it refused this request and did
    ///    nothing: 400, 402, 403, 404, 405, 406, 409, 410, 412, 415, 422, 428.
    ///
    /// Anything else may follow a write that ran: 408, 413, 429 and 431 that
    /// name a person (a handler can reach them after it has begun), 401 and
    /// 419 (the session rule deals with them), every 5xx, a timeout or a
    /// broken connection, an answer too large to read, any 2xx or 3xx, any
    /// other status, and every answer after a redirect (the write was taken;
    /// the page it led to failed).
    pub(crate) fn write_may_have_run(&self) -> bool {
        if !self.carried_write {
            return false;
        }
        if !self.first_hop {
            return true;
        }
        if self.made_here || self.error == Some(TransportError::Offline) {
            return false;
        }
        if self.error.is_some() {
            return true;
        }
        let refused_for_its_size = !self.names_someone && matches!(self.status, 413 | 414 | 431);
        let refused_by_the_handler = self.names_someone
            && matches!(
                self.status,
                400 | 402 | 403 | 404 | 405 | 406 | 409 | 410 | 412 | 415 | 422 | 428
            );
        !(refused_for_its_size || refused_by_the_handler)
    }
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
        self.inner.event_start = self.inner.next.request + 1;
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
            Event::RegionRetry { screen, region } => {
                self.acted_on(screen);
                self.shell_again();
                self.fetch_region(screen, &region, None)
            }
            Event::More {
                screen,
                id,
                before: false,
            } => {
                self.acted_on(screen);
                self.more(screen, &id)
            }
            Event::More {
                screen,
                id,
                before: true,
            } => {
                self.acted_on(screen);
                self.rows_before(screen, &id)
            }
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
                // The session may have run out, or changed, while it was away.
                self.session_unsure();
                // A token that could not be sent before is tried again.
                self.push_sync();
            }
            Event::Background => {
                self.inner.background = true;
                // The pause is not kept while the app is away: coming back asks.
                if let Some(timer) = self.inner.shell_timer.take() {
                    self.cancel_timer(timer);
                }
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
                // The client ended the session while the core waited for the
                // push token's removal: the wait is over with this, and the
                // removal went with the rest above.
                let waited = std::mem::take(&mut self.inner.signing_out);
                self.push_forget();
                self.inner.has_shell = false;
                self.inner.user = None;
                self.shell_settled();
                // A snapshot that waited for the shell was the one who left's.
                self.inner.restore = None;
                self.view.app = None;
                self.view.destinations.clear();
                self.view.stage = Stage::Starting;
                self.touch(Change::App);
                self.touch(Change::Nav);
                self.forget_all();
                if waited {
                    // The client was told the session is ending, and every
                    // `session_ending` is followed by one `forget_session`,
                    // whoever ends the wait.
                    self.session_forgotten();
                } else {
                    self.start();
                }
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
                Some(TimerFor::Shell) => {
                    // A new pause: a request that gets through may ask once more.
                    self.inner.shell_timer = None;
                    self.inner.shell_nudged = false;
                    self.shell_again();
                }
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
                    // The person tapped the upgrade's button.
                    self.open_outside(&url, false, By::Tap);
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
            .find(|route| self.inner.origin.on_route(&route.matches, url))
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
                // Made here, not by the app: no person to compare.
                self.inner.synthetic = true;
                self.answered(Response {
                    request,
                    status: 502,
                    ..Response::default()
                });
                self.inner.synthetic = false;
            }
        }
        // What is left is failed all the same. What failing it asks for in
        // turn waits for the next event's update, so no asker is left waiting.
        for request in std::mem::take(&mut self.inner.unsent) {
            self.inner.synthetic = true;
            self.answered(Response {
                request,
                status: 502,
                ..Response::default()
            });
            self.inner.synthetic = false;
        }
    }

    /// Says, once for the event, that an address an answer gave does not
    /// read and was not followed: of what kind, and its scheme and host, never its path.
    pub(crate) fn said_refused(&mut self, kind: &str, address: &str) {
        if std::mem::replace(&mut self.inner.refusal_said, true) {
            return;
        }
        let site = crate::origin::site_of(address);
        self.report("address", format!("{kind}: {site}"));
    }

    /// Asks the client to open an address outside the app: the one way an
    /// `open_url` effect is made. Only a whole `http` or `https` address that
    /// reads, written again from its parts, or an address of a scheme the
    /// system handles, is ever opened; anything else is dropped. The
    /// system's own (mail, the dialler, messages, maps) open only when a tap
    /// stands behind them: an answer by itself takes the person to a web
    /// page, never to those, and what it asked is said, without the address.
    pub(crate) fn open_outside(&mut self, url: &str, inside: bool, by: By) {
        let Some(url) = openable(url) else {
            return;
        };
        let web = matches!(
            crate::origin::scheme_of(&url).as_deref(),
            Some("http" | "https")
        );
        match by {
            By::Answer(kind) if !web => {
                if !std::mem::replace(&mut self.inner.refusal_said, true) {
                    let site = crate::origin::site_of(&url);
                    self.report("address", format!("{kind} with no tap: {site}"));
                }
            }
            _ => self.effects.push(Effect::OpenUrl { url, inside }),
        }
    }

    /// Opens an address of the app's own outside it, as a whole address:
    /// always a web one.
    pub(crate) fn open_ours_outside(&mut self, url: &str) {
        if let Some(url) = self.inner.origin.absolute(url) {
            self.open_outside(&url, false, By::Answer("external"));
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
            Notice::Unsaved | Notice::SecondStep | Notice::SavedNotRefreshed => Tone::Warning,
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
            // The person tapped the message's button.
            self.post(screen, &action.href, action.fields, None, By::Tap);
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

    /// Asks for the shell even when an older request for it is on its way.
    pub(crate) fn fetch_shell_anew(&mut self) {
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
        self.inner.shell_failure = Some(failure);
        // This launch's shell did not come while the one kept on the device is
        // drawn. If the request did not get through, the kept bars stay and the
        // shell is asked for again; an answer that is not a shell gives the
        // blank one.
        if self.inner.shell_kept {
            return match failure {
                Failure::Offline | Failure::Timeout | Failure::Server => self.shell_unreached(),
                _ => self.no_shell(),
            };
        }
        // A shell that is already drawn stays; only a first load can fail the app.
        if !self.inner.has_shell {
            self.view.stage = Stage::Failed;
            self.view.failure = Some(failure);
            self.touch(Change::Nav);
        }
    }

    /// This launch's shell request did not get through (no network, a
    /// timeout, a 5xx, a 429) and the kept shell is drawn: the bars stay, the
    /// screens show what could not load, a waiting snapshot keeps waiting,
    /// and the shell is asked for again after a pause that doubles up to a
    /// minute (and when the person retries, when the app comes to the front,
    /// and when a request of the app gets through).
    fn shell_unreached(&mut self) {
        self.inner.shell_wanted = true;
        if self.inner.shell_timer.is_none() {
            let pause = self
                .inner
                .shell_pause
                .max(1_000)
                .saturating_mul(2)
                .min(60_000);
            self.inner.shell_pause = pause;
            let timer = self.timer(pause, TimerFor::Shell);
            self.inner.shell_timer = Some(timer);
        }
    }

    /// Asks for the shell again when it is wanted: it could not be fetched,
    /// and nothing is asking now.
    pub(crate) fn shell_again(&mut self) {
        if self.inner.shell_wanted {
            self.fetch_shell();
        }
    }

    /// A request of the app got through. When the shell could not be fetched
    /// because the network was gone, it is asked for again, once in each
    /// pause; when the shell route itself answered with an error, only the
    /// pause asks.
    fn shell_nudge(&mut self) {
        if self.inner.shell_wanted
            && !self.inner.shell_nudged
            && matches!(
                self.inner.shell_failure,
                Some(Failure::Offline | Failure::Timeout)
            )
        {
            self.inner.shell_nudged = true;
            self.fetch_shell();
        }
    }

    /// A shell has answered, or the person is gone: nothing is wanted again.
    pub(crate) fn shell_settled(&mut self) {
        self.inner.shell_wanted = false;
        self.inner.shell_pause = 0;
        self.inner.shell_nudged = false;
        self.inner.shell_failure = None;
        if let Some(timer) = self.inner.shell_timer.take() {
            self.cancel_timer(timer);
        }
    }

    /// What the shell asked for gave no shell: a server with no shell still
    /// has pages, one destination, its first page. The shell kept on the
    /// device gives way to the same and stays so until a shell answers: it
    /// was the last launch's, and nothing says this launch's server still
    /// holds it. A shell already drawn from an answer stays.
    fn no_shell(&mut self) {
        if self.inner.shell_kept {
            // The person the kept shell named was the last launch's word, so
            // its going is no sign-out, and nothing kept goes with it.
            self.inner.user = None;
            // A snapshot waited for a shell that did not come: it is not
            // used, and nothing is left holding it.
            if self.inner.restore.take().is_some() {
                self.report(
                    "snapshot",
                    "a snapshot is not used: this launch's shell did not give a shell".to_owned(),
                );
            }
        } else if self.inner.has_shell {
            return;
        }
        self.install(Shell::new("", ""), false);
    }

    fn shell_answered(&mut self, response: Response) {
        // An answer asked before the point from which a fresh one is needed
        // (the session may have changed since) is dropped whole: not
        // installed, not shown, not kept. The fresh one is on its way.
        if response.request < self.inner.cache.fresh_from {
            return;
        }
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
        // A server that asks to wait has not answered the question.
        if response.status == 429 {
            return self.shell_failed(Failure::Server);
        }
        if !(200..300).contains(&response.status) || media != CONTENT_TYPE {
            return self.no_shell();
        }
        let header = decode::shell_stamp_of(&response);
        // Read where there is room for it, like any other answer.
        let body = response.body.as_str();
        // A shell larger than any answer may be is not read: it fails below
        // as one whose shape does not read.
        let value: Value = (body.len() <= navree_native::MAX_BODY)
            .then(|| decode::with_room(|| serde_json::from_str(body).ok()).flatten())
            .flatten()
            .unwrap_or(Value::Null);
        if let Some(upgrade) = value
            .get("upgrade")
            .cloned()
            .and_then(|upgrade| serde_json::from_value(upgrade).ok())
        {
            self.view.upgrade = Some(newer_app(upgrade));
            self.view.stage = Stage::Upgrade;
            // An app too old for its server is not waiting for a shell.
            self.shell_settled();
            return self.touch(Change::Nav);
        }
        match decode::with_room(move || serde_json::from_value::<Shell>(value))
            .unwrap_or_else(|| Err(serde::de::Error::custom("the shell could not be read")))
        {
            Ok(mut shell) if shell.navree == MAJOR => {
                self.shell_pictures(&mut shell);
                self.inner.naming = response
                    .header(navree_native::PERSON_HEADER)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned);
                // The stamp of the shell now held: its own, or the one its answer named.
                let stamp = (!shell.stamp.is_empty())
                    .then(|| shell.stamp.clone())
                    .or(header);
                self.install(shell, false);
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
                self.no_shell();
            }
        }
    }

    /// Draws the app from the shell kept at the last launch. False when it
    /// does not read.
    pub(crate) fn shell_from_cache(&mut self, body: &str) -> bool {
        if body.len() <= navree_native::MAX_BODY
            && let Some(Ok(mut shell)) = decode::with_room(|| serde_json::from_str::<Shell>(body))
            && shell.navree == MAJOR
        {
            self.shell_pictures(&mut shell);
            self.inner.cache.shell = Some(body.to_owned());
            self.install(shell, true);
            // This launch's shell request may have failed before the kept
            // shell was read (with no network it fails in milliseconds, the
            // file takes longer): the end is the same as in the other order.
            // One that did not get through leaves the kept bars and asks for
            // the shell again; one that answered with no shell it can use
            // (another major, a redirect to another site) gives the blank one.
            let asking = self
                .inner
                .pending
                .values()
                .any(|asked| asked.purpose == Pending::Shell);
            if !asking {
                match self.inner.shell_failure {
                    Some(Failure::Offline | Failure::Timeout | Failure::Server) => {
                        self.shell_unreached();
                    }
                    Some(_) => self.no_shell(),
                    None => {}
                }
            }
            return true;
        }
        false
    }

    /// Writes the pictures of the shell's user menu as a client is given
    /// them, read against the app's home.
    fn shell_pictures(&self, shell: &mut Shell) {
        let Some(user) = shell.user.as_mut() else {
            return;
        };
        let sources = decode::Sources {
            origin: &self.inner.origin,
            from: "/",
        };
        // Read where there is room for it, like the trees of a page.
        let menu = &mut user.menu;
        decode::with_room(|| menu.iter_mut().for_each(|node| sources.within(node)));
    }

    /// The address a destination opens at.
    pub(crate) fn destination_url(&self, href: &str) -> String {
        match self.inner.origin.resolve(href, "/") {
            Target::Inside(url) => url,
            _ => "/".to_owned(),
        }
    }

    /// Draws the app from a shell. `from_device`: the shell is the one kept on the
    /// device, drawn before this launch's has answered. It draws the app's
    /// name and its top-level destinations, each by its label and icon, and
    /// nothing else of what the shell says: not the context line, the group
    /// labels, the items below the top level, a count, the person or their
    /// menu. A snapshot waits for this launch's shell.
    fn install(&mut self, shell: Shell, from_device: bool) {
        // Who it names: by the id its answer said, else by name and email.
        // The kept shell is no answer: a request that failed before it was
        // read is not forgotten.
        if !from_device {
            self.shell_settled();
        }
        let naming = self.inner.naming.take();
        let user = shell.user.as_ref().map(|user| Who {
            id: naming.clone(),
            name: user.name.clone(),
            email: user.email.clone(),
        });
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
            let (group, badge, items) = if from_device {
                (None, None, Vec::new())
            } else {
                (group, item.badge, item.items)
            };
            self.view.destinations.push(Destination {
                label: item.label,
                icon: item.icon,
                href: item.href,
                badge,
                group,
                items,
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
        // The person as a client is given them carries the core's key for
        // them, by which a client tells one person from another. It is the
        // core's alone to write: whatever a server wrote there is written over.
        let key = self.inner.user.as_ref().map(Who::key);
        let signed_in = shell.user.map(|mut user| {
            user.key = key;
            user
        });
        self.view.app = Some(App {
            name: shell.name,
            context: if from_device { None } else { shell.context },
            home: shell.home,
            theme: shell.theme,
            user: if from_device { None } else { signed_in },
        });
        self.inner.shell_kept = from_device;
        if self.view.stage != Stage::Upgrade {
            self.view.stage = Stage::Ready;
            self.view.failure = None;
        }
        self.touch(Change::App);
        self.touch(Change::Nav);
        // A snapshot, and the links that wait for the shell, wait for this
        // launch's: the kept shell's person is not yet anybody's word.
        if from_device && self.inner.restore.is_some() {
            return;
        }
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
        // The removal of a push token is the session's before, and the wait
        // for it ends with the word that the session is forgotten: it stays.
        self.drop_pending(|purpose| !matches!(purpose, Pending::Shell | Pending::PushGone));
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
        // Nobody may be signed in any more, wherever the way in leads.
        self.who_may_have_changed(false);
        let url = match self.resolve(href, under) {
            Target::Inside(url) => url,
            Target::Outside(url) => {
                return self.open_outside(&url, false, By::Answer("sign-in"));
            }
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
        let name = route.map_or_else(
            || self.inner.origin.where_of(&url),
            |route| route.matches.clone(),
        );
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

    /// Writes the paths `keys` of a device call as whole addresses on the
    /// app's origin, as a picture's source is written: what the system is
    /// handed (a page shared) or the client fetches (a file shared, the
    /// options of a passkey) is never a path for it to read against
    /// something. One that does not read as one of the app's addresses is
    /// taken out, and that is said: the name of what was taken out, never the
    /// address.
    fn whole_in(&mut self, call: &mut Value, keys: &[&str]) {
        let Some(call) = call.as_object_mut() else {
            return;
        };
        let mut dropped = Vec::new();
        for key in keys {
            let Some(path) = call.get(*key).and_then(Value::as_str) else {
                continue;
            };
            match self.inner.origin.absolute(path) {
                Some(whole) => {
                    call.insert((*key).to_owned(), Value::String(whole));
                }
                None => {
                    call.remove(*key);
                    dropped.push(*key);
                }
            }
        }
        for key in dropped {
            self.report(
                "device",
                format!(
                    "the `{key}` of a device call is not an address of the app, and was left out"
                ),
            );
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
            // Every device call here is one a tap started.
            Call::OpenUrl(open) => self.open_outside(open.url.as_str(), open.inside, By::Tap),
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
                let mut value = serde_json::to_value(&call);
                // What the system is handed or the client fetches is written whole.
                let addresses: &[&str] = match &call {
                    Call::Share(_) => &["href", "file"],
                    Call::Passkey(_) => &["options"],
                    _ => &[],
                };
                if !addresses.is_empty()
                    && let Ok(value) = value.as_mut()
                {
                    self.whole_in(value, addresses);
                }
                let number = self.run(value, then.is_some());
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
        self.post(
            screen,
            then.href.as_str(),
            fields,
            None,
            By::Answer("device reply"),
        );
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
        let answer: Option<Value> = (good && response.body.len() <= navree_native::MAX_BODY)
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
        let answer: Option<Value> = (response.body.len() <= navree_native::MAX_BODY)
            .then(|| serde_json::from_str(&response.body).ok())
            .flatten();
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

    /// The person acted on a screen: answers refused for their size no longer
    /// count against it.
    pub(crate) fn acted_on(&mut self, id: ScreenId) {
        if let Some(inner) = self.inner.screens.get_mut(&id) {
            inner.refused = 0;
        }
    }

    fn refresh(&mut self, id: ScreenId) {
        self.acted_on(id);
        self.shell_again();
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
        let same = self.inner.origin.where_of(&view.url) == self.inner.origin.where_of(url)
            && view.screen().is_some();
        // A question the screen asked was about what it showed before.
        view.question = None;
        inner.held = None;
        inner.form_ids.clear();
        inner.field_ids.clear();
        inner.server_ids.clear();
        inner.next_names.clear();
        let mut names = Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
            next: &mut inner.next_names,
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
                .find(|route| self.inner.origin.on_route(&route.matches, url))
                .is_none_or(|route| route.chrome);
        }
        view.title = Some(screen.title.clone()).filter(|title| !title.is_empty());
        inner.weight = crate::weight::of_screen(&screen);
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
        self.live_fetches_dropped(id);
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
        // A page in the web view shares the cookies: the session may change there.
        self.session_unsure();
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
            .find(|route| self.inner.origin.on_route(&route.matches, url))
            .is_none_or(|route| route.chrome);
        self.drop_pending(|purpose| matches!(purpose, Pending::Region { screen, .. } | Pending::More { screen, .. } if *screen == id));
        self.live_fetches_dropped(id);
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
            } => {
                // A page for the web view is never kept: an older kept copy goes.
                self.forget_kept(url, true);
                self.show_web(id, url, &page, title, why);
            }
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
        // A request of the app that got through, once it is dealt with, asks
        // for a shell that could not be fetched again (unless it was asked
        // for already).
        let got_through = got_through(&response);
        let not_the_shell = self
            .inner
            .pending
            .get(&response.request)
            .is_some_and(|asked| asked.purpose != Pending::Shell);
        self.answered_once(response);
        if got_through && not_the_shell {
            self.shell_nudge();
        }
    }

    /// What an answer says of itself (see [`Heard`]).
    fn heard(&self, asked: &Asked, response: &Response) -> Heard {
        let first_hop = asked.hops == 0
            && (response.url.is_empty()
                || self.inner.origin.absolute(&asked.url).as_deref()
                    == Some(response.url.as_str()));
        Heard {
            error: response.error,
            status: response.status,
            names_someone: response
                .header(navree_native::PERSON_HEADER)
                .is_some_and(|id| !id.is_empty()),
            first_hop,
            carried_write: matches!(
                asked.purpose,
                Pending::Send { write: true, .. } | Pending::Move { .. }
            ),
            made_here: self.inner.synthetic,
        }
    }

    fn answered_once(&mut self, response: Response) {
        // An answer nobody waits for any more changes nothing.
        let Some(asked) = self.inner.pending.remove(&response.request) else {
            return;
        };
        let heard = self.heard(&asked, &response);
        self.inner.write_may_have_run = heard.write_may_have_run();
        let response = self.same_session(&asked, heard, response);
        let response = self.keeping(&asked, response);
        // The push token's removal is over with its first answer, whatever
        // it is: a redirect of it is not followed. The client was told the
        // session is ending in the update that asked for the removal, and
        // may have taken the session's cookies away since: a second request
        // would go without them and could not say whose device to forget,
        // and a client that refuses what a leaving session asks would never
        // answer it.
        if asked.purpose == Pending::PushGone {
            return self.push_gone_answered();
        }
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
            _ => {}
        }
        // A part of a page reads its pictures against the page it stands on.
        let page = match asked.purpose {
            Pending::Region { screen, .. }
            | Pending::More { screen, .. }
            | Pending::Before { screen, .. }
            | Pending::Changes { screen, .. } => {
                self.view.screens.get(&screen).map(|view| view.url.clone())
            }
            _ => None,
        };
        let status = response.status;
        let (answer, past_a_cap) = decode::read_sized(
            response,
            &asked.url,
            page.as_deref(),
            &self.inner.origin,
            &self.config,
        );
        // A write the server took whose answer was too large to read.
        let saved_unread = (past_a_cap && status < 400).then_some(status);
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
            Pending::Page { screen, visit } => {
                self.page_answered(screen, visit, &asked, answer);
                // What its live parts heard while the page was on its way is
                // asked of the page now on the screen.
                self.live_owed(screen);
            }
            Pending::Send {
                screen,
                visit,
                key,
                write,
                in_place,
            } => self.send_answered(
                screen,
                visit,
                &key,
                (write, saved_unread.is_some()),
                in_place,
                &asked,
                answer,
            ),
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
        if self.inner.origin.where_of(asked) == self.inner.origin.where_of(url) {
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
                self.open_outside(&url, false, By::Answer("redirect"));
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
                // A snapshot waits for this launch's shell, drawn over the kept
                // one: no screen is opened in its place while the shell is asked.
                if self.inner.shell_kept
                    && self.inner.restore.is_some()
                    && self
                        .inner
                        .pending
                        .values()
                        .any(|asked| asked.purpose == Pending::Shell)
                {
                    return;
                }
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
        self.inner.size_said = false;
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
    use crate::{Config, Query};
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

    #[test]
    fn what_a_share_hands_the_system_and_fetches_is_a_whole_address_or_nothing() {
        let mut client = Client::new(Platform::Android, FormFactor::Phone);
        client.caps = vec!["share".to_owned()];
        let mut core = Core::new(Config::new("https://shop.example", client));
        core.send(Event::Start);
        core.send(Event::Response(Response {
            request: 1,
            status: 404,
            ..Response::default()
        }));
        let share = |core: &mut Core, call: serde_json::Value| {
            let action = serde_json::from_value(serde_json::json!({"t": "device", "call": call}))
                .expect("a device action");
            let update = core.send(Event::Act {
                screen: Some(1),
                action,
                form: None,
                overlay: None,
                text: None,
                now: None,
                ask: None,
                from: None,
            });
            update
                .effects
                .into_iter()
                .find_map(|effect| match effect {
                    Effect::Device { device, .. } => Some(device),
                    _ => None,
                })
                .expect("the share is started")
        };
        let device = share(
            &mut core,
            serde_json::json!({"f": "share", "title": "Order CS-20481", "href": "/orders/1?tab=items", "file": "/orders/1/invoice%202026.pdf", "url": "https://pay.example/r/7"}),
        );
        assert_eq!(device["href"], "https://shop.example/orders/1?tab=items");
        assert_eq!(
            device["file"],
            "https://shop.example/orders/1/invoice%202026.pdf"
        );
        // Another site's page is shared as it was written.
        assert_eq!(device["url"], "https://pay.example/r/7");
        // A path the core does not read as one of the app's addresses (white
        // space other than a plain space) is not handed over at all.
        let device = share(
            &mut core,
            serde_json::json!({"f": "share", "title": "Order", "href": "/orders/\u{3000}1", "file": "/orders/\u{3000}2"}),
        );
        assert!(
            device.get("href").is_none() && device.get("file").is_none(),
            "{device}"
        );
        assert_eq!(device["title"], "Order");
    }

    /// The address a passkey call has the client fetch is written whole too,
    /// and an address that is taken out of a device call is said.
    #[test]
    fn what_a_passkey_fetches_is_a_whole_address_or_nothing_and_a_dropped_one_is_said() {
        let mut client = Client::new(Platform::Android, FormFactor::Phone);
        client.caps = vec!["passkey".to_owned(), "share".to_owned()];
        let mut core = Core::new(Config::new("https://shop.example", client));
        core.send(Event::Start);
        core.send(Event::Response(Response {
            request: 1,
            status: 404,
            ..Response::default()
        }));
        let mut call = |call: serde_json::Value| {
            let action = serde_json::from_value(serde_json::json!({"t": "device", "call": call}))
                .expect("a device action");
            core.send(Event::Act {
                screen: Some(1),
                action,
                form: None,
                overlay: None,
                text: None,
                now: None,
                ask: None,
                from: None,
            })
            .effects
        };
        let device_of = |effects: &[Effect]| {
            effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Device { device, .. } => Some(device.clone()),
                    _ => None,
                })
                .expect("the call is started")
        };
        let said = |effects: &[Effect]| -> Vec<String> {
            effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Report { what, detail } if what == "device" => Some(detail.clone()),
                    _ => None,
                })
                .collect()
        };
        let effects = call(
            serde_json::json!({"f": "passkey", "mode": "sign_in", "options": "/auth/passkeys/options"}),
        );
        assert_eq!(
            device_of(&effects)["options"],
            "https://shop.example/auth/passkeys/options"
        );
        assert!(said(&effects).is_empty(), "{effects:?}");
        // A path the core does not read as one of the app's addresses is not
        // handed over, and that is said, with the name of what was dropped
        // and never the address.
        let effects = call(
            serde_json::json!({"f": "passkey", "mode": "sign_in", "options": "/auth/\u{3000}options"}),
        );
        assert!(device_of(&effects).get("options").is_none());
        let reports = said(&effects);
        assert_eq!(reports.len(), 1, "{effects:?}");
        assert!(
            reports[0].contains("options") && !reports[0].contains("auth"),
            "{reports:?}"
        );
        let effects = call(
            serde_json::json!({"f": "share", "title": "Order", "href": "/orders/\u{3000}1", "file": "/orders/\u{3000}2"}),
        );
        let reports = said(&effects);
        assert_eq!(reports.len(), 2, "{reports:?}");
        assert!(
            reports[0].contains("href") && reports[1].contains("file"),
            "{reports:?}"
        );
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

    /// An answer to a write that failed, or that is larger than any answer
    /// may be, still counts as a write for what is kept and for the other
    /// screens: the server may have written before it failed. A write that
    /// never left the device (no network) does not.
    #[test]
    fn a_write_answered_with_a_failure_counts_as_a_write_unless_it_never_left_the_device() {
        let page = r#"{"navree":1,"build":"b1","vocab":0,"screen":{"title":"Order","body":{"t":"form","id":"note","method":"post","action":"/orders/1/note","children":[{"t":"field","id":"text","name":"text","input":{"t":"text"}},{"t":"button","label":"Save","action":{"t":"submit"}}]}}}"#;
        let too_large = format!("{page}{}", " ".repeat(navree_native::MAX_BODY));
        let failures = [
            (
                "no network",
                false,
                Response {
                    error: Some(TransportError::Offline),
                    ..Response::default()
                },
            ),
            (
                "a timeout after the request was sent",
                true,
                Response {
                    error: Some(TransportError::Timeout),
                    ..Response::default()
                },
            ),
            (
                "a connection that failed in some other way",
                true,
                Response {
                    error: Some(TransportError::Other),
                    ..Response::default()
                },
            ),
            (
                "a 503",
                true,
                Response {
                    status: 503,
                    content_type: Some("text/html".to_owned()),
                    body: "<p>".to_owned(),
                    ..Response::default()
                },
            ),
            (
                "a 413: the server did not take the body",
                false,
                Response {
                    status: 413,
                    content_type: Some("application/json".to_owned()),
                    body: "{}".to_owned(),
                    ..Response::default()
                },
            ),
            (
                "a 429: a layer may answer in place of an answer that was lost",
                true,
                Response {
                    status: 429,
                    ..Response::default()
                },
            ),
            (
                "an answer too large",
                true,
                Response {
                    status: 200,
                    content_type: Some(CONTENT_TYPE.to_owned()),
                    body: too_large,
                    ..Response::default()
                },
            ),
        ];
        for (what, counts, failure) in failures {
            let mut core = started();
            core.send(Event::Response(Response {
                request: 2,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: page.to_owned(),
                ..Response::default()
            }));
            let epoch = core.inner.epoch;
            let sent = core.send(Event::Act {
                screen: Some(1),
                action: serde_json::from_value(serde_json::json!({"t": "submit"}))
                    .expect("an action"),
                form: Some("note".to_owned()),
                overlay: None,
                text: None,
                now: None,
                ask: None,
                from: None,
            });
            let request = sent
                .effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Fetch(request) if request.method == Verb::Post => Some(request.request),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{what}: the form is sent: {sent:?}"));
            core.send(Event::Response(Response { request, ..failure }));
            assert_eq!(
                core.inner.epoch > epoch,
                counts,
                "{what}: counted as a write, or not"
            );
        }
    }

    /// The one decision, status by status: whether an answer is the app's
    /// word on who is signed in, and whether the write it answers can have
    /// run, by whether it names a person and which hop it answers.
    #[test]
    fn what_an_answer_says_of_the_session_and_of_a_write_status_by_status() {
        let heard = |status: u16, names_someone: bool, first_hop: bool| Heard {
            error: None,
            status,
            names_someone,
            first_hop,
            carried_write: true,
            made_here: false,
        };
        // (status, the app's word when it names nobody, a write ran when its
        // own answer names nobody, a write ran when its own answer names her)
        let table = [
            (200, true, true, true),
            (204, true, true, true),
            (303, true, true, true),
            (400, true, true, false),
            (401, true, true, true),
            (402, true, true, false),
            (403, true, true, false),
            (404, true, true, false),
            (405, true, true, false),
            (406, true, true, false),
            (408, false, true, true),
            (409, true, true, false),
            (410, true, true, false),
            (411, true, true, true),
            (412, true, true, false),
            (413, false, false, true),
            (414, false, false, true),
            (415, true, true, false),
            (418, true, true, true),
            (419, true, true, true),
            (422, true, true, false),
            (428, true, true, false),
            (429, false, true, true),
            (431, false, false, true),
            (500, false, true, true),
            (502, false, true, true),
            (503, false, true, true),
            (504, false, true, true),
        ];
        for (status, speaks, ran_nobody, ran_her) in table {
            assert_eq!(
                heard(status, false, true).speaks_for_the_session(),
                speaks,
                "{status}"
            );
            assert!(
                heard(status, true, true).speaks_for_the_session(),
                "{status} naming a person"
            );
            assert_eq!(
                heard(status, false, true).write_may_have_run(),
                ran_nobody,
                "{status} naming nobody"
            );
            assert_eq!(
                heard(status, true, true).write_may_have_run(),
                ran_her,
                "{status} naming a person"
            );
            for names in [false, true] {
                assert!(
                    heard(status, names, false).write_may_have_run(),
                    "{status} after a redirect"
                );
                let read = Heard {
                    carried_write: false,
                    ..heard(status, names, true)
                };
                assert!(!read.write_may_have_run(), "{status} answering a read");
            }
        }
        let lost = |error, first_hop| Heard {
            error: Some(error),
            ..heard(0, false, first_hop)
        };
        assert!(!lost(TransportError::Offline, true).write_may_have_run());
        assert!(lost(TransportError::Offline, false).write_may_have_run());
        assert!(lost(TransportError::Timeout, true).write_may_have_run());
        assert!(lost(TransportError::Other, true).write_may_have_run());
        let never_sent = Heard {
            made_here: true,
            ..heard(502, false, true)
        };
        assert!(!never_sent.write_may_have_run());
    }

    /// A write counts as never made only when the server cannot have run it:
    /// on the answer to the request that carried it, with no redirect
    /// followed, one that names nobody and refuses it for its size (413, 414,
    /// 431), or one that names the person and refuses it as a handler does
    /// (a 422, a 409, ...), whatever its body. Any other answer may follow a
    /// write that ran: a 408 or a 429, a 413 that names the person, a 401,
    /// the page a redirect led to, and no network once a redirect was taken.
    #[test]
    fn a_write_counts_as_never_made_only_when_the_server_cannot_have_run_it() {
        let page = r#"{"navree":1,"build":"b1","vocab":0,"screen":{"title":"Order","body":{"t":"form","id":"note","method":"post","action":"/orders/1/note","children":[{"t":"field","id":"text","name":"text","input":{"t":"text"}},{"t":"button","label":"Save","action":{"t":"submit"}}]}}}"#;
        let her = || {
            vec![(
                "x-navree-person".to_owned(),
                "0193a4b2-0000-7000-8000-00000000ra01".to_owned(),
            )]
        };
        let json = |status: u16| Response {
            status,
            content_type: Some("application/json".to_owned()),
            body: "{}".to_owned(),
            ..Response::default()
        };
        let moved = Response {
            status: 303,
            location: Some("/orders/42".to_owned()),
            ..Response::default()
        };
        // The framework answers a native client's error in the native format.
        let native = |status: u16, names_her: bool| {
            Response {
            status,
            content_type: Some(CONTENT_TYPE.to_owned()),
            headers: if names_her { her() } else { Vec::new() },
            body: r#"{"navree":1,"build":"b1","vocab":0,"ops":[{"op":"toast","text":"That didn't work"}]}"#
                .to_owned(),
            ..Response::default()
        }
        };
        let cases: Vec<(&str, Vec<Response>, bool)> = vec![
            (
                "a 303 to the order, whose page is answered 429 in the native format",
                vec![moved.clone(), native(429, false)],
                true,
            ),
            (
                "a 303 to the order, whose page is answered 500 in the native format",
                vec![moved.clone(), native(500, false)],
                true,
            ),
            (
                "a 303 to the order, whose page is answered 403 in the native format",
                vec![moved.clone(), native(403, true)],
                true,
            ),
            ("a 429 in the native format", vec![native(429, false)], true),
            ("a 503 in the native format", vec![native(503, false)], true),
            (
                "a 413 that names her, in the native format",
                vec![native(413, true)],
                true,
            ),
            (
                "a 422 that names her, in the native format",
                vec![native(422, true)],
                false,
            ),
            (
                "a 422 that names nobody, in the native format",
                vec![native(422, false)],
                true,
            ),
            (
                "a 409 that names her, in the native format",
                vec![native(409, true)],
                false,
            ),
            (
                "a 401 that names her, in the native format",
                vec![native(401, true)],
                true,
            ),
            (
                "a 413 that names nobody, in the native format",
                vec![native(413, false)],
                false,
            ),
            (
                "a 303 to the order, whose page is answered 429",
                vec![moved.clone(), json(429)],
                true,
            ),
            (
                "a 303 to the order, whose page is answered 413",
                vec![moved.clone(), json(413)],
                true,
            ),
            (
                "a 303 to the order, then no network",
                vec![
                    moved,
                    Response {
                        error: Some(TransportError::Offline),
                        ..Response::default()
                    },
                ],
                true,
            ),
            ("a 429", vec![json(429)], true),
            ("a 408", vec![json(408)], true),
            (
                "no network on the request that carried it",
                vec![Response {
                    error: Some(TransportError::Offline),
                    ..Response::default()
                }],
                false,
            ),
            (
                "no network after a redirect the transport followed itself",
                vec![Response {
                    error: Some(TransportError::Offline),
                    url: "https://shop.example/orders/42".to_owned(),
                    ..Response::default()
                }],
                true,
            ),
            (
                "a 413 that names her",
                vec![Response {
                    headers: her(),
                    ..json(413)
                }],
                true,
            ),
            (
                "a 413 that names nobody, as a page",
                vec![Response {
                    status: 413,
                    content_type: Some("text/html".to_owned()),
                    body: "<h1>413 Request Entity Too Large</h1>".to_owned(),
                    ..Response::default()
                }],
                false,
            ),
            ("a 413 that names nobody", vec![json(413)], false),
            ("a 414 that names nobody", vec![json(414)], false),
            (
                "a 431 that names nobody, with nothing in it",
                vec![Response {
                    status: 431,
                    ..Response::default()
                }],
                false,
            ),
        ];
        for (what, answers, counts) in cases {
            let mut core = started();
            core.send(Event::Response(Response {
                request: 2,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: page.to_owned(),
                ..Response::default()
            }));
            let epoch = core.inner.epoch;
            let mut update = core.send(Event::Act {
                screen: Some(1),
                action: serde_json::from_value(serde_json::json!({"t": "submit"}))
                    .expect("an action"),
                form: Some("note".to_owned()),
                overlay: None,
                text: None,
                now: None,
                ask: None,
                from: None,
            });
            for answer in answers {
                let request = update
                    .effects
                    .iter()
                    .find_map(|effect| match effect {
                        Effect::Fetch(request) if !request.url.ends_with(SHELL_PATH) => {
                            Some(request.request)
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("{what}: a request to answer: {update:?}"));
                update = core.send(Event::Response(Response { request, ..answer }));
            }
            assert_eq!(
                core.inner.epoch > epoch,
                counts,
                "{what}: counted as a write, or not"
            );
        }
    }

    /// A move is a write. When its answer does not say whether the server
    /// moved the row (a timeout, a 5xx, a 429), it counts as a write and the
    /// page is asked for again, so the row stands where the server has it; it
    /// is not put back by guess. Only when the server cannot have run it (a
    /// 413 that names nobody, on the first hop) is the row simply put back.
    #[test]
    fn a_move_whose_answer_does_not_say_counts_as_a_write() {
        // The framework answers a native client's error in the native format.
        let native = |status: u16, names_her: bool| {
            Response {
            status,
            content_type: Some(CONTENT_TYPE.to_owned()),
            headers: if names_her {
                vec![(
                    "x-navree-person".to_owned(),
                    "0193a4b2-0000-7000-8000-00000000ra01".to_owned(),
                )]
            } else {
                Vec::new()
            },
            body: r#"{"navree":1,"build":"b1","vocab":0,"ops":[{"op":"toast","text":"That didn't work"}]}"#
                .to_owned(),
            ..Response::default()
        }
        };
        let json = |status: u16| Response {
            status,
            content_type: Some("application/json".to_owned()),
            body: "{}".to_owned(),
            ..Response::default()
        };
        let cases: Vec<(&str, Response, bool)> = vec![
            (
                "a timeout",
                Response {
                    error: Some(TransportError::Timeout),
                    ..Response::default()
                },
                true,
            ),
            ("a 503", json(503), true),
            ("a 429", json(429), true),
            ("a 413 that names nobody", json(413), false),
            ("a 503 in the native format", native(503, false), true),
            ("a 429 in the native format", native(429, false), true),
            (
                "a 422 that names her, in the native format",
                native(422, true),
                false,
            ),
        ];
        for (what, answer, counts) in cases {
            let mut core = started();
            core.send(Event::Response(Response {
                request: 2,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: include_str!("../transcripts/fixtures/made/packing.json").to_owned(),
                ..Response::default()
            }));
            let epoch = core.inner.epoch;
            let moved = core.send(Event::Move {
                screen: 1,
                item: "p-2".to_owned(),
                from: "to-pack".to_owned(),
                to: "packed".to_owned(),
                before: Some("p-7".to_owned()),
            });
            let request = moved
                .effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Fetch(request) if request.method == Verb::Post => Some(request.request),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{what}: the move is posted: {moved:?}"));
            let update = core.send(Event::Response(Response { request, ..answer }));
            let holder = core
                .view
                .screens
                .get(&1)
                .and_then(ScreenView::screen)
                .and_then(|screen| crate::lists::list_holding(screen, "p-2"));
            let asked_again = update.effects.iter().any(|effect| {
                matches!(effect, Effect::Fetch(request) if request.method == Verb::Get && request.url == "https://shop.example/")
            });
            assert_eq!(
                core.inner.epoch > epoch,
                counts,
                "{what}: counted as a write, or not"
            );
            assert_eq!(
                asked_again, counts,
                "{what}: the page is asked for again, or not: {update:?}"
            );
            let stands = if counts { "packed" } else { "to-pack" };
            assert_eq!(
                holder.as_deref(),
                Some(stands),
                "{what}: where the row stands"
            );
        }
    }

    /// A form sent with a wrong value and refused by the app's own handler
    /// (a 422 in the native format, naming the person, with the form's
    /// errors) is no write: what is kept and the other screens stay as they
    /// were, the errors are shown, and nothing fetched before it is thrown
    /// away.
    #[test]
    fn a_form_the_handler_refused_is_no_write() {
        let her = || {
            vec![(
                "x-navree-person".to_owned(),
                "0193a4b2-0000-7000-8000-00000000ra01".to_owned(),
            )]
        };
        let named = |request: u64, status: u16, body: &str| {
            Event::Response(Response {
                request,
                status,
                content_type: Some(CONTENT_TYPE.to_owned()),
                headers: her(),
                body: body.to_owned(),
                ..Response::default()
            })
        };
        let mut config = Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        );
        config.cache.persist = true;
        let mut core = Core::new(config);
        core.send(Event::Start);
        core.send(Event::Cache {
            key: "boot".to_owned(),
            value: None,
        });
        core.send(named(
            1,
            200,
            include_str!("../transcripts/fixtures/store/shell.json"),
        ));
        core.send(named(
            2,
            200,
            include_str!("../transcripts/fixtures/made/proof-upload.json"),
        ));
        core.send(Event::Select { destination: 1 });
        let orders = core.inner.next.request;
        core.send(named(
            orders,
            200,
            include_str!("../transcripts/fixtures/store/orders.json"),
        ));
        core.send(Event::Select { destination: 0 });
        let epoch = core.inner.epoch;
        let sent = core.send(Event::Act {
            screen: Some(1),
            action: serde_json::from_value(serde_json::json!({"t": "submit"})).expect("an action"),
            form: Some("proof".to_owned()),
            overlay: None,
            text: None,
            now: None,
            ask: None,
            from: None,
        });
        let request = sent
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Fetch(request) if request.method == Verb::Post => Some(request.request),
                _ => None,
            })
            .expect("the form is sent");
        let update = core.send(named(
            request,
            422,
            r#"{"navree":1,"build":"152bd84abf4d","vocab":0,"ops":[{"op":"errors","form":"proof","fields":{"note":["Write at least ten letters"]}}]}"#,
        ));
        assert_eq!(core.inner.epoch, epoch, "no write: {update:?}");
        assert!(
            !update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::CacheClear)),
            "what is kept stays: {update:?}"
        );
        let other = core
            .view
            .screens
            .keys()
            .copied()
            .find(|id| *id != 1)
            .expect("the orders screen");
        assert!(
            !core.inner.screens[&other].stale,
            "the other screen stays as it was"
        );
        let errors = &core.view.screens[&1].forms["proof"].fields["note"].errors;
        assert_eq!(
            errors,
            &["Write at least ten letters"],
            "the errors are shown"
        );
    }

    /// A write that worked and is answered with a page too large to read is
    /// saved: the person is told it was saved and that the screen could not
    /// be brought up to date, not that it failed, and it counts as a write. A
    /// write the server refused is a failure, whatever the size of its answer.
    #[test]
    fn a_write_that_worked_answered_too_large_to_read_says_it_was_saved() {
        let page = r#"{"navree":1,"build":"b1","vocab":0,"screen":{"title":"Order","body":{"t":"form","id":"note","method":"post","action":"/orders/1/note","children":[{"t":"field","id":"text","name":"text","input":{"t":"text"}},{"t":"button","label":"Save","action":{"t":"submit"}}]}}}"#;
        let too_large = format!("{page}{}", " ".repeat(navree_native::MAX_BODY));
        for (status, said) in [
            (200, "saved_not_refreshed"),
            (303, "saved_not_refreshed"),
            (422, "failed"),
        ] {
            let mut core = started();
            core.send(Event::Response(Response {
                request: 2,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: page.to_owned(),
                ..Response::default()
            }));
            let epoch = core.inner.epoch;
            let sent = core.send(Event::Act {
                screen: Some(1),
                action: serde_json::from_value(serde_json::json!({"t": "submit"}))
                    .expect("an action"),
                form: Some("note".to_owned()),
                overlay: None,
                text: None,
                now: None,
                ask: None,
                from: None,
            });
            let request = sent
                .effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Fetch(request) if request.method == Verb::Post => Some(request.request),
                    _ => None,
                })
                .expect("the form is sent");
            let update = core.send(Event::Response(Response {
                request,
                status,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: too_large.clone(),
                ..Response::default()
            }));
            let notices: Vec<serde_json::Value> = update
                .effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Message(message) => message.notice,
                    _ => None,
                })
                .map(|notice| serde_json::to_value(notice).expect("a notice writes"))
                .collect();
            assert_eq!(notices, [said], "{status}: {update:?}");
            assert!(core.inner.epoch > epoch, "{status}: counts as a write");
        }
    }

    /// Signing out with a push token: the update that asks for the token's
    /// removal says, after that request, that the session is ending; the
    /// session is forgotten when the removal is answered, as before. With no
    /// token nothing is said ahead of `forget_session`.
    #[test]
    fn a_sign_out_with_a_push_token_says_the_session_is_ending_after_the_removal() {
        let mut core = started();
        core.inner.push_route = Some("/api/notifications/push-devices".to_owned());
        core.inner.push.registered = Some("fcm-token-2604".to_owned());
        core.signed_out_on_purpose();
        let effects = std::mem::take(&mut core.effects);
        let removal = effects
            .iter()
            .position(
                |effect| matches!(effect, Effect::Fetch(request) if request.method == Verb::Delete),
            )
            .unwrap_or_else(|| panic!("the token's removal is asked for: {effects:?}"));
        let ending = effects
            .iter()
            .position(|effect| matches!(effect, Effect::SessionEnding))
            .unwrap_or_else(|| panic!("the session is said to be ending: {effects:?}"));
        assert!(removal < ending, "after the removal's request: {effects:?}");
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::ForgetSession)),
            "the session is forgotten only when the removal is answered: {effects:?}"
        );
        let Effect::Fetch(request) = &effects[removal] else {
            unreachable!()
        };
        let answered = core.send(Event::Response(Response {
            request: request.request,
            status: 204,
            ..Response::default()
        }));
        assert!(
            answered
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::ForgetSession)),
            "forget_session still follows: {answered:?}"
        );

        // With no token, nothing is said ahead of forgetting the session.
        let mut core = started();
        core.inner.push_route = Some("/api/notifications/push-devices".to_owned());
        core.signed_out_on_purpose();
        let effects = std::mem::take(&mut core.effects);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::SessionEnding))
                && effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::ForgetSession)),
            "{effects:?}"
        );
    }

    /// A core whose person signed out with a push token: it has asked for
    /// the token's removal, said the session is ending, and waits. Gives the
    /// core and the number of the removal's request.
    fn waiting_for_the_removal() -> (Core, u64) {
        let mut core = started();
        core.inner.push_route = Some("/api/notifications/push-devices".to_owned());
        core.inner.push.registered = Some("fcm-token-2604".to_owned());
        core.signed_out_on_purpose();
        let effects = std::mem::take(&mut core.effects);
        let removal = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Fetch(request) if request.method == Verb::Delete => Some(request.request),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the token's removal is asked for: {effects:?}"));
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::SessionEnding)),
            "the session is said to be ending: {effects:?}"
        );
        assert!(core.inner.signing_out, "the core waits for the removal");
        (core, removal)
    }

    /// Where the first effect `is` accepts stands among `effects`.
    fn place_of(effects: &[Effect], what: &str, is: impl Fn(&Effect) -> bool) -> usize {
        effects
            .iter()
            .position(is)
            .unwrap_or_else(|| panic!("{what}: {effects:?}"))
    }

    fn asks_for_the_shell(effect: &Effect) -> Option<u64> {
        match effect {
            Effect::Fetch(request) if request.url.ends_with(SHELL_PATH) => Some(request.request),
            _ => None,
        }
    }

    /// The client is told the session is ending, and then ends the session
    /// itself before the removal is answered (the person signs out in the
    /// client too, or another window's sign-out reaches this core). The wait
    /// is over with that, and the word that follows `session_ending` is
    /// still said, once: a client that began its sign-out at the first word
    /// is not left waiting for the second.
    #[test]
    fn a_client_that_ends_the_session_while_the_removal_is_awaited_is_still_told_to_forget_it() {
        let (mut core, removal) = waiting_for_the_removal();
        let update = core.send(Event::SignedOut);
        let effects = &update.effects;
        let dropped = place_of(
            effects,
            "the removal is no longer wanted",
            |effect| matches!(effect, Effect::Cancel { request } if *request == removal),
        );
        let forgotten = place_of(effects, "the session is forgotten", |effect| {
            matches!(effect, Effect::ForgetSession)
        });
        let shell = place_of(effects, "the shell is asked for", |effect| {
            asks_for_the_shell(effect).is_some()
        });
        assert!(
            dropped < forgotten && forgotten < shell,
            "the removal goes, then the word, then the start: {effects:?}"
        );
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(effect, Effect::ForgetSession))
                .count(),
            1,
            "said once: {effects:?}"
        );
        assert!(!core.inner.signing_out, "the wait is over");

        // The removal's answer, if it still comes, is nobody's.
        let late = core.send(Event::Response(Response {
            request: removal,
            status: 204,
            ..Response::default()
        }));
        assert!(
            !late.effects.iter().any(|effect| matches!(
                effect,
                Effect::ForgetSession | Effect::SessionEnding | Effect::Fetch(_)
            )),
            "an answer nobody waits for says nothing: {late:?}"
        );

        // With nothing awaited, a client that ends the session itself is told
        // nothing it did not ask for: it has forgotten the session already.
        let mut core = started();
        let update = core.send(Event::SignedOut);
        assert!(
            !update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::ForgetSession)),
            "{update:?}"
        );
        assert!(
            update
                .effects
                .iter()
                .any(|effect| asks_for_the_shell(effect).is_some()),
            "the app starts again: {update:?}"
        );
    }

    /// The removal of the push token is over with its first answer, whatever
    /// that is. A redirect of it is not followed: the next hop would be asked
    /// after the client was told the session is ending, so it would go
    /// without the session's cookies, or not at all from a client that
    /// refuses what a leaving session asks, and the core would wait for ever.
    #[test]
    fn a_redirect_of_the_removal_is_not_followed_and_ends_the_wait() {
        for status in [301, 302, 303, 307, 308] {
            let (mut core, removal) = waiting_for_the_removal();
            let answered = core.send(Event::Response(Response {
                request: removal,
                status,
                location: Some("/api/notifications/push-devices/".to_owned()),
                ..Response::default()
            }));
            let effects = &answered.effects;
            assert!(
                !effects.iter().any(|effect| matches!(
                    effect,
                    Effect::Fetch(request) if asks_for_the_shell(effect).is_none()
                        || request.method == Verb::Delete
                )),
                "{status}: nothing is asked but the shell: {effects:?}"
            );
            let forgotten = place_of(effects, "the session is forgotten", |effect| {
                matches!(effect, Effect::ForgetSession)
            });
            let shell = place_of(effects, "the shell is asked for", |effect| {
                asks_for_the_shell(effect).is_some()
            });
            assert!(forgotten < shell, "{status}: {effects:?}");
            assert!(!core.inner.signing_out, "{status}: the wait is over");
            assert!(
                !core
                    .inner
                    .pending
                    .values()
                    .any(|asked| asked.purpose == Pending::PushGone),
                "{status}: no removal is on its way"
            );
        }
    }

    /// What the core asks while it waits for the removal is asked in the
    /// session that is ending: a client may refuse it and never answer. The
    /// app coming to the front asks for a shell then. When the session is
    /// forgotten that request is dropped, and the shell is asked for anew
    /// after the word, so the app does not stay on its start screen.
    #[test]
    fn a_shell_asked_for_while_the_removal_is_awaited_is_asked_again_once_the_session_is_forgotten()
    {
        let (mut core, removal) = waiting_for_the_removal();
        let front = core.send(Event::Foreground);
        let meanwhile = front
            .effects
            .iter()
            .find_map(asks_for_the_shell)
            .unwrap_or_else(|| panic!("coming to the front asks for a shell: {front:?}"));

        // The client refused that request, and answers only the removal.
        let answered = core.send(Event::Response(Response {
            request: removal,
            status: 204,
            ..Response::default()
        }));
        let effects = &answered.effects;
        let dropped = place_of(
            effects,
            "the shell asked meanwhile is dropped",
            |effect| matches!(effect, Effect::Cancel { request } if *request == meanwhile),
        );
        let forgotten = place_of(effects, "the session is forgotten", |effect| {
            matches!(effect, Effect::ForgetSession)
        });
        let anew = place_of(effects, "the shell is asked for anew", |effect| {
            asks_for_the_shell(effect).is_some_and(|request| request != meanwhile)
        });
        assert!(
            dropped < forgotten && forgotten < anew,
            "dropped, then the word, then asked anew: {effects:?}"
        );
        let anew = asks_for_the_shell(&effects[anew]).expect("a shell request");

        // An answer to the one asked meanwhile, from a client that did send
        // it, is nobody's: only the one asked anew starts the app.
        let stale = core.send(Event::Response(Response {
            request: meanwhile,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            body: include_str!("../transcripts/fixtures/made/shell-app-nobody.json").to_owned(),
            ..Response::default()
        }));
        assert!(
            !stale
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Fetch(_))),
            "an answer nobody waits for asks nothing: {stale:?}"
        );
        assert_eq!(core.view.stage, Stage::Starting, "still starting");
        core.send(Event::Response(Response {
            request: anew,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            body: include_str!("../transcripts/fixtures/made/shell-app-nobody.json").to_owned(),
            ..Response::default()
        }));
        assert_eq!(core.view.stage, Stage::Ready, "the app starts");
    }

    /// Somebody signs in while the removal is still awaited: the client sent
    /// the shell that was asked for in the wait, it named nobody, and a
    /// sign-in followed within the removal's few seconds. The change of
    /// person does not take the removal away, so the wait still ends with
    /// its word; and what the person who signed in asked for is not dropped
    /// with the wait.
    #[test]
    fn a_sign_in_while_the_removal_is_awaited_leaves_the_wait_to_end_with_its_word() {
        const RAVNEET: &str = "0193a4b2-0000-7000-8000-00000000ra01";
        const HARPREET: &str = "0193a4b2-0000-7000-8000-00000000hs02";
        let hers = include_str!("../transcripts/fixtures/store/shell.json");
        let nobody = include_str!("../transcripts/fixtures/made/shell-nobody.json");
        let his = include_str!("../transcripts/fixtures/made/shell-other.json");
        // Ravneet is signed in, the server holds her device's token, and she signs out.
        let mut core = signed_in_as(Some(RAVNEET), hers);
        core.inner.push_route = Some("/api/notifications/push-devices".to_owned());
        core.inner.push.registered = Some("fcm-token-2604".to_owned());
        core.signed_out_on_purpose();
        let effects = std::mem::take(&mut core.effects);
        let removal = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Fetch(request) if request.method == Verb::Delete => Some(request.request),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the token's removal is asked for: {effects:?}"));
        // The app comes to the front: a shell is asked for, sent, and names nobody.
        let front = core.send(Event::Foreground);
        let asked = front
            .effects
            .iter()
            .find_map(asks_for_the_shell)
            .unwrap_or_else(|| panic!("coming to the front asks for a shell: {front:?}"));
        core.send(native(asked, nobody, None, false));
        // Harpreet signs in at once, and the shell asked for after it names him.
        let again = core.send(Event::Foreground);
        let asked = again
            .effects
            .iter()
            .find_map(asks_for_the_shell)
            .unwrap_or_else(|| panic!("the shell is asked for again: {again:?}"));
        let named = core.send(native(asked, his, Some(HARPREET), false));
        assert_eq!(
            core.view
                .app
                .as_ref()
                .and_then(|app| app.user.as_ref())
                .map(|user| user.name.as_str()),
            Some("Harpreet Singh")
        );
        assert!(
            !named
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Cancel { request } if *request == removal)),
            "the removal went with the person before: the wait would never end: {named:?}"
        );
        assert!(core.inner.signing_out, "the core still waits");
        let his_pages: Vec<u64> = core
            .inner
            .pending
            .iter()
            .filter(|(_, asked)| matches!(asked.purpose, Pending::Page { .. }))
            .map(|(request, _)| *request)
            .collect();
        assert!(!his_pages.is_empty(), "his first page is asked for");

        // The removal is answered: the word is said, once, and his page is still on its way.
        let answered = core.send(Event::Response(Response {
            request: removal,
            status: 204,
            ..Response::default()
        }));
        assert_eq!(
            answered
                .effects
                .iter()
                .filter(|effect| matches!(effect, Effect::ForgetSession))
                .count(),
            1,
            "{answered:?}"
        );
        assert!(!core.inner.signing_out, "the wait is over");
        assert!(
            his_pages
                .iter()
                .all(|request| core.inner.pending.contains_key(request)),
            "what he asked for went with the wait: {answered:?}"
        );
    }

    /// Signing out ends what a snapshot waited for, as signing out on purpose
    /// does: the person who comes back is told by the shell, and the snapshot
    /// of the one who left is nobody's.
    #[test]
    fn a_snapshot_that_waits_for_the_shell_is_dropped_when_the_person_signs_out() {
        let mut config = Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        );
        config.cache.persist = true;
        let mut core = Core::new(config);
        let waiting = serde_json::json!({
            "version": 1,
            "origin": "https://shop.example",
            "person": "1c54d473261f6f17cb6193202ce56588716e85757946078ac43d03b22f93a35e",
            "selected": "/orders",
            "destinations": [{"href": "/orders", "stack": [{"url": "/orders"}]}],
        });
        core.send(Event::Restore { snapshot: waiting });
        assert_eq!(
            core.snapshot().person.as_deref(),
            Some("1c54d473261f6f17cb6193202ce56588716e85757946078ac43d03b22f93a35e"),
            "the snapshot waits, and is what is given back"
        );
        core.send(Event::SignedOut);
        assert_eq!(core.snapshot().person, None);
        assert!(core.snapshot().destinations.is_empty());
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
        // Its kind, and never its path: a path can hold a token.
        assert_eq!(reports(&update), ["link: a path"]);
        let update = core.send(tap("https://user@evil.example/reset/s3cret?x=1"));
        assert_eq!(reports(&update), ["link: https://evil.example"]);
        // Nor a name or a password before the host.
        let update = core.send(tap("https://ravneet:s3cret@evil.example/x"));
        assert_eq!(reports(&update), ["link: https://evil.example"]);
        let update = core.send(tap("javascript:alert(1)"));
        assert_eq!(reports(&update), ["link: javascript:"]);
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
        assert_eq!(reports(&update), ["redirect: a path"]);
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

    /// In an app where no page allows keeping, opening pages writes nothing
    /// to the device but the shell.
    #[test]
    fn a_page_never_kept_is_never_written_or_taken_away() {
        let mut config = Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        );
        config.cache.persist = true;
        let mut core = Core::new(config);
        let mut written = Vec::new();
        let mut note = |update: Update| {
            written.extend(
                update
                    .effects
                    .into_iter()
                    .filter_map(|effect| match effect {
                        Effect::CacheWrite { key, .. } if key != "boot" => Some(key),
                        _ => None,
                    }),
            );
        };
        note(core.send(Event::Start));
        let page = |request: u64| {
            Event::Response(Response {
                request,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: include_str!("../transcripts/fixtures/store/orders.json").to_owned(),
                ..Response::default()
            })
        };
        // The first page answers before what was kept is read.
        note(core.send(Event::Response(Response {
            request: 1,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            body: include_str!("../transcripts/fixtures/store/shell.json").to_owned(),
            ..Response::default()
        })));
        note(core.send(page(2)));
        note(core.send(Event::Cache {
            key: "boot".to_owned(),
            value: None,
        }));
        note(core.send(Event::Select { destination: 1 }));
        note(core.send(page(3)));
        note(core.send(Event::Select { destination: 2 }));
        note(core.send(page(4)));
        assert!(written.is_empty(), "{written:?}");
    }

    #[test]
    fn a_shell_larger_than_any_answer_may_be_is_not_read() {
        let shell = include_str!("../transcripts/fixtures/store/shell.json");
        let new = || {
            Core::new(Config::new(
                "https://shop.example",
                Client::new(Platform::Android, FormFactor::Phone),
            ))
        };
        let with = |body: String| {
            let mut core = new();
            core.send(Event::Start);
            let update = core.send(Event::Response(Response {
                request: 1,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body,
                ..Response::default()
            }));
            (core, update)
        };
        let (core, _) = with(shell.to_owned());
        assert_eq!(core.view().destinations.len(), 3);

        // The same shell, padded past the cap with white space.
        let padded = format!("{shell}{}", " ".repeat(navree_native::MAX_BODY));
        let (core, update) = with(padded.clone());
        assert_eq!(core.view().destinations.len(), 1, "{:?}", core.view());
        assert!(
            update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "decode")),
            "{update:?}"
        );
        // A kept shell that large is not drawn either.
        assert!(new().shell_from_cache(shell));
        assert!(!new().shell_from_cache(&padded));
    }

    /// The shell kept on the device draws the app's name and its top-level
    /// destinations only, and gives way to the blank shell whenever this
    /// launch's shell request does not give a shell.
    #[test]
    fn the_kept_shell_draws_the_app_and_gives_way_when_this_launchs_shell_is_not_one() {
        let kept = include_str!("../transcripts/fixtures/made/kept-boot-workspace.json");
        let started = || {
            let mut config = Config::new(
                "https://shop.example",
                Client::new(Platform::Android, FormFactor::Phone),
            );
            config.cache.persist = true;
            let mut core = Core::new(config);
            core.send(Event::Start);
            core.send(Event::Cache {
                key: "boot".to_owned(),
                value: Some(kept.to_owned()),
            });
            core
        };
        let core = started();
        let view = core.view();
        let app = view.app.as_ref().expect("the kept shell is drawn");
        assert_eq!(app.name, "Charkha Supply Co.");
        assert_eq!((app.context.as_deref(), app.user.is_some()), (None, false));
        let labels: Vec<&str> = view
            .destinations
            .iter()
            .map(|destination| destination.label.as_str())
            .collect();
        assert_eq!(labels, ["Dashboard", "Orders"]);
        for destination in &view.destinations {
            assert!(destination.icon.is_some(), "{destination:?}");
            assert!(
                destination.badge.is_none()
                    && destination.group.is_none()
                    && destination.items.is_empty(),
                "{destination:?}"
            );
        }

        let shell_answer = |status: u16, content_type: &str, body: &str| Response {
            request: 1,
            status,
            content_type: Some(content_type.to_owned()),
            body: body.to_owned(),
            ..Response::default()
        };
        let answers = [
            ("a 404", shell_answer(404, "text/html", "<p>")),
            ("a 401", shell_answer(401, "text/html", "<p>")),
            (
                "an answer of another kind",
                shell_answer(200, "text/html", "<p>"),
            ),
            (
                "an answer that does not read",
                shell_answer(200, CONTENT_TYPE, r#"{"navree":1,"build":"#),
            ),
            (
                "a shell of another major",
                shell_answer(
                    200,
                    CONTENT_TYPE,
                    r#"{"navree":9,"build":"b","name":"x","home":"/"}"#,
                ),
            ),
        ];
        for (what, answer) in answers {
            let mut core = started();
            core.send(Event::Response(answer));
            let view = core.view();
            let app = view
                .app
                .as_ref()
                .unwrap_or_else(|| panic!("{what}: no app"));
            assert_eq!(
                (
                    app.name.as_str(),
                    app.context.as_deref(),
                    app.user.is_some()
                ),
                ("", None, false),
                "{what}"
            );
            assert_eq!(view.destinations.len(), 1, "{what}");
            let blank = &view.destinations[0];
            assert!(
                blank.label.is_empty() && blank.badge.is_none() && blank.items.is_empty(),
                "{what}: {blank:?}"
            );
        }
    }

    /// A shell request that did not get through (no network, a timeout, a
    /// 5xx, a 429) leaves the kept bars as they are and is asked for again,
    /// after a pause that doubles up to a minute.
    #[test]
    fn the_kept_shell_stays_and_the_shell_is_asked_for_again_when_the_request_did_not_get_through()
    {
        let kept = include_str!("../transcripts/fixtures/made/kept-boot-workspace.json");
        let started = || {
            let mut config = Config::new(
                "https://shop.example",
                Client::new(Platform::Android, FormFactor::Phone),
            );
            config.cache.persist = true;
            let mut core = Core::new(config);
            core.send(Event::Start);
            core.send(Event::Cache {
                key: "boot".to_owned(),
                value: Some(kept.to_owned()),
            });
            core
        };
        let timer_of = |update: &Update| {
            update.effects.iter().find_map(|effect| match effect {
                Effect::Timer { timer, ms } => Some((*timer, *ms)),
                _ => None,
            })
        };
        let shell_of = |update: &Update| {
            update.effects.iter().find_map(|effect| match effect {
                Effect::Fetch(request) if request.url.ends_with(SHELL_PATH) => {
                    Some(request.request)
                }
                _ => None,
            })
        };
        let failures = [
            (
                "a 503",
                Response {
                    status: 503,
                    content_type: Some("text/html".to_owned()),
                    body: "<p>".to_owned(),
                    ..Response::default()
                },
            ),
            (
                "a 429",
                Response {
                    status: 429,
                    ..Response::default()
                },
            ),
            (
                "no network",
                Response {
                    error: Some(TransportError::Offline),
                    ..Response::default()
                },
            ),
            (
                "a timeout",
                Response {
                    error: Some(TransportError::Timeout),
                    ..Response::default()
                },
            ),
        ];
        for (what, failure) in failures {
            let mut core = started();
            let update = core.send(Event::Response(Response {
                request: 1,
                ..failure
            }));
            let view = core.view();
            let app = view
                .app
                .as_ref()
                .unwrap_or_else(|| panic!("{what}: no app"));
            assert_eq!(app.name, "Charkha Supply Co.", "{what}: the kept bars stay");
            assert_eq!(view.destinations.len(), 2, "{what}");
            let (timer, ms) =
                timer_of(&update).unwrap_or_else(|| panic!("{what}: no pause: {update:?}"));
            assert_eq!(ms, 2_000, "{what}");
            // The pause over, the shell is asked for again.
            let again = core.send(Event::Timer { timer });
            assert!(shell_of(&again).is_some(), "{what}: {again:?}");
        }

        // The pause doubles, up to a minute.
        let mut core = started();
        let mut update = core.send(Event::Response(Response {
            request: 1,
            error: Some(TransportError::Offline),
            ..Response::default()
        }));
        let mut pauses = Vec::new();
        for _ in 0..8 {
            let (timer, ms) = timer_of(&update).expect("a pause");
            pauses.push(ms);
            let again = core.send(Event::Timer { timer });
            let request = shell_of(&again).expect("the shell is asked for");
            update = core.send(Event::Response(Response {
                request,
                error: Some(TransportError::Timeout),
                ..Response::default()
            }));
        }
        assert_eq!(
            pauses,
            [2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000, 60_000]
        );
    }

    /// Whichever comes first, the shell request's failure or the kept shell
    /// being read, the end is the same: the kept bars, and the shell asked for
    /// again. A request that fails with no network fails in milliseconds, long
    /// before the file the kept shell is read from.
    #[test]
    fn a_shell_request_that_failed_before_the_kept_shell_was_read_is_asked_for_again() {
        let kept = include_str!("../transcripts/fixtures/made/kept-boot-workspace.json");
        let failures = [
            (
                "no network",
                Response {
                    error: Some(TransportError::Offline),
                    ..Response::default()
                },
            ),
            (
                "a 503",
                Response {
                    status: 503,
                    content_type: Some("text/html".to_owned()),
                    ..Response::default()
                },
            ),
        ];
        for (what, failure) in failures {
            let mut config = Config::new(
                "https://shop.example",
                Client::new(Platform::Android, FormFactor::Phone),
            );
            config.cache.persist = true;
            let mut core = Core::new(config);
            core.send(Event::Start);
            core.send(Event::Response(Response {
                request: 1,
                ..failure
            }));
            let read = core.send(Event::Cache {
                key: "boot".to_owned(),
                value: Some(kept.to_owned()),
            });
            let view = core.view();
            assert_eq!(view.stage, crate::Stage::Ready, "{what}");
            assert_eq!(
                view.app.as_ref().map(|app| app.name.as_str()),
                Some("Charkha Supply Co."),
                "{what}: the kept bars"
            );
            let timer = read
                .effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Timer { timer, ms: 2_000 } => Some(*timer),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!("{what}: no pause after the kept shell was read: {read:?}")
                });
            // The pause over, the shell is asked for again.
            let again = core.send(Event::Timer { timer });
            assert!(
                again.effects.iter().any(|effect| matches!(
                    effect,
                    Effect::Fetch(request) if request.url.ends_with(SHELL_PATH)
                )),
                "{what}: {again:?}"
            );
        }
    }

    /// Whichever comes first, a shell answer that is no shell or the kept
    /// shell being read, the end is the same: the blank shell. A shell of
    /// another major, or one that redirects to another site, before the kept
    /// shell is read leaves the kept shell undrawn.
    #[test]
    fn a_shell_that_is_not_one_before_the_kept_shell_is_read_gives_the_blank_shell() {
        let kept = include_str!("../transcripts/fixtures/made/kept-boot-workspace.json");
        let answers = [
            (
                "a shell of another major",
                Response {
                    status: 200,
                    content_type: Some(CONTENT_TYPE.to_owned()),
                    body: r#"{"navree":9,"build":"b","name":"x","home":"/"}"#.to_owned(),
                    ..Response::default()
                },
            ),
            (
                "a redirect to another site",
                Response {
                    status: 302,
                    location: Some("https://elsewhere.example/shell".to_owned()),
                    ..Response::default()
                },
            ),
            (
                "a 404",
                Response {
                    status: 404,
                    content_type: Some("text/html".to_owned()),
                    body: "<p>".to_owned(),
                    ..Response::default()
                },
            ),
        ];
        for (what, answer) in answers {
            let mut config = Config::new(
                "https://shop.example",
                Client::new(Platform::Android, FormFactor::Phone),
            );
            config.cache.persist = true;
            let mut core = Core::new(config);
            core.send(Event::Start);
            core.send(Event::Response(Response {
                request: 1,
                ..answer
            }));
            core.send(Event::Cache {
                key: "boot".to_owned(),
                value: Some(kept.to_owned()),
            });
            let view = core.view();
            assert_eq!(view.stage, crate::Stage::Ready, "{what}");
            let app = view
                .app
                .as_ref()
                .unwrap_or_else(|| panic!("{what}: no app"));
            assert_eq!(app.name, "", "{what}: the kept shell is not drawn");
            assert_eq!(view.destinations.len(), 1, "{what}");
            assert!(view.destinations[0].label.is_empty(), "{what}");
        }
    }

    /// A Windows app signs in through the browser by the hand-over: the
    /// session starts with a `state` and an S256 challenge, and its return is
    /// taken only while that sign-in is open, with its `state`, on the address
    /// the sign-in named (the app's scheme). Anything else is neither
    /// exchanged nor opened as a page. Without the hand-over's routes the
    /// browser is not used at all: its cookies are not the app's, so nothing
    /// that comes back from it is opened with the app's.
    #[test]
    fn a_windows_app_signs_in_through_the_browser_by_the_hand_over_alone() {
        let windows = |sign_in: serde_json::Value| -> Core {
            let config = serde_json::json!({
                "origin": "https://shop.example",
                "app_id": "CharkhaSupply_8wekyb3d8bbwe",
                "client": {"platform": "windows", "form": "desktop", "vocab": 0, "text_percent": 100},
                "sign_in": sign_in,
            });
            Core::new(serde_json::from_value(config).expect("a configuration"))
        };
        let native = |request: u64, body: &str| {
            Event::Response(Response {
                request,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: body.to_owned(),
                ..Response::default()
            })
        };
        let fetches = |update: &Update| -> Vec<String> {
            update
                .effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Fetch(request) => Some(request.url.clone()),
                    _ => None,
                })
                .collect()
        };
        let sign_in = |update: &Update| {
            update.effects.iter().find_map(|effect| match effect {
                Effect::SignIn { url, callback, .. } => Some((url.clone(), callback.clone())),
                _ => None,
            })
        };
        let tap = |href: &str| Event::Act {
            screen: Some(1),
            action: serde_json::from_value(serde_json::json!({"t": "navigate", "href": href}))
                .expect("an action"),
            form: None,
            overlay: None,
            text: None,
            now: None,
            ask: None,
            from: None,
        };

        // With the hand-over's routes.
        let mut core = windows(serde_json::json!({"callback": "charkha", "paths": ["/sign-in"]}));
        core.send(Event::Start);
        core.send(native(
            1,
            include_str!("../transcripts/fixtures/made/shell-app-nobody-windows.json"),
        ));
        core.send(native(
            2,
            include_str!("../transcripts/fixtures/made/welcome.json"),
        ));
        let asked = core.send(tap("/sign-in"));
        let request = asked
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Random { request, .. } => Some(*request),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the hand-over asks for random bytes: {asked:?}"));
        let opened = core.send(Event::Random {
            request,
            bytes: "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f30".to_owned(),
        });
        let (url, callback) = sign_in(&opened).expect("the session opens");
        assert_eq!(
            url,
            "https://shop.example/auth/app/sign-in?platform=windows&app=CharkhaSupply_8wekyb3d8bbwe&code_challenge=658WgAyQKf_KhWlXY9I8Os5xARz0DpNUrNgQIF4lD4c&code_challenge_method=S256&state=ISIjJCUmJygpKissLS4vMA"
        );
        assert_eq!(callback.as_deref(), Some("charkha"));
        let state = "ISIjJCUmJygpKissLS4vMA";
        for (what, planted) in [
            (
                "another state",
                "charkha://signed-in?code=PLANTED000&state=not-the-state".to_owned(),
            ),
            ("no state", "charkha://signed-in?code=PLANTED000".to_owned()),
            (
                "another way back than the sign-in named",
                format!("https://shop.example/signed-in?code=PLANTED000&state={state}"),
            ),
        ] {
            let update = core.send(Event::SignInDone { url: Some(planted) });
            assert!(
                fetches(&update).is_empty(),
                "{what}: nothing is exchanged or opened: {update:?}"
            );
        }
        let back = core.send(Event::SignInDone {
            url: Some(format!("charkha://signed-in?code=Tq7uX0b2c9&state={state}")),
        });
        let exchange = back
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Fetch(request) if request.url.ends_with("/api/auth/app/exchange") => {
                    request.body.clone()
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the code is exchanged: {back:?}"));
        assert!(
            matches!(&exchange, Body::Json { text } if text.contains(r#""platform":"windows""#) && text.contains("CharkhaSupply_8wekyb3d8bbwe")),
            "{exchange:?}"
        );
        // With no sign-in open, a link on the scheme is an ordinary link: one
        // to a place the shell does not know is not opened, and nothing is
        // exchanged.
        for planted in [
            format!("charkha://signed-in?code=PLANTED000&state={state}"),
            "charkha://signed-in?code=PLANTED000".to_owned(),
        ] {
            for event in [
                Event::OpenLink {
                    url: planted.clone(),
                },
                Event::SignInDone {
                    url: Some(planted.clone()),
                },
            ] {
                let update = core.send(event);
                assert!(
                    !fetches(&update).iter().any(|url| url.contains("signed-in")),
                    "{planted}: not opened: {update:?}"
                );
            }
        }

        // Without them, the browser is not used, and what comes back from it is not opened.
        let mut core = windows(serde_json::json!({"callback": "charkha", "paths": ["/auth/*"]}));
        core.send(Event::Start);
        core.send(native(
            1,
            include_str!("../transcripts/fixtures/store/shell.json"),
        ));
        core.send(native(
            2,
            include_str!("../transcripts/fixtures/store/dashboard.json"),
        ));
        let tapped = core.send(tap("/auth/google"));
        assert!(sign_in(&tapped).is_none(), "{tapped:?}");
        let back = core.send(Event::SignInDone {
            url: Some("charkha://signed-in?token=PLANTED000".to_owned()),
        });
        assert!(fetches(&back).is_empty(), "{back:?}");
    }

    /// A link is a sign-in's return only for a sign-in that is open, on the
    /// way back that sign-in named and with its `state`. With no callback in
    /// the shell and no sign-in open, a link on the app's scheme that carries
    /// a `code`, a `state` or an `error` of its own is the ordinary link it
    /// is; with one open, a link with another `state`, or on another path
    /// than the callback, is too.
    #[test]
    fn a_link_is_a_sign_ins_return_only_for_the_sign_in_that_is_open() {
        let state = "ISIjJCUmJygpKissLS4vMA";
        let bytes = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f30";
        let core_on = |platform: &str, shell: &str, open: bool| -> Core {
            let config = serde_json::json!({
                "origin": "https://shop.example",
                "app_id": "com.charkha.supply",
                "client": {"platform": platform, "form": "phone", "vocab": 0, "text_percent": 100},
                "sign_in": {"callback": "charkha", "paths": ["/sign-in"]},
            });
            let mut core = Core::new(serde_json::from_value(config).expect("a configuration"));
            core.send(Event::Start);
            core.send(Event::Response(Response {
                request: 1,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: shell.to_owned(),
                ..Response::default()
            }));
            core.send(Event::Response(Response {
                request: 2,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: include_str!("../transcripts/fixtures/made/welcome.json").to_owned(),
                ..Response::default()
            }));
            if open {
                core.send(Event::Act {
                    screen: Some(1),
                    action: serde_json::from_value(
                        serde_json::json!({"t": "navigate", "href": "/sign-in"}),
                    )
                    .expect("an action"),
                    form: None,
                    overlay: None,
                    text: None,
                    now: None,
                    ask: None,
                    from: None,
                });
                core.send(Event::Random {
                    request: 1,
                    bytes: bytes.to_owned(),
                });
            }
            core
        };
        let asked = |update: &Update| -> Vec<String> {
            update
                .effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Fetch(request) => Some(request.url.clone()),
                    _ => None,
                })
                .collect()
        };
        let no_callback =
            include_str!("../transcripts/fixtures/made/shell-app-nobody-windows.json");
        let callback = include_str!("../transcripts/fixtures/made/shell-app-nobody.json");

        // No sign-in open, no callback named: each is the ordinary link it is.
        for link in [
            "charkha://orders/7?code=DIWALI24",
            "charkha://orders/7?state=open",
            "charkha://orders/7?error=none",
        ] {
            let mut core = core_on("windows", no_callback, false);
            let update = core.send(Event::OpenLink {
                url: link.to_owned(),
            });
            let path = link.trim_start_matches("charkha:/");
            assert!(
                asked(&update)
                    .iter()
                    .any(|url| url == &format!("https://shop.example{path}")),
                "{link} is opened: {update:?}"
            );
        }

        // A sign-in open, no callback named: only this sign-in's state is its return.
        let mut core = core_on("windows", no_callback, true);
        let update = core.send(Event::OpenLink {
            url: "charkha://orders/7?state=open".to_owned(),
        });
        assert!(
            asked(&update)
                .iter()
                .any(|url| url == "https://shop.example/orders/7?state=open"),
            "another state is an ordinary link: {update:?}"
        );
        let update = core.send(Event::OpenLink {
            url: format!("charkha://signed-in?code=Tq7uX0b2c9&state={state}"),
        });
        assert!(
            asked(&update)
                .iter()
                .any(|url| url.ends_with("/api/auth/app/exchange")),
            "this sign-in's return is exchanged: {update:?}"
        );

        // A sign-in open, a callback named: a look-alike on another path is an ordinary link.
        let mut core = core_on("ios", callback, true);
        let update = core.send(Event::OpenLink {
            url: format!("https://shop.example/orders/7?code=Tq7uX0b2c9&state={state}"),
        });
        let urls = asked(&update);
        assert!(
            urls.iter()
                .any(|url| url.starts_with("https://shop.example/orders/7"))
                && !urls.iter().any(|url| url.ends_with("/exchange")),
            "{update:?}"
        );
    }

    /// With no callback in the shell, a link on the app's scheme that carries
    /// both a `code` and a `state` is a sign-in's return whether a sign-in is
    /// open or not: the open sign-in takes it when the `state` is its own,
    /// and otherwise it is dropped and said, never opened as a page. That is
    /// the return of a sign-in another took the place of, one that comes
    /// twice, and one that comes after its sign-in ended.
    #[test]
    fn a_return_of_a_sign_in_that_is_over_is_never_a_page() {
        let first = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f30";
        let second = "65666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f808182838485868788898a8b8c8d8e8f9091929394";
        let (state_one, state_two) = ("ISIjJCUmJygpKissLS4vMA", "hYaHiImKi4yNjo-QkZKTlA");
        let config = serde_json::json!({
            "origin": "https://shop.example",
            "app_id": "CharkhaSupply_8wekyb3d8bbwe",
            "client": {"platform": "windows", "form": "desktop", "vocab": 0, "text_percent": 100},
            "sign_in": {"callback": "charkha", "paths": ["/sign-in"]},
            "deep_links": "origin",
        });
        let mut core = Core::new(serde_json::from_value(config).expect("a configuration"));
        core.send(Event::Start);
        for (request, body) in [
            (
                1,
                include_str!("../transcripts/fixtures/made/shell-app-nobody-windows.json"),
            ),
            (2, include_str!("../transcripts/fixtures/made/welcome.json")),
        ] {
            core.send(Event::Response(Response {
                request,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: body.to_owned(),
                ..Response::default()
            }));
        }
        let begin = |core: &mut Core, random: u64, bytes: &str| {
            core.send(Event::Act {
                screen: Some(1),
                action: serde_json::from_value(
                    serde_json::json!({"t": "navigate", "href": "/sign-in"}),
                )
                .expect("an action"),
                form: None,
                overlay: None,
                text: None,
                now: None,
                ask: None,
                from: None,
            });
            core.send(Event::Random {
                request: random,
                bytes: bytes.to_owned(),
            });
        };
        let link = |core: &mut Core, url: String| core.send(Event::OpenLink { url });
        let fetched = |update: &Update| -> Vec<String> {
            update
                .effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Fetch(request) => Some(request.url.clone()),
                    _ => None,
                })
                .collect()
        };
        let said = |update: &Update| {
            update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "sign_in"))
        };
        // Two taps: the second sign-in takes the place of the first.
        begin(&mut core, 1, first);
        begin(&mut core, 2, second);
        let replaced = link(
            &mut core,
            format!("charkha://signed-in?code=OLD0000001&state={state_one}"),
        );
        assert!(
            fetched(&replaced).is_empty() && said(&replaced),
            "the replaced one's return is dropped and said: {replaced:?}"
        );
        // The second one's return is taken, and comes again while it is exchanged.
        let taken = link(
            &mut core,
            format!("charkha://signed-in?code=NEW0000002&state={state_two}"),
        );
        assert!(
            fetched(&taken).iter().any(|url| url.ends_with("/exchange")),
            "{taken:?}"
        );
        let twice = link(
            &mut core,
            format!("charkha://signed-in?code=NEW0000002&state={state_two}"),
        );
        assert!(
            fetched(&twice).is_empty() && said(&twice),
            "the duplicate is dropped and said: {twice:?}"
        );
        // After the sign-in ended.
        let exchange = core.inner.next.request;
        core.send(Event::Response(Response {
            request: exchange,
            status: 400,
            ..Response::default()
        }));
        let late = link(
            &mut core,
            format!("charkha://signed-in?code=NEW0000002&state={state_two}"),
        );
        assert!(
            fetched(&late).is_empty() && said(&late),
            "a late one is dropped and said: {late:?}"
        );
        // A link with a code alone, a state alone or an error alone is an ordinary link.
        for url in [
            "charkha://orders/7?code=DIWALI24",
            "charkha://orders/7?state=open",
            "charkha://orders/7?error=none",
        ] {
            let update = link(&mut core, url.to_owned());
            assert!(
                fetched(&update)
                    .iter()
                    .any(|asked| asked.starts_with("https://shop.example/orders/7")),
                "{url} is opened: {update:?}"
            );
        }
    }

    /// A link that arrives from outside is followed as a link on the web is:
    /// it is only ever a read, a `get` of a page, never a write and never a
    /// form sent. An address on the hand-over's own way back is never opened
    /// as a page, whether or not a sign-in is open: the open sign-in takes it
    /// when it carries its `state`, and otherwise it is dropped.
    #[test]
    fn a_link_from_outside_is_only_a_read_and_the_way_back_is_never_a_page() {
        let state = "ISIjJCUmJygpKissLS4vMA";
        let core_on = |platform: &str, shell: &str| -> Core {
            let config = serde_json::json!({
                "origin": "https://shop.example",
                "app_id": "com.charkha.supply",
                "client": {"platform": platform, "form": "phone", "vocab": 0, "text_percent": 100},
                "sign_in": {"callback": "charkha", "paths": ["/sign-in"]},
                "deep_links": "origin",
            });
            let mut core = Core::new(serde_json::from_value(config).expect("a configuration"));
            core.send(Event::Start);
            core.send(Event::Response(Response {
                request: 1,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: shell.to_owned(),
                ..Response::default()
            }));
            core.send(Event::Response(Response {
                request: 2,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: include_str!("../transcripts/fixtures/made/welcome.json").to_owned(),
                ..Response::default()
            }));
            core
        };
        let link = |core: &mut Core, url: &str| {
            core.send(Event::OpenLink {
                url: url.to_owned(),
            })
        };
        let fetches = |update: &Update| -> Vec<Request> {
            update
                .effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Fetch(request) => Some(request.clone()),
                    _ => None,
                })
                .collect()
        };
        let windows = include_str!("../transcripts/fixtures/made/shell-app-nobody-windows.json");
        let apple = include_str!("../transcripts/fixtures/made/shell-app-nobody.json");

        // Only ever a read, whatever the link names.
        let mut reads = 0;
        for (platform, shell) in [("windows", windows), ("ios", apple)] {
            for url in [
                "charkha://orders/7?intent=cancel",
                "charkha://orders/new?item=lota&qty=2",
                "charkha://signed-in?token=T0K3N",
                "https://shop.example/orders/7/cancel?confirm=1",
                "https://shop.example/reports",
                "https://shop.example/sign-out",
            ] {
                let mut core = core_on(platform, shell);
                let update = link(&mut core, url);
                for request in fetches(&update) {
                    assert!(
                        request.method == Verb::Get && request.body.is_none() && !request.xsrf,
                        "{platform} {url}: only a read: {request:?}"
                    );
                    reads += 1;
                }
            }
        }
        assert!(reads >= 8, "the links were followed: {reads}");

        // The way back the shell names is never a page.
        let callback = "https://shop.example/app/signed-in";
        let mut core = core_on("ios", apple);
        for url in [
            format!("{callback}?code=Tq7uX0b2c9&state={state}"),
            format!("{callback}/?token=T0K3N"),
            callback.to_owned(),
        ] {
            let update = link(&mut core, &url);
            assert!(
                fetches(&update).is_empty(),
                "with no sign-in open, {url} is dropped: {update:?}"
            );
        }
        core.send(Event::Act {
            screen: Some(1),
            action: serde_json::from_value(
                serde_json::json!({"t": "navigate", "href": "/sign-in"}),
            )
            .expect("an action"),
            form: None,
            overlay: None,
            text: None,
            now: None,
            ask: None,
            from: None,
        });
        core.send(Event::Random {
            request: 1,
            bytes: "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f30".to_owned(),
        });
        let update = link(
            &mut core,
            &format!("{callback}?code=PLANTED000&state=not-the-state"),
        );
        assert!(
            fetches(&update).is_empty(),
            "with a sign-in open, another state is dropped: {update:?}"
        );
        let update = link(
            &mut core,
            &format!("{callback}?code=Tq7uX0b2c9&state={state}"),
        );
        let taken = fetches(&update);
        assert!(
            taken.len() == 1 && taken[0].url.ends_with("/api/auth/app/exchange"),
            "the open sign-in takes its own: {update:?}"
        );
    }

    /// The retry keeps its pace: a request that got through asks for the shell
    /// again at most once in each pause, and only while the shell's own last
    /// failure was the network's or a timeout; the pause is not kept while
    /// the app is away, and an upgrade ends it.
    #[test]
    fn the_shell_retry_keeps_its_pace() {
        let kept = include_str!("../transcripts/fixtures/made/kept-boot-workspace.json");
        let started = |failure: Response| {
            let mut config = Config::new(
                "https://shop.example",
                Client::new(Platform::Android, FormFactor::Phone),
            );
            config.cache.persist = true;
            let mut core = Core::new(config);
            core.send(Event::Start);
            core.send(Event::Cache {
                key: "boot".to_owned(),
                value: Some(kept.to_owned()),
            });
            let failed = core.send(Event::Response(Response {
                request: 1,
                ..failure
            }));
            (core, failed)
        };
        let fetches = |update: &Update| -> Vec<(u64, bool)> {
            update
                .effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::Fetch(request) => {
                        Some((request.request, request.url.ends_with(SHELL_PATH)))
                    }
                    _ => None,
                })
                .collect()
        };
        let unreached = |request: u64| Response {
            request,
            error: Some(TransportError::Offline),
            ..Response::default()
        };
        let overloaded = |request: u64| Response {
            request,
            status: 503,
            content_type: Some("text/html".to_owned()),
            ..Response::default()
        };
        // A request of the app that got through: the shells it asks for.
        let got_through = |core: &mut Core, no_shell: &dyn Fn(u64) -> Response| -> usize {
            core.fetch(
                Pending::Send {
                    screen: 1,
                    visit: 0,
                    key: "search".to_owned(),
                    write: false,
                    in_place: true,
                },
                Method::Get,
                "/orders",
                None,
                PAGE_LIMIT,
            );
            let request = core.inner.next.request;
            core.settle();
            let update = core.send(Event::Response(Response {
                request,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                body: include_str!("../transcripts/fixtures/store/orders.json").to_owned(),
                ..Response::default()
            }));
            let asked: Vec<u64> = fetches(&update)
                .into_iter()
                .filter(|(_, shell)| *shell)
                .map(|(request, _)| request)
                .collect();
            for request in &asked {
                core.send(Event::Response(no_shell(*request)));
            }
            asked.len()
        };
        let timer_of = |update: &Update| {
            update.effects.iter().find_map(|effect| match effect {
                Effect::Timer { timer, .. } => Some(*timer),
                _ => None,
            })
        };

        // The network: a request that got through asks for the shell once in
        // the pause, and not again until the pause is over.
        let (mut core, failed) = started(Response {
            error: Some(TransportError::Offline),
            ..Response::default()
        });
        let timer = timer_of(&failed).expect("a pause");
        assert_eq!(got_through(&mut core, &unreached), 1, "the first asks");
        assert_eq!(got_through(&mut core, &unreached), 0, "once in the pause");
        assert_eq!(got_through(&mut core, &unreached), 0, "once in the pause");
        // The pause is over: the shell is asked for by the timer, fails, and
        // the next pause has one more request's worth of asking.
        let ended = core.send(Event::Timer { timer });
        let (request, _) = fetches(&ended)
            .into_iter()
            .find(|(_, shell)| *shell)
            .expect("the timer asks");
        let failed = core.send(Event::Response(unreached(request)));
        assert!(timer_of(&failed).is_some(), "the next pause");
        assert_eq!(
            got_through(&mut core, &unreached),
            1,
            "the next pause's one"
        );

        // The shell route itself answers with an error: only the timer asks.
        let (mut core, _) = started(Response {
            status: 503,
            content_type: Some("text/html".to_owned()),
            ..Response::default()
        });
        for _ in 0..3 {
            assert_eq!(
                got_through(&mut core, &overloaded),
                0,
                "a shell that answers 503 is not asked for by every request that got through"
            );
        }

        // The app goes away: the pause is called off. It comes back: the
        // shell is asked for, and the pause starts again if that fails.
        let (mut core, failed) = started(Response {
            error: Some(TransportError::Offline),
            ..Response::default()
        });
        let timer = failed
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Timer { timer, .. } => Some(*timer),
                _ => None,
            })
            .expect("a pause");
        let away = core.send(Event::Background);
        assert!(
            away.effects.iter().any(
                |effect| matches!(effect, Effect::CancelTimer { timer: gone } if *gone == timer)
            ),
            "{away:?}"
        );
        let back = core.send(Event::Foreground);
        let (request, _) = *fetches(&back)
            .iter()
            .find(|(_, shell)| *shell)
            .expect("the shell is asked for");
        let again = core.send(Event::Response(unreached(request)));
        assert!(
            again
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Timer { .. })),
            "{again:?}"
        );

        // An upgrade ends the wanting: the pause is called off.
        let (mut core, failed) = started(Response {
            error: Some(TransportError::Offline),
            ..Response::default()
        });
        let timer = failed
            .effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Timer { timer, .. } => Some(*timer),
                _ => None,
            })
            .expect("a pause");
        let asked = core.send(Event::Timer { timer });
        let (request, _) = *fetches(&asked)
            .iter()
            .find(|(_, shell)| *shell)
            .expect("the shell is asked for");
        let upgrade = core.send(Event::Response(Response {
            request,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            body: r#"{"upgrade":{"min_app":"9.0.0","url":"https://shop.example/app"}}"#.to_owned(),
            ..Response::default()
        }));
        assert!(
            !core.inner.shell_wanted && core.inner.shell_timer.is_none(),
            "an upgrade ends the wanting: {upgrade:?}"
        );
    }

    /// A place is not put back over what the person began: the first page,
    /// asked for while the shell could not be reached, stays when words were
    /// typed into it that differ from what it gave, or a send from it is on
    /// its way or waiting; the snapshot is dropped, with a report. A page
    /// they have only looked at gives way to the place as before.
    #[test]
    fn a_snapshot_is_not_put_back_over_what_the_person_began() {
        let snapshot = serde_json::json!({
            "version": 1,
            "origin": "https://shop.example",
            "person": "1c54d473261f6f17cb6193202ce56588716e85757946078ac43d03b22f93a35e",
            "selected": "/",
            "destinations": [{"href": "/", "stack": [{"url": "/"}, {"url": "/orders/1/pay"}]}],
        });
        let answered = |body: &str, request: u64| Response {
            request,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            headers: vec![(
                "x-navree-person".to_owned(),
                "0193a4b2-0000-7000-8000-00000000ra01".to_owned(),
            )],
            body: body.to_owned(),
            ..Response::default()
        };
        let submit = || Event::Act {
            screen: Some(1),
            action: serde_json::from_value(serde_json::json!({"t": "submit"})).expect("an action"),
            form: Some("memo".to_owned()),
            overlay: None,
            text: None,
            now: None,
            ask: None,
            from: None,
        };
        let typed = |text: &str| Event::Field {
            screen: 1,
            form: Some("memo".to_owned()),
            field: "memo-text".to_owned(),
            value: crate::FieldValue::Text {
                text: text.to_owned(),
            },
        };
        let cases: Vec<(&str, Vec<Event>, bool)> = vec![
            ("looked at", Vec::new(), true),
            ("typed", vec![typed("Call the courier")], false),
            (
                "typed and then taken back to what the page gave",
                vec![typed("Call the courier"), typed("")],
                true,
            ),
            ("a send on its way", vec![submit()], false),
            (
                "a send on its way, and one waiting",
                vec![submit(), typed("Again"), submit()],
                false,
            ),
        ];
        for (what, begun, gives_way) in cases {
            let mut config = Config::new(
                "https://shop.example",
                Client::new(Platform::Android, FormFactor::Phone),
            );
            config.cache.persist = true;
            let mut core = Core::new(config);
            core.send(Event::Restore {
                snapshot: snapshot.clone(),
            });
            core.send(Event::Cache {
                key: "boot".to_owned(),
                value: Some(include_str!("../transcripts/fixtures/made/kept-boot.json").to_owned()),
            });
            core.send(Event::Response(Response {
                request: 1,
                error: Some(TransportError::Offline),
                ..Response::default()
            }));
            core.send(Event::Response(answered(
                include_str!("../transcripts/fixtures/made/home-memo.json"),
                2,
            )));
            for event in begun {
                core.send(event);
            }
            let update = core.send(Event::Response(answered(
                include_str!("../transcripts/fixtures/store/shell.json"),
                3,
            )));
            let reported = update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "snapshot"));
            let stack = &core.view.destinations[0].stack;
            if gives_way {
                assert!(
                    stack.len() == 2 && !reported,
                    "{what}: the place is put back: {stack:?} {update:?}"
                );
            } else {
                assert!(
                    stack == &[1] && reported,
                    "{what}: the first page stays and the snapshot is dropped: {stack:?} {update:?}"
                );
            }
        }
    }

    /// What the person did on the first page beyond looking counts as having
    /// begun there too: a search typed, a row chosen, another tab chosen. The
    /// place is not put back over it.
    #[test]
    fn a_snapshot_is_not_put_back_over_a_search_a_chosen_row_or_a_tab() {
        let snapshot = serde_json::json!({
            "version": 1,
            "origin": "https://shop.example",
            "person": "1c54d473261f6f17cb6193202ce56588716e85757946078ac43d03b22f93a35e",
            "selected": "/",
            "destinations": [{"href": "/", "stack": [{"url": "/"}, {"url": "/orders/1/pay"}]}],
        });
        let answered = |body: &str, request: u64| Response {
            request,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            headers: vec![(
                "x-navree-person".to_owned(),
                "0193a4b2-0000-7000-8000-00000000ra01".to_owned(),
            )],
            body: body.to_owned(),
            ..Response::default()
        };
        let cases: Vec<(&str, Option<Event>)> = vec![
            ("looked at", None),
            (
                "searched",
                Some(Event::Search {
                    screen: 1,
                    text: "Simran".to_owned(),
                }),
            ),
            (
                "a row chosen",
                Some(Event::Choose {
                    screen: 1,
                    table: "dispatch".to_owned(),
                    row: Some("1042".to_owned()),
                    on: true,
                    across: false,
                }),
            ),
            (
                "another tab chosen",
                Some(Event::Tab {
                    screen: 1,
                    id: "parts".to_owned(),
                    index: 1,
                }),
            ),
        ];
        for (what, begun) in cases {
            let mut config = Config::new(
                "https://shop.example",
                Client::new(Platform::Android, FormFactor::Phone),
            );
            config.cache.persist = true;
            let mut core = Core::new(config);
            core.send(Event::Restore {
                snapshot: snapshot.clone(),
            });
            core.send(Event::Cache {
                key: "boot".to_owned(),
                value: Some(include_str!("../transcripts/fixtures/made/kept-boot.json").to_owned()),
            });
            core.send(Event::Response(Response {
                request: 1,
                error: Some(TransportError::Offline),
                ..Response::default()
            }));
            core.send(Event::Response(answered(
                include_str!("../transcripts/fixtures/made/home-begun.json"),
                2,
            )));
            let gives_way = begun.is_none();
            if let Some(event) = begun {
                core.send(event);
            }
            let shell = (1..=core.inner.next.request)
                .rev()
                .find(|request| {
                    core.inner
                        .pending
                        .get(request)
                        .is_some_and(|asked| asked.purpose == Pending::Shell)
                })
                .unwrap_or(core.inner.next.request + 1);
            if !core.inner.pending.contains_key(&shell) {
                core.send(Event::Refresh { screen: 1 });
            }
            let shell = (1..=core.inner.next.request)
                .rev()
                .find(|request| {
                    core.inner
                        .pending
                        .get(request)
                        .is_some_and(|asked| asked.purpose == Pending::Shell)
                })
                .expect("the shell is asked for");
            let update = core.send(Event::Response(answered(
                include_str!("../transcripts/fixtures/store/shell.json"),
                shell,
            )));
            let reported = update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "snapshot"));
            let stack = &core.view.destinations[0].stack;
            if gives_way {
                assert!(
                    stack.len() == 2 && !reported,
                    "{what}: the place is put back: {stack:?} {update:?}"
                );
            } else {
                assert!(
                    stack == &[1] && reported,
                    "{what}: the first page stays and the snapshot is dropped: {stack:?} {update:?}"
                );
            }
        }
    }

    /// A snapshot the released core wrote (`made/snapshot-released.json`,
    /// made by the core at 1.2.4 with a form of a short id and one of an id
    /// longer than 64 bytes, each with a field of no id of its own) is read:
    /// the place comes back, and what was typed comes back into the field of
    /// the short form. The other field's name changed with how a long id's
    /// stem is made, so its words go nowhere, and into no other field.
    #[test]
    fn a_snapshot_the_released_core_wrote_is_read() {
        let snapshot: serde_json::Value = serde_json::from_str(include_str!(
            "../transcripts/fixtures/made/snapshot-released.json"
        ))
        .expect("the snapshot reads");
        let answered = |body: &str, request: u64| {
            Event::Response(Response {
                request,
                status: 200,
                content_type: Some(CONTENT_TYPE.to_owned()),
                headers: vec![(
                    "x-navree-person".to_owned(),
                    "0193a4b2-0000-7000-8000-00000000ra01".to_owned(),
                )],
                body: body.to_owned(),
                ..Response::default()
            })
        };
        let fetched = |update: &Update| {
            update
                .effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Fetch(request) => Some((request.request, request.url.clone())),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("a fetch: {update:?}"))
        };
        let mut core = Core::new(Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        ));
        let started = core.send(Event::Restore { snapshot });
        let (shell, _) = fetched(&started);
        let back = core.send(answered(
            include_str!("../transcripts/fixtures/store/shell.json"),
            shell,
        ));
        let stack: Vec<String> = core.view.destinations[0]
            .stack
            .iter()
            .map(|id| core.view.screens[id].url.clone())
            .collect();
        assert_eq!(stack, ["/", "/orders/1"], "the place comes back");
        let (order, url) = fetched(&back);
        assert_eq!(url, "https://shop.example/orders/1");
        core.send(answered(
            r#"{"navree":1,"build":"152bd84abf4d","vocab":0,"screen":{"title":"CS-20481","body":{"t":"text","value":"2 phulkari dupattas"}}}"#,
            order,
        ));
        let (first, _) = fetched(&core.send(Event::Back));
        core.send(answered(
            include_str!("../transcripts/fixtures/made/home-two-forms.json"),
            first,
        ));
        let typed: Vec<(String, String)> = core.view.screens[&1]
            .forms
            .values()
            .flat_map(|form| form.fields.iter())
            .filter_map(|(name, field)| match &field.value {
                crate::FieldValue::Text { text } if !text.is_empty() => {
                    Some((name.clone(), text.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            typed,
            [("note.memo".to_owned(), "Call the courier".to_owned())],
            "only the short form's words come back"
        );
    }

    /// A snapshot asked for before this launch's shell has answered names
    /// nobody the server has not named in this launch, and is not a
    /// visitor's: the client must not store it over a person's.
    #[test]
    fn a_snapshot_asked_for_before_this_launchs_shell_answers_says_its_person_is_not_known() {
        let unknown = |core: &Core| core.part(&Query::Snapshot)["person_unknown"] == true;
        let mut config = Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        );
        config.cache.persist = true;
        let mut core = Core::new(config);
        core.send(Event::Start);
        assert!(unknown(&core), "no shell yet");
        core.send(Event::Cache {
            key: "boot".to_owned(),
            value: Some(
                include_str!("../transcripts/fixtures/made/kept-boot-workspace.json").to_owned(),
            ),
        });
        assert!(unknown(&core), "only the kept shell");
        let snapshot = core.part(&Query::Snapshot);
        assert!(
            snapshot["destinations"].is_null() && snapshot["person"].is_null(),
            "{snapshot}"
        );
        // The shell of this launch answers, and names her.
        core.send(Event::Response(Response {
            request: 1,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            headers: vec![(
                "x-navree-person".to_owned(),
                "0193a4b2-0000-7000-8000-00000000ra01".to_owned(),
            )],
            body: include_str!("../transcripts/fixtures/made/shell-workspace.json").to_owned(),
            ..Response::default()
        }));
        assert!(!unknown(&core), "the shell named her");
        assert!(core.part(&Query::Snapshot)["person"].is_string());
        // A snapshot waiting for the shell is the one given back, as it was.
        let mut config = Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        );
        config.cache.persist = true;
        let mut core = Core::new(config);
        core.send(Event::Restore {
            snapshot: serde_json::json!({"version": 1, "origin": "https://shop.example", "person": "abc"}),
        });
        let waiting = core.part(&Query::Snapshot);
        assert_eq!(waiting["person"], "abc");
        assert!(waiting["person_unknown"].is_null());
        // And a snapshot that says its person is not known is not used.
        let mut core = Core::new(Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        ));
        let update = core.send(Event::Restore {
            snapshot: serde_json::json!({"version": 1, "origin": "https://shop.example", "person_unknown": true}),
        });
        assert!(
            update
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "snapshot")),
            "{update:?}"
        );
    }

    /// Before the list is read, any read answered without the keep header
    /// is remembered, and taken away once the list holds its address: a
    /// search form's query as much as a screen's page.
    #[test]
    fn a_read_answered_before_the_list_is_read_is_taken_away_once_it_is() {
        let mut config = Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        );
        config.cache.persist = true;
        let mut core = Core::new(config);
        core.send(Event::Start);
        core.send(Event::Response(Response {
            request: 1,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            body: include_str!("../transcripts/fixtures/store/shell.json").to_owned(),
            ..Response::default()
        }));
        // A form's own read, not a screen's page, answered without the header.
        core.fetch(
            Pending::Send {
                screen: 1,
                visit: 0,
                key: "search".to_owned(),
                write: false,
                in_place: true,
            },
            Method::Get,
            "/orders",
            None,
            PAGE_LIMIT,
        );
        let request = core.inner.next.request;
        core.settle();
        core.send(Event::Response(Response {
            request,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            body: include_str!("../transcripts/fixtures/store/orders.json").to_owned(),
            ..Response::default()
        }));
        let update = core.send(Event::Cache {
            key: "boot".to_owned(),
            value: Some(include_str!("../transcripts/fixtures/made/kept-boot.json").to_owned()),
        });
        assert!(
            update.effects.iter().any(|effect| matches!(
                effect,
                Effect::CacheWrite { key, value: None }
                    if key == "page:0193a4b2-0000-7000-8000-00000000ra01:https://shop.example/orders"
            )),
            "{update:?}"
        );
    }

    /// A client's store, shared by the cores that run over it: what each
    /// writes, empties and reads, in the order the effects come.
    #[derive(Default)]
    struct Store(std::collections::BTreeMap<String, String>);

    impl Store {
        /// Sends `event` to `core` and carries out what it asks of the store,
        /// answering its reads at once.
        fn send(&mut self, core: &mut Core, event: Event) {
            let mut events = vec![event];
            while let Some(event) = events.pop() {
                for effect in core.send(event).effects {
                    match effect {
                        Effect::CacheWrite {
                            key,
                            value: Some(value),
                        } => {
                            self.0.insert(key, value);
                        }
                        Effect::CacheWrite { key, value: None } => {
                            self.0.remove(&key);
                        }
                        Effect::CacheClear => self.0.clear(),
                        Effect::CacheRead { key } => events.insert(
                            0,
                            Event::Cache {
                                value: self.0.get(&key).cloned(),
                                key,
                            },
                        ),
                        _ => {}
                    }
                }
            }
        }
    }

    fn kept_core() -> Core {
        let mut config = Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        );
        config.cache.persist = true;
        Core::new(config)
    }

    fn native(request: u64, body: &str, person: Option<&str>, keep: bool) -> Event {
        let mut headers = Vec::new();
        if keep {
            headers.push(("x-navree-keep".to_owned(), "1".to_owned()));
        }
        if let Some(person) = person {
            headers.push(("x-navree-person".to_owned(), person.to_owned()));
        }
        Event::Response(Response {
            request,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            headers,
            body: body.to_owned(),
            ..Response::default()
        })
    }

    /// A core whose first shell is `shell`, answered for the person of `id`,
    /// or with no id when the server names nobody by one.
    fn signed_in_as(id: Option<&str>, shell: &str) -> Core {
        let mut core = Core::new(Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        ));
        core.send(Event::Start);
        core.send(native(1, shell, id, false));
        core
    }

    /// The key a client is given for the person signed in.
    fn key_of(core: &Core) -> Option<String> {
        core.view.app.as_ref()?.user.as_ref()?.key.clone()
    }

    /// The person a client is given carries the core's key for them. When
    /// the server named them by id, the key is made from the id: it is the
    /// name a snapshot is for, and never the id itself. Two accounts of one
    /// name and no email are two people, and one account is one person
    /// whatever its name and email become.
    #[test]
    fn the_person_signed_in_carries_a_key_made_from_the_id_the_server_named_them_by() {
        const FIRST: &str = "7f1c2a90-0001-4000-8000-00000000a001";
        const SECOND: &str = "7f1c2a90-0002-4000-8000-00000000b002";
        let harpreet = include_str!("../transcripts/fixtures/made/shell-harpreet-no-email.json");
        let first = signed_in_as(Some(FIRST), harpreet);
        let key = key_of(&first).expect("the person signed in has a key");
        assert_eq!(key.len(), 64, "a hash: {key}");
        assert!(
            !key.contains(FIRST) && !key.contains("Harpreet"),
            "nothing of the person is in it: {key}"
        );
        assert_eq!(
            first.snapshot().person.as_deref(),
            Some(key.as_str()),
            "the key is the name a snapshot is for"
        );
        // The same account at another launch has the same key.
        assert_eq!(
            key_of(&signed_in_as(Some(FIRST), harpreet)),
            Some(key.clone())
        );
        // Another account of the same name and no email has another.
        let other = key_of(&signed_in_as(Some(SECOND), harpreet)).expect("a key");
        assert_ne!(other, key, "two accounts of one name have one key");
        // The same account under another name and with an email has the same.
        let renamed = include_str!("../transcripts/fixtures/made/shell-other.json");
        assert_eq!(
            key_of(&signed_in_as(Some(FIRST), renamed)),
            Some(key),
            "one account has two keys when its name and email change"
        );
    }

    /// A person the server names without an id is told by their name and
    /// email, which is all there is to tell them by: the key is made from
    /// those, and is not the key of a person of that name who is named by
    /// id. Nobody signed in is nobody: there is no person, and so no key.
    #[test]
    fn a_person_named_without_an_id_has_a_key_made_from_their_name_and_email() {
        let no_email = include_str!("../transcripts/fixtures/made/shell-harpreet-no-email.json");
        let with_email = include_str!("../transcripts/fixtures/made/shell-other.json");
        let ravneet = include_str!("../transcripts/fixtures/store/shell.json");
        let named = signed_in_as(None, no_email);
        let key = key_of(&named).expect("a person named without an id has a key");
        assert!(
            key.len() == 64 && !key.contains("Harpreet"),
            "a hash, with nothing of the person in it: {key}"
        );
        assert_eq!(
            named.snapshot().person,
            None,
            "no snapshot is for a person named without an id"
        );
        assert_eq!(
            key_of(&signed_in_as(None, no_email)),
            Some(key.clone()),
            "the same name and email have two keys"
        );
        let emailed = key_of(&signed_in_as(None, with_email)).expect("a key");
        let another = key_of(&signed_in_as(None, ravneet)).expect("a key");
        assert!(
            emailed != key && another != key && another != emailed,
            "another email, or another name, has the same key"
        );
        assert_ne!(
            key_of(&signed_in_as(
                Some("7f1c2a90-0001-4000-8000-00000000a001"),
                no_email
            )),
            Some(key),
            "a person named by id has the key of one named without"
        );

        let nobody = include_str!("../transcripts/fixtures/made/shell-nobody.json");
        let visitor = signed_in_as(None, nobody);
        assert!(
            visitor
                .view
                .app
                .as_ref()
                .is_some_and(|app| app.user.is_none()),
            "nobody is signed in, and a person is named"
        );
        assert_eq!(key_of(&visitor), None);
    }

    /// The key is the core's to write. A server leaves it out, and one a
    /// server writes is written over: no answer can make two people one to
    /// a client, or one person two.
    #[test]
    fn a_key_a_server_writes_is_written_over() {
        const FIRST: &str = "7f1c2a90-0001-4000-8000-00000000a001";
        let shell = include_str!("../transcripts/fixtures/made/shell-harpreet-no-email.json");
        let written = shell.replacen(
            "\"name\": \"Harpreet Singh\",",
            "\"name\": \"Harpreet Singh\", \"key\": \"written-by-a-server\",",
            1,
        );
        assert_ne!(written, shell, "the shell names Harpreet Singh");
        for id in [Some(FIRST), None] {
            let key = key_of(&signed_in_as(id, &written)).expect("a key");
            assert_ne!(key, "written-by-a-server", "{id:?}: the server's is kept");
            assert_eq!(
                Some(key),
                key_of(&signed_in_as(id, shell)),
                "{id:?}: what a server wrote changed the key"
            );
        }
    }

    /// Two cores over one store, one still naming Ravneet and one naming a
    /// visitor, never mix their pages: the next launch draws nothing of
    /// Ravneet's for a visitor.
    #[test]
    fn two_cores_over_one_store_never_mix_their_owners_pages() {
        const RAVNEET: &str = "0193a4b2-0000-7000-8000-00000000ra01";
        let shell = include_str!("../transcripts/fixtures/store/shell.json");
        let nobody = include_str!("../transcripts/fixtures/made/shell-nobody.json");
        let hers = include_str!("../transcripts/fixtures/made/home.json");
        let visitors = include_str!("../transcripts/fixtures/made/welcome.json");
        let mut store = Store::default();
        // A window signed out keeps the visitor's first page.
        let mut a = kept_core();
        store.send(&mut a, Event::Start);
        store.send(&mut a, native(1, nobody, None, false));
        store.send(&mut a, native(2, visitors, None, true));
        // Another window, where Ravneet is signed in, asks for its first page.
        let mut b = kept_core();
        store.send(&mut b, Event::Start);
        store.send(&mut b, native(1, shell, Some(RAVNEET), false));
        // Ravneet's page answers, and is kept as hers.
        store.send(&mut b, native(2, hers, Some(RAVNEET), true));
        // The signed-out window keeps another page, and writes its list again,
        // still a visitor's, with its first page on it.
        store.send(
            &mut a,
            Event::Act {
                screen: None,
                action: navree_native::Action::navigate("/catalogue"),
                form: None,
                overlay: None,
                text: None,
                now: None,
                ask: None,
                from: None,
            },
        );
        store.send(&mut a, native(3, visitors, None, true));
        // The next launch, as a visitor.
        let mut c = kept_core();
        store.send(&mut c, Event::Start);
        store.send(&mut c, native(1, nobody, None, false));
        let front = c.front().unwrap();
        let drawn = c.view.screens[&front]
            .screen()
            .map(|screen| screen.title.clone());
        assert_ne!(
            drawn.as_deref(),
            Some("Overview"),
            "Ravneet's page was drawn"
        );
    }

    #[test]
    fn a_picture_in_the_shell_s_user_menu_is_written_whole() {
        let mut core = Core::new(Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        ));
        core.send(Event::Start);
        let shell = include_str!("../transcripts/fixtures/made/shell-routes.json").replacen(
            r#""menu": ["#,
            r#""menu": [{"t": "image", "src": "me.png"}, {"t": "avatar", "name": "R", "src": "javascript:1"},"#,
            1,
        );
        core.send(Event::Response(Response {
            request: 1,
            status: 200,
            content_type: Some(CONTENT_TYPE.to_owned()),
            body: shell,
            ..Response::default()
        }));
        let menu = &core.view.app.as_ref().unwrap().user.as_ref().unwrap().menu;
        assert!(
            matches!(&menu[0], navree_native::node::Node::Image(image) if image.src == "https://shop.example/me.png"),
            "{menu:?}"
        );
        assert!(
            matches!(&menu[1], navree_native::node::Node::Avatar(avatar) if avatar.src.is_none()),
            "{menu:?}"
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
        assert_eq!(reports(&update), ["shell: a path"]);
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

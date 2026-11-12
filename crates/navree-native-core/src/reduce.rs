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
use navree_native::node::Method;
use navree_native::shell::{NavItem, Route, Shell};
use navree_native::{CLIENT_HEADER, CONTENT_TYPE, MAJOR, NATIVE_HEADER, SHELL_PATH, Screen, Tone};
use serde_json::Value;

use crate::decode::{self, Answer, Read};
use crate::effect::{Body, Change, Effect, Request, Update};
use crate::event::{Event, Response, StreamState, TransportError};
use crate::origin::{Target, path_of, route_matches};
use crate::state::{
    App, Content, Destination, DeviceCall, Failure, Message, MessageInner, Modal, Notice, Pending, Stage,
    ScreenId, ScreenInner, ScreenView, TimerFor,
};
use crate::tree::{Names, Survey, has_id, opens, roots, walk};
use crate::Core;

/// How long a page may take, in milliseconds.
const PAGE_LIMIT: u32 = 30_000;
/// How long the answer to a form or a button may take.
pub(crate) const SEND_LIMIT: u32 = 60_000;

/// What an answer gives a screen to show.
pub(crate) enum Shown {
    /// A tree, with the status and the body it came in.
    Native {
        screen: Box<Screen>,
        status: u16,
        body: String,
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
            } => {
                if let Some(screen) = screen.or(self.front()) {
                    self.act(screen, action, form, overlay, text);
                }
            }
            Event::Field {
                screen,
                form,
                field,
                value,
            } => self.field_changed(screen, form.as_deref(), &field, value),
            Event::Back => self.back(),
            Event::PopTo { screen } => self.pop_to(screen),
            Event::Dismissed { screen } => {
                // The sign-in a session's end put up is not the person's to take away.
                if !self.view.modals.iter().any(|modal| modal.locked && modal.stack.contains(&screen)) {
                    self.dismissed(screen);
                }
            }
            Event::Overlay { screen, id, open } => self.set_open(screen, &id, open),
            Event::Refresh { screen } => self.refresh(screen),
            Event::RegionRetry { screen, region } => self.fetch_region(screen, &region, None),
            Event::More { screen, id } => self.more(screen, &id),
            Event::RegionGo { screen, region, href } => {
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
            Event::Foreground => self.inner.background = false,
            Event::Background => {
                self.inner.background = true;
                // A hidden app holds no stream; what its regions miss is fetched when it is back.
                if let Some(inner) = self.front().and_then(|front| self.inner.screens.get_mut(&front)) {
                    inner.missed = true;
                }
            }
            Event::SignedOut => {
                // A shell asked for before now would be the old session's.
                self.drop_pending(|_| true);
                self.wipe();
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
            Event::DeviceResult { call, ok, fields } => self.device_result(call, ok, fields),
            Event::SignInDone { url } => self.sign_in_done(url.as_deref()),
            Event::Focused { screen, serial } => {
                if let Some(view) = self.view.screens.get_mut(&screen)
                    && view.focus.as_ref().is_some_and(|focus| focus.serial == serial)
                {
                    view.focus = None;
                    self.touch(Change::Screen { id: screen });
                }
            }
            Event::OpenUpgrade => {
                if let Some(upgrade) = &self.view.upgrade {
                    let url = upgrade.url.clone();
                    self.effects.push(Effect::OpenUrl { url });
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
        let from = self.view.screens.get(&screen).map_or("/", |screen| screen.url.as_str());
        self.inner.origin.resolve(href, from)
    }

    /// The first route of the shell that names the address.
    pub(crate) fn route(&self, url: &str) -> Option<&Route> {
        self.inner.routes.iter().find(|route| route_matches(&route.matches, url))
    }

    fn headers(&self, form: bool) -> Vec<(String, String)> {
        let mut client = self.config.client.to_header();
        let lib: String = self
            .config
            .lib
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
            .collect();
        if !lib.is_empty() && !client.contains("lib=") {
            client.push_str("; lib=");
            client.push_str(&lib);
        }
        let mut headers = vec![
            (NATIVE_HEADER.to_owned(), MAJOR.to_string()),
            (CLIENT_HEADER.to_owned(), client),
            ("accept".to_owned(), CONTENT_TYPE.to_owned()),
        ];
        if form {
            headers.push(("content-type".to_owned(), "application/x-www-form-urlencoded".to_owned()));
        }
        headers
    }

    /// Asks for `url`, an address inside. A post always says it is a form,
    /// even one with nothing in it: without that the server refuses it.
    pub(crate) fn fetch(&mut self, purpose: Pending, method: Method, url: &str, body: Option<Body>, limit: u32) {
        self.inner.next.request += 1;
        let request = self.inner.next.request;
        let post = method == Method::Post;
        let body = match body {
            None if post => Some(Body::Form { text: String::new() }),
            body => body,
        };
        let headers = self.headers(matches!(body, Some(Body::Form { .. })));
        self.inner.pending.insert(request, (purpose, url.to_owned()));
        self.effects.push(Effect::Fetch(Request {
            request,
            method,
            url: self.inner.origin.absolute(url),
            headers,
            body,
            timeout_ms: limit,
            xsrf: post,
        }));
    }

    /// Forgets the requests on their way that `is` accepts: their answers are
    /// no longer wanted.
    pub(crate) fn drop_pending(&mut self, is: impl Fn(&Pending) -> bool) {
        let gone: Vec<u64> = self
            .inner
            .pending
            .iter()
            .filter(|(_, (purpose, _))| is(purpose))
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
            Notice::Unsaved => Tone::Warning,
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

    fn report(&mut self, what: &str, detail: String) {
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
        let from = about.screen.filter(|screen| self.view.holds(*screen)).or(self.front());
        if !action.post {
            self.navigate(from, &action.href, Present::Push);
        } else if let Some(screen) = from {
            self.post(screen, &action.href, Vec::new(), None);
        }
    }

    // Starting, and the shell.

    fn start(&mut self) {
        let asking = self.inner.pending.values().any(|(purpose, _)| *purpose == Pending::Shell);
        if asking {
            return;
        }
        if self.view.stage == Stage::Failed {
            self.view.stage = Stage::Starting;
            self.view.failure = None;
            self.touch(Change::Nav);
        }
        if self.config.cache.persist && !self.inner.has_shell {
            self.effects.push(Effect::CacheRead { key: "boot".to_owned() });
        }
        self.fetch(Pending::Shell, Method::Get, SHELL_PATH, None, PAGE_LIMIT);
    }

    pub(crate) fn fetch_shell(&mut self) {
        let asking = self.inner.pending.values().any(|(purpose, _)| *purpose == Pending::Shell);
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
        let media = response.content_type.as_deref().unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        if !(200..300).contains(&response.status) || media != CONTENT_TYPE {
            // A server with no shell still has pages: one destination, its first page.
            if !self.inner.has_shell {
                self.install(Shell::new("", ""));
            }
            return;
        }
        let value: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
        if let Some(upgrade) = value.get("upgrade").cloned().and_then(|upgrade| serde_json::from_value(upgrade).ok()) {
            self.view.upgrade = Some(upgrade);
            self.view.stage = Stage::Upgrade;
            return self.touch(Change::Nav);
        }
        match serde_json::from_value::<Shell>(value) {
            Ok(shell) if shell.navree == MAJOR => {
                self.install(shell);
                self.remember_shell(response.body);
                // This shell is at least as new as any stamp seen before it:
                // the next envelope says which stamp it goes with.
                self.inner.shell_stamp = None;
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

    pub(crate) fn shell_from_cache(&mut self, body: &str) {
        if let Ok(shell) = serde_json::from_str::<Shell>(body)
            && shell.navree == MAJOR
        {
            self.inner.cache.shell = Some(body.to_owned());
            self.install(shell);
        }
    }

    /// The address a destination opens at.
    pub(crate) fn destination_url(&self, href: &str) -> String {
        match self.inner.origin.resolve(href, "/") {
            Target::Inside(url) => url,
            _ => "/".to_owned(),
        }
    }

    fn install(&mut self, shell: Shell) {
        let user = shell.user.as_ref().map(|user| (user.name.clone(), user.email.clone()));
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
            if self.inner.build.is_some() {
                self.forget_pages();
            }
            self.inner.build = Some(shell.build.clone());
        }
        self.inner.routes = shell.routes;
        let mut items: Vec<(Option<String>, NavItem)> = Vec::new();
        for group in shell.nav.groups {
            for item in group.items {
                if matches!(self.inner.origin.resolve(&item.href, "/"), Target::Inside(_)) {
                    items.push((group.label.clone(), item));
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
        let was = self.view.destinations.get(self.view.selected).map(|destination| destination.href.clone());
        let mut old: Vec<Destination> = std::mem::take(&mut self.view.destinations);
        for (group, item) in items {
            let kept = old.iter().position(|destination| destination.href == item.href);
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
        if let Some(launch) = self.inner.launch.take() {
            self.opened(&launch);
        }
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
        let ids: Vec<ScreenId> = self.view.screens.keys().copied().filter(|id| !covers.contains(id)).collect();
        for id in ids {
            let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
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
            self.drop_pending(|purpose| match purpose {
                Pending::Page { screen, .. } | Pending::Region { screen, .. } | Pending::More { screen, .. } | Pending::Send { screen, .. } => *screen == id,
                Pending::Shell => false,
            });
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

    /// Takes note of the build an answer belongs to. A new one empties what
    /// is kept and asks for the shell again. Nothing on screen is thrown away:
    /// what was typed stays.
    fn note_build(&mut self, build: &str) {
        if build.is_empty() || self.inner.build.as_deref() == Some(build) {
            return;
        }
        let first = self.inner.build.is_none();
        self.inner.build = Some(build.to_owned());
        if !first {
            self.forget_pages();
            self.fetch_shell();
        }
    }

    /// What every envelope is read for, whatever it answers. True when the
    /// answer asks for a newer app, and holds nothing else.
    pub(crate) fn note(&mut self, read: &mut Read, screen: Option<ScreenId>) -> bool {
        if !read.missing.is_empty() {
            let what = match read.missing.first().map(String::as_str) {
                Some("client:decode") => "decode",
                Some("client:depth") => "depth",
                _ => "kind",
            };
            self.report(what, read.missing.join(" "));
        }
        if let Some(upgrade) = read.envelope.upgrade.take() {
            self.view.upgrade = Some(upgrade);
            self.view.stage = Stage::Upgrade;
            self.touch(Change::Nav);
            return true;
        }
        let build = std::mem::take(&mut read.envelope.build);
        self.note_build(&build);
        if let Some(app) = read.app.take() {
            if self.inner.app_stamp.as_ref().is_some_and(|known| *known != app) {
                self.forget_pages();
            }
            self.inner.app_stamp = Some(app);
        }
        if let Some(shell) = read.shell.take() {
            // The user, the destinations or a count changed.
            if self.inner.shell_stamp.as_ref().is_some_and(|known| *known != shell) {
                self.fetch_shell();
            }
            self.inner.shell_stamp = Some(shell);
        }
        for call in std::mem::take(&mut read.device) {
            self.device(&call, screen);
        }
        false
    }

    // Device calls a reply starts.

    fn device(&mut self, call: &Value, screen: Option<ScreenId>) {
        let asked = call.get("call").unwrap_or(call);
        let text = |key: &str| asked.get(key).and_then(Value::as_str).map(str::to_owned);
        let Some(name) = text("name") else {
            return;
        };
        // A reply may start only calls that need no tap.
        const NO_TAP: [&str; 4] = ["haptic", "badge", "review", "cancel_notification"];
        if !NO_TAP.contains(&name.as_str()) {
            return self.report("device", format!("a reply may not start `{name}`"));
        }
        let args = asked.get("args").cloned().unwrap_or(Value::Null);
        if name == "haptic" {
            let kind = args.get("kind").and_then(Value::as_str).unwrap_or("success").to_owned();
            return self.effects.push(Effect::Haptic { kind });
        }
        self.inner.next.call += 1;
        let id = self.inner.next.call;
        self.inner.calls.insert(
            id,
            DeviceCall {
                screen,
                then: text("then"),
                ticket: text("ticket"),
            },
        );
        self.effects.push(Effect::Device { call: id, name, args });
    }

    fn device_result(&mut self, call: u64, ok: bool, mut fields: Vec<navree_native::Pair>) {
        let Some(known) = self.inner.calls.remove(&call) else {
            return;
        };
        let (Some(then), Some(screen)) = (known.then, known.screen.or(self.front())) else {
            return;
        };
        let pair = |name: &str, value: &str| navree_native::Pair {
            name: name.to_owned(),
            value: value.to_owned(),
        };
        fields.push(pair("ok", if ok { "1" } else { "0" }));
        if let Some(ticket) = known.ticket {
            fields.push(pair("ticket", &ticket));
        }
        self.post(screen, &then, fields, None);
    }

    fn sign_in_done(&mut self, url: Option<&str>) {
        if !std::mem::take(&mut self.inner.signing_in) {
            return;
        }
        // The person gave up: nothing changed.
        let Some(url) = url else {
            return;
        };
        // Who is signed in may have changed, and with it everything on screen.
        self.fetch_shell();
        for inner in self.inner.screens.values_mut() {
            inner.stale = true;
        }
        self.inner.epoch += 1;
        self.forget_pages();
        self.opened(url);
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
        self.view.screens.insert(id, ScreenView::new(id, url.to_owned(), chrome));
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
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
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
        self.drop_pending(|purpose| matches!(purpose, Pending::Page { screen, .. } if *screen == id));
        self.fetch(Pending::Page { screen: id, visit }, Method::Get, &url, None, PAGE_LIMIT);
        self.touch_screen(id);
    }

    fn refresh(&mut self, id: ScreenId) {
        self.fetch_page(id, true);
    }

    /// Forgets a screen that no stack holds any more.
    pub(crate) fn close(&mut self, id: ScreenId) {
        self.view.screens.remove(&id);
        if let Some(inner) = self.inner.screens.remove(&id) {
            for timer in inner.query_timers.into_values().chain(inner.live.into_values().filter_map(|sub| sub.timer)) {
                self.cancel_timer(timer);
            }
        }
        self.drop_pending(|purpose| match purpose {
            Pending::Page { screen, .. } | Pending::Region { screen, .. } | Pending::More { screen, .. } => *screen == id,
            // The answer to a write is still wanted: it says whether it worked.
            Pending::Send { .. } | Pending::Shell => false,
        });
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
    pub(crate) fn show_native(&mut self, id: ScreenId, url: &str, mut screen: Screen, status: u16, kept: bool) {
        let (Some(view), Some(inner)) = (self.view.screens.get_mut(&id), self.inner.screens.get_mut(&id)) else {
            return;
        };
        let same = path_of(&view.url) == path_of(url) && view.screen().is_some();
        inner.form_ids.clear();
        inner.field_ids.clear();
        inner.server_ids.clear();
        let mut names = Names {
            forms: &mut inner.form_ids,
            fields: &mut inner.field_ids,
            server: &mut inner.server_ids,
        };
        names.name(&mut screen.actions, None);
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
                    Some(navree_native::Node::Region(old)) if !old.children.is_empty() => std::mem::take(&mut old.children),
                    _ => continue,
                };
                if let Some(navree_native::Node::Region(new)) = crate::tree::find_in_mut(&mut screen, &is) {
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
            view.chrome = self.inner.routes.iter().find(|route| route_matches(&route.matches, url)).is_none_or(|route| route.chrome);
        }
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
        }
        self.touch_screen(id);
    }

    /// Puts the page in the web view on a screen.
    pub(crate) fn show_web(&mut self, id: ScreenId, url: &str, page: &str, title: Option<String>, why: Vec<String>) {
        let Some(view) = self.view.screens.get_mut(&id) else {
            return;
        };
        self.inner.next.web += 1;
        view.url = url.to_owned();
        view.content = Content::Web {
            url: self.inner.origin.absolute(page),
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
        view.chrome = self.inner.routes.iter().find(|route| route_matches(&route.matches, url)).is_none_or(|route| route.chrome);
        self.drop_pending(|purpose| matches!(purpose, Pending::Region { screen, .. } | Pending::More { screen, .. } if *screen == id));
        self.touch_screen(id);
    }

    pub(crate) fn show(&mut self, id: ScreenId, url: &str, shown: Shown) {
        match shown {
            Shown::Native { screen, status, body } => {
                self.remember(url, status, &body);
                self.show_native(id, url, *screen, status, false);
            }
            Shown::Web { url: page, title, why } => self.show_web(id, url, &page, title, why),
        }
    }

    /// What an envelope gives a screen to show, if it gives anything.
    pub(crate) fn shown(&self, url: &str, status: u16, read: &mut Read, body: String) -> Option<Shown> {
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
        if !read.missing.is_empty() && (read.envelope.screen.is_some() || read.envelope.ops.is_empty()) {
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
        })
    }

    // Answers.

    fn answered(&mut self, response: Response) {
        // An answer nobody waits for any more changes nothing.
        let Some((purpose, asked)) = self.inner.pending.remove(&response.request) else {
            return;
        };
        if purpose == Pending::Shell {
            return self.shell_answered(response);
        }
        let answer = decode::read(response, &asked, &self.inner.origin, &self.config);
        match purpose {
            Pending::Shell => {}
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
            Pending::More { screen, visit, id, was } => self.more_answered(screen, visit, &id, &was, answer),
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
            Some(present @ (Present::Full | Present::Sheet)) => Some((route.matches.clone(), present)),
            _ => None,
        }
    }

    /// Presents what a redirect led to over the app, and leaves the screen
    /// that asked as it was: the person comes back to it.
    pub(crate) fn cover(&mut self, under: ScreenId, route: (String, Present), url: &str, shown: Option<Shown>) {
        if let (Some(view), Some(inner)) = (self.view.screens.get_mut(&under), self.inner.screens.get_mut(&under)) {
            view.loading = false;
            view.refreshing = false;
            inner.stale = true;
            self.touch_screen(under);
        }
        // One cover for one sign-in, however many requests were sent there.
        let up = self.view.modals.iter().any(|modal| {
            modal.stack.first().and_then(|first| self.inner.screens.get(first)).is_some_and(|inner| inner.cover.as_ref() == Some(&route.0))
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
            Some(Shown::Native { screen, status, .. }) => self.show_native(id, url, *screen, status, false),
            Some(shown) => self.show(id, url, shown),
            None => self.fetch_page(id, false),
        }
    }

    fn page_answered(&mut self, id: ScreenId, visit: u64, asked: &str, answer: Answer) {
        // The person asked for something else on this screen, or left it.
        if self.inner.screens.get(&id).map(|inner| inner.visit) != Some(visit) {
            return;
        }
        let blank = self.view.screens.get(&id).is_some_and(|view| matches!(view.content, Content::Blank));
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
                self.effects.push(Effect::OpenUrl { url });
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
            } => {
                if self.note(&mut read, Some(id)) {
                    return settle(self, None);
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
                let left = self.apply_ops(id, read.envelope.ops, false);
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
                let stale = self.inner.screens.get(&front).is_some_and(|inner| inner.stale);
                let loading = self.view.screens.get(&front).is_some_and(|view| view.loading);
                if stale && !loading {
                    self.fetch_page(front, false);
                }
            }
            None => {
                let Some(href) = self.view.destinations.get(self.view.selected).map(|destination| destination.href.clone()) else {
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
        self.ensure_front();
        let front = self.front();
        let stacked = self.view.front_stack().len() > 1;
        let covered = front.and_then(|front| self.view.screens.get(&front)).is_some_and(|view| !view.overlays.is_empty());
        let first = self.view.selected == 0 || !self.config.back_to_first;
        let can_go_back = !self.view.modals.is_empty() || stacked || covered || !first;
        if self.view.front != front || self.view.can_go_back != can_go_back {
            if let Some(inner) = self.view.front.and_then(|was| self.inner.screens.get_mut(&was)) {
                inner.missed = true;
            }
            self.view.front = front;
            self.view.can_go_back = can_go_back;
            self.touch(Change::Nav);
        }
        self.sync_streams();
        let changed = std::mem::take(&mut self.changed);
        let whole = |screen: &ScreenId| changed.contains(&Change::Screen { id: *screen });
        // A list whose rows were added says so once: the rows say its state too.
        let grew = |screen: &ScreenId, list: &str| {
            changed.iter().any(|change| matches!(change, Change::Rows { screen: other, id, count, .. } if other == screen && id == list && *count > 0))
        };
        let changed = changed
            .iter()
            .filter(|change| match change {
                Change::Screen { id } => self.view.holds(*id),
                Change::Form { screen, .. } => self.view.holds(*screen) && !whole(screen),
                Change::Rows { screen, id, count, .. } => self.view.holds(*screen) && !whole(screen) && (*count > 0 || !grew(screen, id)),
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
        let mut config = Config::new("https://shop.example", client);
        config.lib = "0.1.0".to_owned();
        let mut core = Core::new(config);
        let update = core.send(Event::Start);
        let Effect::Fetch(request) = &update.effects[0] else {
            panic!("{update:?}");
        };
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.headers[0], ("x-navree-native".to_owned(), "1".to_owned()));
        assert!(request.headers[1].1.starts_with("platform=ios; "), "{:?}", request.headers[1]);
        assert!(request.headers[1].1.ends_with("; lib=0.1.0"), "{:?}", request.headers[1]);
        assert_eq!(request.headers[2], ("accept".to_owned(), CONTENT_TYPE.to_owned()));
        assert_eq!(request.headers.len(), 3);
        assert!(request.body.is_none() && !request.xsrf);

        core.fetch(Pending::Shell, Method::Post, "/sign-out", None, SEND_LIMIT);
        let Some(Effect::Fetch(post)) = core.effects.last() else {
            panic!();
        };
        assert_eq!(post.body, Some(Body::Form { text: String::new() }));
        assert_eq!(post.headers[3], ("content-type".to_owned(), "application/x-www-form-urlencoded".to_owned()));
        assert!(post.xsrf);
    }
}

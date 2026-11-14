//! Signing in through the system's own sign-in session, and handing that
//! sign-in over to the app.
//!
//! The session has cookies of its own, so a sign-in in it signs nobody in
//! here. The shell names two routes for the hand-over: where the session
//! starts, and where its one-time code is exchanged for a session of the
//! app's own. The core carries the whole of it:
//!
//! 1. It asks the client for random bytes (a `random` effect) and makes from
//!    them a verifier, which stays here, and a `state`. The address the
//!    session opens at carries the verifier's SHA-256 and the `state`.
//! 2. The session's return is taken only while a session the core opened is
//!    open, and only with that `state`. A return that arrives any other way,
//!    as a link another app opened, is not exchanged and shows nothing.
//! 3. The code and the verifier are posted to the exchange route. Only an
//!    answer of `200` is a sign-in: the shell is fetched again, and the shell
//!    says who is signed in. The verifier is forgotten whatever came of it.
//!
//! One sign-in at a time: one started while a session is open takes its
//! place, and the return of the one before is then refused by its `state`.
//!
//! A sign-in that needs a second step (a code from an authenticator app) is
//! finished on a page of the app, opened here with the app's own cookies and
//! never in the system's session, which does not have them.

use navree_native::Platform;
use navree_native::action::Present;
use navree_native::shell::{SignInExchange, SignInPlatform};
use sha2::{Digest, Sha256};

use crate::Core;
use crate::effect::{Change, Effect, Part};
use crate::event::{Response, TransportError};
use crate::forms::encode;
use crate::origin::{Target, path_of};
use crate::state::{Failure, HandOver, Modal, Notice, Pending, ScreenId};

/// The fewest random bytes a sign-in is made from: 32 for the verifier and
/// 16 for the `state`.
pub(crate) const RANDOM_BYTES: usize = 48;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64 for an address, without padding.
pub(crate) fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = u32::from(chunk[0]) << 16
            | u32::from(chunk.get(1).copied().unwrap_or(0)) << 8
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        for at in 0..=chunk.len() {
            let six = (group >> (18 - 6 * at)) & 63;
            out.push(char::from(ALPHABET[six as usize]));
        }
    }
    out
}

/// The S256 challenge of a verifier: the SHA-256 of its characters, as
/// base64 for an address.
pub(crate) fn challenge_of(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    let bytes: &[u8] = hash.as_ref();
    base64url(bytes)
}

/// The SHA-256 of `text`, as hex: a name for something that is not kept itself.
pub(crate) fn fingerprint(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    let bytes: &[u8] = hash.as_ref();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Reads hex, two characters a byte.
fn unhex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim().as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let digit = |byte: u8| {
        char::from(byte)
            .to_digit(16)
            .and_then(|digit| u8::try_from(digit).ok())
    };
    text.chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

/// Decodes one value of a query.
fn decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => out.push(b' '),
            b'%' if at + 2 < bytes.len() => match std::str::from_utf8(&bytes[at + 1..at + 3])
                .ok()
                .and_then(unhex)
            {
                Some(byte) => {
                    out.extend(byte);
                    at += 2;
                }
                None => out.push(b'%'),
            },
            other => out.push(other),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The value of `name` in the query of `address`.
pub(crate) fn query_value(address: &str, name: &str) -> Option<String> {
    let query = address.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or("");
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (decoded(key) == name).then(|| decoded(value))
    })
}

/// Whether two texts are the same, in time that does not say where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |differs, (a, b)| differs | (a ^ b))
            == 0
}

fn text(name: &str, value: &str) -> Part {
    Part::Text {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

impl Core {
    /// The family of systems this app is for, as the server's list of apps
    /// names them.
    fn family(&self) -> Option<SignInPlatform> {
        match self.config.client.platform {
            Platform::Ios
            | Platform::Ipados
            | Platform::Macos
            | Platform::Watchos
            | Platform::Tvos
            | Platform::Visionos => Some(SignInPlatform::Apple),
            Platform::Android | Platform::Wearos => Some(SignInPlatform::Android),
            _ => None,
        }
    }

    /// Opens the system's sign-in session. With a shell that names the
    /// hand-over's routes the session opens there, whatever page asked for
    /// it: the pages inside it offer every way in the app has. Without one,
    /// the session opens at `url`, an address inside.
    pub(crate) fn sign_in_session(&mut self, url: &str, ephemeral: bool) {
        if self.inner.sign_in.is_none() {
            // A session opens only at an address on one of the app's origins.
            let Some(url) = self.inner.origin.absolute(url) else {
                return self.notice(Notice::Failed, self.front());
            };
            self.inner.signing_in = true;
            self.effects.push(Effect::SignIn {
                url,
                callback: self.config.sign_in.callback.clone(),
                ephemeral,
            });
            return;
        }
        if self.family().is_none() || self.config.app_id.is_empty() {
            self.notice(Notice::Failed, self.front());
            return self.report(
                "sign_in",
                "a sign-in can't be handed over: the app's id or its system is not known"
                    .to_owned(),
            );
        }
        match self.inner.hand_over {
            // The bytes for one are already asked for, or its code is being exchanged.
            Some(HandOver::Wanted { .. } | HandOver::Exchanging) => {}
            // A session that is open, or none: this one takes its place.
            Some(HandOver::Open { .. }) | None => {
                self.inner.next.random += 1;
                let request = self.inner.next.random;
                self.inner.hand_over = Some(HandOver::Wanted { request, ephemeral });
                self.effects.push(Effect::Random {
                    request,
                    bytes: u32::try_from(RANDOM_BYTES).unwrap_or(48),
                });
            }
        }
    }

    /// The random bytes a sign-in waits for came: the session opens.
    pub(crate) fn random(&mut self, request: u64, hex: &str) {
        let Some(HandOver::Wanted {
            request: wanted,
            ephemeral,
        }) = self.inner.hand_over.clone()
        else {
            return;
        };
        if wanted != request {
            return;
        }
        self.inner.hand_over = None;
        let (Some(routes), Some(family)) = (self.inner.sign_in.clone(), self.family()) else {
            return;
        };
        let bytes = unhex(hex).unwrap_or_default();
        // Bytes that are all alike are no secret: a client that has no random
        // generator must not sign anybody in.
        let alike = bytes
            .first()
            .is_some_and(|first| bytes.iter().all(|byte| byte == first));
        if bytes.len() < RANDOM_BYTES || alike {
            self.notice(Notice::Failed, self.front());
            return self.report(
                "sign_in",
                format!(
                    "a sign-in needs {RANDOM_BYTES} random bytes; {} came",
                    bytes.len()
                ),
            );
        }
        let verifier = base64url(&bytes[..32]);
        let state = base64url(&bytes[32..RANDOM_BYTES]);
        let family = match family {
            SignInPlatform::Android => "android",
            _ => "apple",
        };
        let mut said = vec![
            text("platform", family),
            text("app", &self.config.app_id),
            text("code_challenge", &challenge_of(&verifier)),
            text("code_challenge_method", "S256"),
            text("state", &state),
        ];
        // Where the session comes back is handed to the system: only the
        // app's own link or the scheme it was built with.
        let callback = routes
            .callback
            .clone()
            .or_else(|| self.config.sign_in.callback.clone());
        if routes.callback.as_deref().is_some_and(|callback| {
            !self
                .inner
                .origin
                .callback_allowed(callback, self.config.sign_in.callback.as_deref())
        }) {
            self.notice(Notice::Failed, self.front());
            return self.report(
                "sign_in",
                "the shell's callback of a hand-over is neither the app's own link nor its scheme"
                    .to_owned(),
            );
        }
        if let Some(callback) = &routes.callback {
            said.push(text("redirect_uri", callback));
        }
        let Some(start) = self.inner.origin.absolute(routes.start.as_str()) else {
            self.notice(Notice::Failed, self.front());
            return self.report(
                "sign_in",
                "the shell's start of a hand-over is not on the app's origins".to_owned(),
            );
        };
        let url = format!(
            "{start}{}{}",
            if start.contains('?') { '&' } else { '?' },
            encode(&said)
        );
        self.inner.hand_over = Some(HandOver::Open { state, verifier });
        self.effects.push(Effect::SignIn {
            url,
            callback,
            ephemeral,
        });
    }

    /// Whether an address is the one a sign-in session comes back on.
    pub(crate) fn is_return(&self, address: &str) -> bool {
        let Some(routes) = &self.inner.sign_in else {
            return false;
        };
        let place = |address: &str| {
            address
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .trim_end_matches('/')
                .to_owned()
        };
        match &routes.callback {
            Some(callback) => place(address) == place(callback),
            // The client knows its own link: a link on the app's own scheme
            // that carries a code and a state is one.
            None => {
                let scheme = self.config.sign_in.callback.as_deref().unwrap_or("");
                let own = !scheme.is_empty()
                    && address
                        .strip_prefix(scheme)
                        .is_some_and(|rest| rest.starts_with(':'));
                own && query_value(address, "code").is_some()
                    && query_value(address, "state").is_some()
            }
        }
    }

    /// A return of a sign-in session that came as a link from outside. It
    /// is taken as the session's own only while a session is open, where the
    /// `state` is then held to; at any other time it is a link somebody
    /// planted, and nothing is done with it or shown for it.
    pub(crate) fn returned_as_a_link(&mut self, address: &str) {
        if matches!(self.inner.hand_over, Some(HandOver::Open { .. })) {
            return self.sign_in_done(Some(address));
        }
        self.report(
            "sign_in",
            "a sign-in's return arrived with no sign-in open, and was not used".to_owned(),
        );
    }

    /// The system's sign-in session ended: with the address it came back on,
    /// or with none when the person closed it.
    pub(crate) fn sign_in_done(&mut self, url: Option<&str>) {
        let Some(routes) = self.inner.sign_in.clone() else {
            return self.session_returned(url);
        };
        let Some(HandOver::Open { state, verifier }) = self.inner.hand_over.clone() else {
            // No session of the core's own is open: whatever this is, it is not used.
            if url.is_some() {
                self.report(
                    "sign_in",
                    "a sign-in's return arrived with no sign-in open, and was not used".to_owned(),
                );
            }
            return;
        };
        // The person closed the sheet: nothing changed, and the sign-in ends.
        let Some(url) = url else {
            self.inner.hand_over = None;
            return;
        };
        // A return is this sign-in's only with this sign-in's state, compared
        // before anything else is read of it. One with another state, or none,
        // changes nothing: the sign-in stays open.
        let returned = query_value(url, "state").unwrap_or_default();
        if !same(&returned, &state) {
            return self.report(
                "sign_in",
                "a sign-in's return carried a state that is not this sign-in's, and was not used"
                    .to_owned(),
            );
        }
        // One return for one session, whatever it holds.
        self.inner.hand_over = None;
        // The person said no: that is the sheet closed, and nothing failed.
        if query_value(url, "error").as_deref() == Some("access_denied") {
            return;
        }
        let (Some(code), Some(family)) = (
            query_value(url, "code").filter(|code| !code.is_empty()),
            self.family(),
        ) else {
            self.notice(Notice::Failed, self.front());
            return self.report("sign_in", "a sign-in's return carried no code".to_owned());
        };
        let exchange = SignInExchange {
            platform: family,
            app: self.config.app_id.clone(),
            code,
            code_verifier: verifier,
        };
        let Ok(body) = serde_json::to_string(&exchange) else {
            return;
        };
        if self.post_json(Pending::Exchange, routes.exchange.as_str(), body) {
            self.inner.hand_over = Some(HandOver::Exchanging);
        }
    }

    /// The exchange route answered. Only `200` is a sign-in.
    pub(crate) fn exchange_answered(&mut self, response: &Response) {
        self.inner.hand_over = None;
        let screen = self.front();
        match response.error {
            Some(TransportError::Timeout) => return self.failed(Failure::Timeout, screen),
            Some(_) => return self.failed(Failure::Offline, screen),
            None => {}
        }
        if response.status == 200 && response.left.is_none() {
            return self.signed_in();
        }
        // The code was used, ran out or is another app's; or the server would
        // not hear it. The person starts again.
        self.notice(Notice::Failed, screen);
        self.report(
            "sign_in",
            format!("the exchange route answered {}", response.status),
        );
    }

    /// Somebody may be signed in now: the shell says who, and everything on
    /// screen is theirs to see again.
    pub(crate) fn signed_in(&mut self) {
        self.who_may_have_changed(true);
        for inner in self.inner.screens.values_mut() {
            inner.stale = true;
        }
        self.inner.epoch += 1;
        self.forget_pages();
        // A way in that a session's end put up has done its work.
        if let Some(cover) = self.view.modals.iter().position(|modal| modal.locked) {
            self.take_modals_from(cover);
            self.touch(Change::Nav);
        }
        if let Some(front) = self.front() {
            self.fetch_page(front, false);
        }
    }

    /// A way in has done its work: a write from the screen `id` led away from
    /// it. It goes, with any way in a session's end had put up under it, and
    /// the shell is fetched again: the shell says who is signed in now.
    pub(crate) fn way_in_done(&mut self, id: ScreenId) {
        match self
            .view
            .modals
            .iter()
            .position(|modal| modal.locked || modal.stack.contains(&id))
        {
            Some(at) => {
                self.take_modals_from(at);
                self.touch(Change::Nav);
            }
            None => self.dismissed(id),
        }
        self.who_may_have_changed(true);
    }

    /// A session with no hand-over ended: where it came back to is opened
    /// with the app's own cookies.
    fn session_returned(&mut self, url: Option<&str>) {
        if !std::mem::take(&mut self.inner.signing_in) {
            return;
        }
        // The person gave up: nothing changed.
        let Some(url) = url else {
            return;
        };
        // Who is signed in may have changed, and with it everything on screen.
        self.who_may_have_changed(true);
        for inner in self.inner.screens.values_mut() {
            inner.stale = true;
        }
        self.inner.epoch += 1;
        self.forget_pages();
        self.opened(url);
    }

    /// A sign-in the app carried out itself needs a second step. Its page is
    /// opened inside the app, over what is there, with where to go afterwards.
    /// False when neither the call nor the shell names a page for it.
    pub(crate) fn second_step(&mut self, own: Option<&str>) -> bool {
        let shell = self
            .inner
            .sign_in
            .as_ref()
            .and_then(|routes| routes.second_step.as_ref())
            .map(|page| page.as_str().to_owned());
        let Some(page) = own.map(str::to_owned).or(shell) else {
            return false;
        };
        // Afterwards: the screen the person was on, under any way in that is up.
        let locked: Vec<ScreenId> = self
            .view
            .modals
            .iter()
            .filter(|modal| modal.locked)
            .flat_map(|modal| modal.stack.clone())
            .collect();
        let under = self
            .view
            .modals
            .iter()
            .rev()
            .filter(|modal| !modal.locked)
            .find_map(|modal| modal.stack.last().copied());
        let under = under.or_else(|| {
            self.view
                .destinations
                .get(self.view.selected)
                .and_then(|destination| destination.stack.last().copied())
        });
        let next = under
            .filter(|id| !locked.contains(id))
            .and_then(|id| self.view.screens.get(&id))
            .map_or_else(|| "/".to_owned(), |view| view.url.clone());
        let address = format!(
            "{page}{}{}",
            if page.contains('?') { '&' } else { '?' },
            encode(&[text("next", &next)])
        );
        let Target::Inside(url) = self.inner.origin.resolve(&address, "/") else {
            return false;
        };
        let id = self.open(&url);
        if let (Some(view), Some(inner)) = (
            self.view.screens.get_mut(&id),
            self.inner.screens.get_mut(&id),
        ) {
            view.chrome = false;
            // When a write from it leads away from this page, it has done its work.
            inner.cover = Some(path_of(&url));
            inner.second_step = true;
        }
        self.view.modals.push(Modal {
            present: Present::Task,
            locked: false,
            stack: vec![id],
        });
        self.touch(Change::Nav);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_of_a_verifier_is_the_one_the_standard_gives() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        // FIPS 180-4: the hash of nothing, and of "abc".
        assert_eq!(
            fingerprint(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            fingerprint("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn bytes_are_written_as_base64_for_an_address_without_padding() {
        // RFC 4648, section 10, with the address-safe alphabet and no padding.
        for (bytes, written) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64url(bytes.as_bytes()), written);
        }
        assert_eq!(base64url(&[0xfb, 0xff, 0xfe]), "-__-");
        // 32 bytes make the 43 characters a verifier needs at least.
        assert_eq!(base64url(&[7; 32]).len(), 43);
    }

    #[test]
    fn hex_and_a_query_are_read() {
        assert_eq!(unhex("00ff10"), Some(vec![0, 255, 16]));
        assert_eq!(unhex("0"), None);
        assert_eq!(unhex("zz"), None);
        let back = "https://charkha.example/app/signed-in?code=Tq7%2BuX0b2&state=a%20b+c#top";
        assert_eq!(query_value(back, "code").as_deref(), Some("Tq7+uX0b2"));
        assert_eq!(query_value(back, "state").as_deref(), Some("a b c"));
        assert_eq!(query_value(back, "error"), None);
        assert_eq!(query_value("shop:/signed-in", "code"), None);
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }
}

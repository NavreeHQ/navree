//! [`Shell`]: the app's chrome as data. A client fetches it once from
//! [`SHELL_PATH`](crate::SHELL_PATH), and again when the `build` of an answer
//! changes, and draws it as its system does: a tab bar on a phone, a sidebar
//! on a tablet or a desktop, a list on a watch.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::action::Present;
use crate::device::OriginPath;
use crate::node::{Icon, Node};

fn is_true(value: &bool) -> bool {
    *value
}

fn yes() -> bool {
    true
}

/// One destination of the app.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavItem {
    /// Its name.
    pub label: String,
    /// Its icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Its address.
    pub href: String,
    /// A count beside it, in the page's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// The destinations under it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<NavItem>,
}

/// A group of destinations, with or without a label.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavGroup {
    /// The group's label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Its destinations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<NavItem>,
}

/// The app's destinations.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nav {
    /// The groups, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<NavGroup>,
}

/// The person signed in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// Their name.
    pub name: String,
    /// Their email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// What their menu offers: links and buttons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub menu: Vec<Node>,
}

/// How the screens under an address are presented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// The address, where `*` stands for one segment: `/orders/*/edit`.
    #[serde(rename = "match")]
    pub matches: String,
    /// How a link to it is presented.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub present: Option<Present>,
    /// Whether the app's navigation stays around it. A sign-in screen has none.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub chrome: bool,
}

/// The brand's part in the look: it tints the system's own theme and never
/// replaces it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// The theme's name in the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The accent, as `#rrggbb`, on a light ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// The accent on a dark ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_dark: Option<String>,
}

/// Where a client registers for push messages: the one route a device's push
/// token reaches the server by.
///
/// The token never travels through a page's call: [`RegisterPush`] only asks
/// for the system's permission, and the client posts the token itself, as
/// JSON, to `register`, on the app's own origin, with the session's cookies
/// and the session's token in `x-xsrf-token`. It does so at every launch, once
/// the system hands it a token, and again whenever the token changes. The body
/// is [`PushDevice`](crate::device::PushDevice).
///
/// - `201`: the device is registered. The answer never holds the token.
/// - `401`: nobody is signed in. `403`: the sign-in has no session (a personal
///   API token). `400`: a body that does not read, or a platform the route does
///   not know. `422`: a token or an app id that is wrong, as
///   `{"errors": {"token": [...]}}`. `429`: more than 30 registrations and
///   removals in a minute.
///
/// A device is taken away with `DELETE` to the same route and the body
/// `{"token": "..."}`: `204`, or `404` when it is not this person's. A client
/// does so when the person signs out, before the session ends (see
/// [`Op::SignedOut`](crate::Op::SignedOut)); the server also ends a device with
/// its session, so a phone that is signed out stops receiving even when the
/// request never arrives.
///
/// [`RegisterPush`]: crate::device::RegisterPush
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Push {
    /// The route a device's token is posted to and deleted from:
    /// `/api/notifications/push-devices`.
    pub register: OriginPath,
}

/// The device features the app uses.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// The features, by name. They can only narrow what the client was built
    /// with, never widen it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<String>,
    /// Push messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push: Option<Push>,
    /// Whether the person may lock the app behind the device's own unlock,
    /// and after how long away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<crate::device::AppLock>,
}

/// How the app signs a person in on its own sign-in pages, in the system's own
/// sign-in session, and how that sign-in is handed over to the app: an
/// authorization code proved with a PKCE verifier (S256 only). The pages offer
/// every way in the app has, in as many steps as it takes. Every route is on
/// the app's own origin.
///
/// 1. The client makes a `code_verifier` (43 to 128 characters of
///    `A-Za-z0-9-._~`), the `code_challenge` (the unpadded base64url of the
///    verifier's SHA-256) and a random `state`.
/// 2. It opens the system's sign-in session (`ASWebAuthenticationSession` on
///    Apple systems, an Auth Tab or a Custom Tab on Android) at `start` with the
///    query `platform` (`apple` or `android`), `app` (the app's bundle id or
///    package name), `code_challenge`, `code_challenge_method=S256` and `state`,
///    and `redirect_uri` when it sends one, which must be `callback`. The route
///    answers `400` (`invalid_hand_over`) for an app it does not know, an app
///    with no callback, a missing, malformed or `plain` challenge, or another
///    `redirect_uri`.
/// 3. The person signs in on the app's pages inside the session, which has
///    cookies of its own. Once they are signed in, the session ends at the
///    app's callback with `?code=`, a one-time code of 43 characters that works
///    for 60 seconds, and the client's `state`.
/// 4. The client takes the callback only while its own sign-in session is open
///    and only with its own `state`. A callback that arrives any other way (an
///    ordinary link another app opened) is never exchanged.
/// 5. The client posts [`SignInExchange`] as JSON to `exchange` from its own
///    HTTP stack, with the session's token in `x-xsrf-token` (the cookie comes
///    with any earlier `GET` that set it). `200`: the person is signed in, with
///    a session of the app's own that lasts as a remember-me one does; the
///    answer sets the session cookie and a new `XSRF-TOKEN` and holds the user,
///    and the client fetches the shell and the screens again. `400`
///    (`invalid_hand_over`): the code was used, expired, is another app's, or
///    the verifier is wrong (the first attempt burns the code): the person
///    starts again at 1. `403`: no session token. `429`: more than 30 tries in
///    a minute.
/// 6. The client forgets the verifier after one exchange, whatever came of it.
///
/// The sign-in session's browser is signed out again when the hand-over ends,
/// unless it was signed in before the hand-over began. An app whose callback is
/// a custom scheme, which another app could claim, never gets a sign-in the
/// browser already had: the person signs in afresh.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignIn {
    /// Where the system's sign-in session starts: `/auth/app/sign-in`. It is
    /// opened in the session, never in the app's own web view, so the sign-in
    /// pages of identity providers accept it.
    pub start: OriginPath,
    /// Where the code is exchanged for a session: `/api/auth/app/exchange`.
    pub exchange: OriginPath,
    /// The app's own link the sign-in session comes back on, as an absolute
    /// link: an `https` link the system opens in the app (a universal link, an
    /// App Link) or, only where the server allows one, a link of the app's own
    /// scheme. Left out when the client knows its own; a server that has one
    /// for each platform answers each client with its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback: Option<String>,
    /// The page of the app that finishes a second step (a code from an
    /// authenticator app, a recovery code) after a sign-in the app carried out
    /// itself, such as a [`PlatformSignIn`](crate::device::PlatformSignIn):
    /// `/sign-in/two-factor`. The client opens it inside the app, with the app's
    /// own cookies, never in the system's sign-in session, which does not have
    /// them, and with `?next=` the address to go to afterwards. The page answers
    /// with a redirect to `next` once the second step is done, and the client
    /// then treats the sign-in as complete. A call that names a page of its own
    /// uses that one instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_step: Option<OriginPath>,
}

/// Which family of systems an app is for, as the server's list of apps names
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SignInPlatform {
    /// Apple's systems: the app is named by its bundle id.
    Apple,
    /// Android: the app is named by its package name.
    Android,
}

/// What a client posts to [`SignIn::exchange`], as JSON: the code the callback
/// carried and the verifier it made. The server holds the verifier's SHA-256
/// against the challenge the sign-in session was started with.
///
/// ```
/// use navree_native::shell::{SignInExchange, SignInPlatform};
///
/// let exchange = SignInExchange {
///     platform: SignInPlatform::Apple,
///     app: "com.charkha.supply".to_owned(),
///     code: "Tq7uX0b2".to_owned(),
///     code_verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_owned(),
/// };
/// let json = serde_json::to_value(&exchange).unwrap();
/// assert_eq!(json["platform"], "apple");
/// assert_eq!(json["code_verifier"], "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
/// // What it shows when it is logged holds neither the code nor the verifier.
/// assert!(!format!("{exchange:?}").contains("dBjftJeZ"));
/// ```
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInExchange {
    /// The family of systems the app is for.
    pub platform: SignInPlatform,
    /// The app's bundle id or package name, as it started the sign-in session.
    pub app: String,
    /// The one-time code from the callback.
    pub code: String,
    /// The verifier whose S256 challenge started the sign-in session: 43 to 128
    /// characters. It is secret until it is posted.
    pub code_verifier: String,
}

impl fmt::Debug for SignInExchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignInExchange")
            .field("platform", &self.platform)
            .field("app", &self.app)
            .field("code", &"[redacted]")
            .field("code_verifier", &"[redacted]")
            .finish()
    }
}

/// The app's chrome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shell {
    /// The protocol's major version.
    pub navree: u32,
    /// The server's build.
    pub build: String,
    /// The document's stamp, which every envelope carries as `shell` while
    /// this document is in force. [`Shell::stamped`] sets it from the rest of
    /// the document.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stamp: String,
    /// The app's name.
    pub name: String,
    /// A line under the name: the workspace, the environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// The address of the app's first screen.
    pub home: String,
    /// The brand's part in the look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// The destinations.
    #[serde(default)]
    pub nav: Nav,
    /// The person signed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// How the screens under an address are presented, the first match first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
    /// The device features the app uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
    /// How the app signs a person in and hands the sign-in over to the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<SignIn>,
}

impl Shell {
    /// The chrome of the app `name`, with nothing in it yet.
    pub fn new(build: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            navree: crate::MAJOR,
            build: build.into(),
            stamp: String::new(),
            name: name.into(),
            context: None,
            home: "/".to_owned(),
            theme: None,
            nav: Nav::default(),
            user: None,
            routes: Vec::new(),
            device: None,
            sign_in: None,
        }
    }

    /// The same document, stamped by what it holds: a 64-bit FNV-1a hash of
    /// its JSON without the stamp, as 16 hex digits. The same document always
    /// gets the same stamp, so a server need not keep it.
    #[must_use]
    pub fn stamped(mut self) -> Self {
        self.stamp.clear();
        let json = serde_json::to_vec(&self).unwrap_or_default();
        let hash = json.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
        self.stamp = format!("{hash:016x}");
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_keeps_the_chrome_unless_it_says_not() {
        let routes: Vec<Route> = serde_json::from_str(
            r#"[{"match":"/orders/new","present":"sheet"},{"match":"/sign-in","present":"full","chrome":false}]"#,
        )
        .unwrap();
        assert!(routes[0].chrome);
        assert_eq!(routes[0].present, Some(Present::Sheet));
        assert!(!routes[1].chrome);
        assert_eq!(
            serde_json::to_string(&routes).unwrap(),
            r#"[{"match":"/orders/new","present":"sheet"},{"match":"/sign-in","present":"full","chrome":false}]"#
        );
    }

    #[test]
    fn a_shell_names_the_routes_the_core_calls() {
        let shell: Shell = serde_json::from_str(
            r#"{"navree":1,"build":"b","name":"Charkha Supply Co.","home":"/",
                "sign_in":{"start":"/auth/app/sign-in","exchange":"/api/auth/app/exchange",
                           "callback":"https://charkha.example/app/signed-in",
                           "second_step":"/sign-in/two-factor"},
                "device":{"push":{"register":"/api/notifications/push-devices"}}}"#,
        )
        .unwrap();
        let sign_in = shell.sign_in.as_ref().unwrap();
        assert_eq!(sign_in.start.as_str(), "/auth/app/sign-in");
        assert_eq!(sign_in.exchange.as_str(), "/api/auth/app/exchange");
        assert_eq!(sign_in.callback.as_deref(), Some("https://charkha.example/app/signed-in"));
        assert_eq!(sign_in.second_step.as_ref().map(OriginPath::as_str), Some("/sign-in/two-factor"));
        let push = shell.device.as_ref().and_then(|device| device.push.as_ref()).unwrap();
        assert_eq!(push.register.as_str(), "/api/notifications/push-devices");
        // Only what the shell names is written; a shell with none says nothing of it.
        let written = serde_json::to_value(Shell::new("b", "Charkha Supply Co.")).unwrap();
        assert!(written.get("sign_in").is_none());
        let minimal: SignIn =
            serde_json::from_str(r#"{"start":"/auth/app/sign-in","exchange":"/api/auth/app/exchange"}"#).unwrap();
        assert_eq!((minimal.callback, minimal.second_step), (None, None));
    }

    #[test]
    fn a_route_of_the_shell_can_not_leave_the_origin() {
        for json in [
            r#"{"start":"https://evil.example/sign-in","exchange":"/api/auth/app/exchange"}"#,
            r#"{"start":"/auth/app/sign-in","exchange":"//evil.example/exchange"}"#,
            r#"{"start":"/auth/app/sign-in","exchange":"/api/auth/app/exchange","second_step":"/a/../b"}"#,
            r#"{"start":"/auth/app/sign-in"}"#,
        ] {
            assert!(serde_json::from_str::<SignIn>(json).is_err(), "{json}");
        }
        assert!(serde_json::from_str::<Push>(r#"{"register":"https://evil.example/push"}"#).is_err());
    }

    #[test]
    fn the_exchange_is_written_as_the_server_reads_it_and_shows_no_secret() {
        let exchange = SignInExchange {
            platform: SignInPlatform::Android,
            app: "com.charkha.supply".to_owned(),
            code: "k3xq-one-time-code".to_owned(),
            code_verifier: "a-verifier-of-at-least-forty-three-characters-0123456789".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&exchange).unwrap(),
            concat!(
                r#"{"platform":"android","app":"com.charkha.supply","code":"k3xq-one-time-code","#,
                r#""code_verifier":"a-verifier-of-at-least-forty-three-characters-0123456789"}"#
            )
        );
        let shown = format!("{exchange:?}");
        assert!(shown.contains("com.charkha.supply"), "{shown}");
        assert!(!shown.contains("k3xq") && !shown.contains("verifier-of"), "{shown}");
        // The server refuses a platform it has no app list for.
        assert!(serde_json::from_str::<SignInExchange>(
            r#"{"platform":"windows","app":"a","code":"c","code_verifier":"v"}"#
        )
        .is_err());
    }

    #[test]
    fn a_shell_is_stamped_by_what_it_holds() {
        let shell = Shell::new("b1", "Charkha Supply Co.").stamped();
        assert_eq!(shell.stamp.len(), 16);
        assert!(shell.stamp.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(shell.clone().stamped(), shell, "stamping twice changes nothing");
        let mut renamed = shell.clone();
        renamed.home = "/orders".to_owned();
        assert_ne!(renamed.stamped().stamp, shell.stamp);
    }
}

//! Device features: the closed catalogue of what a page may ask of the device
//! a native client runs on, the calls that ask, and the results posted back.
//!
//! The types keep two kinds of call apart:
//! - [`Call`]: what a tap on a control may start, as
//!   [`Action::Device`](crate::Action::Device). Most of these prompt for a
//!   permission, show the system's own sheet or return something private, so
//!   they run only from a person's tap.
//! - [`ReplyCall`]: what a server's reply may start, as
//!   [`Op::Device`](crate::Op::Device): only calls that need no tap and show
//!   nothing private (a haptic, the app's badge, cancelling a notification,
//!   the store's review prompt). A call that needs a tap has no variant here,
//!   so it can't be put in an op.
//!
//! A call with a result says where the result is posted ([`Then`]), with a
//! [`Ticket`] the server sealed when it wrote the call. The client posts the
//! ticket back as [`TICKET_FIELD`] and the outcome as [`OUTCOME_FIELD`]
//! (`ok` with the result's fields, or a [`Refusal`]), as a form. A post is
//! refused without a ticket, after it expires, from another session, for
//! another feature, route or call, and the second time.
//!
//! Two calls also speak to the framework's own routes, which read JSON: a
//! [`Confirm`] spends its ticket at its challenge route and posts its result
//! with none; a [`PlatformSignIn`] posts its token to its own sign-in route,
//! with the sealed one-time state its nonce route gave, and has no `then`.
//! Each says how.
//!
//! What a call's arguments and results hold is the client's to fill and the
//! server's to doubt: a location or a scanned code is a claim. Only a
//! signature ([`Confirmed`]) grants anything.

use std::fmt;

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::node::{is_default, is_false};

/// The form field a result post carries its [`Ticket`] in.
pub const TICKET_FIELD: &str = "_call";

/// The form field a result post says how the call went in: [`OUTCOME_OK`], or
/// a [`Refusal`] by name.
pub const OUTCOME_FIELD: &str = "outcome";

/// The outcome of a call that produced its result. The result's own fields
/// are posted beside it.
pub const OUTCOME_OK: &str = "ok";

/// A device feature: the closed catalogue a client is built with. A client
/// refuses a feature it was not built with, or that its system lacks, with a
/// [`Refusal`], and draws the call's [`Alt`] instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Feature {
    /// A tap, a buzz or a knock under the finger.
    Haptic,
    /// Copying text (never reading it).
    Clipboard,
    /// The system's share sheet.
    Share,
    /// The camera, the photo library and the files, through a form's file field.
    Capture,
    /// The system's sign-in session, for signing in through another site.
    AuthSession,
    /// Opening a page outside the app, or in the system's in-app browser.
    OpenUrl,
    /// Push messages.
    Push,
    /// Notifications the device schedules itself, and the app's badge.
    LocalNotification,
    /// Passkeys: signing in and registering one.
    Passkey,
    /// Approving an action with a signature the person's face, finger or
    /// device code unlocks.
    Confirm,
    /// Where the device is, once.
    LocationOnce,
    /// Reading a barcode or a QR code with the camera.
    ScanCode,
    /// Locking the app behind the device's own unlock.
    AppLock,
    /// Signing in with Apple or Google, in the system's own sheet.
    PlatformSignIn,
    /// The store's rating prompt.
    RequestReview,
    /// This app's page in the system's settings.
    OpenSettings,
}

impl Feature {
    /// Every feature, in the order of the catalogue.
    pub const ALL: [Self; 16] = [
        Self::Haptic,
        Self::Clipboard,
        Self::Share,
        Self::Capture,
        Self::AuthSession,
        Self::OpenUrl,
        Self::Push,
        Self::LocalNotification,
        Self::Passkey,
        Self::Confirm,
        Self::LocationOnce,
        Self::ScanCode,
        Self::AppLock,
        Self::PlatformSignIn,
        Self::RequestReview,
        Self::OpenSettings,
    ];

    /// The feature as the descriptor's `caps` and a shell's `device.caps`
    /// name it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Haptic => "haptic",
            Self::Clipboard => "clipboard",
            Self::Share => "share",
            Self::Capture => "capture",
            Self::AuthSession => "auth_session",
            Self::OpenUrl => "open_url",
            Self::Push => "push",
            Self::LocalNotification => "local_notification",
            Self::Passkey => "passkey",
            Self::Confirm => "confirm",
            Self::LocationOnce => "location_once",
            Self::ScanCode => "scan_code",
            Self::AppLock => "app_lock",
            Self::PlatformSignIn => "platform_sign_in",
            Self::RequestReview => "request_review",
            Self::OpenSettings => "open_settings",
        }
    }

    /// The feature named `name`, or nothing.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| feature.as_str() == name)
    }
}

/// Why a call produced no result. Posted as [`OUTCOME_FIELD`] by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Refusal {
    /// This system, version or form factor has no way to do it.
    Unsupported,
    /// The app was not built with the feature.
    NotBuilt,
    /// The person said no, now or before.
    Denied,
    /// Something above the person forbids it: device management, parental
    /// controls, the region.
    Restricted,
    /// The hardware is missing, nothing is enrolled, or a device key was
    /// invalidated.
    Unavailable,
    /// The person closed the system's sheet.
    Cancelled,
    /// A call of the same kind is already running.
    Busy,
    /// Anything else. The detail stays in the client's own debug log.
    Failed,
}

/// Why text is not a path on the app's own origin.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathError {
    /// It does not start with a single `/`: it has a scheme, or is relative.
    NotAPath,
    /// It names another host: it starts with `//`, also once decoded.
    OtherOrigin,
    /// It holds a backslash, which some systems read as a slash.
    Backslash,
    /// It climbs out of a folder with `..`.
    DotSegment,
    /// It holds a control character, a space, or an escape that is not one.
    Control,
    /// It is longer than [`OriginPath::MAX`] bytes.
    TooLong,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAPath => "not a path that starts with a single `/`",
            Self::OtherOrigin => "a path that names another host",
            Self::Backslash => "a path with a backslash",
            Self::DotSegment => "a path that climbs with `..`",
            Self::Control => "a path with a control character, a space or a bad escape",
            Self::TooLong => "a path that is too long",
        })
    }
}

impl std::error::Error for PathError {}

/// Decodes `%XX` escapes once. `None` when an escape is not two hex digits.
fn decode_once(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = bytes.get(at + 1..at + 3)?;
            let text = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(text, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    Some(out)
}

fn is_control(byte: u8) -> bool {
    byte <= b' ' || byte == 0x7f
}

/// A path, with its query, on the app's own origin: where a result is posted
/// or a client is sent. It can't hold a scheme or a host, so a server can't
/// point a device result at another site; that is checked when it is made and
/// when it is read.
///
/// ```
/// use navree_native::device::{OriginPath, PathError};
///
/// assert_eq!(OriginPath::parse("/payments/42/approve").unwrap().as_str(), "/payments/42/approve");
/// assert_eq!(OriginPath::parse("//evil.example"), Err(PathError::OtherOrigin));
/// assert_eq!(OriginPath::parse("/%2F%2Fevil.example"), Err(PathError::OtherOrigin));
/// assert_eq!(OriginPath::parse("https://evil.example/"), Err(PathError::NotAPath));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OriginPath(String);

impl OriginPath {
    /// The most bytes a path may hold.
    pub const MAX: usize = 2048;

    /// Reads `text` as a path on the app's own origin.
    pub fn parse(text: &str) -> Result<Self, PathError> {
        if text.len() > Self::MAX {
            return Err(PathError::TooLong);
        }
        if text.bytes().any(is_control) {
            return Err(PathError::Control);
        }
        let path = text.split(['?', '#']).next().unwrap_or_default();
        if !path.starts_with('/') {
            return Err(PathError::NotAPath);
        }
        // Once decoded, a space is a name's own (`report%202026.pdf`); a line
        // break or a NUL is not.
        let decoded = decode_once(text).ok_or(PathError::Control)?;
        if decoded.iter().any(|byte| *byte < b' ' || *byte == 0x7f) {
            return Err(PathError::Control);
        }
        let decoded_path = decode_once(path).ok_or(PathError::Control)?;
        if decoded_path.contains(&b'\\') || path.contains('\\') {
            return Err(PathError::Backslash);
        }
        if decoded_path.starts_with(b"//") || path.starts_with("//") {
            return Err(PathError::OtherOrigin);
        }
        if decoded_path.split(|byte| *byte == b'/').any(|segment| segment == b"..") {
            return Err(PathError::DotSegment);
        }
        Ok(Self(text.to_owned()))
    }

    /// The path and its query, as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path without its query or fragment.
    pub fn path(&self) -> &str {
        self.0.split(['?', '#']).next().unwrap_or_default()
    }
}

impl fmt::Display for OriginPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&str> for OriginPath {
    type Error = PathError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        Self::parse(text)
    }
}

impl Serialize for OriginPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for OriginPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

/// Why text is not an `https` address on another site.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UrlError {
    /// It does not start with `https://`.
    NotHttps,
    /// It names no host.
    NoHost,
    /// It carries a user name or password before the host.
    UserInfo,
    /// It holds a control character, a space or a backslash.
    Control,
    /// It is longer than [`HttpsUrl::MAX`] bytes.
    TooLong,
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotHttps => "not an `https://` address",
            Self::NoHost => "an address with no host",
            Self::UserInfo => "an address with a user name before its host",
            Self::Control => "an address with a control character, a space or a backslash",
            Self::TooLong => "an address that is too long",
        })
    }
}

impl std::error::Error for UrlError {}

/// An `https` address on another site, for a call that leaves the app. It
/// can't carry another scheme, or a user name before the host.
///
/// ```
/// use navree_native::device::{HttpsUrl, UrlError};
///
/// assert!(HttpsUrl::parse("https://pay.example/session/7").is_ok());
/// assert_eq!(HttpsUrl::parse("http://pay.example/"), Err(UrlError::NotHttps));
/// assert_eq!(HttpsUrl::parse("https://me@evil.example/"), Err(UrlError::UserInfo));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HttpsUrl(String);

impl HttpsUrl {
    /// The most bytes an address may hold.
    pub const MAX: usize = 4096;

    /// Reads `text` as an `https` address.
    pub fn parse(text: &str) -> Result<Self, UrlError> {
        if text.len() > Self::MAX {
            return Err(UrlError::TooLong);
        }
        if text.bytes().any(|byte| is_control(byte) || byte == b'\\') {
            return Err(UrlError::Control);
        }
        let Some(rest) = text.strip_prefix("https://") else {
            return Err(UrlError::NotHttps);
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.contains('@') {
            return Err(UrlError::UserInfo);
        }
        let host = match authority.rsplit_once(':') {
            Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => host,
            _ => authority,
        };
        if host.is_empty() {
            return Err(UrlError::NoHost);
        }
        Ok(Self(text.to_owned()))
    }

    /// The address, as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HttpsUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<&str> for HttpsUrl {
    type Error = UrlError;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        Self::parse(text)
    }
}

impl Serialize for HttpsUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for HttpsUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

/// What seals and opens tickets: an authenticated cipher whose key only the
/// server holds. A ticket that does not open, or opens to something else, is
/// refused.
pub trait Sealer {
    /// Seals `plain` into text a client carries without being able to read or
    /// change it.
    fn seal(&self, plain: &[u8]) -> String;

    /// Opens what [`seal`](Self::seal) wrote; `None` when it was not sealed
    /// with this key or was changed.
    fn open(&self, sealed: &str) -> Option<Vec<u8>>;
}

/// What a ticket holds once it is opened. Only the server reads it, and its
/// `Debug` leaves out the session and the nonce, which are never logged.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// The feature the call belongs to.
    pub feature: Feature,
    /// What the ticket is for: a digest of the call's arguments and of the
    /// fields the handler binds (a payment's id and amount). The server
    /// computes it the same way when it writes the call and when the result
    /// comes back.
    pub digest: String,
    /// The session the ticket was written for.
    pub session: String,
    /// Where the result may be posted.
    pub href: OriginPath,
    /// When the ticket stops being good, in seconds since 1970 (UTC).
    pub exp: i64,
    /// What makes the ticket one of a kind: it is spent on first use.
    pub nonce: String,
}

impl fmt::Debug for Claims {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Claims")
            .field("feature", &self.feature)
            .field("digest", &self.digest)
            .field("href", &self.href)
            .field("exp", &self.exp)
            .finish_non_exhaustive()
    }
}

/// What a handler expects of the ticket a result came with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expected<'a> {
    /// The feature whose result it reads.
    pub feature: Feature,
    /// The digest it computes from the call and the posted fields.
    pub digest: &'a str,
    /// The session of the request.
    pub session: &'a str,
    /// The path the result was posted to, without its query.
    pub href: &'a str,
}

/// Why a ticket was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TicketError {
    /// The post carried no ticket.
    Missing,
    /// The ticket does not open with the server's key, or was changed.
    Unreadable,
    /// The ticket's time is up.
    Expired,
    /// The ticket was written for another session.
    OtherSession,
    /// The ticket was written for another feature.
    OtherFeature,
    /// The ticket was written for another route.
    OtherRoute,
    /// The ticket was written for another call: an argument changed.
    OtherCall,
    /// The ticket was used before.
    Used,
}

impl fmt::Display for TicketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Missing => "the result came with no ticket",
            Self::Unreadable => "the ticket does not open",
            Self::Expired => "the ticket has expired",
            Self::OtherSession => "the ticket belongs to another session",
            Self::OtherFeature => "the ticket is for another feature",
            Self::OtherRoute => "the ticket is for another route",
            Self::OtherCall => "the ticket is for another call",
            Self::Used => "the ticket was used before",
        })
    }
}

impl std::error::Error for TicketError {}

/// A sealed, one-time ticket: the server writes it into a call that posts a
/// result, and takes it back with the result. A client can carry it but can't
/// read, make or change one. Its text is never written to a log, so `Debug`
/// shows none of it.
///
/// ```
/// use navree_native::device::{Claims, Expected, Feature, OriginPath, Sealer, Ticket, TicketError};
/// use std::collections::HashSet;
///
/// // A stand-in for the server's cipher.
/// struct Plain;
/// impl Sealer for Plain {
///     fn seal(&self, plain: &[u8]) -> String {
///         format!("sealed:{}", String::from_utf8_lossy(plain))
///     }
///     fn open(&self, sealed: &str) -> Option<Vec<u8>> {
///         sealed.strip_prefix("sealed:").map(|plain| plain.as_bytes().to_vec())
///     }
/// }
///
/// let claims = Claims {
///     feature: Feature::LocationOnce,
///     digest: "d1".to_owned(),
///     session: "s1".to_owned(),
///     href: OriginPath::parse("/stores/nearest").unwrap(),
///     exp: 1_800_000_000,
///     nonce: "n1".to_owned(),
/// };
/// let ticket = Ticket::seal(&claims, &Plain);
/// let expected = Expected {
///     feature: Feature::LocationOnce,
///     digest: "d1",
///     session: "s1",
///     href: "/stores/nearest",
/// };
/// let mut spent = HashSet::new();
/// let mut spend = |nonce: &str, _exp: i64| spent.insert(nonce.to_owned());
/// assert_eq!(ticket.redeem(&Plain, &expected, 1_700_000_000, &mut spend), Ok(claims));
/// assert_eq!(
///     ticket.redeem(&Plain, &expected, 1_700_000_000, &mut spend),
///     Err(TicketError::Used)
/// );
/// ```
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ticket(String);

impl fmt::Debug for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ticket(..)")
    }
}

impl From<String> for Ticket {
    /// The ticket as a result post carried it, to be opened.
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl Ticket {
    /// The most bytes a ticket's text holds. It is text a form and an address
    /// carry as they are: letters, digits, `-`, `_` and `.`.
    pub const MAX: usize = 4096;

    /// Seals `claims` with the server's key.
    pub fn seal(claims: &Claims, sealer: &impl Sealer) -> Self {
        let plain = serde_json::to_vec(claims).unwrap_or_default();
        Self(sealer.seal(&plain))
    }

    /// The sealed text, as a call carries it.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Opens the ticket, checking nothing but that it is the server's. Text
    /// longer than [`MAX`](Self::MAX) is not opened.
    pub fn open(&self, sealer: &impl Sealer) -> Result<Claims, TicketError> {
        if self.0.is_empty() {
            return Err(TicketError::Missing);
        }
        if self.0.len() > Self::MAX {
            return Err(TicketError::Unreadable);
        }
        let plain = sealer.open(&self.0).ok_or(TicketError::Unreadable)?;
        serde_json::from_slice(&plain).map_err(|_| TicketError::Unreadable)
    }

    /// Opens the ticket, checks it against what the handler expects at `now`
    /// (seconds since 1970, UTC), and spends it. `spend` records the nonce
    /// until the ticket's expiry and says whether it was new: it must do both
    /// at once (an insert that fails when the key is there), so two posts of
    /// the same result can't both pass.
    pub fn redeem(
        &self,
        sealer: &impl Sealer,
        expected: &Expected<'_>,
        now: i64,
        spend: impl FnOnce(&str, i64) -> bool,
    ) -> Result<Claims, TicketError> {
        let claims = self.open(sealer)?;
        if now >= claims.exp {
            return Err(TicketError::Expired);
        }
        if claims.session != expected.session {
            return Err(TicketError::OtherSession);
        }
        if claims.feature != expected.feature {
            return Err(TicketError::OtherFeature);
        }
        if claims.href.path() != expected.href {
            return Err(TicketError::OtherRoute);
        }
        if claims.digest != expected.digest {
            return Err(TicketError::OtherCall);
        }
        if !spend(&claims.nonce, claims.exp) {
            return Err(TicketError::Used);
        }
        Ok(claims)
    }
}

/// Where a call's result is posted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Then {
    /// The route on the app's own origin the result is posted to, as a form.
    pub href: OriginPath,
    /// The ticket the post carries back. For a [`Confirm`] it is the one posted
    /// to the call's `challenge` route, which spends it: the result is posted
    /// without one.
    pub ticket: Ticket,
    /// When the ticket expires, in seconds since 1970 (UTC), so a client
    /// showing a kept screen fetches it again instead of posting a dead ticket.
    pub exp: i64,
}

/// What a client shows instead when a call can't start here: the system lacks
/// it, or the app was not built with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Alt {
    /// Nothing: the control is not drawn.
    Hide,
    /// The control leads to this page instead.
    Navigate {
        /// The page.
        href: OriginPath,
    },
    /// A line of text in the control's place.
    Text {
        /// The text.
        text: String,
    },
    /// A stand-in from a newer server, which this build does not know: the
    /// control is not drawn.
    #[serde(other)]
    Unknown,
}

/// How a haptic feels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HapticKind {
    /// Something worked.
    #[default]
    Success,
    /// Something needs attention.
    Warning,
    /// Something failed.
    Error,
    /// A choice moved.
    Selection,
    /// A light knock.
    Light,
    /// A knock.
    Medium,
    /// A heavy knock.
    Heavy,
}

/// A tap, a buzz or a knock under the finger. A system without one does
/// nothing; a haptic is never refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Haptic {
    /// How it feels.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub kind: HapticKind,
}

impl Haptic {
    /// Something worked.
    pub const fn success() -> Self {
        Self {
            kind: HapticKind::Success,
        }
    }
}

/// Copies text to the clipboard. The client never reads the clipboard.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyText {
    /// The text, at most 64 KiB.
    pub text: String,
    /// Whether it is a secret: the system hides it from previews, keeps it on
    /// this device and forgets it after a while.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sensitive: bool,
}

/// Opens the system's share sheet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    /// A title for what is shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Words to share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A page of the app to share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<OriginPath>,
    /// A page of another site to share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<HttpsUrl>,
    /// A file of the app's to share: the client fetches it first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<OriginPath>,
}

/// What [`Share`] posts: whether the person finished sharing. A system that
/// can't always tell says `true` when the sheet closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shared {
    /// Whether it was shared.
    pub completed: bool,
}

/// Opens a page of another site, in the system's browser or its in-app one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenUrl {
    /// The page.
    pub url: HttpsUrl,
    /// Whether it opens inside the app, in the system's in-app browser.
    #[serde(default, skip_serializing_if = "is_false")]
    pub inside: bool,
}

/// Signs in through another site, in the system's own sign-in session: the
/// one sign-in pages of identity providers accept. The session starts at
/// `start` (the app's redirect to the provider) and ends at the app's own
/// callback, which the client then loads with its own cookies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    /// The app's route that sends the person to the provider.
    pub start: OriginPath,
    /// Whether the session keeps no cookies of its own, so the person signs in
    /// afresh.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ephemeral: bool,
}

/// Asks for permission to send push messages and registers the device. The
/// device's token goes to the framework's own route; the call's continuation
/// gets the outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterPush {
    /// The kinds of message the app sends, so the system can offer each its
    /// own setting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
}

/// The service that carries a device's push messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PushProvider {
    /// Apple's.
    Apns,
    /// Google's Firebase Cloud Messaging.
    Fcm,
    /// Windows'.
    Wns,
}

/// Which of the service's environments a token belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushEnvironment {
    /// A build from the developer's machine: only an iOS debug build has a
    /// token the service's sandbox takes.
    Sandbox,
    /// A build from a store, and every Android build.
    #[default]
    Production,
}

/// The system a push token belongs to, as the push route names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PushPlatform {
    /// Apple's systems: the token is one of Apple's push service, as hex.
    Ios,
    /// Android: the token is one of Firebase Cloud Messaging.
    Android,
}

/// What a client posts to the shell's push route
/// ([`Push::register`](crate::shell::Push::register)), as JSON, to register
/// the device: its own token and the app it belongs to. The server trusts
/// nothing in it but for delivery, and ties the device to the session it is
/// posted with, so the device ends with that session.
///
/// ```
/// use navree_native::device::{PushDevice, PushEnvironment, PushPlatform};
///
/// let device = PushDevice {
///     platform: PushPlatform::Ios,
///     token: "9f2c4a7e00b1d35c8e6a1f4b7d0c2e5a".to_owned(),
///     app_id: "com.charkha.supply".to_owned(),
///     environment: PushEnvironment::Production,
/// };
/// // A store build is the usual one, and is left out.
/// assert_eq!(
///     serde_json::to_string(&device).unwrap(),
///     r#"{"platform":"ios","token":"9f2c4a7e00b1d35c8e6a1f4b7d0c2e5a","app_id":"com.charkha.supply"}"#
/// );
/// ```
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushDevice {
    /// The system.
    pub platform: PushPlatform,
    /// The device's token. Apple's: the device token as hex, even in length, 16
    /// to 200 characters (`deviceToken.map { String(format: "%02x", $0) }`).
    /// Firebase's: the registration token, 16 to 4096 characters of
    /// `A-Za-z0-9_-:.`. It is personal: never logged.
    pub token: String,
    /// The app's id with the service: its bundle id or package name. The server
    /// takes only the ids its configuration names.
    pub app_id: String,
    /// The service's environment the token is for.
    #[serde(default, skip_serializing_if = "is_default")]
    pub environment: PushEnvironment,
}

impl fmt::Debug for PushDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushDevice")
            .field("platform", &self.platform)
            .field("token", &"[redacted]")
            .field("app_id", &self.app_id)
            .field("environment", &self.environment)
            .finish()
    }
}

/// What [`RegisterPush`] posts to the framework's push route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushRegistered {
    /// The service.
    pub provider: PushProvider,
    /// Its environment.
    pub environment: PushEnvironment,
    /// The device's token. It is personal: never logged.
    pub token: String,
    /// The app's id with the service: its bundle id or package name.
    pub app_id: String,
    /// This installation of the app, so a reinstall replaces its old token.
    pub install_id: String,
    /// The person's locale, for the messages' words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

/// When a notification is shown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
#[non_exhaustive]
pub enum When {
    /// At a moment.
    At {
        /// Seconds since 1970 (UTC).
        unix: i64,
    },
    /// After a while.
    After {
        /// How long, in seconds.
        secs: u32,
    },
    /// Every day at a time, in the device's time zone.
    Daily {
        /// The hour, 0 to 23.
        hour: u8,
        /// The minute, 0 to 59.
        minute: u8,
    },
    /// A time from a newer server, which this build does not know: the
    /// notification is not scheduled.
    #[serde(other)]
    Unknown,
}

/// Schedules a notification on the device. The first one asks the person for
/// permission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notify {
    /// The notification's id: one with the same id takes its place.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// When it is shown.
    pub at: When,
    /// The page a tap on it opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<OriginPath>,
    /// Notifications of one thread are grouped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
}

/// Takes back a scheduled or shown notification.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelNotification {
    /// The notification's id.
    pub id: String,
}

/// Sets the count on the app's icon, or clears it. A system with no counts
/// shows a dot or nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetBadge {
    /// The count; none clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// A page of the system's settings for this app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SettingsPage {
    /// The app's own page: its permissions.
    #[default]
    App,
    /// Its notifications.
    Notifications,
}

/// Opens this app's page in the system's settings, so a person who said no
/// can say yes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSettings {
    /// Which page.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub page: SettingsPage,
}

/// What a passkey call does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasskeyMode {
    /// Makes a new passkey for the signed-in person.
    Register,
    /// Signs in with one.
    SignIn,
}

/// Signs in with a passkey, or registers one, in the system's own sheet. The
/// client fetches the request's options from `options` when the person taps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Passkey {
    /// Sign in or register.
    pub mode: PasskeyMode,
    /// The route that answers the request's options (WebAuthn's own JSON).
    pub options: OriginPath,
    /// Whether the system offers the passkey from the keyboard's suggestions
    /// instead of a sheet.
    #[serde(default, skip_serializing_if = "is_false")]
    pub conditional: bool,
}

/// What [`Passkey`] posts: the credential, as WebAuthn's `toJSON()` writes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyCredential {
    /// The credential's JSON.
    pub credential: String,
}

/// How a confirmation is signed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConfirmMethod {
    /// With a passkey, the person verified.
    Passkey,
    /// With a key held by this device alone, unlocked by the person's face or
    /// finger.
    DeviceKey,
    /// Whichever the device has.
    #[default]
    Either,
}

/// Asks the person to approve an action with their face, finger or device
/// code, as a signature over a challenge the server derives from the action
/// itself: approving ₹100 can't approve ₹1,00,000.
///
/// It takes two requests, both with the session's cookies and its token in
/// `x-xsrf-token`, with the system's own sheet between them:
///
/// 1. On a tap, the client posts `{"ticket": <then.ticket>}` as JSON to
///    `challenge`. The route spends the ticket and answers the passkey request's
///    options: `{"publicKey": {"challenge", "rpId", "timeout",
///    "userVerification": "required", "allowCredentials": [...]}}`. A ticket
///    that is old, used or another session's is refused (`422`,
///    `invalid_ticket`): the client fetches the screen again and the call has a
///    new one.
/// 2. The client prompts with the system's sheet (on Android a public key
///    credential request made from `publicKey`; on Apple systems an assertion
///    request with the challenge decoded from base64url, user verification
///    required and the allowed credentials), then posts [`Confirmed`] to
///    `then.href` as a form, with `outcome` as every call's result is. It
///    carries no ticket: the challenge stands for it, which is good for 120
///    seconds, for this session and this action only, and is used up by the
///    first attempt. A signature the server refuses is `403`
///    (`confirmation_refused`).
///
/// A person who closes the sheet is a refusal ([`Refusal::Cancelled`]): the
/// client posts `outcome` alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confirm {
    /// What is being approved, at most 120 characters: `Approve payment`.
    pub reason: String,
    /// The route the call's ticket is posted to and turned into a challenge at:
    /// `/api/auth/confirm/options` by default.
    pub challenge: OriginPath,
    /// Its details, at most 120 characters: `₹1,00,000 to Gurpreet Kaur`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// How it is signed.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub method: ConfirmMethod,
}

/// Which method signed a confirmation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConfirmedBy {
    /// A passkey: what a confirmation is signed with unless it says otherwise,
    /// and what a method this build does not know is read as.
    #[default]
    Passkey,
    /// The device's own key.
    DeviceKey,
}

/// What [`Confirm`] posts to its continuation, as a form, beside `outcome`: the
/// signature, which the server checks against the challenge it issued for this
/// session and this action.
///
/// A passkey's signature is its assertion, as WebAuthn's `toJSON()` writes it
/// (`id`, `rawId`, `type`, `response` with `clientDataJSON`, `authenticatorData`,
/// `signature` and `userHandle`), as JSON text in `credential`. That is all
/// there is to post: the challenge is inside it, in the client data, so there
/// is nothing to name beside it. A device key's signature carries no such
/// assertion, so `challenge` names the one it signed, with `key_id` and
/// `signature`.
///
/// ```
/// use navree_native::device::{Confirmed, ConfirmedBy};
///
/// let assertion = r#"{"id":"AQID","rawId":"AQID","type":"public-key","response":{"clientDataJSON":"e30","authenticatorData":"AQ","signature":"MEUC","userHandle":null}}"#;
/// let posted: Confirmed = serde_json::from_value(serde_json::json!({ "credential": assertion })).unwrap();
/// assert_eq!(posted.method, ConfirmedBy::Passkey);
/// assert_eq!(posted.credential.as_deref(), Some(assertion));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confirmed {
    /// A passkey's assertion, as WebAuthn's `toJSON()` writes it, as JSON text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    /// The method that signed. The passkey when left out, and when it is a
    /// method this build does not know.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub method: ConfirmedBy,
    /// The id of the challenge a device key's signature answers. A passkey's
    /// assertion holds its own challenge, so it posts none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge: Option<String>,
    /// The device key's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    /// The device key's signature, base64url.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// How exactly a location is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accuracy {
    /// To a few kilometres: enough for a city.
    #[default]
    Approximate,
    /// As exactly as the device can.
    Precise,
}

/// Asks where the device is, once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocateOnce {
    /// How exactly.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub accuracy: Accuracy,
    /// How old a fix the device already has may be, in seconds.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub max_age_secs: u32,
    /// How long to wait for a fix, in seconds; the system's own wait when 0.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub timeout_secs: u32,
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

/// What [`LocateOnce`] posts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Located {
    /// Latitude, in degrees.
    pub lat: f64,
    /// Longitude, in degrees.
    pub lon: f64,
    /// How far off it may be, in metres.
    pub accuracy_m: f64,
    /// When the fix was taken, in seconds since 1970 (UTC).
    pub at: i64,
    /// Whether it is precise: `false` when the person allowed only an
    /// approximate location.
    pub precise: bool,
}

/// The kind of a barcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CodeFormat {
    /// QR.
    Qr,
    /// EAN-13.
    Ean13,
    /// EAN-8.
    Ean8,
    /// UPC-A.
    UpcA,
    /// UPC-E.
    UpcE,
    /// Code 128.
    Code128,
    /// Code 39.
    Code39,
    /// Code 93.
    Code93,
    /// Interleaved 2 of 5.
    Itf,
    /// PDF417.
    Pdf417,
    /// Aztec.
    Aztec,
    /// Data Matrix.
    DataMatrix,
}

/// Reads a barcode or a QR code with the camera.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanCode {
    /// The kinds it looks for; every kind when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "crate::lenient::known")]
    pub formats: Vec<CodeFormat>,
    /// What the scanner tells the person: `Scan the SKU on the box`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// What [`ScanCode`] posts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scanned {
    /// The kind of code read.
    pub format: CodeFormat,
    /// What it holds, at most 4 KiB.
    pub value: String,
}

/// Locks the app behind the device's own unlock (a face, a finger, the device
/// code) after it has been away a while. The person turns it on in the app's
/// settings. It keeps a casual holder out; it proves nothing to the server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppLock {
    /// How long the app may be away before it locks, in seconds.
    pub after_secs: u32,
}

/// Who a platform sign-in is with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SignInProvider {
    /// Sign in with Apple.
    Apple,
    /// Sign in with Google.
    Google,
}

/// Signs in with Apple or Google in the system's own sheet, where the system
/// has one for the provider; elsewhere (Google on an Apple system, Apple on
/// Android) the client signs in through `web` as an [`AuthSession`].
///
/// On a tap, the client posts to `nonce` (JSON, no body), which answers
/// `{"nonce", "state", "expires_in"}`; it opens the provider's sheet with the
/// nonce (Apple's as the lower-case hex SHA-256 of it, Google's as it is),
/// then posts [`SignedIn`] as JSON to `sign_in`, with the session's cookie.
/// No ticket and no `then`: the sealed `state`, used once, stands for a
/// ticket, and the answer is the person signed in, after which the client
/// fetches the shell and the screen again.
///
/// When the person's sign-in needs a second step (a code from an
/// authenticator app), the route answers `{"two_factor_required": true}` and
/// nobody is signed in yet: the person's session waits for the step. The client
/// opens `second_step` inside the app, with the app's own cookies, and never in
/// the system's sign-in session, which does not have them; the page leads on
/// when the step is done, and the client then fetches the shell and the screen
/// as it does after any sign-in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformSignIn {
    /// Who with.
    pub provider: SignInProvider,
    /// The route that hands out a nonce and a state for one attempt:
    /// `/api/auth/social/apple/native/nonce`.
    pub nonce: OriginPath,
    /// The route the token is posted to: `/api/auth/social/apple/native/session`.
    pub sign_in: OriginPath,
    /// Where the web's sign-in with the same provider starts, for a system
    /// with no sheet for it: `/api/auth/social/google/redirect`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<OriginPath>,
    /// The app's client id with the provider, where its sheet needs one
    /// (Google's server client id on Android).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The page of the app that finishes a second step: `/sign-in/two-factor`.
    /// Left out, the shell's [`SignIn::second_step`](crate::shell::SignIn::second_step)
    /// is the page; with neither, the client has no page for it and tells the
    /// person to sign in another way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_step: Option<OriginPath>,
}

/// What [`PlatformSignIn`] posts, as JSON: the body the app's sign-in route
/// reads. The server verifies the token with the provider and checks the
/// state and the nonce in it; nothing here is trusted on its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedIn {
    /// The provider's signed identity token.
    pub id_token: String,
    /// The state the nonce route gave with the nonce.
    pub state: String,
    /// The authorization code, where the provider gives one (Apple).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_code: Option<String>,
    /// Who the person is, where the provider said (Apple does the first time
    /// only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<SignedInUser>,
}

/// Who signed in, as Apple tells it the first time.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedInUser {
    /// Their name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<PersonName>,
}

/// A person's name, in the provider's own words for its parts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonName {
    /// The given name.
    #[serde(rename = "firstName", default, skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    /// The family name.
    #[serde(rename = "lastName", default, skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
}

/// A result a call posts back, and the feature it belongs to.
pub trait FeatureResult: Serialize + DeserializeOwned {
    /// The feature.
    const FEATURE: Feature;
}

impl FeatureResult for Shared {
    const FEATURE: Feature = Feature::Share;
}

impl FeatureResult for PushRegistered {
    const FEATURE: Feature = Feature::Push;
}

impl FeatureResult for PasskeyCredential {
    const FEATURE: Feature = Feature::Passkey;
}

impl FeatureResult for Confirmed {
    const FEATURE: Feature = Feature::Confirm;
}

impl FeatureResult for Located {
    const FEATURE: Feature = Feature::LocationOnce;
}

impl FeatureResult for Scanned {
    const FEATURE: Feature = Feature::ScanCode;
}

impl FeatureResult for SignedIn {
    const FEATURE: Feature = Feature::PlatformSignIn;
}

/// A call a tap on a control may start. `f` names it in JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "f", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Call {
    /// A haptic.
    Haptic(Haptic),
    /// Copies text.
    Copy(CopyText),
    /// The share sheet.
    Share(Share),
    /// Opens another site.
    OpenUrl(OpenUrl),
    /// Signs in through another site.
    AuthSession(AuthSession),
    /// Registers for push messages.
    RegisterPush(RegisterPush),
    /// Schedules a notification.
    Notify(Notify),
    /// Takes back a notification.
    CancelNotification(CancelNotification),
    /// Sets the app's badge.
    SetBadge(SetBadge),
    /// Opens the app's settings.
    OpenSettings(OpenSettings),
    /// A passkey.
    Passkey(Passkey),
    /// Approves an action.
    Confirm(Confirm),
    /// Where the device is.
    LocateOnce(LocateOnce),
    /// Reads a code.
    ScanCode(ScanCode),
    /// Signs in with Apple or Google.
    PlatformSignIn(PlatformSignIn),
    /// A call from a newer server, which this build does not know: refused
    /// as [`Refusal::Unsupported`], with the call's `alt` drawn.
    #[serde(other)]
    Unknown,
}

impl Call {
    /// The feature the call belongs to; none for an unknown call.
    pub fn feature(&self) -> Option<Feature> {
        Some(match self {
            Self::Haptic(_) => Feature::Haptic,
            Self::Copy(_) => Feature::Clipboard,
            Self::Share(_) => Feature::Share,
            Self::OpenUrl(_) => Feature::OpenUrl,
            Self::AuthSession(_) => Feature::AuthSession,
            Self::RegisterPush(_) => Feature::Push,
            Self::Notify(_) | Self::CancelNotification(_) | Self::SetBadge(_) => {
                Feature::LocalNotification
            }
            Self::OpenSettings(_) => Feature::OpenSettings,
            Self::Passkey(_) => Feature::Passkey,
            Self::Confirm(_) => Feature::Confirm,
            Self::LocateOnce(_) => Feature::LocationOnce,
            Self::ScanCode(_) => Feature::ScanCode,
            Self::PlatformSignIn(_) => Feature::PlatformSignIn,
            Self::Unknown => return None,
        })
    }

    /// Whether the call may only run from a tap a person just made: it asks
    /// for a permission, shows the system's own sheet over the screen, or
    /// returns something private. A client refuses it from anything else.
    pub fn needs_gesture(&self) -> bool {
        !matches!(
            self,
            Self::Haptic(_) | Self::SetBadge(_) | Self::CancelNotification(_)
        )
    }

    /// Whether the call posts a result to its continuation, and so needs a
    /// [`Then`]. A platform sign-in posts to its own route instead.
    pub fn posts(&self) -> bool {
        matches!(
            self,
            Self::Share(_)
                | Self::RegisterPush(_)
                | Self::Passkey(_)
                | Self::Confirm(_)
                | Self::LocateOnce(_)
                | Self::ScanCode(_)
        )
    }
}

macro_rules! into_call {
    ($($variant:ident($ty:ty),)+) => {
        $(
            impl From<$ty> for Call {
                fn from(call: $ty) -> Self {
                    Self::$variant(call)
                }
            }
        )+
    };
}

into_call! {
    Haptic(Haptic),
    Copy(CopyText),
    Share(Share),
    OpenUrl(OpenUrl),
    AuthSession(AuthSession),
    RegisterPush(RegisterPush),
    Notify(Notify),
    CancelNotification(CancelNotification),
    SetBadge(SetBadge),
    OpenSettings(OpenSettings),
    Passkey(Passkey),
    Confirm(Confirm),
    LocateOnce(LocateOnce),
    ScanCode(ScanCode),
    PlatformSignIn(PlatformSignIn),
}

/// A call a server's reply may start, as [`Op::Device`](crate::Op::Device):
/// only calls that need no tap and show nothing private. There is no variant
/// for anything else, so an op can't open the camera or ask where the device
/// is:
///
/// ```compile_fail
/// use navree_native::Op;
/// use navree_native::device::{Call, LocateOnce};
///
/// let op = Op::Device {
///     call: Call::LocateOnce(LocateOnce::default()),
/// };
/// ```
///
/// ```
/// use navree_native::Op;
/// use navree_native::device::{Haptic, ReplyCall};
///
/// let op = Op::Device {
///     call: ReplyCall::Haptic(Haptic::success()),
/// };
/// assert_eq!(
///     serde_json::to_string(&op).unwrap(),
///     r#"{"op":"device","call":{"f":"haptic","kind":"success"}}"#
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "f", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReplyCall {
    /// A haptic.
    Haptic(Haptic),
    /// Sets the app's badge.
    SetBadge(SetBadge),
    /// Takes back a notification.
    CancelNotification(CancelNotification),
    /// The store's rating prompt. The system decides whether it shows; a page
    /// never offers it from a button.
    RequestReview,
    /// A call from a newer server, or one that needs a tap: skipped.
    #[serde(other)]
    Unknown,
}

impl ReplyCall {
    /// The feature the call belongs to; none for an unknown call.
    pub fn feature(&self) -> Option<Feature> {
        Some(match self {
            Self::Haptic(_) => Feature::Haptic,
            Self::SetBadge(_) | Self::CancelNotification(_) => Feature::LocalNotification,
            Self::RequestReview => Feature::RequestReview,
            Self::Unknown => return None,
        })
    }
}

impl From<Haptic> for ReplyCall {
    fn from(call: Haptic) -> Self {
        Self::Haptic(call)
    }
}

impl From<SetBadge> for ReplyCall {
    fn from(call: SetBadge) -> Self {
        Self::SetBadge(call)
    }
}

impl From<CancelNotification> for ReplyCall {
    fn from(call: CancelNotification) -> Self {
        Self::CancelNotification(call)
    }
}

/// A call on a control, with where its result goes and what stands in its
/// place where it can't start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCall {
    /// The call.
    pub call: Call,
    /// Where its result is posted: every call that [`posts`](Call::posts)
    /// has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub then: Option<Then>,
    /// What the client shows where the call can't start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<Alt>,
}

impl DeviceCall {
    /// The call, with no continuation and no stand-in.
    pub fn new(call: impl Into<Call>) -> Self {
        Self {
            call: call.into(),
            then: None,
            alt: None,
        }
    }

    /// The same call, posting its result as `then` says.
    #[must_use]
    pub fn then(mut self, then: Then) -> Self {
        self.then = Some(then);
        self
    }

    /// The same call, with `alt` drawn where it can't start.
    #[must_use]
    pub fn alt(mut self, alt: Alt) -> Self {
        self.alt = Some(alt);
        self
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn a_path_on_the_origin_is_kept_as_written() {
        for good in [
            "/",
            "/payments/42/approve",
            "/orders?sort=-total&filter[status][in]=open",
            "/search?q=%2F%2Fnot-a-host",
            "/files/report%202026.pdf",
            "/a/./b",
            "/notes#latest",
        ] {
            assert_eq!(OriginPath::parse(good).unwrap().as_str(), good);
        }
        assert_eq!(
            OriginPath::parse("/orders?sort=total").unwrap().path(),
            "/orders"
        );
    }

    #[test]
    fn a_path_that_leaves_the_origin_is_refused() {
        for (bad, why) in [
            ("", PathError::NotAPath),
            ("orders", PathError::NotAPath),
            ("https://evil.example/x", PathError::NotAPath),
            ("javascript:alert(1)", PathError::NotAPath),
            ("%2F%2Fevil.example", PathError::NotAPath),
            ("//evil.example", PathError::OtherOrigin),
            ("/%2F%2Fevil.example", PathError::OtherOrigin),
            ("/%2fevil.example", PathError::OtherOrigin),
            ("/\\evil.example", PathError::Backslash),
            ("/%5Cevil.example", PathError::Backslash),
            ("/orders/../admin", PathError::DotSegment),
            ("/orders/%2e%2e/admin", PathError::DotSegment),
            ("/..", PathError::DotSegment),
            ("/a b", PathError::Control),
            ("/a\nb", PathError::Control),
            ("/a%0d%0aSet-Cookie:x", PathError::Control),
            ("/a%zz", PathError::Control),
            ("/a%2", PathError::Control),
        ] {
            assert_eq!(OriginPath::parse(bad), Err(why), "{bad:?}");
        }
        assert_eq!(
            OriginPath::parse(&format!("/{}", "a".repeat(OriginPath::MAX))),
            Err(PathError::TooLong)
        );
    }

    #[test]
    fn a_path_from_the_wire_is_checked_as_it_is_read() {
        let then = r#"{"href":"//evil.example/steal","ticket":"t","exp":1}"#;
        assert!(serde_json::from_str::<Then>(then).is_err());
        let call = r#"{"call":{"f":"share","href":"/\\evil.example"}}"#;
        assert!(serde_json::from_str::<DeviceCall>(call).is_err());
    }

    #[test]
    fn an_address_on_another_site_must_be_https_with_no_user() {
        for good in [
            "https://pay.example/session/7",
            "https://example.com",
            "https://example.com:8443/a?b=c",
        ] {
            assert_eq!(HttpsUrl::parse(good).unwrap().as_str(), good);
        }
        for (bad, why) in [
            ("http://example.com/", UrlError::NotHttps),
            ("HTTPS://example.com/", UrlError::NotHttps),
            ("javascript:alert(1)", UrlError::NotHttps),
            ("/orders", UrlError::NotHttps),
            ("https://", UrlError::NoHost),
            ("https:///path", UrlError::NoHost),
            ("https://:443/", UrlError::NoHost),
            ("https://me@evil.example/", UrlError::UserInfo),
            ("https://good.example:pw@evil.example/", UrlError::UserInfo),
            ("https://good.example\\@evil.example/", UrlError::Control),
            ("https://a b/", UrlError::Control),
        ] {
            assert_eq!(HttpsUrl::parse(bad), Err(why), "{bad:?}");
        }
    }

    /// A stand-in for the server's cipher: a keyed checksum beside the plain
    /// text, enough to tell a changed or foreign ticket.
    struct Toy(&'static str);

    impl Toy {
        fn sum(&self, plain: &str) -> u64 {
            format!("{}{plain}", self.0)
                .bytes()
                .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
                })
        }
    }

    impl Sealer for Toy {
        fn seal(&self, plain: &[u8]) -> String {
            let plain = String::from_utf8(plain.to_vec()).unwrap();
            format!("{:016x}{plain}", self.sum(&plain))
        }

        fn open(&self, sealed: &str) -> Option<Vec<u8>> {
            let (sum, plain) = sealed.split_at_checked(16)?;
            (u64::from_str_radix(sum, 16).ok()? == self.sum(plain)).then(|| plain.into())
        }
    }

    fn claims() -> Claims {
        Claims {
            feature: Feature::Confirm,
            digest: "payments.approve/42/100000".to_owned(),
            session: "session-a".to_owned(),
            href: OriginPath::parse("/payments/42/approve").unwrap(),
            exp: 1_000,
            nonce: "n-1".to_owned(),
        }
    }

    fn expected() -> Expected<'static> {
        Expected {
            feature: Feature::Confirm,
            digest: "payments.approve/42/100000",
            session: "session-a",
            href: "/payments/42/approve",
        }
    }

    #[test]
    fn a_ticket_is_good_once_for_its_session_feature_route_and_call() {
        let key = Toy("server key");
        let ticket = Ticket::seal(&claims(), &key);
        let mut spent = HashSet::new();
        let mut spend = |nonce: &str, _exp: i64| spent.insert(nonce.to_owned());
        assert_eq!(
            ticket.redeem(&key, &expected(), 999, &mut spend),
            Ok(claims())
        );
        assert_eq!(
            ticket.redeem(&key, &expected(), 999, &mut spend),
            Err(TicketError::Used)
        );
    }

    #[test]
    fn a_ticket_is_refused_for_anything_it_was_not_written_for() {
        let key = Toy("server key");
        let ticket = Ticket::seal(&claims(), &key);
        let fresh = |_: &str, _: i64| true;
        let refused = |expected: Expected<'_>, now: i64| ticket.redeem(&key, &expected, now, fresh);
        assert_eq!(refused(expected(), 1_000), Err(TicketError::Expired));
        assert_eq!(
            refused(
                Expected {
                    session: "session-b",
                    ..expected()
                },
                1
            ),
            Err(TicketError::OtherSession)
        );
        assert_eq!(
            refused(
                Expected {
                    feature: Feature::LocationOnce,
                    ..expected()
                },
                1
            ),
            Err(TicketError::OtherFeature)
        );
        assert_eq!(
            refused(
                Expected {
                    href: "/payments/43/approve",
                    ..expected()
                },
                1
            ),
            Err(TicketError::OtherRoute)
        );
        // The amount changed after the ticket was written.
        assert_eq!(
            refused(
                Expected {
                    digest: "payments.approve/42/100",
                    ..expected()
                },
                1
            ),
            Err(TicketError::OtherCall)
        );
        assert_eq!(
            Ticket::from(String::new()).redeem(&key, &expected(), 1, fresh),
            Err(TicketError::Missing)
        );
        assert_eq!(
            ticket.redeem(&Toy("another key"), &expected(), 1, fresh),
            Err(TicketError::Unreadable)
        );
        let mut changed = ticket.as_str().to_owned();
        changed = changed.replace("session-a", "session-b");
        assert_eq!(
            Ticket::from(changed).redeem(&key, &expected(), 1, fresh),
            Err(TicketError::Unreadable)
        );
        assert_eq!(format!("{ticket:?}"), "Ticket(..)");
        let long = Ticket::from("a".repeat(Ticket::MAX + 1));
        assert_eq!(long.open(&key), Err(TicketError::Unreadable));
        let shown = format!("{:?}", claims());
        assert!(!shown.contains("session-a") && !shown.contains("n-1"), "{shown}");
    }

    #[test]
    fn only_a_haptic_the_badge_and_a_cancelled_notification_need_no_tap() {
        let calls: Vec<Call> = vec![
            Haptic::success().into(),
            CopyText::default().into(),
            Share::default().into(),
            OpenUrl {
                url: HttpsUrl::parse("https://example.com").unwrap(),
                inside: false,
            }
            .into(),
            AuthSession {
                start: OriginPath::parse("/auth/google/redirect").unwrap(),
                ephemeral: false,
            }
            .into(),
            RegisterPush::default().into(),
            Notify {
                id: "n".to_owned(),
                title: "t".to_owned(),
                body: None,
                at: When::After { secs: 60 },
                href: None,
                thread: None,
            }
            .into(),
            CancelNotification::default().into(),
            SetBadge::default().into(),
            OpenSettings::default().into(),
            Passkey {
                mode: PasskeyMode::SignIn,
                options: OriginPath::parse("/auth/passkeys/options").unwrap(),
                conditional: false,
            }
            .into(),
            Confirm {
                reason: "Approve".to_owned(),
                challenge: OriginPath::parse("/api/auth/confirm/options").unwrap(),
                detail: None,
                method: ConfirmMethod::Either,
            }
            .into(),
            LocateOnce::default().into(),
            ScanCode::default().into(),
            PlatformSignIn {
                provider: SignInProvider::Apple,
                nonce: OriginPath::parse("/api/auth/social/apple/native/nonce").unwrap(),
                sign_in: OriginPath::parse("/api/auth/social/apple/native/session").unwrap(),
                web: None,
                client_id: None,
                second_step: None,
            }
            .into(),
        ];
        let tapless: Vec<_> = calls
            .iter()
            .filter(|call| !call.needs_gesture())
            .map(Call::feature)
            .collect();
        assert_eq!(
            tapless,
            [
                Some(Feature::Haptic),
                Some(Feature::LocalNotification),
                Some(Feature::LocalNotification)
            ]
        );
        assert!(calls.iter().all(|call| call.feature().is_some()));
        let posting: Vec<_> = calls
            .iter()
            .filter(|call| call.posts())
            .filter_map(Call::feature)
            .collect();
        assert_eq!(
            posting,
            [
                Shared::FEATURE,
                PushRegistered::FEATURE,
                PasskeyCredential::FEATURE,
                Confirmed::FEATURE,
                Located::FEATURE,
                Scanned::FEATURE,
            ]
        );
        // A sign-in posts its token to its own route, not to a continuation.
        assert!(calls.iter().any(|call| call.feature() == Some(SignedIn::FEATURE) && !call.posts()));
    }

    #[test]
    fn a_tap_only_call_written_into_an_op_by_hand_is_read_as_unknown() {
        for call in [
            r#"{"f":"locate_once","accuracy":"precise"}"#,
            r#"{"f":"confirm","reason":"Approve"}"#,
            r#"{"f":"scan_code"}"#,
            r#"{"f":"register_push"}"#,
        ] {
            let op: crate::Op =
                serde_json::from_str(&format!(r#"{{"op":"device","call":{call}}}"#)).unwrap();
            assert_eq!(
                op,
                crate::Op::Device {
                    call: ReplyCall::Unknown
                },
                "{call}"
            );
        }
    }

    #[test]
    fn a_confirmation_posts_the_assertion_and_needs_nothing_else_for_a_passkey() {
        let assertion = r#"{"id":"AQID","rawId":"AQID","type":"public-key","response":{"clientDataJSON":"e30","authenticatorData":"AQ","signature":"MEUC","userHandle":null}}"#;
        let confirmed: Confirmed = serde_json::from_value(serde_json::json!({ "credential": assertion })).unwrap();
        assert_eq!(
            confirmed,
            Confirmed {
                credential: Some(assertion.to_owned()),
                ..Confirmed::default()
            }
        );
        // The passkey is the usual method; a method this build does not know is read as it.
        assert_eq!(confirmed.method, ConfirmedBy::Passkey);
        let newer: Confirmed =
            serde_json::from_str(r#"{"method":"retina","credential":"{}"}"#).unwrap();
        assert_eq!(newer.method, ConfirmedBy::Passkey);
        // What is written is the assertion alone: the challenge is inside it.
        let written = serde_json::to_value(&confirmed).unwrap();
        assert_eq!(written, serde_json::json!({ "credential": assertion }));
    }

    #[test]
    fn a_confirmation_by_a_device_key_names_the_challenge_it_signed() {
        let confirmed: Confirmed = serde_json::from_str(
            r#"{"method":"device_key","challenge":"c-91","key_id":"k-1","signature":"MEUCIQ"}"#,
        )
        .unwrap();
        assert_eq!(
            confirmed,
            Confirmed {
                method: ConfirmedBy::DeviceKey,
                challenge: Some("c-91".to_owned()),
                key_id: Some("k-1".to_owned()),
                signature: Some("MEUCIQ".to_owned()),
                credential: None,
            }
        );
        assert_eq!(
            serde_json::to_string(&confirmed).unwrap(),
            r#"{"method":"device_key","challenge":"c-91","key_id":"k-1","signature":"MEUCIQ"}"#
        );
    }

    #[test]
    fn a_platform_sign_in_may_name_the_page_of_its_second_step() {
        let call: Call = serde_json::from_str(
            r#"{"f":"platform_sign_in","provider":"apple",
                "nonce":"/api/auth/social/apple/native/nonce",
                "sign_in":"/api/auth/social/apple/native/session",
                "second_step":"/sign-in/two-factor"}"#,
        )
        .unwrap();
        let Call::PlatformSignIn(sign_in) = call else {
            panic!("{call:?}");
        };
        assert_eq!(sign_in.second_step.as_ref().map(OriginPath::as_str), Some("/sign-in/two-factor"));
        // Left out, it is not written; the shell's page is the one used.
        let plain = PlatformSignIn {
            second_step: None,
            ..sign_in
        };
        assert!(!serde_json::to_string(&plain).unwrap().contains("second_step"));
        // The page is the app's own: a page of another site is refused.
        assert!(serde_json::from_str::<PlatformSignIn>(
            r#"{"provider":"google","nonce":"/n","sign_in":"/s","second_step":"https://evil.example/2fa"}"#
        )
        .is_err());
    }

    #[test]
    fn a_push_device_is_written_as_the_route_reads_it_and_shows_no_token() {
        let device = PushDevice {
            platform: PushPlatform::Ios,
            token: "9f2c4a7e00b1d35c8e6a1f4b7d0c2e5a".to_owned(),
            app_id: "com.charkha.supply".to_owned(),
            environment: PushEnvironment::Sandbox,
        };
        assert_eq!(
            serde_json::to_string(&device).unwrap(),
            concat!(
                r#"{"platform":"ios","token":"9f2c4a7e00b1d35c8e6a1f4b7d0c2e5a","#,
                r#""app_id":"com.charkha.supply","environment":"sandbox"}"#
            )
        );
        // A store build is the usual one, and is left out.
        let store = PushDevice {
            platform: PushPlatform::Android,
            environment: PushEnvironment::Production,
            ..device.clone()
        };
        let json = serde_json::to_string(&store).unwrap();
        assert!(!json.contains("environment"), "{json}");
        assert_eq!(serde_json::from_str::<PushDevice>(&json).unwrap(), store);
        let shown = format!("{device:?}");
        assert!(shown.contains("com.charkha.supply") && !shown.contains("9f2c4a7e"), "{shown}");
        // The route has no other platform.
        assert!(serde_json::from_str::<PushDevice>(r#"{"platform":"windows","token":"t","app_id":"a"}"#).is_err());
    }

    #[test]
    fn every_feature_reads_back_from_its_name() {
        for feature in Feature::ALL {
            assert_eq!(Feature::parse(feature.as_str()), Some(feature));
            assert_eq!(
                serde_json::to_value(feature).unwrap(),
                serde_json::Value::from(feature.as_str())
            );
        }
    }
}

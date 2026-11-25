//! The site's native apps (configuration section `auth.apps`): the
//! association files Apple and Android fetch from the site before they let
//! an app open its links or use its passkeys and passwords, and the passkey
//! origins of the Android apps.
//!
//! - `GET /.well-known/apple-app-site-association`: `applinks` (the apps
//!   that open the site's [`paths`](AppsOptions::paths)) and
//!   `webcredentials` (the apps that share its passkeys and passwords).
//! - `GET /.well-known/assetlinks.json`: one statement per Android app, with
//!   `delegate_permission/common.handle_all_urls` (its links, narrowed to
//!   the paths on Android 15 and later) and
//!   `delegate_permission/common.get_login_creds` (its passkeys and
//!   passwords).
//!
//! Both are `application/json`, answered at their exact path with no
//! redirect, and not mounted at all when nothing is configured for them.
//!
//! Windows has no such file. A Windows app is listed (`auth.apps.windows`)
//! for the sign-in hand-over alone: by the id it names itself with (its
//! package family name when it is installed from a package, the id it was
//! built with when it is not) and the link on its own scheme the hand-over
//! ends on. Windows opens such a link in whichever program registered the
//! scheme, and nothing proves to the site which program asks, so a Windows
//! app is held to what an Apple or Android app with a custom scheme is held
//! to: it never gets a sign-in the browser already had.

use std::collections::BTreeMap;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::{HeaderValue, Method};
use navree_config::{Config, ConfigSection};
use navree_http::{Controller, IntoResponse, Response, RouteDef};
use navree_kernel::{BoxError, BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::Deserialize;
use serde_json::{Value, json};

/// The path of Apple's association file.
pub const APPLE_ASSOCIATION_PATH: &str = "/.well-known/apple-app-site-association";

/// The path of Android's association file.
pub const ANDROID_ASSOCIATION_PATH: &str = "/.well-known/assetlinks.json";

/// The prefix of the origin an Android app's passkey assertions carry: the
/// base64url SHA-256 of the app's signing certificate follows it.
pub const ANDROID_ORIGIN_PREFIX: &str = "android:apk-key-hash:";

/// The longest path pattern, in bytes.
const MAX_PATH_LEN: usize = 1024;

/// The site's native apps (configuration section `auth.apps`).
///
/// ```toml
/// [auth.apps]
/// paths = ["/orders/*", "/invite/*"]   # links that open in the apps
/// exclude = ["/orders/*/print"]        # never opened in the apps
///
/// [auth.apps.apple.main]
/// team_id = "ABCDE12345"
/// bundle_id = "com.example.orders"
/// # links = true                       # applinks
/// # credentials = true                 # webcredentials: passkeys and passwords
///
/// [auth.apps.android.main]
/// package = "com.example.orders"
/// fingerprints = ["14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5"]
/// # links = true                       # handle_all_urls
/// # credentials = true                 # get_login_creds, and the passkey origin
/// # callback = "https://example.com/app/signed-in"  # the sign-in hand-over
///
/// [auth.apps.windows.main]
/// package_family_name = "ExampleOrders_8wekyb3d8bbwe"  # a build installed from a package
/// app_id = "com.example.orders"                        # a build with no package
/// callback = "com.example.orders:/signed-in"           # the sign-in hand-over
/// ```
///
/// An Android app with `credentials` adds its signatures to the passkey
/// origins (`android:apk-key-hash:<base64url SHA-256>`), so its passkey
/// assertions are accepted beside `auth.passkeys.origins`. In production
/// list the Play App Signing certificate; a debug certificate only for
/// development builds.
///
/// A Windows app is listed for the sign-in hand-over: see [`WindowsApp`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(key = "auth.apps", docs = "/docs/security/passwordless/", validate = Self::check_values)]
#[serde(default)]
#[non_exhaustive]
pub struct AppsOptions {
    /// The paths that open in the apps instead of the browser (`/orders/*`;
    /// `*` matches any characters, `?` one). Empty: no links are claimed.
    pub paths: Vec<String>,
    /// Paths that never open in the apps, even when [`paths`](Self::paths)
    /// match them.
    pub exclude: Vec<String>,
    /// The Apple apps (iOS, iPadOS, macOS, watchOS, visionOS), by name.
    pub apple: BTreeMap<String, AppleApp>,
    /// The Android apps, by name.
    pub android: BTreeMap<String, AndroidApp>,
    /// The Windows apps, by name.
    pub windows: BTreeMap<String, WindowsApp>,
    /// Whether a browser that signed in only to hand its sign-in over to an
    /// app stays signed in afterwards (default `false`: it is signed out,
    /// and only the app holds a session). A browser that was signed in
    /// before the hand-over stays signed in once the person confirms,
    /// unless the app's callback is a custom scheme (the browser is then
    /// signed out and the person signs in again for the app).
    pub keep_browser_signed_in: bool,
    /// Whether the site answers `GET` on the path of each `https` callback
    /// with an empty page (`Cache-Control: no-store`, `Referrer-Policy:
    /// no-referrer`), for a browser the system did not hand to the app, so
    /// the code in its address is neither kept nor sent on (default
    /// `false`). Turn it on when the site has no page of its own at that
    /// path; an app that serves that path itself leaves it off.
    pub callback_page: bool,
}

impl Default for AppsOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            exclude: Vec::new(),
            apple: BTreeMap::new(),
            android: BTreeMap::new(),
            windows: BTreeMap::new(),
            keep_browser_signed_in: false,
            callback_page: false,
        }
    }
}

/// An Apple app (`auth.apps.apple.<name>`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct AppleApp {
    /// The ten-character Team ID of the developer account.
    pub team_id: String,
    /// The bundle identifier (`com.example.orders`).
    pub bundle_id: String,
    /// Whether the app opens the site's paths (`applinks`).
    pub links: bool,
    /// Whether the app uses the site's passkeys and passwords
    /// (`webcredentials`).
    pub credentials: bool,
    /// Where the sign-in hand-over sends the app its one-time code: an
    /// `https` universal link of the app. Without it the app cannot use the
    /// hand-over.
    pub callback: Option<String>,
    /// Whether [`callback`](Self::callback) may be a custom scheme
    /// (`com.example.orders:/signed-in`). Any app on the device can claim a
    /// scheme; leave it off unless the app cannot use a universal link.
    pub custom_scheme: bool,
}

impl Default for AppleApp {
    fn default() -> Self {
        Self {
            team_id: String::new(),
            bundle_id: String::new(),
            links: true,
            credentials: true,
            callback: None,
            custom_scheme: false,
        }
    }
}

impl AppleApp {
    /// `TeamID.bundleID`, as the association file names the app.
    pub fn app_id(&self) -> String {
        format!("{}.{}", self.team_id.trim(), self.bundle_id.trim())
    }
}

/// An Android app (`auth.apps.android.<name>`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct AndroidApp {
    /// The package name (`com.example.orders`).
    pub package: String,
    /// The SHA-256 fingerprints of the certificates the app is signed with,
    /// as `keytool` and the Play Console print them (32 hex bytes separated
    /// by colons).
    pub fingerprints: Vec<String>,
    /// Whether the app opens the site's links (`handle_all_urls`).
    pub links: bool,
    /// Whether the app uses the site's passkeys and passwords
    /// (`get_login_creds`); its signatures become passkey origins.
    pub credentials: bool,
    /// Where the sign-in hand-over sends the app its one-time code: an
    /// `https` App Link of the app. Without it the app cannot use the
    /// hand-over.
    pub callback: Option<String>,
    /// Whether [`callback`](Self::callback) may be a custom scheme. Any app
    /// on the device can claim a scheme; leave it off unless the app cannot
    /// use an App Link.
    pub custom_scheme: bool,
}

impl Default for AndroidApp {
    fn default() -> Self {
        Self {
            package: String::new(),
            fingerprints: Vec::new(),
            links: true,
            credentials: true,
            callback: None,
            custom_scheme: false,
        }
    }
}

impl AndroidApp {
    /// The fingerprints as the association file writes them (upper-case
    /// hex, colon-separated); unreadable ones are left out.
    fn normalized_fingerprints(&self) -> Vec<String> {
        self.fingerprints
            .iter()
            .filter_map(|fingerprint| fingerprint_bytes(fingerprint))
            .map(|bytes| {
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(":")
            })
            .collect()
    }
}

/// A Windows app (`auth.apps.windows.<name>`), listed for the sign-in
/// hand-over.
///
/// A build names itself by one id: its package family name when it runs
/// from an installed package, the id it was built with when it does not.
/// An app that ships both ways gives both; one of the two is required.
///
/// The hand-over ends on a link of the app's own scheme, which the app
/// registers with Windows. Any program can register a scheme, and nothing
/// proves to the site which program asked, so a Windows app is held to what
/// an Apple or Android app with `custom_scheme` is held to: the code works
/// only with the verifier of the app that started the hand-over (PKCE,
/// S256), and the app never gets a sign-in the browser already had (the
/// person is asked first, is signed out there and signs in again). A link
/// of the site (`https`) and a port of the machine (`http://127.0.0.1`) are
/// not taken as a Windows app's callback.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct WindowsApp {
    /// The package family name (`ExampleOrders_8wekyb3d8bbwe`), by which a
    /// build installed from a package names itself: the package's name, `_`
    /// and the thirteen characters of its publisher id, exactly as
    /// `Get-AppxPackage` prints `PackageFamilyName`.
    pub package_family_name: Option<String>,
    /// The id a build with no package was built with (`com.example.orders`),
    /// by which such a build names itself.
    pub app_id: Option<String>,
    /// Where the sign-in hand-over sends the app its one-time code: a link
    /// on the app's own scheme (`com.example.orders:/signed-in`), written as
    /// it is sent (the scheme in lower case, 2 to 39 characters), with no
    /// user, password, query or fragment. Required.
    pub callback: Option<String>,
}

impl WindowsApp {
    /// The ids a build of the app may name itself by.
    fn ids(&self) -> impl Iterator<Item = &str> {
        self.package_family_name
            .as_deref()
            .into_iter()
            .chain(self.app_id.as_deref())
    }
}

impl AppsOptions {
    /// Checks the values and joins the problems (the boot calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }

    /// Every problem with the options, naming the configuration keys.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        for (key, patterns) in [("paths", &self.paths), ("exclude", &self.exclude)] {
            if patterns.iter().any(|pattern| !is_path_pattern(pattern)) {
                errors.push(format!(
                    "auth.apps.{key} must be paths starting with `/`, without spaces, \
                     at most {MAX_PATH_LEN} bytes"
                ));
            }
        }
        for (name, app) in &self.apple {
            let key = format!("auth.apps.apple.{name}");
            if !is_team_id(&app.team_id) {
                errors.push(format!(
                    "{key}.team_id must be the ten-character Team ID (A-Z, 0-9)"
                ));
            }
            if !is_bundle_id(&app.bundle_id) {
                errors.push(format!(
                    "{key}.bundle_id must be a bundle identifier (A-Z, a-z, 0-9, `-` and `.`)"
                ));
            }
            check_callback(
                &key,
                app.callback.as_deref(),
                app.custom_scheme,
                &mut errors,
            );
        }
        for (name, app) in &self.android {
            let key = format!("auth.apps.android.{name}");
            if !is_package_name(&app.package) {
                errors.push(format!(
                    "{key}.package must be an Android package name (com.example.app)"
                ));
            }
            if app.fingerprints.is_empty()
                || app
                    .fingerprints
                    .iter()
                    .any(|fingerprint| fingerprint_bytes(fingerprint).is_none())
            {
                errors.push(format!(
                    "{key}.fingerprints must list SHA-256 certificate fingerprints \
                     (32 hex bytes separated by `:`)"
                ));
            }
            check_callback(
                &key,
                app.callback.as_deref(),
                app.custom_scheme,
                &mut errors,
            );
        }
        // An id names one app: a hand-over that names it must find one callback.
        let mut named: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, app) in &self.windows {
            let key = format!("auth.apps.windows.{name}");
            if app.package_family_name.is_none() && app.app_id.is_none() {
                errors.push(format!(
                    "{key} must name the app: package_family_name (a build installed from a \
                     package), app_id (a build with no package), or both"
                ));
            }
            if app
                .package_family_name
                .as_deref()
                .is_some_and(|family| !is_package_family_name(family))
            {
                errors.push(format!(
                    "{key}.package_family_name must be a package family name: the package's \
                     name (3 to 50 of A-Z, a-z, 0-9, `.` and `-`), `_` and the 13 characters \
                     of its publisher id (ExampleOrders_8wekyb3d8bbwe)"
                ));
            }
            if app.app_id.as_deref().is_some_and(|id| !is_bundle_id(id)) {
                errors.push(format!(
                    "{key}.app_id must be the id the app was built with (A-Z, a-z, 0-9, `-` \
                     and `.`)"
                ));
            }
            if !app.callback.as_deref().is_some_and(is_own_scheme_link) {
                errors.push(format!(
                    "{key}.callback must be a link on the app's own scheme, written as it is \
                     sent (`com.example.orders:/signed-in`: the scheme in lower case, 2 to 39 \
                     characters, not https and not one a browser or Windows opens itself; no \
                     user, password, query or fragment)"
                ));
            }
            for id in app.ids() {
                if let Some(other) = named.insert(id, name.as_str())
                    && other != name.as_str()
                {
                    errors.push(format!(
                        "{key} names the app `{id}`, which auth.apps.windows.{other} names \
                         too: an app is listed once"
                    ));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The `apple-app-site-association` document, or `None` when no Apple
    /// app opens links (with [`paths`](Self::paths) set) or uses
    /// credentials.
    pub fn apple_association(&self) -> Option<Value> {
        let linked: Vec<String> = self
            .apple
            .values()
            .filter(|app| app.links)
            .map(AppleApp::app_id)
            .collect();
        let credentials: Vec<String> = self
            .apple
            .values()
            .filter(|app| app.credentials)
            .map(AppleApp::app_id)
            .collect();
        let mut document = serde_json::Map::new();
        if !linked.is_empty() && !self.paths.is_empty() {
            document.insert(
                "applinks".to_owned(),
                json!({ "details": [{ "appIDs": linked, "components": self.components() }] }),
            );
        }
        if !credentials.is_empty() {
            document.insert("webcredentials".to_owned(), json!({ "apps": credentials }));
        }
        (!document.is_empty()).then_some(Value::Object(document))
    }

    /// The `assetlinks.json` statements, or `None` without an Android app
    /// that opens links or uses credentials.
    pub fn android_association(&self) -> Option<Value> {
        let statements: Vec<Value> = self
            .android
            .values()
            .filter(|app| app.links || app.credentials)
            .map(|app| {
                let mut relations = Vec::new();
                if app.links {
                    relations.push("delegate_permission/common.handle_all_urls");
                }
                if app.credentials {
                    relations.push("delegate_permission/common.get_login_creds");
                }
                let mut statement = json!({
                    "relation": relations,
                    "target": {
                        "namespace": "android_app",
                        "package_name": app.package.trim(),
                        "sha256_cert_fingerprints": app.normalized_fingerprints(),
                    },
                });
                if app.links && !self.paths.is_empty() {
                    statement["relation_extensions"] = json!({
                        "delegate_permission/common.handle_all_urls": {
                            "dynamic_app_link_components": self.components(),
                        },
                    });
                }
                statement
            })
            .collect();
        (!statements.is_empty()).then_some(Value::Array(statements))
    }

    /// The passkey origins of the Android apps that use credentials:
    /// `android:apk-key-hash:` and the base64url (no padding) SHA-256 of
    /// each signing certificate.
    pub fn passkey_origins(&self) -> Vec<String> {
        let mut origins: Vec<String> = self
            .android
            .values()
            .filter(|app| app.credentials)
            .flat_map(|app| app.fingerprints.iter())
            .filter_map(|fingerprint| fingerprint_bytes(fingerprint))
            .map(|bytes| format!("{ANDROID_ORIGIN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)))
            .collect();
        origins.sort();
        origins.dedup();
        origins
    }

    /// The path components both files share: exclusions first (the first
    /// match decides), then the paths.
    fn components(&self) -> Vec<Value> {
        self.exclude
            .iter()
            .map(|path| json!({ "/": path, "exclude": true }))
            .chain(self.paths.iter().map(|path| json!({ "/": path })))
            .collect()
    }
}

/// An app that can take part in the sign-in hand-over: one of `auth.apps`
/// with a callback.
#[cfg_attr(not(feature = "session"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HandOverApp {
    /// `apple`, `android` or `windows`.
    pub(crate) platform: &'static str,
    /// The bundle id, the package name, or the id a Windows build names
    /// itself by.
    pub(crate) id: String,
    /// Where its code goes.
    pub(crate) callback: String,
}

#[cfg_attr(not(feature = "session"), allow(dead_code))]
impl AppsOptions {
    /// The app `id` of `platform`, when it has a hand-over callback: the
    /// bundle id of an `apple` app, the package name of an `android` one,
    /// and for `windows` either id of an app (its package family name or
    /// the id it was built with).
    pub(crate) fn hand_over_app(&self, platform: &str, id: &str) -> Option<HandOverApp> {
        let found = |callback: &Option<String>, app_id: &str, platform: &'static str| {
            let callback = callback.clone()?;
            (app_id.trim() == id).then(|| HandOverApp {
                platform,
                id: id.to_owned(),
                callback,
            })
        };
        match platform {
            "apple" => self
                .apple
                .values()
                .find_map(|app| found(&app.callback, &app.bundle_id, "apple")),
            "android" => self
                .android
                .values()
                .find_map(|app| found(&app.callback, &app.package, "android")),
            "windows" => self.windows.values().find_map(|app| {
                let callback = app.callback.clone()?;
                app.ids().any(|own| own == id).then(|| HandOverApp {
                    platform: "windows",
                    id: id.to_owned(),
                    callback,
                })
            }),
            _ => None,
        }
    }

    /// The callback a native client of `platform` is told to come back on, in
    /// the shell document. `apple`, `android`: that of the first app that has
    /// one. `windows`: the one callback its apps have between them; when they
    /// have several, none is named, and each client comes back on the scheme
    /// it was built with (a client refuses a callback that is not on its own
    /// scheme, so naming one app's to another would keep it from signing in).
    pub(crate) fn shell_callback(&self, platform: &str) -> Option<String> {
        match platform {
            "apple" => self.apple.values().find_map(|app| app.callback.clone()),
            "android" => self.android.values().find_map(|app| app.callback.clone()),
            "windows" => {
                let mut callbacks = self
                    .windows
                    .values()
                    .filter_map(|app| app.callback.as_deref());
                let first = callbacks.next()?;
                callbacks
                    .all(|other| other == first)
                    .then(|| first.to_owned())
            }
            _ => None,
        }
    }

    /// Whether any app has a hand-over callback.
    pub(crate) fn has_hand_over(&self) -> bool {
        self.apple.values().any(|app| app.callback.is_some())
            || self.android.values().any(|app| app.callback.is_some())
            || self.windows.values().any(|app| app.callback.is_some())
    }

    /// Every app's callback, by its configuration key
    /// (`auth.apps.apple.<name>`).
    fn callbacks(&self) -> impl Iterator<Item = (String, &str)> {
        let apple = self.apple.iter().filter_map(|(name, app)| {
            Some((format!("auth.apps.apple.{name}"), app.callback.as_deref()?))
        });
        let android = self.android.iter().filter_map(|(name, app)| {
            Some((
                format!("auth.apps.android.{name}"),
                app.callback.as_deref()?,
            ))
        });
        let windows = self.windows.iter().filter_map(|(name, app)| {
            Some((
                format!("auth.apps.windows.{name}"),
                app.callback.as_deref()?,
            ))
        });
        apple.chain(android).chain(windows)
    }

    /// Checks that every `https` callback is on the host of `site` (the
    /// app's address, `app.url`): an https callback is an app link of the
    /// site, which the system opens in the app only for the site the app is
    /// associated with. Elsewhere the code would go to a page the site does
    /// not answer for. A `site` that is not `https` (a development address
    /// such as `http://localhost:3000`) checks nothing.
    pub(crate) fn check_callback_hosts(&self, site: &str) -> Result<(), Vec<String>> {
        let Some(host) = url::Url::parse(site)
            .ok()
            .filter(|site| site.scheme() == "https")
            .and_then(|site| site.host_str().map(str::to_ascii_lowercase))
        else {
            return Ok(());
        };
        let errors: Vec<String> = self
            .callbacks()
            .filter(|(_, callback)| {
                url::Url::parse(callback).ok().is_some_and(|url| {
                    is_https_scheme(&url)
                        && url.host_str().map(str::to_ascii_lowercase).as_deref()
                            != Some(host.as_str())
                })
            })
            .map(|(key, _)| {
                format!(
                    "{key}.callback must be on the site's own host ({host}, from app.url): an \
                     https callback is an app link of the site"
                )
            })
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The paths the site answers with an empty page for a browser that
    /// lands on an `https` callback ([`callback_page`](Self::callback_page)):
    /// each once, and only a plain path a route can be (not `/`).
    pub(crate) fn callback_page_paths(&self) -> Vec<String> {
        if !self.callback_page {
            return Vec::new();
        }
        let mut paths: Vec<String> = self
            .callbacks()
            .filter_map(|(_, callback)| url::Url::parse(callback).ok())
            .filter(is_https_scheme)
            .map(|url| url.path().to_owned())
            .filter(|path| is_plain_path(path))
            .collect();
        paths.sort();
        paths.dedup();
        paths
    }
}

/// Whether `path` is `/` and segments of unreserved characters, with no
/// empty, `.` or `..` segment and no trailing slash (not `/` itself).
#[cfg_attr(not(feature = "session"), allow(dead_code))]
fn is_plain_path(path: &str) -> bool {
    path.len() > 1
        && path.starts_with('/')
        && path[1..].split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
        })
}

/// Checks an app's hand-over callback: an absolute `https` URL with a host
/// and no user, password or fragment; a custom scheme only when the app
/// allows one, and never a scheme a browser loads or runs itself.
fn check_callback(
    key: &str,
    callback: Option<&str>,
    custom_scheme: bool,
    errors: &mut Vec<String>,
) {
    let Some(callback) = callback else {
        return;
    };
    let fine = url::Url::parse(callback).ok().is_some_and(|url| {
        let plain = url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && callback.len() <= 2048;
        if is_https_scheme(&url) {
            return plain && url.host_str().is_some_and(|host| !host.is_empty());
        }
        !is_browsers_scheme(&url.scheme().to_ascii_lowercase()) && custom_scheme && plain
    });
    if !fine {
        errors.push(format!(
            "{key}.callback must be an https URL of the app (a universal link or an App \
             Link), or a custom scheme with {key}.custom_scheme = true"
        ));
    }
}

/// Whether a browser loads, runs or hands on a link of `scheme` (in lower
/// case) itself: never a scheme of an app's own.
fn is_browsers_scheme(scheme: &str) -> bool {
    matches!(
        scheme,
        "http"
            | "javascript"
            | "data"
            | "file"
            | "blob"
            | "about"
            | "ftp"
            | "ws"
            | "wss"
            | "intent"
            | "content"
            | "mailto"
            | "tel"
            | "sms"
    )
}

/// Whether Windows or a program that comes with it opens a link of `scheme`
/// (in lower case) itself: its settings, its store and its installer
/// (`ms-...`), its browser, its search, its shell.
fn is_windows_scheme(scheme: &str) -> bool {
    scheme.starts_with("ms-")
        || matches!(
            scheme,
            "microsoft-edge"
                | "search"
                | "search-ms"
                | "shell"
                | "vbscript"
                | "res"
                | "view-source"
        )
}

/// Whether `callback` is a link on a scheme of an app's own, as the callback
/// of a Windows app must be:
///
/// - written as it is sent (what the URL standard makes of it is the same
///   text, so the scheme is in lower case and nothing needs encoding): the
///   address the hand-over redirects to, the callback the shell names to the
///   app and the `redirect_uri` the app sends are then one string;
/// - a scheme of 2 to 39 characters, as Windows registers one, that is not
///   `https` and not one a browser or Windows opens itself;
/// - a path or a host after the scheme, and no user, password, query or
///   fragment (the code and the state are the whole query), at most 2048
///   bytes.
fn is_own_scheme_link(callback: &str) -> bool {
    url::Url::parse(callback).ok().is_some_and(|url| {
        let scheme = url.scheme();
        callback.len() <= 2048
            && url.as_str() == callback
            && (2..=39).contains(&scheme.len())
            && !is_https_scheme(&url)
            && !is_browsers_scheme(scheme)
            && !is_windows_scheme(scheme)
            && !url.cannot_be_a_base()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

/// Whether `name` is a package family name, as Windows writes one: the
/// package's name (3 to 50 letters, digits, `.` and `-`), `_`, and the
/// publisher id, 13 characters of the 32 it is written in (digits and lower
/// case letters without `i`, `l`, `o` and `u`).
fn is_package_family_name(name: &str) -> bool {
    name.rsplit_once('_').is_some_and(|(package, publisher)| {
        (3..=50).contains(&package.len())
            && package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
            && publisher.len() == 13
            && publisher.bytes().all(|b| {
                matches!(b, b'0'..=b'9' | b'a'..=b'h' | b'j' | b'k' | b'm' | b'n' | b'p'..=b't' | b'v'..=b'z')
            })
    })
}

/// Whether `url`'s scheme is `https`, in any case: the one reading of a
/// callback's scheme, for the configuration check and the hand-over alike.
fn is_https_scheme(url: &url::Url) -> bool {
    url.scheme().eq_ignore_ascii_case("https")
}

/// Whether `text` is an absolute `https` URL (`HTTPS://` too), read as
/// [`check_callback`] reads a callback.
#[cfg_attr(not(feature = "session"), allow(dead_code))]
pub(crate) fn is_https_url(text: &str) -> bool {
    url::Url::parse(text).is_ok_and(|parsed| is_https_scheme(&parsed))
}

/// Whether `origin` is an Android app's passkey origin:
/// `android:apk-key-hash:` and exactly 32 bytes in base64url without
/// padding.
pub(crate) fn is_android_origin(origin: &str) -> bool {
    origin
        .strip_prefix(ANDROID_ORIGIN_PREFIX)
        .filter(|hash| hash.len() == 43)
        .and_then(|hash| URL_SAFE_NO_PAD.decode(hash).ok())
        .is_some_and(|bytes| {
            bytes.len() == 32
                && URL_SAFE_NO_PAD.encode(&bytes) == origin[ANDROID_ORIGIN_PREFIX.len()..]
        })
}

/// The 32 bytes of a `AA:BB:..` SHA-256 fingerprint (either case).
fn fingerprint_bytes(fingerprint: &str) -> Option<[u8; 32]> {
    let parts: Vec<&str> = fingerprint.trim().split(':').collect();
    if parts.len() != 32 {
        return None;
    }
    let mut bytes = [0_u8; 32];
    for (byte, part) in bytes.iter_mut().zip(parts) {
        if part.len() != 2 {
            return None;
        }
        *byte = u8::from_str_radix(part, 16).ok()?;
    }
    Some(bytes)
}

fn is_path_pattern(pattern: &str) -> bool {
    pattern.starts_with('/')
        && pattern.len() <= MAX_PATH_LEN
        && !pattern
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
}

fn is_team_id(id: &str) -> bool {
    id.len() == 10
        && id
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn is_bundle_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 155
        && !id.starts_with('.')
        && !id.ends_with('.')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

fn is_package_name(name: &str) -> bool {
    let segments: Vec<&str> = name.split('.').collect();
    segments.len() >= 2
        && segments.iter().all(|segment| {
            let mut chars = segment.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// `auth.apps` from the app's `Config`, or the defaults (no apps). With an
/// `https` `app.url`, an `https` callback on another host fails the boot.
pub(crate) async fn apps_options(r: &Resolver) -> Result<AppsOptions, BoxError> {
    match r.app().try_get::<Config>().await {
        Some(config) => {
            let apps = config.typed::<AppsOptions>()?;
            if let Some(site) = config.app_url() {
                apps.check_callback_hosts(&site)
                    .map_err(|errors| errors.join("; "))?;
            }
            Ok(apps)
        }
        None => Ok(AppsOptions::default()),
    }
}

/// The association files, rendered once at boot.
pub(crate) struct AppLinksController {
    apple: Option<Arc<[u8]>>,
    android: Option<Arc<[u8]>>,
}

impl Injectable for AppLinksController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<AppsOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let apps = r.get::<AppsOptions>().await?;
            let render = |document: Option<Value>| {
                document.map(|document| Arc::from(document.to_string().into_bytes()))
            };
            Ok(AppLinksController {
                apple: render(apps.apple_association()),
                android: render(apps.android_association()),
            })
        })
    }
}

/// A `200 application/json` answer with `body`.
fn json_file(body: &Arc<[u8]>) -> Response {
    let mut response = body.to_vec().into_response();
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    response
}

impl Controller for AppLinksController {
    fn prefix(&self) -> &'static str {
        "!/"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let mut routes = Vec::new();
        let meta = |summary: &str| crate::controller::api(summary, &[]);
        if let Some(body) = self.apple.clone() {
            routes.push(
                RouteDef::on(
                    Method::GET,
                    format!("!{APPLE_ASSOCIATION_PATH}"),
                    move || {
                        let body = body.clone();
                        async move { json_file(&body) }
                    },
                )
                .public()
                .name("auth.apps.apple")
                .api(meta("Apple's app site association")),
            );
        }
        if let Some(body) = self.android.clone() {
            routes.push(
                RouteDef::on(
                    Method::GET,
                    format!("!{ANDROID_ASSOCIATION_PATH}"),
                    move || {
                        let body = body.clone();
                        async move { json_file(&body) }
                    },
                )
                .public()
                .name("auth.apps.android")
                .api(meta("Android's digital asset links")),
            );
        }
        routes
    }
}

#[cfg(test)]
mod tests {
    use navree_config::ConfigSources;

    use super::*;

    const FINGERPRINT: &str = "14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5";

    fn load(sources: ConfigSources) -> Result<AppsOptions, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<AppsOptions>()
    }

    fn configured() -> AppsOptions {
        load(
            ConfigSources::new()
                .set("auth.apps.paths", json!(["/orders/*", "/invite/*"]))
                .set("auth.apps.exclude", json!(["/orders/*/print"]))
                .set("auth.apps.apple.main.team_id", "ABCDE12345")
                .set("auth.apps.apple.main.bundle_id", "com.example.orders")
                .set("auth.apps.android.main.package", "com.example.orders")
                .set(
                    "auth.apps.android.main.fingerprints",
                    json!([FINGERPRINT.to_lowercase()]),
                ),
        )
        .unwrap()
    }

    #[test]
    fn the_apple_file_lists_links_and_credentials() {
        let document = configured().apple_association().unwrap();
        assert_eq!(
            document,
            json!({
                "applinks": { "details": [{
                    "appIDs": ["ABCDE12345.com.example.orders"],
                    "components": [
                        { "/": "/orders/*/print", "exclude": true },
                        { "/": "/orders/*" },
                        { "/": "/invite/*" },
                    ],
                }] },
                "webcredentials": { "apps": ["ABCDE12345.com.example.orders"] },
            })
        );

        // Credentials only: no links section; no paths: no links either.
        let mut apps = configured();
        apps.apple.get_mut("main").unwrap().links = false;
        assert_eq!(
            apps.apple_association().unwrap(),
            json!({ "webcredentials": { "apps": ["ABCDE12345.com.example.orders"] } })
        );
        let mut apps = configured();
        apps.paths.clear();
        apps.apple.get_mut("main").unwrap().credentials = false;
        assert_eq!(apps.apple_association(), None);
    }

    #[test]
    fn the_android_file_lists_relations_and_paths() {
        let document = configured().android_association().unwrap();
        assert_eq!(
            document,
            json!([{
                "relation": [
                    "delegate_permission/common.handle_all_urls",
                    "delegate_permission/common.get_login_creds",
                ],
                "target": {
                    "namespace": "android_app",
                    "package_name": "com.example.orders",
                    "sha256_cert_fingerprints": [FINGERPRINT],
                },
                "relation_extensions": {
                    "delegate_permission/common.handle_all_urls": {
                        "dynamic_app_link_components": [
                            { "/": "/orders/*/print", "exclude": true },
                            { "/": "/orders/*" },
                            { "/": "/invite/*" },
                        ],
                    },
                },
            }])
        );
        let mut apps = configured();
        apps.android.get_mut("main").unwrap().links = false;
        let document = apps.android_association().unwrap();
        assert_eq!(
            document[0]["relation"],
            json!(["delegate_permission/common.get_login_creds"])
        );
        assert!(document[0].get("relation_extensions").is_none());
    }

    #[test]
    fn android_signatures_become_passkey_origins() {
        let origins = configured().passkey_origins();
        assert_eq!(
            origins,
            ["android:apk-key-hash:FG3pg8VzBlDY7rmVLzT8ZBagg0LmHb6oigSWsj_PROU"]
        );
        assert!(origins.iter().all(|origin| is_android_origin(origin)));
        let mut apps = configured();
        apps.android.get_mut("main").unwrap().credentials = false;
        assert!(apps.passkey_origins().is_empty());
    }

    #[test]
    fn android_origins_are_read_exactly() {
        let hash = "FG3pg8VzBlDY7rmVLzT8ZBagg0LmHb6oigSWsj_PROU";
        assert!(is_android_origin(&format!("android:apk-key-hash:{hash}")));
        for refused in [
            format!("android:apk-key-hash:{hash}="),
            format!("android:apk-key-hash:{}", &hash[..42]),
            format!("android:apk-key-hash:{hash}A"),
            format!("android:apk-key-hash:{}", hash.replace('_', "/")),
            format!("Android:apk-key-hash:{hash}"),
            format!("android:apk-key-hash: {hash}"),
            "android:apk-key-hash:".to_owned(),
            // The last character carries spare bits: only one spelling of
            // the 32 bytes is the origin.
            format!("android:apk-key-hash:{}V", &hash[..42]),
        ] {
            assert!(!is_android_origin(&refused), "{refused}");
        }
    }

    #[test]
    fn nothing_is_served_without_apps() {
        let apps = load(ConfigSources::new()).unwrap();
        assert_eq!(apps, AppsOptions::default());
        assert_eq!(apps.apple_association(), None);
        assert_eq!(apps.android_association(), None);
        assert!(apps.passkey_origins().is_empty());
    }

    #[test]
    fn bad_ids_and_fingerprints_are_refused() {
        for (key, value) in [
            ("auth.apps.paths", json!(["orders/*"])),
            ("auth.apps.exclude", json!(["/a b"])),
            ("auth.apps.apple.main.team_id", json!("abcde12345")),
            ("auth.apps.apple.main.team_id", json!("ABCDE1234")),
            ("auth.apps.apple.main.bundle_id", json!("com..example")),
            ("auth.apps.android.main.package", json!("orders")),
            ("auth.apps.android.main.package", json!("com.9example")),
            ("auth.apps.android.main.fingerprints", json!([])),
            ("auth.apps.android.main.fingerprints", json!(["14:6D"])),
            (
                "auth.apps.android.main.fingerprints",
                json!([FINGERPRINT.replace(':', "")]),
            ),
        ] {
            let mut sources = ConfigSources::new()
                .set("auth.apps.apple.main.team_id", "ABCDE12345")
                .set("auth.apps.apple.main.bundle_id", "com.example.orders")
                .set("auth.apps.android.main.package", "com.example.orders")
                .set("auth.apps.android.main.fingerprints", json!([FINGERPRINT]));
            sources = sources.set(key, value.clone());
            let error = load(sources).expect_err(key).to_string();
            let field = key.rsplit('.').next().unwrap();
            assert!(error.contains(field), "{key} = {value}: {error}");
        }
    }

    #[test]
    fn hand_over_callbacks_are_https_unless_a_scheme_is_allowed() {
        let with = |callback: &str, custom_scheme: bool| {
            load(
                ConfigSources::new()
                    .set("auth.apps.apple.main.team_id", "ABCDE12345")
                    .set("auth.apps.apple.main.bundle_id", "com.example.orders")
                    .set("auth.apps.apple.main.callback", callback)
                    .set("auth.apps.apple.main.custom_scheme", custom_scheme),
            )
        };
        let apps = with("https://example.com/app/signed-in", false).unwrap();
        assert_eq!(
            apps.hand_over_app("apple", "com.example.orders")
                .map(|app| app.callback),
            Some("https://example.com/app/signed-in".to_owned())
        );
        assert!(apps.has_hand_over());
        assert_eq!(apps.hand_over_app("android", "com.example.orders"), None);
        assert_eq!(apps.hand_over_app("apple", "com.example.other"), None);
        for refused in [
            "http://example.com/app/signed-in",
            "https://user:pass@example.com/app",
            "https://example.com/app#code",
            "javascript:alert(1)",
            "data:text/html,x",
            "/app/signed-in",
            "com.example.orders:/signed-in",
        ] {
            let error = with(refused, false).expect_err(refused).to_string();
            assert!(error.contains("callback"), "{refused}: {error}");
        }
        assert!(with("com.example.orders:/signed-in", true).is_ok());
        assert!(with("javascript:alert(1)", true).is_err());
        assert!(!configured().has_hand_over());
    }

    #[test]
    fn https_callbacks_are_on_the_sites_own_host() {
        let apps = load(
            ConfigSources::new()
                .set("auth.apps.apple.main.team_id", "ABCDE12345")
                .set("auth.apps.apple.main.bundle_id", "com.example.orders")
                .set(
                    "auth.apps.apple.main.callback",
                    "https://example.com/app/signed-in",
                )
                .set("auth.apps.android.main.package", "com.example.orders")
                .set("auth.apps.android.main.fingerprints", json!([FINGERPRINT]))
                .set(
                    "auth.apps.android.main.callback",
                    "com.example.orders:/signed-in",
                )
                .set("auth.apps.android.main.custom_scheme", true),
        )
        .unwrap();
        assert_eq!(apps.check_callback_hosts("https://example.com"), Ok(()));
        assert_eq!(
            apps.check_callback_hosts("https://EXAMPLE.com:8443/shop"),
            Ok(())
        );
        let errors = apps
            .check_callback_hosts("https://shop.example.com")
            .unwrap_err();
        // The https callback only: a custom scheme has no host to check.
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with("auth.apps.apple.main.callback must be on the site's own host"),
            "{errors:?}"
        );
        assert!(errors[0].contains("shop.example.com"), "{errors:?}");
        // A development address is not the site the apps are linked to.
        assert_eq!(apps.check_callback_hosts("http://localhost:3000"), Ok(()));
    }

    #[test]
    fn an_upper_case_scheme_is_https_everywhere() {
        let apps = load(
            ConfigSources::new()
                .set("auth.apps.apple.main.team_id", "ABCDE12345")
                .set("auth.apps.apple.main.bundle_id", "com.example.orders")
                .set(
                    "auth.apps.apple.main.callback",
                    "HTTPS://example.com/app/signed-in",
                ),
        )
        .expect("an https callback needs no custom_scheme");
        assert!(is_https_url("HTTPS://example.com/app/signed-in"));
        assert!(!is_https_url("com.example.orders:/signed-in"));
        assert_eq!(apps.check_callback_hosts("https://example.com"), Ok(()));
        assert_eq!(
            apps.check_callback_hosts("https://shop.example.com")
                .map_err(|errors| errors.len()),
            Err(1)
        );
        let mut apps = apps;
        apps.callback_page = true;
        assert_eq!(apps.callback_page_paths(), ["/app/signed-in"]);
    }

    #[test]
    fn the_site_answers_each_https_callback_path_once() {
        let mut apps = load(
            ConfigSources::new()
                .set("auth.apps.apple.main.team_id", "ABCDE12345")
                .set("auth.apps.apple.main.bundle_id", "com.example.orders")
                .set(
                    "auth.apps.apple.main.callback",
                    "https://example.com/app/signed-in?from=ios",
                )
                .set("auth.apps.apple.kiosk.team_id", "ABCDE12345")
                .set("auth.apps.apple.kiosk.bundle_id", "com.example.kiosk")
                .set("auth.apps.apple.kiosk.callback", "https://example.com/")
                .set("auth.apps.android.main.package", "com.example.orders")
                .set("auth.apps.android.main.fingerprints", json!([FINGERPRINT]))
                .set(
                    "auth.apps.android.main.callback",
                    "https://example.com/app/signed-in",
                )
                .set("auth.apps.android.legacy.package", "com.example.legacy")
                .set(
                    "auth.apps.android.legacy.fingerprints",
                    json!([FINGERPRINT]),
                )
                .set(
                    "auth.apps.android.legacy.callback",
                    "com.example.legacy:/signed-in",
                )
                .set("auth.apps.android.legacy.custom_scheme", true),
        )
        .unwrap();
        // Off by default: an app may serve that path itself.
        assert!(!apps.callback_page);
        assert!(apps.callback_page_paths().is_empty());
        apps.callback_page = true;
        // Once for both apps; never the site's root; never a scheme.
        assert_eq!(apps.callback_page_paths(), ["/app/signed-in"]);
        for (path, plain) in [
            ("/app/signed-in", true),
            ("/a.b/c_d~e-f", true),
            ("/", false),
            ("/app/", false),
            ("/app//in", false),
            ("/app/../in", false),
            ("/app/%7Bid%7D", false),
        ] {
            assert_eq!(is_plain_path(path), plain, "{path}");
        }
    }

    const FAMILY: &str = "CharkhaSupply_8wekyb3d8bbwe";

    /// The settings with one Windows app, and `more` over them.
    fn windows(more: &[(&str, Value)]) -> Result<AppsOptions, navree_config::ConfigError> {
        let mut sources = ConfigSources::new()
            .set("auth.apps.windows.main.package_family_name", FAMILY)
            .set("auth.apps.windows.main.app_id", "com.charkha.supply")
            .set("auth.apps.windows.main.callback", "charkha:/signed-in");
        for (key, value) in more {
            sources = sources.set(*key, value.clone());
        }
        load(sources)
    }

    #[test]
    fn a_windows_app_is_listed_by_either_id_with_a_callback_on_its_own_scheme() {
        let apps = windows(&[]).unwrap();
        assert!(apps.has_hand_over());
        for id in [FAMILY, "com.charkha.supply"] {
            assert_eq!(
                apps.hand_over_app("windows", id),
                Some(HandOverApp {
                    platform: "windows",
                    id: id.to_owned(),
                    callback: "charkha:/signed-in".to_owned(),
                }),
                "{id}"
            );
            // The id of a Windows app names no app of another system.
            assert_eq!(apps.hand_over_app("apple", id), None);
            assert_eq!(apps.hand_over_app("android", id), None);
        }
        for other in [
            "",
            "main",
            "CharkhaSupply",
            "charkhasupply_8wekyb3d8bbwe",
            "CharkhaSupply_8wekyb3d8bbwe ",
            "com.charkha",
            "com.charkha.supply.kiosk",
        ] {
            assert_eq!(apps.hand_over_app("windows", other), None, "{other:?}");
        }
        assert_eq!(apps.hand_over_app("Windows", FAMILY), None);
        assert_eq!(
            apps.shell_callback("windows").as_deref(),
            Some("charkha:/signed-in")
        );
        assert_eq!(apps.shell_callback("apple"), None);
        // Windows has no file by which a site names its apps, and no passkey origin.
        assert_eq!(apps.apple_association(), None);
        assert_eq!(apps.android_association(), None);
        assert!(apps.passkey_origins().is_empty());
        // A scheme is no link of the site: no host to check, no page to answer.
        assert_eq!(
            apps.check_callback_hosts("https://shop.example.com"),
            Ok(())
        );
        let mut apps = apps;
        apps.callback_page = true;
        assert!(apps.callback_page_paths().is_empty());
        // One id is enough.
        for (kept, id) in [
            ("package_family_name", FAMILY),
            ("app_id", "com.charkha.supply"),
        ] {
            let one = load(
                ConfigSources::new()
                    .set(&format!("auth.apps.windows.main.{kept}"), id)
                    .set("auth.apps.windows.main.callback", "charkha://signed-in"),
            )
            .unwrap_or_else(|error| panic!("{kept} alone: {error}"));
            assert_eq!(
                one.hand_over_app("windows", id).map(|app| app.callback),
                Some("charkha://signed-in".to_owned()),
                "{kept} alone"
            );
        }
    }

    #[test]
    fn a_windows_app_needs_an_id_of_the_right_shape() {
        let none =
            load(ConfigSources::new().set("auth.apps.windows.main.callback", "charkha:/signed-in"))
                .expect_err("no id")
                .to_string();
        assert!(
            none.contains("auth.apps.windows.main must name the app"),
            "{none}"
        );
        for (field, value) in [
            // No publisher id, one of another length, one in capitals, one with a
            // letter the 32 do not have (`i`, `l`, `o`, `u`).
            ("package_family_name", "CharkhaSupply"),
            ("package_family_name", "CharkhaSupply_8wekyb3d8bbw"),
            ("package_family_name", "CharkhaSupply_8wekyb3d8bbwe1"),
            ("package_family_name", "CharkhaSupply_8WEKYB3D8BBWE"),
            ("package_family_name", "CharkhaSupply_8wekyb3d8bbwi"),
            // A package's name of 3 to 50 letters, digits, `.` and `-`.
            ("package_family_name", "Ch_8wekyb3d8bbwe"),
            ("package_family_name", "Charkha Supply_8wekyb3d8bbwe"),
            ("package_family_name", "Charkha_Supply_8wekyb3d8bbwe"),
            ("app_id", "com..charkha"),
            ("app_id", ".charkha"),
            ("app_id", "com.charkha.supply/x"),
            ("app_id", "com.charkha supply"),
            ("app_id", "Charkha_Supply"),
        ] {
            let key = format!("auth.apps.windows.main.{field}");
            let error = windows(&[(key.as_str(), json!(value))])
                .expect_err(value)
                .to_string();
            assert!(error.contains(&key), "{field} = {value:?}: {error}");
        }
        assert!(!is_package_family_name("") && !is_package_family_name("_"));
        assert!(is_package_family_name(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe"
        ));
        assert!(is_package_family_name(&format!(
            "{}_0123456789abc",
            "a".repeat(50)
        )));
        assert!(!is_package_family_name(&format!(
            "{}_0123456789abc",
            "a".repeat(51)
        )));
    }

    #[test]
    fn a_windows_callback_is_a_link_on_the_apps_own_scheme_and_nothing_else() {
        for callback in [
            "charkha:/signed-in",
            "charkha://signed-in",
            "charkha:/",
            "com.charkha.supply:/signed-in",
            "charkha-supply+in:/auth/signed-in",
        ] {
            let apps = windows(&[("auth.apps.windows.main.callback", json!(callback))])
                .unwrap_or_else(|error| panic!("{callback}: {error}"));
            assert_eq!(
                apps.hand_over_app("windows", FAMILY)
                    .map(|app| app.callback)
                    .as_deref(),
                Some(callback)
            );
        }
        for refused in [
            // A link of the site, a port of the machine, and what a browser opens itself.
            "https://charkha.example/app/signed-in",
            "HTTPS://charkha.example/app/signed-in",
            "http://127.0.0.1:5123/signed-in",
            "http://localhost/signed-in",
            "javascript:alert(1)",
            "data:text/html,x",
            "file:///C:/signed-in",
            "mailto:simran@charkha.example",
            // What Windows opens itself.
            "ms-settings:/signed-in",
            "ms-appinstaller:/signed-in",
            "microsoft-edge:/signed-in",
            "search-ms:/signed-in",
            "shell:/signed-in",
            // Not written as it is sent: a capital in the scheme, a space.
            "Charkha:/signed-in",
            "CHARKHA://signed-in",
            "charkha:/signed in",
            // A scheme of one character (a drive's letter) or of more than 39.
            "c:/signed-in",
            "abcdefghijklmnopqrstuvwxyzabcdefghijklmn:/signed-in",
            // A user, a password, a query, a fragment; no path or host; no scheme.
            "charkha://user@signed-in",
            "charkha://user:pass@signed-in",
            "charkha:/signed-in?from=windows",
            "charkha:/signed-in?state=planted",
            "charkha:/signed-in#code",
            "charkha:signed-in",
            "charkha",
            "/signed-in",
            "",
        ] {
            let error = windows(&[("auth.apps.windows.main.callback", json!(refused))])
                .expect_err(refused)
                .to_string();
            assert!(
                error.contains("auth.apps.windows.main.callback"),
                "{refused}: {error}"
            );
        }
        // A callback is required: a Windows app is listed for the hand-over alone.
        let error =
            load(ConfigSources::new().set("auth.apps.windows.main.package_family_name", FAMILY))
                .expect_err("no callback")
                .to_string();
        assert!(error.contains("auth.apps.windows.main.callback"), "{error}");
        // The limits: a scheme of 39 characters, a callback of 2048 bytes.
        assert!(is_own_scheme_link(&format!("{}:/x", "a".repeat(39))));
        assert!(!is_own_scheme_link(&format!("{}:/x", "a".repeat(40))));
        assert!(!is_own_scheme_link(&format!(
            "charkha:/{}",
            "x".repeat(2048)
        )));
    }

    #[test]
    fn an_id_names_one_windows_app_and_the_shell_names_a_callback_they_share() {
        let second = |id: &str, callback: &str| {
            windows(&[
                ("auth.apps.windows.kiosk.app_id", json!(id)),
                ("auth.apps.windows.kiosk.callback", json!(callback)),
            ])
        };
        // The same id under two names: a hand-over that names it could go either way.
        let error = second("com.charkha.supply", "charkha-kiosk:/signed-in")
            .expect_err("one id twice")
            .to_string();
        assert!(
            error.contains("names the app `com.charkha.supply`")
                && error.contains("auth.apps.windows.kiosk")
                && error.contains("auth.apps.windows.main"),
            "{error}"
        );
        // Two apps, one way back: the shell names it. Two ways back: it names none.
        let same = second("com.charkha.kiosk", "charkha:/signed-in").unwrap();
        assert_eq!(
            same.shell_callback("windows").as_deref(),
            Some("charkha:/signed-in")
        );
        let two = second("com.charkha.kiosk", "charkha-kiosk:/signed-in").unwrap();
        assert_eq!(two.shell_callback("windows"), None);
        assert_eq!(
            two.hand_over_app("windows", "com.charkha.kiosk")
                .map(|app| app.callback)
                .as_deref(),
            Some("charkha-kiosk:/signed-in")
        );
        assert_eq!(
            two.hand_over_app("windows", FAMILY)
                .map(|app| app.callback)
                .as_deref(),
            Some("charkha:/signed-in")
        );
        assert_eq!(windows(&[]).unwrap().windows.len(), 1);
        assert_eq!(configured().shell_callback("windows"), None);
    }

    // The rules of Apple and Android callbacks are what they were: the list of
    // schemes a browser opens itself is one list, read by both checks.
    #[test]
    fn the_schemes_a_browser_opens_itself_are_refused_for_every_system() {
        for scheme in [
            "http",
            "javascript",
            "data",
            "file",
            "blob",
            "about",
            "ftp",
            "ws",
            "wss",
            "intent",
            "content",
            "mailto",
            "tel",
            "sms",
        ] {
            assert!(is_browsers_scheme(scheme), "{scheme}");
            let callback = format!("{scheme}:/signed-in");
            assert!(!is_own_scheme_link(&callback), "{callback}");
            let apple = load(
                ConfigSources::new()
                    .set("auth.apps.apple.main.team_id", "ABCDE12345")
                    .set("auth.apps.apple.main.bundle_id", "com.example.orders")
                    .set("auth.apps.apple.main.callback", callback.as_str())
                    .set("auth.apps.apple.main.custom_scheme", true),
            );
            assert!(apple.is_err(), "{callback}");
        }
        assert!(!is_browsers_scheme("com.example.orders"));
        assert!(!is_browsers_scheme("https"));
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = AppsOptions::schema().expect("schema");
        assert_eq!(schema.key, "auth.apps");
        assert!(schema.secret_paths().is_empty());
    }
}

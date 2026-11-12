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
/// ```
///
/// An Android app with `credentials` adds its signatures to the passkey
/// origins (`android:apk-key-hash:<base64url SHA-256>`), so its passkey
/// assertions are accepted beside `auth.passkeys.origins`. In production
/// list the Play App Signing certificate; a debug certificate only for
/// development builds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ConfigSection)]
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
}

impl Default for AppleApp {
    fn default() -> Self {
        Self {
            team_id: String::new(),
            bundle_id: String::new(),
            links: true,
            credentials: true,
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
}

impl Default for AndroidApp {
    fn default() -> Self {
        Self {
            package: String::new(),
            fingerprints: Vec::new(),
            links: true,
            credentials: true,
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
            document.insert(
                "webcredentials".to_owned(),
                json!({ "apps": credentials }),
            );
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

/// `auth.apps` from the app's `Config`, or the defaults (no apps).
pub(crate) async fn apps_options(r: &Resolver) -> Result<AppsOptions, BoxError> {
    match r.app().try_get::<Config>().await {
        Some(config) => Ok(config.typed::<AppsOptions>()?),
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
                RouteDef::on(Method::GET, format!("!{APPLE_ASSOCIATION_PATH}"), move || {
                    let body = body.clone();
                    async move { json_file(&body) }
                })
                .public()
                .name("auth.apps.apple")
                .api(meta("Apple's app site association")),
            );
        }
        if let Some(body) = self.android.clone() {
            routes.push(
                RouteDef::on(Method::GET, format!("!{ANDROID_ASSOCIATION_PATH}"), move || {
                    let body = body.clone();
                    async move { json_file(&body) }
                })
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
    fn the_section_describes_itself() {
        let schema = AppsOptions::schema().expect("schema");
        assert_eq!(schema.key, "auth.apps");
        assert!(schema.secret_paths().is_empty());
    }
}

//! `navree native:doctor`: what a native build still needs, checked without
//! changing anything.
//!
//! Each item says it is ready, missing or not applicable, and for anything
//! missing the one next step. Nothing is built or booted: the app's settings
//! are read the way the app reads them (`navree_navigator::Project`), its
//! manifest tells whether the native preview is on, and the machine is asked
//! for the toolchains of each system. Secrets (the push keys, the sign-in
//! private key) are checked for being there and never shown.

use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Serialize;
use serde_json::Value;

use crate::native::Preview;

/// The oldest JDK the Android build runs on.
pub const MIN_JDK: u32 = 17;

/// The oldest Android platform (compile SDK) the client builds against.
pub const MIN_ANDROID_PLATFORM: u32 = 36;

/// The oldest Xcode the Apple clients build with.
pub const MIN_XCODE: u32 = 26;

/// Where an association file is served.
pub const APPLE_FILE: &str = "/.well-known/apple-app-site-association";
/// Where Android's association file is served.
pub const ANDROID_FILE: &str = "/.well-known/assetlinks.json";

/// How an item stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Nothing more to do.
    Ready,
    /// A native build needs it and it is not there.
    Missing,
    /// This app, or this machine, has no use for it.
    NotApplicable,
}

/// What an item belongs to, for the order of the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    /// The app's manifest.
    App,
    /// The app's settings.
    Settings,
    /// The Android toolchain.
    Android,
    /// The Apple toolchain.
    Apple,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Self::App => "The app",
            Self::Settings => "Settings",
            Self::Android => "Android toolchain",
            Self::Apple => "Apple toolchain",
        }
    }
}

/// One thing a native build needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    /// A stable name for tools.
    pub id: &'static str,
    /// What it belongs to.
    pub group: Group,
    /// What it is.
    pub title: String,
    /// How it stands.
    pub status: Status,
    /// What was found, or what is wrong.
    pub detail: String,
    /// The one next step, for something missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

impl Item {
    fn new(id: &'static str, group: Group, title: &str, status: Status, detail: String) -> Self {
        Self {
            id,
            group,
            title: title.to_owned(),
            status,
            detail,
            next: None,
        }
    }

    fn ready(id: &'static str, group: Group, title: &str, detail: impl Into<String>) -> Self {
        Self::new(id, group, title, Status::Ready, detail.into())
    }

    fn missing(
        id: &'static str,
        group: Group,
        title: &str,
        detail: impl Into<String>,
        next: impl Into<String>,
    ) -> Self {
        Self {
            next: Some(next.into()),
            ..Self::new(id, group, title, Status::Missing, detail.into())
        }
    }

    fn not_applicable(
        id: &'static str,
        group: Group,
        title: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(id, group, title, Status::NotApplicable, detail.into())
    }
}

/// Which systems to check for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Scope {
    /// Both.
    #[default]
    All,
    /// The Apple systems (iOS, iPadOS, macOS, ...).
    Ios,
    /// Android.
    Android,
}

impl Scope {
    fn apple(self) -> bool {
        matches!(self, Self::All | Self::Ios)
    }

    fn android(self) -> bool {
        matches!(self, Self::All | Self::Android)
    }

    fn flag(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

/// The system the machine runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    Mac,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
    /// Anything else.
    Other,
}

impl Os {
    fn name(self) -> &'static str {
        match self {
            Self::Mac => "macOS",
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::Other => "this system",
        }
    }
}

/// What the doctor asks of the machine it runs on.
pub trait Machine {
    /// The operating system.
    fn os(&self) -> Os;
    /// An environment variable, when set and not empty.
    fn var(&self, name: &str) -> Option<String>;
    /// The home directory.
    fn home(&self) -> Option<PathBuf>;
    /// Whether `path` is a directory.
    fn is_dir(&self, path: &Path) -> bool;
    /// Whether `path` is a file.
    fn is_file(&self, path: &Path) -> bool;
    /// The names inside the directory `path`.
    fn list(&self, path: &Path) -> Vec<String>;
    /// Runs `program` and returns what it printed (both streams), when it ran.
    fn run(&self, program: &str, args: &[&str]) -> Option<String>;
}

/// The machine this process runs on.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemMachine;

impl Machine for SystemMachine {
    fn os(&self) -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other
        }
    }

    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.trim().is_empty())
    }

    fn home(&self) -> Option<PathBuf> {
        self.var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn list(&self, path: &Path) -> Vec<String> {
        std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn run(&self, program: &str, args: &[&str]) -> Option<String> {
        let output = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Some(text)
    }
}

/// What a running site answered for the two association files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// The site's address.
    pub url: String,
    /// Apple's file: its body, or what went wrong.
    pub apple: Result<String, String>,
    /// Android's file: its body, or what went wrong.
    pub android: Result<String, String>,
}

/// The app's settings: the value at a dotted key (`auth.apps`), when there is
/// one.
pub type Lookup = dyn Fn(&str) -> Option<Value>;

/// Everything the checks read.
pub struct Facts<'a> {
    /// The machine.
    pub machine: &'a dyn Machine,
    /// How the app stands with the native preview.
    pub preview: &'a Preview,
    /// The app's settings by dotted key, or why they did not load.
    pub settings: Result<&'a Lookup, String>,
    /// Which systems to check.
    pub scope: Scope,
    /// A running site's association files, when it was asked.
    pub site: Option<&'a Site>,
}

// --- Reading the settings -------------------------------------------------

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn flag(value: &Value, key: &str, default: bool) -> bool {
    match value.get(key) {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => match text.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" => false,
            _ => default,
        },
        _ => default,
    }
}

/// A list of text, or text with commas (as an environment variable gives it).
fn list(value: &Value, key: &str) -> Vec<String> {
    match value.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the key holds anything.
fn present(value: &Value, key: &str) -> bool {
    match value.get(key) {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Null) | None => false,
        Some(Value::Array(items)) => !items.is_empty(),
        Some(_) => true,
    }
}

// The rules `auth.apps` is checked against when the app starts.

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

fn is_fingerprint(fingerprint: &str) -> bool {
    let parts: Vec<&str> = fingerprint.trim().split(':').collect();
    parts.len() == 32
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_path_pattern(pattern: &str) -> bool {
    pattern.starts_with('/')
        && pattern.len() <= 1024
        && !pattern
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
}

struct AppleApp {
    name: String,
    team_id: String,
    bundle_id: String,
    links: bool,
    credentials: bool,
}

impl AppleApp {
    fn app_id(&self) -> String {
        format!("{}.{}", self.team_id, self.bundle_id)
    }

    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !is_team_id(&self.team_id) {
            problems.push(format!(
                "auth.apps.apple.{}.team_id must be the ten-character Team ID (A-Z, 0-9)",
                self.name
            ));
        }
        if !is_bundle_id(&self.bundle_id) {
            problems.push(format!(
                "auth.apps.apple.{}.bundle_id must be a bundle identifier (A-Z, a-z, 0-9, `-` and `.`)",
                self.name
            ));
        }
        problems
    }
}

struct AndroidApp {
    name: String,
    package: String,
    fingerprints: Vec<String>,
    links: bool,
    credentials: bool,
}

impl AndroidApp {
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !is_package_name(&self.package) {
            problems.push(format!(
                "auth.apps.android.{}.package must be an Android package name (com.example.app)",
                self.name
            ));
        }
        if self.fingerprints.is_empty() || !self.fingerprints.iter().all(|f| is_fingerprint(f)) {
            problems.push(format!(
                "auth.apps.android.{}.fingerprints must list SHA-256 certificate fingerprints (32 hex bytes separated by `:`)",
                self.name
            ));
        }
        problems
    }
}

/// The names and tables under `key`.
fn tables(settings: &Lookup, key: &str) -> Vec<(String, Value)> {
    match settings(key) {
        Some(Value::Object(map)) => map.into_iter().collect(),
        _ => Vec::new(),
    }
}

fn apple_apps(settings: &Lookup) -> Vec<AppleApp> {
    tables(settings, "auth.apps.apple")
        .into_iter()
        .map(|(name, app)| AppleApp {
            name,
            team_id: text(&app, "team_id"),
            bundle_id: text(&app, "bundle_id"),
            links: flag(&app, "links", true),
            credentials: flag(&app, "credentials", true),
        })
        .collect()
}

fn android_apps(settings: &Lookup) -> Vec<AndroidApp> {
    tables(settings, "auth.apps.android")
        .into_iter()
        .map(|(name, app)| AndroidApp {
            name,
            package: text(&app, "package"),
            fingerprints: list(&app, "fingerprints"),
            links: flag(&app, "links", true),
            credentials: flag(&app, "credentials", true),
        })
        .collect()
}

// --- The checks -----------------------------------------------------------

/// Every check, in the order the report lists them.
pub fn diagnose(facts: &Facts<'_>) -> Vec<Item> {
    let mut items = vec![preview_item(facts.preview), settings_item(&facts.settings)];
    match &facts.settings {
        Ok(settings) => config_items(facts, *settings, &mut items),
        Err(_) => items.push(Item::missing(
            "settings.native",
            Group::Settings,
            "Native settings",
            "not checked: the settings did not load",
            "Fix the settings first (`navree config:check` names what is wrong), then run this again",
        )),
    }
    android_items(facts, &mut items);
    apple_items(facts, &mut items);
    items
}

fn preview_item(preview: &Preview) -> Item {
    const TITLE: &str = "Native preview in the app";
    match preview {
        Preview::On => Item::ready(
            "app.preview",
            Group::App,
            TITLE,
            "turned on whenever the app is built",
        ),
        Preview::Feature(feature) => Item::ready(
            "app.preview",
            Group::App,
            TITLE,
            format!(
                "turned on by the app's feature `{feature}` (build with `--features {feature}`; `navree native:coverage` does)"
            ),
        ),
        Preview::Off => Item::missing(
            "app.preview",
            Group::App,
            TITLE,
            "nothing in Cargo.toml turns `ui-native-preview` on, so the app answers native clients with its web pages only",
            "Add \"ui-native-preview\" to the features of the `navree` dependency, or a feature `native-preview = [\"navree/ui-native-preview\"]` and build with it",
        ),
        Preview::Unknown => Item::missing(
            "app.preview",
            Group::App,
            TITLE,
            "Cargo.toml here is not an app's manifest",
            "Run this in the app's directory",
        ),
    }
}

fn settings_item(settings: &Result<&Lookup, String>) -> Item {
    const TITLE: &str = "The app's settings load";
    match settings {
        Ok(_) => Item::ready(
            "settings.load",
            Group::Settings,
            TITLE,
            "config/, .env and APP_* variables read",
        ),
        Err(problem) => Item::missing(
            "settings.load",
            Group::Settings,
            TITLE,
            problem.clone(),
            "Fix what it names; `navree config:check` lists every problem at once",
        ),
    }
}

fn config_items(facts: &Facts<'_>, settings: &Lookup, items: &mut Vec<Item>) {
    let scope = facts.scope;
    let apple = apple_apps(settings);
    let android = android_apps(settings);
    items.push(apple_app_item(scope, &apple));
    items.push(android_app_item(scope, &android));
    items.push(paths_item(settings));
    items.push(
        Links {
            id: "links.apple",
            title: "Apple association file",
            path: APPLE_FILE,
            in_scope: scope.apple(),
            scope,
            has_apps: !apple.is_empty(),
            serves: apple_serves(&apple, settings),
            wanted: apple.iter().map(AppleApp::app_id).collect(),
            site: facts.site.map(|site| (site.url.as_str(), &site.apple)),
        }
        .item(),
    );
    items.push(
        Links {
            id: "links.android",
            title: "Android association file",
            path: ANDROID_FILE,
            in_scope: scope.android(),
            scope,
            has_apps: !android.is_empty(),
            serves: android_serves(&android, settings),
            wanted: android
                .iter()
                .flat_map(|app| {
                    std::iter::once(app.package.clone()).chain(app.fingerprints.iter().cloned())
                })
                .collect(),
            site: facts.site.map(|site| (site.url.as_str(), &site.android)),
        }
        .item(),
    );
    items.push(passkeys_item(settings, &apple, &android));
    items.push(push_apple_item(scope, settings, &apple));
    items.push(push_android_item(scope, settings, &android));
    items.push(social_item(
        "social.apple",
        "Sign in with Apple",
        "apple",
        scope.apple(),
        settings,
        &apple,
    ));
    items.push(social_item(
        "social.google",
        "Sign in with Google",
        "google",
        true,
        settings,
        &apple,
    ));
}

fn names(apps: impl IntoIterator<Item = String>) -> String {
    apps.into_iter().collect::<Vec<_>>().join(", ")
}

fn not_asked(scope: Scope) -> String {
    format!("not asked for (--platform {})", scope.flag())
}

fn apple_app_item(scope: Scope, apps: &[AppleApp]) -> Item {
    const TITLE: &str = "Apple app in auth.apps.apple";
    if !scope.apple() {
        return Item::not_applicable("auth.apple", Group::Settings, TITLE, not_asked(scope));
    }
    if apps.is_empty() {
        return Item::missing(
            "auth.apple",
            Group::Settings,
            TITLE,
            "no Apple app is named; links, passkeys and Sign in with Apple need its Team ID and bundle id",
            "Add `[auth.apps.apple.main]` with `team_id = \"ABCDE12345\"` and `bundle_id = \"com.example.app\"` to config/app.toml",
        );
    }
    let problems: Vec<String> = apps.iter().flat_map(AppleApp::problems).collect();
    if problems.is_empty() {
        let shown = names(
            apps.iter()
                .map(|app| format!("{} ({} on team {})", app.name, app.bundle_id, app.team_id)),
        );
        return Item::ready("auth.apple", Group::Settings, TITLE, shown);
    }
    Item::missing(
        "auth.apple",
        Group::Settings,
        TITLE,
        problems.join("; "),
        "Correct the value named in config/app.toml",
    )
}

fn android_app_item(scope: Scope, apps: &[AndroidApp]) -> Item {
    const TITLE: &str = "Android app in auth.apps.android";
    if !scope.android() {
        return Item::not_applicable("auth.android", Group::Settings, TITLE, not_asked(scope));
    }
    if apps.is_empty() {
        return Item::missing(
            "auth.android",
            Group::Settings,
            TITLE,
            "no Android app is named; links and passkeys need its package and the fingerprint of its signing certificate",
            "Add `[auth.apps.android.main]` with `package = \"com.example.app\"` and `fingerprints = [\"14:6D:...\"]` (the Play App Signing SHA-256) to config/app.toml",
        );
    }
    let problems: Vec<String> = apps.iter().flat_map(AndroidApp::problems).collect();
    if problems.is_empty() {
        let shown = names(apps.iter().map(|app| {
            format!(
                "{} ({}, {} fingerprint{})",
                app.name,
                app.package,
                app.fingerprints.len(),
                if app.fingerprints.len() == 1 { "" } else { "s" }
            )
        }));
        return Item::ready("auth.android", Group::Settings, TITLE, shown);
    }
    Item::missing(
        "auth.android",
        Group::Settings,
        TITLE,
        problems.join("; "),
        "Correct the value named in config/app.toml",
    )
}

fn paths_of(settings: &Lookup) -> (Vec<String>, Vec<String>) {
    let apps = settings("auth.apps").unwrap_or(Value::Null);
    (list(&apps, "paths"), list(&apps, "exclude"))
}

fn paths_item(settings: &Lookup) -> Item {
    const TITLE: &str = "Link paths in auth.apps";
    let (paths, exclude) = paths_of(settings);
    if paths.is_empty() && exclude.is_empty() {
        return Item::not_applicable(
            "auth.paths",
            Group::Settings,
            TITLE,
            "no paths: the apps claim no links of the site (passkeys and passwords still work)",
        );
    }
    let bad: Vec<&String> = paths
        .iter()
        .chain(&exclude)
        .filter(|path| !is_path_pattern(path))
        .collect();
    if bad.is_empty() {
        return Item::ready(
            "auth.paths",
            Group::Settings,
            TITLE,
            format!(
                "{} path{} open in the apps, {} excluded",
                paths.len(),
                if paths.len() == 1 { "" } else { "s" },
                exclude.len()
            ),
        );
    }
    Item::missing(
        "auth.paths",
        Group::Settings,
        TITLE,
        format!(
            "not paths: {}",
            bad.iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "Write each as a path that starts with `/` and has no spaces, such as `/orders/*`",
    )
}

fn apple_serves(apps: &[AppleApp], settings: &Lookup) -> bool {
    let (paths, _) = paths_of(settings);
    apps.iter()
        .any(|app| app.credentials || (app.links && !paths.is_empty()))
}

fn android_serves(apps: &[AndroidApp], settings: &Lookup) -> bool {
    let (paths, _) = paths_of(settings);
    apps.iter()
        .any(|app| app.credentials || (app.links && !paths.is_empty()))
}

/// Every string anywhere in `body`, when it is JSON.
fn strings_of(body: &str) -> Option<Vec<String>> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) => out.push(text.clone()),
            Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            Value::Object(map) => map.values().for_each(|item| walk(item, out)),
            _ => {}
        }
    }
    let value: Value = serde_json::from_str(body).ok()?;
    let mut out = Vec::new();
    walk(&value, &mut out);
    Some(out)
}

/// Whether the JSON `body` has every one of `wanted` as a string in it, and
/// which it lacks when not.
fn names_all(body: &str, wanted: &[String]) -> Result<(), Vec<String>> {
    let Some(have) = strings_of(body) else {
        return Err(vec!["a JSON document".to_owned()]);
    };
    let lacking: Vec<String> = wanted
        .iter()
        .filter(|want| !have.iter().any(|h| h.eq_ignore_ascii_case(want)))
        .cloned()
        .collect();
    if lacking.is_empty() {
        Ok(())
    } else {
        Err(lacking)
    }
}

/// One association file: whether the settings make the app serve it, and what
/// a running site answered for it.
struct Links<'a> {
    id: &'static str,
    title: &'static str,
    path: &'static str,
    in_scope: bool,
    scope: Scope,
    has_apps: bool,
    serves: bool,
    /// What the file must name: the app ids, or the packages and fingerprints.
    wanted: Vec<String>,
    site: Option<(&'a str, &'a Result<String, String>)>,
}

impl Links<'_> {
    fn item(self) -> Item {
        let Self {
            id, title, path, ..
        } = self;
        if !self.in_scope {
            return Item::not_applicable(id, Group::Settings, title, not_asked(self.scope));
        }
        if !self.has_apps {
            return Item::not_applicable(
                id,
                Group::Settings,
                title,
                "no app of this system is named in auth.apps",
            );
        }
        if !self.serves {
            return Item::missing(
                id,
                Group::Settings,
                title,
                format!(
                    "{path} is not served: no app has `credentials = true` or `links = true` with `auth.apps.paths` set"
                ),
                "Set `auth.apps.paths = [\"/orders/*\"]` for the links the app opens, or leave `credentials` on for its passkeys",
            );
        }
        match self.site {
            None => Item::ready(
                id,
                Group::Settings,
                title,
                format!("{path} is served by the app; add --url <site> to ask a running site"),
            ),
            Some((url, Err(problem))) => Item::missing(
                id,
                Group::Settings,
                title,
                format!("{url}{path}: {problem}"),
                format!(
                    "Serve {path} at the site's root with no redirect, as application/json; check the app is deployed with the same auth.apps"
                ),
            ),
            Some((url, Ok(body))) => match names_all(body, &self.wanted) {
                Ok(()) => Item::ready(
                    id,
                    Group::Settings,
                    title,
                    format!("{url}{path} answers and names the app"),
                ),
                Err(lacking) => Item::missing(
                    id,
                    Group::Settings,
                    title,
                    format!("{url}{path} answers but lacks {}", lacking.join(", ")),
                    "Deploy the app with the auth.apps settings of this app, then ask again",
                ),
            },
        }
    }
}

fn passkeys_item(settings: &Lookup, apple: &[AppleApp], android: &[AndroidApp]) -> Item {
    const TITLE: &str = "Passkey origins";
    let passkeys = settings("auth.passkeys").unwrap_or(Value::Null);
    if !flag(&passkeys, "enabled", false) {
        return Item::not_applicable(
            "passkeys",
            Group::Settings,
            TITLE,
            "passkeys are off (auth.passkeys.enabled); turn them on to sign in without a password",
        );
    }
    let rp_id = text(&passkeys, "rp_id");
    let origins = list(&passkeys, "origins");
    let mut problems = Vec::new();
    if rp_id.is_empty() {
        problems.push("auth.passkeys.rp_id is not set".to_owned());
    }
    if origins.is_empty() {
        problems.push("auth.passkeys.origins lists no origin".to_owned());
    }
    let apple_origin = format!("https://{rp_id}");
    if !rp_id.is_empty()
        && apple.iter().any(|app| app.credentials)
        && !origins.contains(&apple_origin)
    {
        problems.push(format!(
            "an Apple app uses passkeys, so auth.passkeys.origins needs {apple_origin}"
        ));
    }
    for app in android.iter().filter(|app| app.credentials) {
        if app.fingerprints.is_empty() {
            problems.push(format!(
                "the Android app `{}` uses passkeys but has no fingerprint, so its origin cannot be built",
                app.name
            ));
        }
    }
    if problems.is_empty() {
        let extra = android.iter().filter(|app| app.credentials).count();
        return Item::ready(
            "passkeys",
            Group::Settings,
            TITLE,
            format!(
                "relying party {rp_id}, {} origin{} listed{}",
                origins.len(),
                if origins.len() == 1 { "" } else { "s" },
                if extra > 0 {
                    format!(
                        ", plus the origin of {extra} Android app{} from its fingerprints",
                        if extra == 1 { "" } else { "s" }
                    )
                } else {
                    String::new()
                }
            ),
        );
    }
    Item::missing(
        "passkeys",
        Group::Settings,
        TITLE,
        problems.join("; "),
        "Set rp_id and origins under [auth.passkeys] in config/app.toml (the site's own origin, `https://<rp_id>` for the Apple app)",
    )
}

fn push_apple_item(scope: Scope, settings: &Lookup, apple: &[AppleApp]) -> Item {
    const TITLE: &str = "Push to Apple devices (notify.push.apns)";
    if !scope.apple() {
        return Item::not_applicable("push.apple", Group::Settings, TITLE, not_asked(scope));
    }
    let apns = settings("notify.push.apns").unwrap_or(Value::Null);
    if !present(&apns, "team_id") && !present(&apns, "key") && !present(&apns, "sandbox_key") {
        return Item::not_applicable(
            "push.apple",
            Group::Settings,
            TITLE,
            "not set up; optional: add [notify.push.apns] when the app sends notifications",
        );
    }
    let mut problems = Vec::new();
    let id10 = |value: &str| value.len() == 10 && value.chars().all(|c| c.is_ascii_alphanumeric());
    if !id10(&text(&apns, "team_id")) {
        problems.push("team_id must be the 10-character team id".to_owned());
    }
    let key = present(&apns, "key");
    let sandbox = present(&apns, "sandbox_key");
    if !key && !sandbox {
        problems.push("key (the .p8 file) is not set".to_owned());
    }
    if key && !id10(&text(&apns, "key_id")) {
        problems.push("key_id must be the 10-character id of key".to_owned());
    }
    if sandbox && !id10(&text(&apns, "sandbox_key_id")) {
        problems.push("sandbox_key_id must be the 10-character id of sandbox_key".to_owned());
    }
    let bundle_ids = list(&apns, "bundle_ids");
    if bundle_ids.is_empty() {
        problems.push("bundle_ids lists no app".to_owned());
    }
    for app in apple {
        if !bundle_ids.contains(&app.bundle_id) {
            problems.push(format!(
                "the app {} (auth.apps.apple.{}) is not in bundle_ids, so its devices cannot register",
                app.bundle_id, app.name
            ));
        }
    }
    if problems.is_empty() {
        return Item::ready(
            "push.apple",
            Group::Settings,
            TITLE,
            format!(
                "key set (not shown){}; apps: {}",
                if sandbox {
                    ", sandbox key set (not shown)"
                } else {
                    ""
                },
                bundle_ids.join(", ")
            ),
        );
    }
    Item::missing(
        "push.apple",
        Group::Settings,
        TITLE,
        format!("notify.push.apns: {}", problems.join("; ")),
        "Fix [notify.push.apns]; keep the key in .env as APP_NOTIFY__PUSH__APNS__KEY",
    )
}

fn push_android_item(scope: Scope, settings: &Lookup, android: &[AndroidApp]) -> Item {
    const TITLE: &str = "Push to Android devices (notify.push.fcm)";
    if !scope.android() {
        return Item::not_applicable("push.android", Group::Settings, TITLE, not_asked(scope));
    }
    let fcm = settings("notify.push.fcm").unwrap_or(Value::Null);
    if !present(&fcm, "service_account") && !present(&fcm, "packages") {
        return Item::not_applicable(
            "push.android",
            Group::Settings,
            TITLE,
            "not set up; optional: add [notify.push.fcm] when the app sends notifications",
        );
    }
    let mut problems = Vec::new();
    match fcm.get("service_account") {
        Some(Value::String(account)) if !account.trim().is_empty() => {
            match serde_json::from_str::<Value>(account) {
                Ok(parsed)
                    if parsed.get("private_key").is_some()
                        && parsed.get("client_email").is_some() => {}
                _ => problems
                    .push("service_account is not the service account's JSON key file".to_owned()),
            }
        }
        // A table written out in the settings is the key file too.
        Some(Value::Object(account)) if account.contains_key("private_key") => {}
        _ => problems.push("service_account (the JSON key file) is not set".to_owned()),
    }
    let packages = list(&fcm, "packages");
    if packages.is_empty() {
        problems.push("packages lists no app".to_owned());
    }
    for app in android {
        if !packages.contains(&app.package) {
            problems.push(format!(
                "the app {} (auth.apps.android.{}) is not in packages, so its devices cannot register",
                app.package, app.name
            ));
        }
    }
    if problems.is_empty() {
        return Item::ready(
            "push.android",
            Group::Settings,
            TITLE,
            format!(
                "service account set (not shown); apps: {}",
                packages.join(", ")
            ),
        );
    }
    Item::missing(
        "push.android",
        Group::Settings,
        TITLE,
        format!("notify.push.fcm: {}", problems.join("; ")),
        "Fix [notify.push.fcm]; keep the key file in .env as APP_NOTIFY__PUSH__FCM__SERVICE_ACCOUNT",
    )
}

fn social_item(
    id: &'static str,
    title: &str,
    kind: &str,
    in_scope: bool,
    settings: &Lookup,
    apple: &[AppleApp],
) -> Item {
    if !in_scope {
        return Item::not_applicable(
            id,
            Group::Settings,
            title,
            "not asked for (--platform android)",
        );
    }
    let social = settings("social").unwrap_or(Value::Null);
    let provider = tables(settings, "social.providers")
        .into_iter()
        .find(|(key, provider)| {
            let own = text(provider, "type");
            if own.is_empty() {
                key == kind
            } else {
                own == kind
            }
        });
    let Some((key, provider)) = provider else {
        return Item::not_applicable(
            id,
            Group::Settings,
            title,
            format!(
                "no `{kind}` provider in social.providers; optional: add one when the app offers {title}"
            ),
        );
    };
    let mut problems = Vec::new();
    if !flag(&social, "routes", true) {
        problems.push("social.routes is off, so the sign-in routes are not served".to_owned());
    }
    if !present(&provider, "client_id") {
        problems.push("client_id is not set".to_owned());
    }
    if kind == "apple" {
        if apple.is_empty() {
            problems.push(
                "the identity token is checked against the bundle ids in auth.apps.apple, and none is named"
                    .to_owned(),
            );
        }
        for field in ["team_id", "key_id", "private_key"] {
            if !present(&provider, field) {
                problems.push(format!(
                    "{field} is not set (Navree needs it to exchange Apple's authorization code and to revoke tokens)"
                ));
            }
        }
    }
    if problems.is_empty() {
        return Item::ready(
            id,
            Group::Settings,
            title,
            format!(
                "provider `{key}` is set up{}",
                if kind == "apple" {
                    " (private key set, not shown)"
                } else {
                    ""
                }
            ),
        );
    }
    Item::missing(
        id,
        Group::Settings,
        title,
        format!("social.providers.{key}: {}", problems.join("; ")),
        format!("Fix [social.providers.{key}] in config/app.toml; keep secrets in .env"),
    )
}

// --- The toolchains -------------------------------------------------------

/// The major version in `java -version`'s first quoted string.
pub fn java_major(output: &str) -> Option<u32> {
    let start = output.find('"')? + 1;
    let version = &output[start..start + output[start..].find('"')?];
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// The version in `xcodebuild -version`'s `Xcode 26.1` line.
pub fn xcode_version(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Xcode "))
        .map(|version| version.trim().to_owned())
}

fn jdk_item(machine: &dyn Machine) -> Item {
    const TITLE: &str = "JDK";
    let java_name = if machine.os() == Os::Windows {
        "java.exe"
    } else {
        "java"
    };
    let mut note = String::new();
    let mut found: Option<(String, String)> = None;
    if let Some(home) = machine.var("JAVA_HOME") {
        let java = Path::new(&home).join("bin").join(java_name);
        if machine.is_file(&java) {
            if let Some(output) = machine.run(&java.to_string_lossy(), &["-version"]) {
                found = Some((format!("JAVA_HOME ({home})"), output));
            }
        } else {
            note = format!("JAVA_HOME is {home}, which has no bin/{java_name}; ");
        }
    }
    if found.is_none()
        && let Some(output) = machine.run("java", &["-version"])
        && output.contains("version")
    {
        found = Some(("java on PATH".to_owned(), output));
    }
    let Some((from, output)) = found else {
        return Item::missing(
            "android.jdk",
            Group::Android,
            TITLE,
            format!("{note}no JDK found"),
            format!(
                "Install a JDK {MIN_JDK} or newer (Temurin or the one in Android Studio) and set JAVA_HOME to it"
            ),
        );
    };
    match java_major(&output) {
        Some(major) if major >= MIN_JDK => Item::ready(
            "android.jdk",
            Group::Android,
            TITLE,
            format!("JDK {major}, from {from}"),
        ),
        Some(major) => Item::missing(
            "android.jdk",
            Group::Android,
            TITLE,
            format!("JDK {major} from {from} is older than the {MIN_JDK} the Android build needs"),
            format!("Install a JDK {MIN_JDK} or newer and set JAVA_HOME to it"),
        ),
        None => Item::missing(
            "android.jdk",
            Group::Android,
            TITLE,
            format!("the version of the JDK from {from} could not be read"),
            format!("Install a JDK {MIN_JDK} or newer and set JAVA_HOME to it"),
        ),
    }
}

/// The Android SDK directory and how it was found.
fn sdk_dir(machine: &dyn Machine) -> (Option<(PathBuf, String)>, Vec<String>) {
    let mut tried = Vec::new();
    for name in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(value) = machine.var(name) {
            let dir = PathBuf::from(&value);
            if machine.is_dir(&dir) {
                return (Some((dir, name.to_owned())), tried);
            }
            tried.push(format!("{name} is {value}, which is not a directory"));
        }
    }
    let home = machine.home();
    let usual: Vec<PathBuf> = match machine.os() {
        Os::Windows => machine
            .var("LOCALAPPDATA")
            .map(|local| PathBuf::from(local).join("Android").join("Sdk"))
            .into_iter()
            .collect(),
        Os::Mac => home
            .map(|h| h.join("Library").join("Android").join("sdk"))
            .into_iter()
            .collect(),
        _ => home
            .map(|h| vec![h.join("Android").join("Sdk"), h.join("Android").join("sdk")])
            .unwrap_or_default(),
    };
    for dir in usual {
        if machine.is_dir(&dir) {
            return (Some((dir, "the usual place".to_owned())), tried);
        }
    }
    (None, tried)
}

fn sdk_items(machine: &dyn Machine, items: &mut Vec<Item>) {
    let (sdk, tried) = sdk_dir(machine);
    let Some((dir, from)) = sdk else {
        let note = if tried.is_empty() {
            String::new()
        } else {
            format!("{}; ", tried.join("; "))
        };
        items.push(Item::missing(
            "android.sdk",
            Group::Android,
            "Android SDK",
            format!("{note}no Android SDK found in ANDROID_HOME, ANDROID_SDK_ROOT or the usual place"),
            "Install Android Studio (or the command-line tools) and set ANDROID_HOME to the SDK folder",
        ));
        return;
    };
    let tools = if machine.is_dir(&dir.join("platform-tools")) {
        "platform-tools present"
    } else {
        "no platform-tools (adb) yet"
    };
    items.push(Item::ready(
        "android.sdk",
        Group::Android,
        "Android SDK",
        format!("{} (from {from}); {tools}", dir.display()),
    ));
    let platforms: Vec<u32> = machine
        .list(&dir.join("platforms"))
        .iter()
        .filter_map(|name| name.strip_prefix("android-"))
        .filter_map(|rest| rest.split('.').next()?.parse().ok())
        .collect();
    let best = platforms.iter().copied().max();
    items.push(match best {
        Some(level) if level >= MIN_ANDROID_PLATFORM => Item::ready(
            "android.platform",
            Group::Android,
            "Android platform",
            format!(
                "android-{level} installed (the client compiles against {MIN_ANDROID_PLATFORM})"
            ),
        ),
        other => Item::missing(
            "android.platform",
            Group::Android,
            "Android platform",
            match other {
                Some(level) => format!(
                    "newest installed is android-{level}, older than android-{MIN_ANDROID_PLATFORM}"
                ),
                None => "no platform installed".to_owned(),
            },
            format!("sdkmanager \"platforms;android-{MIN_ANDROID_PLATFORM}\""),
        ),
    });
    let builds = machine.list(&dir.join("build-tools"));
    items.push(if builds.is_empty() {
        Item::missing(
            "android.build-tools",
            Group::Android,
            "Android build tools",
            "no build-tools installed",
            format!("sdkmanager \"build-tools;{MIN_ANDROID_PLATFORM}.0.0\""),
        )
    } else {
        let mut builds = builds;
        builds.sort();
        Item::ready(
            "android.build-tools",
            Group::Android,
            "Android build tools",
            format!("{} installed", builds.join(", ")),
        )
    });
}

fn android_items(facts: &Facts<'_>, items: &mut Vec<Item>) {
    if !facts.scope.android() {
        for (id, title) in [
            ("android.jdk", "JDK"),
            ("android.sdk", "Android SDK"),
            ("android.platform", "Android platform"),
            ("android.build-tools", "Android build tools"),
        ] {
            items.push(Item::not_applicable(
                id,
                Group::Android,
                title,
                not_asked(facts.scope),
            ));
        }
        return;
    }
    items.push(jdk_item(facts.machine));
    sdk_items(facts.machine, items);
}

fn apple_items(facts: &Facts<'_>, items: &mut Vec<Item>) {
    const TITLE: &str = "Xcode";
    if !facts.scope.apple() {
        items.push(Item::not_applicable(
            "apple.xcode",
            Group::Apple,
            TITLE,
            not_asked(facts.scope),
        ));
        return;
    }
    if facts.machine.os() != Os::Mac {
        items.push(Item::not_applicable(
            "apple.xcode",
            Group::Apple,
            TITLE,
            format!(
                "Apple apps are built on a Mac with Xcode, or by a cloud build that runs on one; this machine runs {}, so there is nothing to install here",
                facts.machine.os().name()
            ),
        ));
        return;
    }
    let output = facts.machine.run("xcodebuild", &["-version"]);
    let version = output.as_deref().and_then(xcode_version);
    items.push(match version {
        Some(version) => {
            let major: u32 = version
                .split('.')
                .next()
                .and_then(|m| m.parse().ok())
                .unwrap_or(0);
            if major >= MIN_XCODE {
                Item::ready("apple.xcode", Group::Apple, TITLE, format!("Xcode {version}"))
            } else {
                Item::missing(
                    "apple.xcode",
                    Group::Apple,
                    TITLE,
                    format!("Xcode {version} is older than the {MIN_XCODE} the Apple clients build with"),
                    format!("Update Xcode to {MIN_XCODE} or newer from the App Store"),
                )
            }
        }
        None => Item::missing(
            "apple.xcode",
            Group::Apple,
            TITLE,
            match output {
                Some(_) => "xcodebuild did not report a version (only the command line tools may be installed)".to_owned(),
                None => "xcodebuild was not found".to_owned(),
            },
            "Install Xcode from the App Store, open it once to accept the license, then run `sudo xcode-select -s /Applications/Xcode.app`",
        ),
    });
}

// --- The report -----------------------------------------------------------

/// How many items stand each way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    /// Ready.
    pub ready: usize,
    /// Missing.
    pub missing: usize,
    /// Not applicable.
    pub not_applicable: usize,
}

/// The doctor's findings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    /// The format's name.
    pub format: &'static str,
    /// Its version.
    pub version: u32,
    /// Whether nothing is missing.
    pub ready: bool,
    /// The items by status.
    pub totals: Totals,
    /// Every item.
    pub items: Vec<Item>,
}

impl Report {
    /// The report of `items`.
    pub fn of(items: Vec<Item>) -> Self {
        let count = |status: Status| items.iter().filter(|i| i.status == status).count();
        let totals = Totals {
            ready: count(Status::Ready),
            missing: count(Status::Missing),
            not_applicable: count(Status::NotApplicable),
        };
        Self {
            format: "navree-native-doctor",
            version: 1,
            ready: totals.missing == 0,
            totals,
            items,
        }
    }

    /// The report as text for a person.
    pub fn render(&self, app: &str) -> String {
        let mut out = format!("Native build readiness of {app}\n");
        for group in [Group::App, Group::Settings, Group::Android, Group::Apple] {
            let items: Vec<&Item> = self.items.iter().filter(|i| i.group == group).collect();
            if items.is_empty() {
                continue;
            }
            out.push_str(&format!("\n{}\n", group.title()));
            for item in items {
                let status = match item.status {
                    Status::Ready => "ready",
                    Status::Missing => "MISSING",
                    Status::NotApplicable => "n/a",
                };
                out.push_str(&format!("  {status:<8}{}: {}\n", item.title, item.detail));
                if let Some(next) = &item.next {
                    out.push_str(&format!("          next: {next}\n"));
                }
            }
        }
        out.push_str(&format!(
            "\n{} ready, {} missing, {} not applicable.\n",
            self.totals.ready, self.totals.missing, self.totals.not_applicable
        ));
        if self.ready {
            out.push_str("Nothing is missing.\n");
        } else {
            out.push_str(
                "Do the next step of each missing item, then run navree native:doctor again.\n",
            );
        }
        out
    }
}

/// The site's association files, asked with no redirect followed.
pub async fn fetch(url: &str) -> Site {
    let base = url.trim_end_matches('/').to_owned();
    // An error only means a process-wide provider is already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build();
    let Ok(client) = client else {
        let problem = Err("cannot build the HTTP client".to_owned());
        return Site {
            url: base,
            apple: problem.clone(),
            android: problem,
        };
    };
    async fn get(client: &reqwest::Client, url: String) -> Result<String, String> {
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|error| format!("cannot be reached ({})", error.without_url()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(if status.is_redirection() {
                format!(
                    "answered {status}: the file must be served at its own address, with no redirect"
                )
            } else {
                format!("answered {status}")
            });
        }
        let json = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("json"));
        let body = response
            .text()
            .await
            .map_err(|error| format!("could not be read ({})", error.without_url()))?;
        if !json {
            return Err("answered, but not as application/json".to_owned());
        }
        Ok(body)
    }
    let apple = get(&client, format!("{base}{APPLE_FILE}")).await;
    let android = get(&client, format!("{base}{ANDROID_FILE}")).await;
    Site {
        url: base,
        apple,
        android,
    }
}

/// What `navree native:doctor` was asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorOptions {
    /// The systems to check.
    pub scope: Scope,
    /// A running site to ask for the association files.
    pub url: Option<String>,
    /// Print JSON instead of text.
    pub json: bool,
}

/// The app's settings by dotted key, as the app reads them, or why they do not
/// load.
fn load_settings(root: &Path) -> Result<Box<Lookup>, String> {
    let project = navree_navigator::Project::find(root).map_err(|error| error.to_string())?;
    let config = project
        .config()
        .map_err(|error| format!("the settings do not load: {error}"))?;
    Ok(Box::new(move |key| config.section::<Value>(key).ok()))
}

/// Runs `navree native:doctor` for the app in `root`; whether nothing is
/// missing.
pub fn run_doctor(
    root: &Path,
    options: &DoctorOptions,
    machine: &dyn Machine,
    out: &mut dyn std::io::Write,
) -> anyhow::Result<bool> {
    let site = match &options.url {
        Some(url) => {
            anyhow::ensure!(
                url.starts_with("http://") || url.starts_with("https://"),
                "--url takes the site's address, such as https://orders.example.com, not `{url}`"
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            Some(runtime.block_on(fetch(url)))
        }
        None => None,
    };
    let preview = crate::native::preview(root);
    let loaded = load_settings(root);
    let facts = Facts {
        machine,
        preview: &preview,
        settings: match &loaded {
            Ok(settings) => Ok(settings.as_ref()),
            Err(problem) => Err(problem.clone()),
        },
        scope: options.scope,
        site: site.as_ref(),
    };
    let report = Report::of(diagnose(&facts));
    if options.json {
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        let app = crate::project::package_name(root).unwrap_or_else(|| "the app".to_owned());
        write!(out, "{}", report.render(&app))?;
    }
    Ok(report.ready)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use serde_json::json;

    use super::*;

    /// A machine described in the test.
    #[derive(Default)]
    struct Fake {
        os: Option<Os>,
        vars: BTreeMap<&'static str, String>,
        home: Option<PathBuf>,
        dirs: BTreeSet<String>,
        files: BTreeSet<String>,
        listings: BTreeMap<String, Vec<String>>,
        programs: BTreeMap<String, String>,
    }

    /// A path with `/` separators, so a test reads the same on every system.
    fn slashed(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    impl Fake {
        fn linux() -> Self {
            Self {
                os: Some(Os::Linux),
                home: Some(PathBuf::from("/home/anu")),
                ..Self::default()
            }
        }

        fn var(mut self, name: &'static str, value: &str) -> Self {
            self.vars.insert(name, value.to_owned());
            self
        }

        fn dir(mut self, path: &str) -> Self {
            self.dirs.insert(slashed(Path::new(path)));
            self
        }

        fn file(mut self, path: &str) -> Self {
            self.files.insert(slashed(Path::new(path)));
            self
        }

        fn listing(mut self, path: &str, names: &[&str]) -> Self {
            self.dirs.insert(slashed(Path::new(path)));
            self.listings.insert(
                slashed(Path::new(path)),
                names.iter().map(|n| (*n).to_owned()).collect(),
            );
            self
        }

        fn program(mut self, program: &str, output: &str) -> Self {
            self.programs
                .insert(slashed(Path::new(program)), output.to_owned());
            self
        }
    }

    impl Machine for Fake {
        fn os(&self) -> Os {
            self.os.unwrap_or(Os::Linux)
        }

        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }

        fn home(&self) -> Option<PathBuf> {
            self.home.clone()
        }

        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.contains(&slashed(path))
        }

        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(&slashed(path))
        }

        fn list(&self, path: &Path) -> Vec<String> {
            self.listings
                .get(&slashed(path))
                .cloned()
                .unwrap_or_default()
        }

        fn run(&self, program: &str, _args: &[&str]) -> Option<String> {
            self.programs.get(&slashed(Path::new(program))).cloned()
        }
    }

    const JAVA_17: &str =
        "openjdk version \"17.0.9\" 2023-10-17\nOpenJDK Runtime Environment (build 17.0.9+9)\n";
    const FINGERPRINT: &str = "14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5";

    fn settings_of(value: Value) -> impl Fn(&str) -> Option<Value> {
        move |key: &str| {
            let mut at = &value;
            for part in key.split('.') {
                at = at.get(part)?;
            }
            Some(at.clone())
        }
    }

    fn full_settings() -> Value {
        json!({
            "auth": {
                "apps": {
                    "paths": ["/orders/*"],
                    "apple": {"main": {"team_id": "ABCDE12345", "bundle_id": "com.example.orders"}},
                    "android": {"main": {"package": "com.example.orders", "fingerprints": [FINGERPRINT]}}
                },
                "passkeys": {"enabled": true, "rp_id": "orders.example.com",
                             "origins": ["https://orders.example.com"]}
            },
            "notify": {"push": {
                "apns": {"team_id": "ABCDE12345", "key_id": "KEYID12345",
                         "key": "-----BEGIN PRIVATE KEY-----SECRET", "bundle_ids": ["com.example.orders"]},
                "fcm": {"service_account": "{\"client_email\":\"a@b\",\"private_key\":\"SECRET\"}",
                        "packages": ["com.example.orders"]}
            }},
            "social": {"providers": {
                "apple": {"client_id": "com.example.orders.web", "team_id": "ABCDE12345",
                          "key_id": "KEYID12345", "private_key": "SECRET"},
                "google": {"client_id": "1234.apps.googleusercontent.com", "client_secret": "SECRET"}
            }}
        })
    }

    fn ready_machine() -> Fake {
        Fake::linux()
            .var("JAVA_HOME", "/opt/jdk")
            .file("/opt/jdk/bin/java")
            .program("/opt/jdk/bin/java", JAVA_17)
            .var("ANDROID_HOME", "/opt/android")
            .dir("/opt/android")
            .dir("/opt/android/platform-tools")
            .listing("/opt/android/platforms", &["android-34", "android-36"])
            .listing("/opt/android/build-tools", &["36.0.0"])
    }

    fn run(machine: &Fake, preview: Preview, settings: Value, scope: Scope) -> Vec<Item> {
        let lookup = settings_of(settings);
        let facts = Facts {
            machine,
            preview: &preview,
            settings: Ok(&lookup),
            scope,
            site: None,
        };
        diagnose(&facts)
    }

    fn item<'a>(items: &'a [Item], id: &str) -> &'a Item {
        items.iter().find(|item| item.id == id).unwrap_or_else(|| {
            panic!(
                "no item {id}: {:?}",
                items.iter().map(|i| i.id).collect::<Vec<_>>()
            )
        })
    }

    fn status_of(items: &[Item], id: &str) -> Status {
        item(items, id).status
    }

    #[test]
    fn the_report_says_ready_missing_or_not_applicable() {
        let items = run(&ready_machine(), Preview::On, full_settings(), Scope::All);
        for id in [
            "app.preview",
            "settings.load",
            "auth.apple",
            "auth.android",
            "links.apple",
            "links.android",
            "passkeys",
            "push.apple",
            "push.android",
            "social.apple",
            "social.google",
            "android.jdk",
            "android.sdk",
            "android.platform",
            "android.build-tools",
        ] {
            assert_eq!(
                status_of(&items, id),
                Status::Ready,
                "{id}: {:?}",
                item(&items, id)
            );
        }
        // This machine is not a Mac.
        let xcode = item(&items, "apple.xcode");
        assert_eq!(xcode.status, Status::NotApplicable);
        assert!(xcode.detail.contains("built on a Mac"), "{}", xcode.detail);
        assert!(xcode.detail.contains("cloud build"), "{}", xcode.detail);
        let report = Report::of(items);
        assert!(report.ready);
        assert_eq!(report.totals.missing, 0);
        let text = report.render("shop");
        assert!(text.contains("Nothing is missing."), "{text}");
        assert!(
            text.contains("ready   JDK: JDK 17, from JAVA_HOME (/opt/jdk)"),
            "{text}"
        );
        assert!(text.contains("n/a     Xcode:"), "{text}");
    }

    #[test]
    fn json_lists_every_item() {
        let items = run(&Fake::linux(), Preview::Off, json!({}), Scope::All);
        let report = Report::of(items.clone());
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["format"], "navree-native-doctor");
        assert_eq!(value["ready"], false);
        assert_eq!(value["items"].as_array().unwrap().len(), items.len());
        let preview = &value["items"][0];
        assert_eq!(preview["id"], "app.preview");
        assert_eq!(preview["status"], "missing");
        assert_eq!(preview["group"], "app");
        assert!(
            preview["next"]
                .as_str()
                .unwrap()
                .contains("ui-native-preview")
        );
        // A ready item has no next step.
        let ready = value["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["status"] == "not-applicable")
            .unwrap();
        assert!(ready.get("next").is_none());
        assert!(value["totals"]["missing"].as_u64().unwrap() > 0);
    }

    #[test]
    fn the_feature_off_names_the_line_to_add() {
        let off = preview_item(&Preview::Off);
        assert_eq!(off.status, Status::Missing);
        assert!(off.next.unwrap().contains("navree/ui-native-preview"));
        let feature = preview_item(&Preview::Feature("native-preview".into()));
        assert_eq!(feature.status, Status::Ready);
        assert!(feature.detail.contains("--features native-preview"));
        assert_eq!(preview_item(&Preview::Unknown).status, Status::Missing);
    }

    #[test]
    fn a_configuration_that_does_not_load_is_missing() {
        let machine = ready_machine();
        let facts = Facts {
            machine: &machine,
            preview: &Preview::On,
            settings: Err("config/app.toml: expected `=`".to_owned()),
            scope: Scope::All,
            site: None,
        };
        let items = diagnose(&facts);
        let load = item(&items, "settings.load");
        assert_eq!(load.status, Status::Missing);
        assert!(load.detail.contains("expected `=`"));
        assert!(load.next.as_deref().unwrap().contains("config:check"));
        assert_eq!(status_of(&items, "settings.native"), Status::Missing);
        // The toolchains are still checked.
        assert_eq!(status_of(&items, "android.jdk"), Status::Ready);
    }

    #[test]
    fn apple_apps_need_a_team_and_a_bundle_id() {
        let mut settings = full_settings();
        settings["auth"]["apps"]["apple"]["main"] =
            json!({"team_id": "abc", "bundle_id": "bad id"});
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let apple = item(&items, "auth.apple");
        assert_eq!(apple.status, Status::Missing);
        assert!(
            apple.detail.contains("auth.apps.apple.main.team_id"),
            "{}",
            apple.detail
        );
        assert!(
            apple.detail.contains("auth.apps.apple.main.bundle_id"),
            "{}",
            apple.detail
        );
        let none = run(&ready_machine(), Preview::On, json!({}), Scope::All);
        let apple = item(&none, "auth.apple");
        assert_eq!(apple.status, Status::Missing);
        assert!(
            apple
                .next
                .as_deref()
                .unwrap()
                .contains("[auth.apps.apple.main]")
        );
    }

    #[test]
    fn android_apps_need_a_package_and_fingerprints() {
        let mut settings = full_settings();
        settings["auth"]["apps"]["android"]["main"] =
            json!({"package": "orders", "fingerprints": ["14:6D"]});
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let android = item(&items, "auth.android");
        assert_eq!(android.status, Status::Missing);
        assert!(
            android
                .detail
                .contains("package must be an Android package name")
        );
        assert!(android.detail.contains("fingerprints must list SHA-256"));
        // A comma-separated list, as an environment variable gives it, is read.
        let mut settings = full_settings();
        settings["auth"]["apps"]["android"]["main"]["fingerprints"] = json!(FINGERPRINT);
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        assert_eq!(status_of(&items, "auth.android"), Status::Ready);
    }

    #[test]
    fn a_secret_is_never_printed() {
        let items = run(&ready_machine(), Preview::On, full_settings(), Scope::All);
        let report = Report::of(items);
        let everything = format!(
            "{}{}",
            report.render("shop"),
            serde_json::to_string(&report).unwrap()
        );
        assert!(!everything.contains("SECRET"), "{everything}");
        assert!(!everything.contains("BEGIN PRIVATE KEY"));
        // Not even when the value is wrong.
        let mut settings = full_settings();
        settings["notify"]["push"]["apns"]["team_id"] = json!("short");
        settings["notify"]["push"]["fcm"]["service_account"] = json!("SECRET not json");
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let everything = format!("{items:?}");
        assert!(!everything.contains("SECRET"), "{everything}");
    }

    #[test]
    fn push_keys_are_checked_for_presence_only() {
        let mut settings = full_settings();
        settings["notify"]["push"]["apns"]["key"] = json!("");
        settings["notify"]["push"]["fcm"]["service_account"] = json!("");
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let apns = item(&items, "push.apple");
        assert_eq!(apns.status, Status::Missing);
        assert!(
            apns.detail.contains("key (the .p8 file) is not set"),
            "{}",
            apns.detail
        );
        let fcm = item(&items, "push.android");
        assert_eq!(fcm.status, Status::Missing);
        assert!(
            fcm.detail
                .contains("service_account (the JSON key file) is not set")
        );
        assert!(
            fcm.next
                .as_deref()
                .unwrap()
                .contains("APP_NOTIFY__PUSH__FCM__SERVICE_ACCOUNT")
        );
    }

    #[test]
    fn push_must_name_the_apps_of_auth_apps() {
        let mut settings = full_settings();
        settings["notify"]["push"]["apns"]["bundle_ids"] = json!(["com.other.app"]);
        settings["notify"]["push"]["fcm"]["packages"] = json!("com.other.app");
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        assert!(
            item(&items, "push.apple")
                .detail
                .contains("com.example.orders (auth.apps.apple.main) is not in bundle_ids")
        );
        assert!(
            item(&items, "push.android")
                .detail
                .contains("is not in packages")
        );
    }

    #[test]
    fn nothing_configured_is_not_applicable_where_optional() {
        let mut settings = full_settings();
        settings["notify"] = json!({});
        settings["social"] = json!({});
        settings["auth"]["passkeys"] = json!({});
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        for id in [
            "push.apple",
            "push.android",
            "social.apple",
            "social.google",
            "passkeys",
        ] {
            assert_eq!(status_of(&items, id), Status::NotApplicable, "{id}");
        }
        assert!(item(&items, "push.apple").detail.contains("optional"));
    }

    #[test]
    fn passkeys_need_the_apple_origin() {
        let mut settings = full_settings();
        settings["auth"]["passkeys"]["origins"] = json!(["https://app.example.com"]);
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let passkeys = item(&items, "passkeys");
        assert_eq!(passkeys.status, Status::Missing);
        assert!(
            passkeys.detail.contains("needs https://orders.example.com"),
            "{}",
            passkeys.detail
        );
        let mut settings = full_settings();
        settings["auth"]["passkeys"] = json!({"enabled": "true"});
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let passkeys = item(&items, "passkeys");
        assert!(
            passkeys.detail.contains("rp_id is not set")
                && passkeys.detail.contains("lists no origin")
        );
        // Ready: the Android app's origin comes from its fingerprint.
        let items = run(&ready_machine(), Preview::On, full_settings(), Scope::All);
        assert!(
            item(&items, "passkeys")
                .detail
                .contains("plus the origin of 1 Android app")
        );
    }

    #[test]
    fn social_sign_in_needs_the_apps_ids() {
        let mut settings = full_settings();
        settings["social"]["providers"]["apple"] = json!({"client_id": "x"});
        settings["social"]["providers"]["google"] = json!({});
        settings["social"]["routes"] = json!(false);
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let apple = item(&items, "social.apple");
        assert_eq!(apple.status, Status::Missing);
        assert!(
            apple.detail.contains("team_id is not set"),
            "{}",
            apple.detail
        );
        assert!(apple.detail.contains("private_key is not set"));
        assert!(apple.detail.contains("social.routes is off"));
        let google = item(&items, "social.google");
        assert!(google.detail.contains("client_id is not set"));
        // A provider under another key is found by its type.
        let mut settings = full_settings();
        let google = settings["social"]["providers"]["google"].take();
        settings["social"]["providers"] =
            json!({"work": {"type": "google", "client_id": google["client_id"]}});
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        assert_eq!(status_of(&items, "social.google"), Status::Ready);
    }

    #[test]
    fn a_jdk_is_found_by_java_home_then_path() {
        let by_home = jdk_item(&ready_machine());
        assert_eq!(by_home.status, Status::Ready);
        assert!(by_home.detail.contains("JAVA_HOME (/opt/jdk)"));
        let by_path =
            jdk_item(&Fake::linux().program("java", "java version \"21.0.1\" 2023-10-17 LTS\n"));
        assert_eq!(by_path.status, Status::Ready);
        assert!(
            by_path.detail.contains("JDK 21, from java on PATH"),
            "{}",
            by_path.detail
        );
        // JAVA_HOME that points nowhere is said, and PATH still counts.
        let stale = jdk_item(
            &Fake::linux()
                .var("JAVA_HOME", "/gone")
                .program("java", JAVA_17),
        );
        assert_eq!(stale.status, Status::Ready);
        let nothing = jdk_item(&Fake::linux().var("JAVA_HOME", "/gone"));
        assert_eq!(nothing.status, Status::Missing);
        assert!(
            nothing
                .detail
                .contains("JAVA_HOME is /gone, which has no bin/java")
        );
        // Windows names the program java.exe.
        let mut windows = Fake::linux()
            .var("JAVA_HOME", "C:/jdk")
            .file("C:/jdk/bin/java.exe")
            .program("C:/jdk/bin/java.exe", JAVA_17);
        windows.os = Some(Os::Windows);
        let found = jdk_item(&windows);
        assert_eq!(found.status, Status::Ready, "{}", found.detail);
    }

    #[test]
    fn an_old_jdk_is_missing_with_the_next_step() {
        let old = jdk_item(&Fake::linux().program("java", "java version \"1.8.0_391\"\n"));
        assert_eq!(old.status, Status::Missing);
        assert!(old.detail.contains("JDK 8"), "{}", old.detail);
        assert!(old.next.unwrap().contains("JDK 17 or newer"));
        assert_eq!(java_major("openjdk version \"17.0.9\" 2023"), Some(17));
        assert_eq!(java_major("openjdk version \"21\" 2023"), Some(21));
        assert_eq!(java_major("java version \"1.8.0_391\""), Some(8));
        assert_eq!(java_major("no quotes"), None);
    }

    #[test]
    fn the_sdk_is_found_by_the_usual_variables_then_the_usual_places() {
        let items = {
            let mut items = Vec::new();
            sdk_items(&ready_machine(), &mut items);
            items
        };
        assert!(
            item(&items, "android.sdk")
                .detail
                .contains("/opt/android (from ANDROID_HOME)")
        );
        // The older variable.
        let mut items = Vec::new();
        sdk_items(
            &Fake::linux().var("ANDROID_SDK_ROOT", "/sdk").dir("/sdk"),
            &mut items,
        );
        assert!(
            item(&items, "android.sdk")
                .detail
                .contains("from ANDROID_SDK_ROOT")
        );
        // The usual place on each system.
        for (os, home, dir) in [
            (Os::Linux, "/home/anu", "/home/anu/Android/Sdk"),
            (Os::Mac, "/Users/anu", "/Users/anu/Library/Android/sdk"),
        ] {
            let mut machine = Fake::linux().dir(dir);
            machine.os = Some(os);
            machine.home = Some(PathBuf::from(home));
            let mut items = Vec::new();
            sdk_items(&machine, &mut items);
            assert!(
                item(&items, "android.sdk")
                    .detail
                    .contains("the usual place"),
                "{os:?}"
            );
        }
        let mut windows = Fake::linux()
            .var("LOCALAPPDATA", "C:/Users/anu/AppData/Local")
            .dir("C:/Users/anu/AppData/Local/Android/Sdk");
        windows.os = Some(Os::Windows);
        let mut items = Vec::new();
        sdk_items(&windows, &mut items);
        assert_eq!(status_of(&items, "android.sdk"), Status::Ready);
        // Nowhere.
        let mut items = Vec::new();
        sdk_items(&Fake::linux().var("ANDROID_HOME", "/typo"), &mut items);
        let missing = item(&items, "android.sdk");
        assert_eq!(missing.status, Status::Missing);
        assert!(
            missing
                .detail
                .contains("ANDROID_HOME is /typo, which is not a directory")
        );
        assert!(missing.next.as_deref().unwrap().contains("ANDROID_HOME"));
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn a_missing_platform_names_the_sdkmanager_line() {
        let mut items = Vec::new();
        let machine = Fake::linux()
            .var("ANDROID_HOME", "/sdk")
            .dir("/sdk")
            .listing("/sdk/platforms", &["android-34", "android-35"]);
        sdk_items(&machine, &mut items);
        let platform = item(&items, "android.platform");
        assert_eq!(platform.status, Status::Missing);
        assert!(
            platform
                .detail
                .contains("android-35, older than android-36")
        );
        assert_eq!(
            platform.next.as_deref(),
            Some("sdkmanager \"platforms;android-36\"")
        );
        let tools = item(&items, "android.build-tools");
        assert_eq!(tools.status, Status::Missing);
        assert_eq!(
            tools.next.as_deref(),
            Some("sdkmanager \"build-tools;36.0.0\"")
        );
        // A newer platform with a minor number counts.
        let machine = Fake::linux()
            .var("ANDROID_HOME", "/sdk")
            .dir("/sdk")
            .listing("/sdk/platforms", &["android-36.1"]);
        let mut items = Vec::new();
        sdk_items(&machine, &mut items);
        assert_eq!(status_of(&items, "android.platform"), Status::Ready);
    }

    #[test]
    fn xcode_is_checked_on_a_mac() {
        let mut mac = Fake::linux().program("xcodebuild", "Xcode 26.1\nBuild version 17B55\n");
        mac.os = Some(Os::Mac);
        let mut items = Vec::new();
        apple_items(
            &Facts {
                machine: &mac,
                preview: &Preview::On,
                settings: Err(String::new()),
                scope: Scope::All,
                site: None,
            },
            &mut items,
        );
        assert_eq!(items[0].status, Status::Ready);
        assert_eq!(items[0].detail, "Xcode 26.1");
        let mut old = Fake::linux().program("xcodebuild", "Xcode 15.4\nBuild version 15F31d\n");
        old.os = Some(Os::Mac);
        let mut items = Vec::new();
        apple_items(
            &Facts {
                machine: &old,
                preview: &Preview::On,
                settings: Err(String::new()),
                scope: Scope::All,
                site: None,
            },
            &mut items,
        );
        assert_eq!(items[0].status, Status::Missing);
        assert!(
            items[0]
                .next
                .as_deref()
                .unwrap()
                .contains("Update Xcode to 26")
        );
        // Only the command line tools.
        let mut tools = Fake::linux().program(
            "xcodebuild",
            "xcode-select: error: tool 'xcodebuild' requires Xcode\n",
        );
        tools.os = Some(Os::Mac);
        let mut items = Vec::new();
        apple_items(
            &Facts {
                machine: &tools,
                preview: &Preview::On,
                settings: Err(String::new()),
                scope: Scope::All,
                site: None,
            },
            &mut items,
        );
        assert!(items[0].detail.contains("only the command line tools"));
        assert_eq!(
            xcode_version("Xcode 27.0 beta\nBuild version 1"),
            Some("27.0 beta".to_owned())
        );
    }

    #[test]
    fn other_systems_are_told_apple_builds_need_a_mac() {
        for os in [Os::Windows, Os::Linux] {
            let mut machine = Fake::linux();
            machine.os = Some(os);
            let mut items = Vec::new();
            apple_items(
                &Facts {
                    machine: &machine,
                    preview: &Preview::On,
                    settings: Err(String::new()),
                    scope: Scope::All,
                    site: None,
                },
                &mut items,
            );
            assert_eq!(items[0].status, Status::NotApplicable);
            assert!(
                items[0]
                    .detail
                    .contains("on a Mac with Xcode, or by a cloud build"),
                "{}",
                items[0].detail
            );
            assert!(items[0].detail.contains(os.name()));
        }
    }

    #[test]
    fn a_system_not_asked_for_is_not_applicable() {
        let items = run(&Fake::linux(), Preview::On, full_settings(), Scope::Android);
        assert_eq!(status_of(&items, "auth.apple"), Status::NotApplicable);
        assert_eq!(status_of(&items, "links.apple"), Status::NotApplicable);
        assert_eq!(status_of(&items, "push.apple"), Status::NotApplicable);
        assert_eq!(status_of(&items, "social.apple"), Status::NotApplicable);
        assert_eq!(status_of(&items, "apple.xcode"), Status::NotApplicable);
        assert!(
            item(&items, "auth.apple")
                .detail
                .contains("--platform android")
        );
        // The Android toolchain is checked: none here.
        assert_eq!(status_of(&items, "android.jdk"), Status::Missing);
        let items = run(&Fake::linux(), Preview::On, full_settings(), Scope::Ios);
        assert_eq!(status_of(&items, "android.jdk"), Status::NotApplicable);
        assert_eq!(status_of(&items, "android.sdk"), Status::NotApplicable);
        assert_eq!(status_of(&items, "auth.android"), Status::NotApplicable);
        assert_eq!(status_of(&items, "push.android"), Status::NotApplicable);
    }

    #[test]
    fn the_association_files_follow_the_settings() {
        // Credentials on (the default) is enough for both files.
        let items = run(&ready_machine(), Preview::On, full_settings(), Scope::All);
        assert!(
            item(&items, "links.apple")
                .detail
                .contains("is served by the app")
        );
        // Links without paths, and no credentials: nothing is served.
        let mut settings = full_settings();
        settings["auth"]["apps"]["paths"] = json!([]);
        settings["auth"]["apps"]["apple"]["main"]["credentials"] = json!(false);
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let apple = item(&items, "links.apple");
        assert_eq!(apple.status, Status::Missing);
        assert!(apple.next.as_deref().unwrap().contains("auth.apps.paths"));
        assert_eq!(status_of(&items, "links.android"), Status::Ready);
        // No app at all: nothing to serve.
        let items = run(&ready_machine(), Preview::On, json!({}), Scope::All);
        assert_eq!(status_of(&items, "links.apple"), Status::NotApplicable);
    }

    fn with_site(site: &Site) -> Vec<Item> {
        let machine = ready_machine();
        let lookup = settings_of(full_settings());
        let facts = Facts {
            machine: &machine,
            preview: &Preview::On,
            settings: Ok(&lookup),
            scope: Scope::All,
            site: Some(site),
        };
        diagnose(&facts)
    }

    #[test]
    fn the_association_files_are_asked_and_compared() {
        let apple = r#"{"applinks":{"details":[{"appIDs":["ABCDE12345.com.example.orders"],"components":[]}]},"webcredentials":{"apps":["ABCDE12345.com.example.orders"]}}"#;
        let android = format!(
            r#"[{{"relation":["delegate_permission/common.handle_all_urls"],"target":{{"namespace":"android_app","package_name":"com.example.orders","sha256_cert_fingerprints":["{FINGERPRINT}"]}}}}]"#
        );
        let site = Site {
            url: "https://orders.example.com".to_owned(),
            apple: Ok(apple.to_owned()),
            android: Ok(android),
        };
        let items = with_site(&site);
        assert_eq!(status_of(&items, "links.apple"), Status::Ready);
        assert!(item(&items, "links.apple").detail.contains("https://orders.example.com/.well-known/apple-app-site-association answers and names the app"));
        assert_eq!(status_of(&items, "links.android"), Status::Ready);
        // A file that names another app.
        let other = Site {
            apple: Ok(apple.replace("com.example.orders", "com.other")),
            android: Ok(
                r#"[{"target":{"package_name":"com.other","sha256_cert_fingerprints":[]}}]"#
                    .to_owned(),
            ),
            ..site.clone()
        };
        let items = with_site(&other);
        let apple = item(&items, "links.apple");
        assert_eq!(apple.status, Status::Missing);
        assert!(
            apple.detail.contains("lacks ABCDE12345.com.example.orders"),
            "{}",
            apple.detail
        );
        let android = item(&items, "links.android");
        assert!(
            android.detail.contains("com.example.orders"),
            "{}",
            android.detail
        );
        assert!(android.detail.contains(FINGERPRINT), "{}", android.detail);
    }

    #[test]
    fn a_file_that_does_not_answer_is_missing() {
        let site = Site {
            url: "https://orders.example.com".to_owned(),
            apple: Err("answered 404 Not Found".to_owned()),
            android: Ok("not json".to_owned()),
        };
        let items = with_site(&site);
        let apple = item(&items, "links.apple");
        assert_eq!(apple.status, Status::Missing);
        assert!(apple.detail.contains("answered 404"), "{}", apple.detail);
        assert!(apple.next.as_deref().unwrap().contains("no redirect"));
        let android = item(&items, "links.android");
        assert_eq!(android.status, Status::Missing);
        assert!(
            android.detail.contains("lacks a JSON document"),
            "{}",
            android.detail
        );
    }

    #[tokio::test]
    async fn the_running_site_is_asked_with_no_redirect_followed() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(APPLE_FILE))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"webcredentials":{"apps":["ABCDE12345.com.example.orders"]}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(ANDROID_FILE))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
            .mount(&server)
            .await;
        let site = fetch(&format!("{}/", server.uri())).await;
        assert_eq!(site.url, server.uri());
        assert!(site.apple.unwrap().contains("webcredentials"));
        let problem = site.android.unwrap_err();
        assert!(problem.contains("no redirect"), "{problem}");

        // A page that is not JSON is not the file.
        let html = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_string("<html>"),
            )
            .mount(&html)
            .await;
        let site = fetch(&html.uri()).await;
        assert!(site.apple.unwrap_err().contains("not as application/json"));

        // A site that is not there.
        let site = fetch("http://127.0.0.1:1").await;
        assert!(site.apple.unwrap_err().contains("cannot be reached"));
    }

    #[test]
    fn the_real_machine_answers_the_questions() {
        let machine = SystemMachine;
        assert!(machine.is_dir(Path::new(env!("CARGO_MANIFEST_DIR"))));
        assert!(machine.is_file(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")));
        assert!(
            machine
                .list(Path::new(env!("CARGO_MANIFEST_DIR")))
                .contains(&"Cargo.toml".to_owned())
        );
        assert!(machine.run("navree-no-such-program", &[]).is_none());
        let expected = if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        };
        assert_eq!(machine.os(), expected);
    }

    #[test]
    fn the_link_paths_are_checked_for_their_shape() {
        let items = run(&ready_machine(), Preview::On, full_settings(), Scope::All);
        assert_eq!(status_of(&items, "auth.paths"), Status::Ready);
        assert!(
            item(&items, "auth.paths")
                .detail
                .contains("1 path open in the apps, 0 excluded")
        );
        let mut settings = full_settings();
        settings["auth"]["apps"]["paths"] = json!(["orders/*", "/ok"]);
        settings["auth"]["apps"]["exclude"] = json!(["/a b"]);
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        let paths = item(&items, "auth.paths");
        assert_eq!(paths.status, Status::Missing);
        assert!(
            paths.detail.contains("`orders/*`, `/a b`"),
            "{}",
            paths.detail
        );
        let mut settings = full_settings();
        settings["auth"]["apps"]["paths"] = json!([]);
        let items = run(&ready_machine(), Preview::On, settings, Scope::All);
        assert_eq!(status_of(&items, "auth.paths"), Status::NotApplicable);
    }

    #[test]
    fn the_paths_of_auth_apps_are_checked_for_their_shape() {
        assert!(is_path_pattern("/orders/*"));
        assert!(!is_path_pattern("orders"));
        assert!(!is_path_pattern("/a b"));
        assert!(is_fingerprint(FINGERPRINT));
        assert!(!is_fingerprint("14:6D"));
        assert!(is_team_id("ABCDE12345") && !is_team_id("abcde12345"));
        assert!(is_bundle_id("com.example.orders") && !is_bundle_id("com..example"));
        assert!(is_package_name("com.example.orders") && !is_package_name("orders"));
    }

    fn app_with(settings: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n[features]\nnative-preview = [\"navree/ui-native-preview\"]\n[dependencies]\nnavree = \"1\"\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config").join("app.toml"), settings).unwrap();
        dir
    }

    const APP_TOML: &str = r#"
[auth.apps.apple.main]
team_id = "ABCDE12345"
bundle_id = "com.example.orders"

[auth.apps.android.main]
package = "com.example.orders"
fingerprints = ["14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5"]

[auth.passkeys]
enabled = true
rp_id = "orders.example.com"
origins = ["https://orders.example.com"]
"#;

    #[test]
    fn an_apps_settings_are_read_without_building_it() {
        let dir = app_with(APP_TOML);
        let mut out = Vec::new();
        let options = DoctorOptions {
            scope: Scope::All,
            ..DoctorOptions::default()
        };
        let ready = run_doctor(dir.path(), &options, &ready_machine(), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        // Push and sign-in are optional and not set up: not applicable, not missing.
        assert!(ready, "{text}");
        assert!(
            text.starts_with("Native build readiness of shop\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "ready   Apple app in auth.apps.apple: main (com.example.orders on team ABCDE12345)"
            ),
            "{text}"
        );
        assert!(
            text.contains("ready   Passkey origins: relying party orders.example.com"),
            "{text}"
        );
        assert!(text.contains("n/a     Push to Apple devices"), "{text}");
        assert!(
            text.contains("turned on by the app's feature `native-preview`"),
            "{text}"
        );
        assert!(text.contains("Nothing is missing."), "{text}");
    }

    #[test]
    fn something_missing_fails_the_run_and_says_the_next_step() {
        let dir = app_with(
            "[auth.apps.apple.main]\nteam_id = \"x\"\nbundle_id = \"com.example.orders\"\n",
        );
        let mut out = Vec::new();
        let ready = run_doctor(
            dir.path(),
            &DoctorOptions::default(),
            &Fake::linux(),
            &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!ready);
        assert!(
            text.contains("MISSING Apple app in auth.apps.apple: auth.apps.apple.main.team_id"),
            "{text}"
        );
        assert!(
            text.contains("          next: Correct the value named in config/app.toml"),
            "{text}"
        );
        assert!(text.contains("MISSING JDK: no JDK found"), "{text}");
        assert!(
            text.contains("Do the next step of each missing item"),
            "{text}"
        );
        // As JSON.
        let mut out = Vec::new();
        let options = DoctorOptions {
            json: true,
            ..DoctorOptions::default()
        };
        run_doctor(dir.path(), &options, &Fake::linux(), &mut out).unwrap();
        let json: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["ready"], false);
    }

    #[test]
    fn outside_an_app_the_settings_are_missing_and_the_machine_is_still_checked() {
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        let ready = run_doctor(
            dir.path(),
            &DoctorOptions::default(),
            &ready_machine(),
            &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!ready);
        assert!(text.contains("MISSING The app's settings load"), "{text}");
        assert!(text.contains("ready   JDK"), "{text}");
    }

    #[test]
    fn the_site_address_must_be_an_address() {
        let dir = app_with(APP_TOML);
        let options = DoctorOptions {
            url: Some("orders.example.com".to_owned()),
            ..DoctorOptions::default()
        };
        let error = run_doctor(dir.path(), &options, &Fake::linux(), &mut Vec::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("not `orders.example.com`"), "{error}");
    }
}

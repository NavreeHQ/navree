//! Grammar and checks for `[package.metadata.navree.plugin]`, and terminal escaping
//! for values read from a manifest or package.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

/// The namespaces the framework keeps for itself, sorted: a community plugin
/// may not use one.
pub const RESERVED_NAMESPACES: &[&str] = &[
    "ai",
    "all",
    "api",
    "app",
    "audit",
    "auth",
    "automations",
    "aux",
    "billing",
    "cache",
    "com0",
    "com1",
    "com2",
    "com3",
    "com4",
    "com5",
    "com6",
    "com7",
    "com8",
    "com9",
    "con",
    "database",
    "directory",
    "documents",
    "domains",
    "env",
    "flags",
    "fleet",
    "fonts",
    "graphql",
    "grpc",
    "hashing",
    "helm",
    "http",
    "http_client",
    "i18n",
    "integrations",
    "layouts",
    "lighthouse",
    "log",
    "logbook",
    "lpt0",
    "lpt1",
    "lpt2",
    "lpt3",
    "lpt4",
    "lpt5",
    "lpt6",
    "lpt7",
    "lpt8",
    "lpt9",
    "mail",
    "mcp",
    "media",
    "micro",
    "navree",
    "notify",
    "nul",
    "oauth",
    "observe",
    "operations",
    "organization",
    "orm",
    "plugins",
    "prn",
    "queue",
    "realtime",
    "redis",
    "roles",
    "schema",
    "search",
    "session",
    "settings",
    "sheets",
    "social",
    "sso",
    "storage",
    "subscriptions",
    "time",
    "ui",
    "users",
    "validation",
    "view",
    "views",
    "webhooks",
    "workflows",
    "workspace",
];

const KNOWN_KEYS: &[&str] = &[
    "namespace",
    "title",
    "module",
    "features",
    "config",
    "env",
    "grants",
    "policy",
];

const POLICY_KEYS: &[&str] = &["img_src", "media_src", "connect_src", "font_src", "wasm"];
const POLICY_DIRECTIVES: &[&str] = &["img_src", "media_src", "connect_src", "font_src"];

const MODULE_GRAMMAR: &str = "`module` must be a path to the module, with an optional `::<Type, ..>` and an optional `()` at the end, and nothing else (no spaces)";

/// The plugin table of a package's `Cargo.toml`, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// The plugin's namespace (`announcements`).
    pub namespace: String,
    /// Optional display title (`Announcements`).
    pub title: Option<String>,
    /// Module path or constructor call (`AnnouncementsModule::from_config()`).
    pub module: ModulePath,
    /// Facade features of `navree` the plugin asks for, in table order.
    pub features: Vec<String>,
    /// Configuration section keys the plugin publishes, in table order.
    pub config: Vec<String>,
    /// Environment variable names the plugin reads, in table order.
    pub env: Vec<String>,
    /// Suggested permissions by role, in the table's order of permissions.
    pub grants: BTreeMap<String, Vec<String>>,
    /// Empty when the table has no `policy`.
    pub policy: PolicyAsk,
}

/// The page sources a plugin asks for, in the table's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyAsk {
    /// `(key, source)` pairs (`("img_src", "https://tile.openstreetmap.org")`).
    pub sources: Vec<(String, String)>,
    /// Whether the plugin asks for WebAssembly (`wasm = true`).
    pub wasm: bool,
}

impl PolicyAsk {
    /// Whether no source and no WebAssembly switch was requested.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty() && !self.wasm
    }
}

/// A `module` value that passed the grammar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModulePath(String);

impl ModulePath {
    /// The value, or the `module` row's reason.
    pub fn parse(text: &str) -> Result<ModulePath, String> {
        if text.len() > 200 {
            return Err("`module` must be at most 200 characters".to_owned());
        }
        let rest = text.strip_suffix("()").unwrap_or(text);
        let mut parser = PathParser {
            text: rest,
            bytes: rest.as_bytes(),
            pos: 0,
            idents: Vec::new(),
        };
        if parser.parse_path().is_none() || parser.pos != parser.bytes.len() {
            return Err(MODULE_GRAMMAR.to_owned());
        }
        for word in parser.idents {
            if crate::names::KEYWORDS.contains(&word) || word == "Self" {
                return Err(format!("`module` uses `{word}`, which is a Rust keyword"));
            }
        }
        Ok(ModulePath(text.to_owned()))
    }

    /// The validated `module` expression as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The module type (`AnnouncementsModule` in `AnnouncementsModule::from_config()`).
    pub fn type_name(&self) -> &str {
        let (path, has_parens) = match self.0.strip_suffix("()") {
            Some(path) => (path, true),
            None => (self.0.as_str(), false),
        };
        let bytes = path.as_bytes();
        let mut segments = Vec::new();
        let mut start = 0;
        let mut index = 0;
        let mut in_turbofish = false;
        while index < bytes.len() {
            if in_turbofish {
                if bytes[index] == b'>' {
                    in_turbofish = false;
                }
                index += 1;
            } else if bytes[index..].starts_with(b"::<") {
                in_turbofish = true;
                index += 3;
            } else if bytes[index..].starts_with(b"::") {
                segments.push(&path[start..index]);
                index += 2;
                start = index;
            } else {
                index += 1;
            }
        }
        segments.push(&path[start..]);
        let chosen = if has_parens && segments.len() >= 2 {
            segments[segments.len() - 2]
        } else {
            segments[segments.len() - 1]
        };
        chosen.split_once("::<").map_or(chosen, |(ident, _)| ident)
    }
}

impl fmt::Display for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

struct PathParser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    idents: Vec<&'a str>,
}

impl<'a> PathParser<'a> {
    fn eat(&mut self, lit: &str) -> bool {
        if self.bytes[self.pos..].starts_with(lit.as_bytes()) {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }

    fn parse_ident(&mut self) -> Option<&'a str> {
        let first = *self.bytes.get(self.pos)?;
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return None;
        }
        let start = self.pos;
        self.pos += 1;
        while let Some(&byte) = self.bytes.get(self.pos) {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let ident = &self.text[start..self.pos];
        if ident == "_" {
            return None;
        }
        self.idents.push(ident);
        Some(ident)
    }

    fn parse_type_path(&mut self) -> Option<()> {
        self.parse_ident()?;
        while self.eat("::") {
            self.parse_ident()?;
        }
        Some(())
    }

    fn parse_type_list(&mut self) -> Option<()> {
        self.parse_type_path()?;
        while self.eat(",") {
            self.parse_type_path()?;
        }
        Some(())
    }

    fn parse_segment(&mut self) -> Option<()> {
        self.parse_ident()?;
        if self.eat("::<") {
            self.parse_type_list()?;
            if !self.eat(">") {
                return None;
            }
        }
        Some(())
    }

    fn parse_path(&mut self) -> Option<()> {
        self.parse_segment()?;
        while self.eat("::") {
            self.parse_segment()?;
        }
        Some(())
    }
}

/// What the check needs to know about the app and the package.
#[derive(Clone, Debug, Default)]
pub struct AppFacts {
    /// Whether the package is a first-party entry of the index.
    pub first_party: bool,
    /// The resolved `navree` version (`1.4.0`).
    pub navree_version: String,
    /// Every feature name declared on the app's `navree` crate.
    pub navree_features: BTreeSet<String>,
    /// Facade features a plugin manifest may turn on.
    pub allowed_features: BTreeSet<String>,
    /// Namespaces kept by the framework.
    pub reserved: BTreeSet<String>,
    /// Additional workspace roles configured by this app.
    pub roles: BTreeSet<String>,
    /// The configured `workspace.demo.guest_role`, if any.
    pub demo_guest_role: Option<String>,
    /// Permissions already granted in the app's role lists.
    pub granted: Vec<Granted>,
}

/// A permission a role list of the app holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Granted {
    /// The permission string (`reports.view` or `reports.*`).
    pub permission: String,
    /// Where the permission was set (`config/workspace.toml`, `APP_WORKSPACE__...`).
    pub source: String,
    /// Package named by a trailing `# plugin:add PACKAGE` mark, if any.
    pub marked_for: Option<String>,
}

/// Checks the plugin table against `app`; the `Err` is the refusal reason.
pub fn read(package: &str, table: &serde_json::Value, app: &AppFacts) -> Result<Manifest, String> {
    let Some(obj) = table.as_object() else {
        return Err("must be a table".to_owned());
    };

    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    for key in keys {
        if !KNOWN_KEYS.contains(&key) {
            return Err(format!(
                "unknown key `{}` (known: namespace, title, module, features, config, env, grants, policy; a newer plugin may need a newer navree)",
                shown(key)
            ));
        }
    }

    let namespace = match obj.get("namespace") {
        Some(value) => {
            let Some(raw) = value.as_str() else {
                return Err("`namespace` must be a string".to_owned());
            };
            raw.to_owned()
        }
        None => default_namespace(package),
    };
    if !is_namespace(&namespace) {
        return Err(
            "`namespace` must be a lower-case letter followed by up to 63 lower-case letters, digits or _"
                .to_owned(),
        );
    }
    if !app.first_party && app.reserved.contains(&namespace) {
        return Err(format!(
            "`namespace`: `{}` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")",
            shown(&namespace)
        ));
    }

    let title = match obj.get("title") {
        Some(value) => {
            let Some(raw) = value.as_str() else {
                return Err("`title` must be a string".to_owned());
            };
            let count = raw.chars().count();
            if !(1..=64).contains(&count) || raw.chars().any(char::is_control) {
                return Err(
                    "`title` must be 1 to 64 characters, with no control characters".to_owned(),
                );
            }
            Some(raw.to_owned())
        }
        None => None,
    };

    let module = match obj.get("module") {
        Some(value) => {
            let Some(raw) = value.as_str() else {
                return Err("`module` must be a string".to_owned());
            };
            ModulePath::parse(raw)?
        }
        None => return Err("`module` is required".to_owned()),
    };

    let features = match obj.get("features") {
        Some(value) => {
            let list = string_list(value, "features")?;
            if list.len() > 32 {
                return Err("`features` may list at most 32 features".to_owned());
            }
            let mut checked = Vec::with_capacity(list.len());
            for feature in list {
                if !is_feature_name(feature) {
                    return Err(format!(
                        "`features`: `{}` must be lower-case letters, digits, `-`, `_` or `.`",
                        shown(feature)
                    ));
                }
                if !app.navree_features.contains(feature) {
                    return Err(format!(
                        "`features`: navree {} has no feature `{}`",
                        shown(&app.navree_version),
                        shown(feature)
                    ));
                }
                if !app.allowed_features.contains(feature) {
                    return Err(format!(
                        "`features`: `{}` is the app owner's choice; turn it on yourself if you want it",
                        shown(feature)
                    ));
                }
                checked.push(feature.to_owned());
            }
            checked
        }
        None => Vec::new(),
    };

    let config = match obj.get("config") {
        Some(value) => {
            let list = string_list(value, "config")?;
            if list.len() > 16 {
                return Err("`config` may list at most 16 sections".to_owned());
            }
            let ns_dot = format!("{namespace}.");
            let mut checked = Vec::with_capacity(list.len());
            for section in list {
                if !section.split('.').all(is_namespace) {
                    return Err(format!(
                        "`config`: `{}` must be lower-case words of letters, digits and `_`, joined by `.` (`announcements`, `announcements.digest`)",
                        shown(section)
                    ));
                }
                if section != namespace && !section.starts_with(&ns_dot) {
                    return Err(format!(
                        "`config`: `{}` is outside the plugin's namespace `{}`; a plugin publishes its own sections only",
                        shown(section),
                        shown(&namespace)
                    ));
                }
                checked.push(section.to_owned());
            }
            checked
        }
        None => Vec::new(),
    };

    let env = match obj.get("env") {
        Some(value) => {
            let list = string_list(value, "env")?;
            if list.len() > 32 {
                return Err("`env` may list at most 32 names".to_owned());
            }
            let prefix = format!("APP_{}__", namespace.to_ascii_uppercase());
            let mut checked = Vec::with_capacity(list.len());
            for name in list {
                if !is_env_shape(name) {
                    return Err(format!(
                        "`env`: `{}` must be upper-case letters, digits and `_`, starting with a letter",
                        shown(name)
                    ));
                }
                let Some(suffix) = name.strip_prefix(&prefix).filter(|rest| !rest.is_empty())
                else {
                    return Err(format!(
                        "`env`: `{}` must start with {prefix}, the plugin's own settings",
                        shown(name)
                    ));
                };
                if suffix.len() > 120 {
                    return Err(format!(
                        "`env`: `{}` must be upper-case letters, digits and `_`, starting with a letter",
                        shown(name)
                    ));
                }
                checked.push(name.to_owned());
            }
            checked
        }
        None => Vec::new(),
    };

    let has_grants = obj.contains_key("grants");
    let grants = match obj.get("grants") {
        Some(value) => {
            let Some(grants_obj) = value.as_object() else {
                return Err("`grants` must be a table of lists of strings".to_owned());
            };
            let mut parsed: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for (role, role_value) in grants_obj {
                let Some(arr) = role_value.as_array() else {
                    return Err("`grants` must be a table of lists of strings".to_owned());
                };
                let mut perms = Vec::with_capacity(arr.len());
                for item in arr {
                    let Some(perm) = item.as_str() else {
                        return Err("`grants` must be a table of lists of strings".to_owned());
                    };
                    perms.push(perm);
                }
                parsed.insert(role.as_str(), perms);
            }
            if parsed.len() > 8 {
                return Err("`grants` may name at most 8 roles".to_owned());
            }
            let ns_dot = format!("{namespace}.");
            let mut checked = BTreeMap::new();
            for (role, perms) in parsed {
                if role == "owner" || app.demo_guest_role.as_deref() == Some(role) {
                    return Err(format!(
                        "`grants`: the role `{}` is the owner or the demo guest role; a plugin suggests no grants to it",
                        shown(role)
                    ));
                }
                if !matches!(role, "admin" | "member" | "viewer") && !app.roles.contains(role) {
                    return Err(format!(
                        "`grants`: the role `{}` is not one of admin, member, viewer or a role of this app",
                        shown(role)
                    ));
                }
                if perms.len() > 32 {
                    return Err(format!(
                        "`grants.{}` may list at most 32 permissions",
                        shown(role)
                    ));
                }
                let mut checked_perms = Vec::with_capacity(perms.len());
                for perm in perms {
                    if perm.chars().count() > 128 {
                        return Err(format!(
                            "`grants.{}`: `{}` is longer than 128 characters",
                            shown(role),
                            shown(perm)
                        ));
                    }
                    if !is_grant_shape(perm) {
                        return Err(format!(
                            "`grants.{}`: `{}` must be lower-case words of letters, digits or `_` joined by `.`, or end in `.*`",
                            shown(role),
                            shown(perm)
                        ));
                    }
                    if !perm.starts_with(&ns_dot) {
                        return Err(format!(
                            "`grants.{}`: `{}` is outside the plugin's namespace `{}`; a plugin suggests grants of its own permissions only",
                            shown(role),
                            shown(perm),
                            shown(&namespace)
                        ));
                    }
                    checked_perms.push(perm.to_owned());
                }
                checked.insert(role.to_owned(), checked_perms);
            }
            checked
        }
        None => BTreeMap::new(),
    };

    let policy = match obj.get("policy") {
        Some(value) => read_policy(value)?,
        None => PolicyAsk::default(),
    };

    if !app.first_party {
        let ns_dot = format!("{namespace}.");
        if let Some(existing) = app.granted.iter().find(|entry| {
            entry.permission.starts_with(&ns_dot)
                && !entry
                    .marked_for
                    .as_deref()
                    .is_some_and(|marked| same_package(marked, package))
        }) {
            return if has_grants {
                Err(format!(
                    "`grants`: the app already grants permissions under `{ns}.` (in {source}); a plugin suggests grants only in a namespace of its own",
                    ns = shown(&namespace),
                    source = existing.source,
                ))
            } else {
                Err(format!(
                    "namespace `{ns}`: the app already grants permissions under `{ns}.` (in {source}); a plugin's namespace must be its own",
                    ns = shown(&namespace),
                    source = existing.source,
                ))
            };
        }
    }

    Ok(Manifest {
        namespace,
        title,
        module,
        features,
        config,
        env,
        grants,
        policy,
    })
}

fn read_policy(value: &serde_json::Value) -> Result<PolicyAsk, String> {
    let Some(policy_obj) = value.as_object() else {
        return Err("`policy` must be a table of lists of strings and a bool, `wasm`".to_owned());
    };
    let mut keys: Vec<&str> = policy_obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    for key in keys {
        if !POLICY_KEYS.contains(&key) {
            return Err(format!(
                "`policy`: unknown key `{}` (known: img_src, media_src, connect_src, font_src, wasm); a page's scripts, styles, workers and frames come from this app only",
                shown(key)
            ));
        }
    }

    let wasm = match policy_obj.get("wasm") {
        Some(flag) => {
            let Some(boolean) = flag.as_bool() else {
                return Err("`policy.wasm` must be true or false".to_owned());
            };
            boolean
        }
        None => false,
    };

    let mut sources = Vec::new();
    for &key in POLICY_DIRECTIVES {
        let Some(entry) = policy_obj.get(key) else {
            continue;
        };
        let Some(arr) = entry.as_array() else {
            return Err(
                "`policy` must be a table of lists of strings and a bool, `wasm`".to_owned(),
            );
        };
        let mut items = Vec::with_capacity(arr.len());
        for item in arr {
            let Some(source) = item.as_str() else {
                return Err(
                    "`policy` must be a table of lists of strings and a bool, `wasm`".to_owned(),
                );
            };
            items.push(source);
        }
        if items.len() > 8 {
            return Err(format!("`policy.{key}` may list at most 8 sources"));
        }
        for source in items {
            if let Some(forms) = source_problem(key, source) {
                return Err(format!(
                    "`policy.{key}`: `{}` is not a source the page policy takes ({forms})",
                    shown(source)
                ));
            }
            sources.push((key.to_owned(), source.to_owned()));
        }
    }

    Ok(PolicyAsk { sources, wasm })
}

fn string_list<'a>(value: &'a serde_json::Value, key: &str) -> Result<Vec<&'a str>, String> {
    let Some(arr) = value.as_array() else {
        return Err(format!("`{key}` must be a list of strings"));
    };
    let mut items = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(text) = item.as_str() else {
            return Err(format!("`{key}` must be a list of strings"));
        };
        items.push(text);
    }
    Ok(items)
}

fn is_feature_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(*byte, b'_' | b'.' | b'-')
        })
}

fn is_env_shape(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_uppercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn is_grant_shape(text: &str) -> bool {
    if let Some(prefix) = text.strip_suffix(".*") {
        prefix.split('.').all(is_namespace)
    } else {
        let mut words = 0;
        for word in text.split('.') {
            if !is_namespace(word) {
                return false;
            }
            words += 1;
        }
        words >= 2
    }
}

fn same_package(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left.bytes().zip(right.bytes()).all(|(a, b)| {
            let norm_a = if a == b'_' { b'-' } else { a };
            let norm_b = if b == b'_' { b'-' } else { b };
            norm_a == norm_b
        })
}

/// The package without `navree-plugin-` or `navree-`, `-` turned into `_`.
pub fn default_namespace(package: &str) -> String {
    package
        .strip_prefix("navree-plugin-")
        .or_else(|| package.strip_prefix("navree-"))
        .unwrap_or(package)
        .replace('-', "_")
}

/// Escapes control and direction-changing characters (`\x1b` up to `0xff`, `\u{202e}` above)
/// and cuts at 80 characters with `...` appended.
pub fn shown(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if needs_escape(ch) {
            let code = ch as u32;
            if code <= 0xff {
                let _ = write!(escaped, "\\x{code:02x}");
            } else {
                let _ = write!(escaped, "\\u{{{code:x}}}");
            }
        } else {
            escaped.push(ch);
        }
    }
    if escaped.chars().count() > 80 {
        let mut cut: String = escaped.chars().take(80).collect();
        cut.push_str("...");
        cut
    } else {
        escaped
    }
}

fn needs_escape(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

/// Whether `text` is a lower-case letter followed by up to 63 lower-case letters, digits or `_`.
pub fn is_namespace(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicyDirective {
    ImgSrc,
    MediaSrc,
    ConnectSrc,
    FontSrc,
}

impl PolicyDirective {
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "img_src" => Some(Self::ImgSrc),
            "media_src" => Some(Self::MediaSrc),
            "connect_src" => Some(Self::ConnectSrc),
            "font_src" => Some(Self::FontSrc),
            _ => None,
        }
    }

    const fn forms(self) -> &'static str {
        match self {
            Self::ImgSrc | Self::MediaSrc => {
                "'self', data:, blob: or an https origin such as https://tile.openstreetmap.org"
            }
            Self::ConnectSrc => "'self', an https origin or a wss origin",
            Self::FontSrc => "'self', data: or an https origin",
        }
    }
}

/// `Some` with the allowed-forms description when `source` is not one the page policy takes for `key`.
pub(crate) fn source_problem(key: &str, source: &str) -> Option<&'static str> {
    let Some(directive) = PolicyDirective::from_key(key) else {
        return Some("unknown directive");
    };
    let taken = match source {
        "'self'" => true,
        "data:" => directive != PolicyDirective::ConnectSrc,
        "blob:" => matches!(
            directive,
            PolicyDirective::ImgSrc | PolicyDirective::MediaSrc
        ),
        _ => match source.split_once("://") {
            Some(("https", origin)) => is_origin(origin),
            Some(("wss", origin)) => directive == PolicyDirective::ConnectSrc && is_origin(origin),
            Some(("http", origin)) => is_local(origin),
            _ => false,
        },
    };
    if taken { None } else { Some(directive.forms()) }
}

/// Whether `authority` is a host name in lower case with at least two labels, which may start
/// with `*.` and have a port after it.
fn is_origin(authority: &str) -> bool {
    let Some((host, _)) = host_and_port(authority) else {
        return false;
    };
    let name = host.strip_prefix("*.").unwrap_or(host);
    name.len() <= 253 && name.split('.').count() >= 2 && name.split('.').all(is_label)
}

/// Whether `authority` is this machine, with a port or without: `localhost` or `127.0.0.1`.
fn is_local(authority: &str) -> bool {
    host_and_port(authority).is_some_and(|(host, _)| host == "localhost" || host == "127.0.0.1")
}

/// A DNS label in lower case: letters, digits and dashes, not starting or ending with a dash,
/// at most 63 characters.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The host and the port of `authority` (`host` or `host:port`); `None` when what stands after
/// the colon is not a port.
fn host_and_port(authority: &str) -> Option<(&str, Option<u16>)> {
    match authority.split_once(':') {
        None => Some((authority, None)),
        Some((host, port)) => Some((host, Some(port_of(port)?))),
    }
}

/// A port as an origin writes it: digits only, no leading zero, from 1 to 65535.
fn port_of(text: &str) -> Option<u16> {
    let written = !text.is_empty()
        && text.len() <= 5
        && !text.starts_with('0')
        && text.bytes().all(|byte| byte.is_ascii_digit());
    if written { text.parse().ok() } else { None }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const PACKAGE: &str = "navree-plugin-announcements";

    fn table(src: &str) -> serde_json::Value {
        let parsed: toml::Value = toml::from_str(src).expect("valid TOML");
        serde_json::to_value(parsed).expect("JSON value")
    }

    fn with_default_module(src: &str) -> serde_json::Value {
        let mut value = table(src);
        let obj = value.as_object_mut().expect("TOML table");
        if !obj.contains_key("module") {
            obj.insert(
                "module".to_owned(),
                serde_json::Value::String("AnnouncementsModule::from_config()".to_owned()),
            );
        }
        value
    }

    fn facts() -> AppFacts {
        AppFacts {
            first_party: false,
            navree_version: "1.4.0".to_owned(),
            navree_features: [
                "access",
                "billing",
                "documents",
                "dock",
                "full",
                "schema",
                "sqlite",
                "ui",
                "ui-native-preview",
                "workspace",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            allowed_features: ["access", "billing", "ui", "workspace"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            reserved: RESERVED_NAMESPACES
                .iter()
                .copied()
                .map(str::to_owned)
                .collect(),
            roles: ["admin", "member", "viewer", "support"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            demo_guest_role: Some("guest".to_owned()),
            granted: Vec::new(),
        }
    }

    #[test]
    fn the_manifest_grammar_refuses_each_shape_with_its_reason() {
        let app = facts();
        let long_module = format!("module = \"{}B\"", "A".repeat(200));
        let many_features = format!("features = [{}]", ["\"ui\""; 33].join(", "));
        let long_perm = format!("announcements.{}", "a".repeat(115));
        assert_eq!(long_perm.len(), 129);
        let long_grant_toml = format!("grants = {{ member = [\"{long_perm}\"] }}");
        let long_grant_reason = format!(
            "`grants.member`: `{}` is longer than 128 characters",
            shown(&long_perm)
        );
        let many_sources = format!(
            "policy = {{ img_src = [{}] }}",
            ["\"https://a.example\""; 9].join(", ")
        );

        let cases: &[(&str, &str)] = &[
            (
                r#"module = "X::new(); std::process::exit(1)""#,
                MODULE_GRAMMAR,
            ),
            (r#"module = "X::new(1)""#, MODULE_GRAMMAR),
            (r#"module = "X:: new()""#, MODULE_GRAMMAR),
            (r#"module = "X::new()()""#, MODULE_GRAMMAR),
            (
                r#"module = "crate::X""#,
                "`module` uses `crate`, which is a Rust keyword",
            ),
            (r#"module = "X::<Vec<u8>>::new()""#, MODULE_GRAMMAR),
            ("module = \"X::new\\n\"", MODULE_GRAMMAR),
            ("module = \"X\\u202e\"", MODULE_GRAMMAR),
            (&long_module, "`module` must be at most 200 characters"),
            (
                r#"features = ["work space"]"#,
                "`features`: `work space` must be lower-case letters, digits, `-`, `_` or `.`",
            ),
            (
                r#"features = ["full"]"#,
                "`features`: `full` is the app owner's choice; turn it on yourself if you want it",
            ),
            (
                r#"features = ["sqlite"]"#,
                "`features`: `sqlite` is the app owner's choice; turn it on yourself if you want it",
            ),
            (
                r#"features = ["ui-native-preview"]"#,
                "`features`: `ui-native-preview` is the app owner's choice; turn it on yourself if you want it",
            ),
            (
                r#"features = ["dock"]"#,
                "`features`: `dock` is the app owner's choice; turn it on yourself if you want it",
            ),
            (
                r#"features = ["documents"]"#,
                "`features`: `documents` is the app owner's choice; turn it on yourself if you want it",
            ),
            (
                r#"features = ["billing2"]"#,
                "`features`: navree 1.4.0 has no feature `billing2`",
            ),
            (&many_features, "`features` may list at most 32 features"),
            (
                r#"env = ["app_key"]"#,
                "`env`: `app_key` must be upper-case letters, digits and `_`, starting with a letter",
            ),
            (
                r#"env = ["APP_WORKSPACE__X"]"#,
                "`env`: `APP_WORKSPACE__X` must start with APP_ANNOUNCEMENTS__, the plugin's own settings",
            ),
            (
                r#"config = ["workspace"]"#,
                "`config`: `workspace` is outside the plugin's namespace `announcements`; a plugin publishes its own sections only",
            ),
            (
                r#"config = ["Announcements"]"#,
                "`config`: `Announcements` must be lower-case words of letters, digits and `_`, joined by `.` (`announcements`, `announcements.digest`)",
            ),
            (
                r#"grants = { member = ["roles.manage"] }"#,
                "`grants.member`: `roles.manage` is outside the plugin's namespace `announcements`; a plugin suggests grants of its own permissions only",
            ),
            (
                r#"grants = { member = ["workspace.*"] }"#,
                "`grants.member`: `workspace.*` is outside the plugin's namespace `announcements`; a plugin suggests grants of its own permissions only",
            ),
            (
                r#"grants = { owner = ["announcements.read"] }"#,
                "`grants`: the role `owner` is the owner or the demo guest role; a plugin suggests no grants to it",
            ),
            (
                r#"grants = { guest = ["announcements.read"] }"#,
                "`grants`: the role `guest` is the owner or the demo guest role; a plugin suggests no grants to it",
            ),
            (
                r#"grants = { auditor = ["announcements.read"] }"#,
                "`grants`: the role `auditor` is not one of admin, member, viewer or a role of this app",
            ),
            (&long_grant_toml, &long_grant_reason),
            (
                r#"grants = { member = ["announcements.Read"] }"#,
                "`grants.member`: `announcements.Read` must be lower-case words of letters, digits or `_` joined by `.`, or end in `.*`",
            ),
            (
                r#"namespace = "app""#,
                "`namespace`: `app` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")",
            ),
            (
                r#"namespace = "views""#,
                "`namespace`: `views` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")",
            ),
            (
                r#"namespace = "workspace""#,
                "`namespace`: `workspace` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")",
            ),
            (
                r#"namespace = "all""#,
                "`namespace`: `all` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")",
            ),
            (
                r#"namespace = "all"
grants = { member = ["all.*"] }"#,
                "`namespace`: `all` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")",
            ),
            (
                r#"namespace = "Announcements""#,
                "`namespace` must be a lower-case letter followed by up to 63 lower-case letters, digits or _",
            ),
            (
                r#"title = """#,
                "`title` must be 1 to 64 characters, with no control characters",
            ),
            (
                r#"script = "build.sh""#,
                "unknown key `script` (known: namespace, title, module, features, config, env, grants, policy; a newer plugin may need a newer navree)",
            ),
            (
                r#"policy = { script_src = ["https://cdn.example"] }"#,
                "`policy`: unknown key `script_src` (known: img_src, media_src, connect_src, font_src, wasm); a page's scripts, styles, workers and frames come from this app only",
            ),
            (
                r#"policy = { img_src = ["*"] }"#,
                "`policy.img_src`: `*` is not a source the page policy takes ('self', data:, blob: or an https origin such as https://tile.openstreetmap.org)",
            ),
            (
                r#"policy = { connect_src = ["data:"] }"#,
                "`policy.connect_src`: `data:` is not a source the page policy takes ('self', an https origin or a wss origin)",
            ),
            (&many_sources, "`policy.img_src` may list at most 8 sources"),
            (
                r#"policy = { wasm = "yes" }"#,
                "`policy.wasm` must be true or false",
            ),
            (
                "policy = 1",
                "`policy` must be a table of lists of strings and a bool, `wasm`",
            ),
            ("module = 7", "`module` must be a string"),
            (r#"features = "ui""#, "`features` must be a list of strings"),
            (
                r#"grants = { member = "announcements.read" }"#,
                "`grants` must be a table of lists of strings",
            ),
        ];

        for &(src, expected) in cases {
            let Err(reason) = read(PACKAGE, &with_default_module(src), &app) else {
                panic!("expected refusal for:\n{src}");
            };
            assert_eq!(reason, expected, "for:\n{src}");
        }

        // Missing `module` (without inserting the default).
        let missing = read(PACKAGE, &table(""), &app).unwrap_err();
        assert_eq!(missing, "`module` is required");

        // Non-table plugin value.
        assert_eq!(
            read(PACKAGE, &serde_json::json!(1), &app).unwrap_err(),
            "must be a table"
        );

        // Permissions already granted under the namespace by the app.
        let mut with_existing = facts();
        with_existing.granted.push(Granted {
            permission: "reports.view".to_owned(),
            source: "config/workspace.toml".to_owned(),
            marked_for: None,
        });
        assert_eq!(
            read(
                "navree-plugin-reports",
                &table(
                    r#"module = "ReportsModule::from_config()"
grants = { member = ["reports.view"] }"#
                ),
                &with_existing
            )
            .unwrap_err(),
            "`grants`: the app already grants permissions under `reports.` (in config/workspace.toml); a plugin suggests grants only in a namespace of its own"
        );
        assert_eq!(
            read(
                "navree-plugin-reports",
                &table(r#"module = "ReportsModule::from_config()""#),
                &with_existing
            )
            .unwrap_err(),
            "namespace `reports`: the app already grants permissions under `reports.` (in config/workspace.toml); a plugin's namespace must be its own"
        );

        // Marked for the same package: accepted.
        let mut marked_same = facts();
        marked_same.granted.push(Granted {
            permission: "reports.view".to_owned(),
            source: "config/workspace.toml".to_owned(),
            marked_for: Some("navree-plugin-reports".to_owned()),
        });
        assert!(
            read(
                "navree-plugin-reports",
                &table(
                    r#"module = "ReportsModule::from_config()"
grants = { member = ["reports.view"] }"#
                ),
                &marked_same
            )
            .is_ok()
        );

        // Marked for another package: refused.
        let mut marked_other = facts();
        marked_other.granted.push(Granted {
            permission: "reports.view".to_owned(),
            source: "config/workspace.toml".to_owned(),
            marked_for: Some("navree-plugin-khata".to_owned()),
        });
        assert_eq!(
            read(
                "navree-plugin-reports",
                &table(
                    r#"module = "ReportsModule::from_config()"
grants = { member = ["reports.view"] }"#
                ),
                &marked_other
            )
            .unwrap_err(),
            "`grants`: the app already grants permissions under `reports.` (in config/workspace.toml); a plugin suggests grants only in a namespace of its own"
        );
    }

    #[test]
    fn the_manifest_ties_config_sections_and_env_names_to_the_namespace() {
        let app = facts();
        let ok = read(
            "navree-plugin-notice-board",
            &table(
                r#"module = "NoticeBoardModule::from_config()"
config = ["notice_board", "notice_board.digest"]
env = ["APP_NOTICE_BOARD__SIGNING_KEY"]"#,
            ),
            &app,
        )
        .expect("default namespace matches config and env");
        assert_eq!(ok.namespace, "notice_board");
        assert_eq!(ok.config, ["notice_board", "notice_board.digest"]);
        assert_eq!(ok.env, ["APP_NOTICE_BOARD__SIGNING_KEY"]);

        assert_eq!(
            read(
                "navree-plugin-notice-board",
                &table(
                    r#"namespace = "notices"
module = "NoticeBoardModule::from_config()"
config = ["notice_board"]"#
                ),
                &app
            )
            .unwrap_err(),
            "`config`: `notice_board` is outside the plugin's namespace `notices`; a plugin publishes its own sections only"
        );

        assert_eq!(
            read(
                "navree-plugin-notice-board",
                &table(
                    r#"namespace = "notices"
module = "NoticeBoardModule::from_config()"
env = ["APP_NOTICE_BOARD__X"]"#
                ),
                &app
            )
            .unwrap_err(),
            "`env`: `APP_NOTICE_BOARD__X` must start with APP_NOTICES__, the plugin's own settings"
        );

        // `plugins/geo`'s manifest shape with `policy = { img_src = [..] }`.
        let geo = read(
            "navree-plugin-geo",
            &table(
                r#"module = "GeoModule::from_config()"
policy = { img_src = ["https://tile.openstreetmap.org"] }"#,
            ),
            &app,
        )
        .expect("geo manifest passes");
        assert_eq!(geo.namespace, "geo");
        assert_eq!(
            geo.policy,
            PolicyAsk {
                sources: vec![(
                    "img_src".to_owned(),
                    "https://tile.openstreetmap.org".to_owned()
                )],
                wasm: false,
            }
        );
    }

    #[test]
    fn the_manifest_refuses_grants_outside_the_namespace_and_to_owner_or_the_demo_guest_role() {
        let app = facts();

        for role in ["owner", "guest"] {
            let src = format!(r#"grants = {{ {role} = ["announcements.read"] }}"#);
            assert_eq!(
                read(PACKAGE, &with_default_module(&src), &app).unwrap_err(),
                format!(
                    "`grants`: the role `{role}` is the owner or the demo guest role; a plugin suggests no grants to it"
                )
            );
        }

        assert_eq!(
            read(
                PACKAGE,
                &with_default_module(r#"grants = { member = ["other.read"] }"#),
                &app
            )
            .unwrap_err(),
            "`grants.member`: `other.read` is outside the plugin's namespace `announcements`; a plugin suggests grants of its own permissions only"
        );

        let accepted = read(
            PACKAGE,
            &with_default_module(
                r#"grants = { member = ["announcements.*", "announcements.read"], support = ["announcements.read"] }"#,
            ),
            &app,
        )
        .expect("member and app role support accepted");
        assert_eq!(
            accepted.grants.get("member").unwrap(),
            &["announcements.*", "announcements.read"]
        );
        assert_eq!(
            accepted.grants.get("support").unwrap(),
            &["announcements.read"]
        );

        let nine_roles = (1..=9)
            .map(|n| format!("role_{n} = [\"announcements.read\"]"))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            read(
                PACKAGE,
                &with_default_module(&format!("grants = {{ {nine_roles} }}")),
                &app
            )
            .unwrap_err(),
            "`grants` may name at most 8 roles"
        );

        let many_perms = (1..=33)
            .map(|n| format!("\"announcements.p_{n}\""))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            read(
                PACKAGE,
                &with_default_module(&format!("grants = {{ member = [{many_perms}] }}")),
                &app
            )
            .unwrap_err(),
            "`grants.member` may list at most 32 permissions"
        );
    }

    #[test]
    fn the_manifest_refuses_a_reserved_namespace_for_a_community_crate() {
        let app = facts();
        assert_eq!(RESERVED_NAMESPACES.len(), 87);

        for ns in [
            "app",
            "views",
            "workspace",
            "all",
            "users",
            "organization",
            "subscriptions",
            "operations",
            "automations",
        ] {
            let src = format!(r#"namespace = "{ns}""#);
            assert_eq!(
                read(PACKAGE, &with_default_module(&src), &app).unwrap_err(),
                format!(
                    "`namespace`: `{ns}` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")"
                )
            );
        }

        assert_eq!(
            read(
                PACKAGE,
                &with_default_module(
                    r#"namespace = "all"
grants = { member = ["all.*"] }"#
                ),
                &app
            )
            .unwrap_err(),
            "`namespace`: `all` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")"
        );

        assert_eq!(
            read(
                "navree-plugin-con",
                &table(r#"module = "ConModule::from_config()""#),
                &app
            )
            .unwrap_err(),
            "`namespace`: `con` is kept by the framework; set another in the manifest and in plugin_info!(namespace = \"..\")"
        );

        let mut first_party = facts();
        first_party.first_party = true;
        let http_client = read(
            "navree-http-client",
            &table(r#"module = "HttpClientModule::from_config()""#),
            &first_party,
        )
        .expect("first-party package keeps its reserved namespace");
        assert_eq!(http_client.namespace, "http_client");
    }

    #[test]
    fn the_manifest_refuses_features_that_are_the_owners_choice() {
        let app = facts();
        for feature in ["full", "documents", "schema", "sqlite", "ui-native-preview"] {
            let src = format!(r#"features = ["{feature}"]"#);
            assert_eq!(
                read(PACKAGE, &with_default_module(&src), &app).unwrap_err(),
                format!(
                    "`features`: `{feature}` is the app owner's choice; turn it on yourself if you want it"
                )
            );
        }
    }

    #[test]
    fn policy_sources_follow_the_page_policys_grammar() {
        for accepted in [
            "'self'",
            "data:",
            "blob:",
            "https://tile.openstreetmap.org",
            "https://*.tiles.example",
            "https://tiles.example:8443",
            "http://localhost:8090",
            "http://127.0.0.1",
        ] {
            assert_eq!(
                source_problem("img_src", accepted),
                None,
                "img_src should accept {accepted:?}"
            );
        }
        assert_eq!(
            source_problem("connect_src", "wss://live.ludhiana.example"),
            None
        );

        for refused in [
            "https://tiles.example/a.png",
            "*",
            "https:",
            "http://tiles.example",
            "'unsafe-inline'",
            "'unsafe-eval'",
            "'wasm-unsafe-eval'",
            "'strict-dynamic'",
            "'nonce-abc'",
            "'sha256-abc'",
            "HTTPS://TILES.EXAMPLE",
            "https://a.example; script-src *",
            "https://a.example b",
            "https://*.com",
            "https://tiles",
            "https://tiles.example:0",
            "https://tiles.example:99999",
            "https://-a.example",
            "https://a..example",
            "'none'",
            "ws://live.example",
        ] {
            assert!(
                source_problem("img_src", refused).is_some(),
                "img_src should refuse {refused:?}"
            );
        }
        assert!(source_problem("connect_src", "data:").is_some());
    }

    #[test]
    fn the_manifest_policy_key_takes_sources_by_the_page_policy_grammar() {
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../navree-ui/tests/fixtures/policy-sources.txt");
        let fixture = std::fs::read_to_string(&fixture_path).expect("policy-sources.txt fixture");
        let app = facts();
        let mut count = 0;

        for (index, line) in fixture.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            count += 1;
            let parts: Vec<&str> = line.split('\t').collect();
            assert_eq!(parts.len(), 3, "line {}: {line:?}", index + 1);
            let (key, source, expected) = (parts[0], parts[1], parts[2]);

            let manifest_json = serde_json::json!({
                "module": "AnnouncementsModule::from_config()",
                "policy": {
                    key: [source],
                },
            });

            match expected {
                "accept" => {
                    assert_eq!(
                        source_problem(key, source),
                        None,
                        "line {}: expected {key} to accept {source:?}",
                        index + 1
                    );
                    assert!(
                        read(PACKAGE, &manifest_json, &app).is_ok(),
                        "line {}: manifest should accept {key} {source:?}",
                        index + 1
                    );
                }
                "refuse" => {
                    let forms = source_problem(key, source).unwrap_or_else(|| {
                        panic!("line {}: expected {key} to refuse {source:?}", index + 1)
                    });
                    let reason = read(PACKAGE, &manifest_json, &app).unwrap_err();
                    assert_eq!(
                        reason,
                        format!(
                            "`policy.{key}`: `{}` is not a source the page policy takes ({forms})",
                            shown(source)
                        ),
                        "line {}",
                        index + 1
                    );
                }
                other => panic!("line {}: unexpected verdict {other:?}", index + 1),
            }
        }
        assert_eq!(count, 136);
    }

    #[test]
    fn module_paths_parse_by_the_grammar() {
        for (input, expected_type) in [
            ("AnnouncementsModule", "AnnouncementsModule"),
            ("AnnouncementsModule::from_config()", "AnnouncementsModule"),
            ("board::BoardModule::new()", "BoardModule"),
            (
                "ClientModule::<reqwest::Client,KhataOptions>::new()",
                "ClientModule",
            ),
            ("khata_client()", "khata_client"),
            ("_Ledger", "_Ledger"),
        ] {
            let parsed = ModulePath::parse(input)
                .unwrap_or_else(|err| panic!("expected {input:?} to parse, got {err}"));
            assert_eq!(parsed.as_str(), input);
            assert_eq!(parsed.type_name(), expected_type, "for {input:?}");
        }

        for bad in [
            "_",
            "::X",
            "X::",
            "X<Y>",
            "X::<>::new()",
            "X::<Y,>::new()",
            "X::<Y, Z>::new()",
            "self::X",
            "Self",
            "X()::new",
            "",
            "ਖਾਤਾ",
        ] {
            assert!(
                ModulePath::parse(bad).is_err(),
                "expected {bad:?} to be refused"
            );
        }
        assert_eq!(
            ModulePath::parse("self::X").unwrap_err(),
            "`module` uses `self`, which is a Rust keyword"
        );
        assert_eq!(
            ModulePath::parse("Self").unwrap_err(),
            "`module` uses `Self`, which is a Rust keyword"
        );
    }

    #[test]
    fn manifest_values_are_printed_escaped_and_cut() {
        assert_eq!(shown("work\u{1b}[2Jspace"), "work\\x1b[2Jspace");
        assert_eq!(shown("\u{202e}txt.exe"), "\\u{202e}txt.exe");
        assert_eq!(shown(&"a".repeat(300)), format!("{}...", "a".repeat(80)));
        assert_eq!(shown("Ludhiana ₹1,00,000"), "Ludhiana ₹1,00,000");

        let app = facts();
        let bad = serde_json::json!({
            "module": "AnnouncementsModule::from_config()",
            "\u{202e}fdp.exe": "1",
        });
        let reason = read(PACKAGE, &bad, &app).unwrap_err();
        assert!(reason.contains("\\u{202e}fdp.exe"), "reason was: {reason}");
        assert!(
            !reason.contains('\u{202e}'),
            "raw bidi control in: {reason}"
        );
    }
}

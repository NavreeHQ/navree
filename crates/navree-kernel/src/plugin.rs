//! [`PluginInfo`] and `plugin_info!`: what a plugin says about itself, the
//! files it ships ([`Publishable`]), and the kernel's lists of first-party
//! plugins and reserved namespaces.

use std::borrow::Cow;

/// What a plugin says about itself: `m.plugin(plugin_info!())`.
///
/// `plugin_info!()` fills it from the `Cargo.toml` of the crate that calls
/// it; [`PluginInfo::new`] and the setters build one by hand. The kernel
/// checks it at boot (see [`ModuleBuilder::plugin`](crate::ModuleBuilder::plugin)).
///
/// ```
/// use navree_kernel::PluginInfo;
///
/// let info = PluginInfo::new(
///     "navree-plugin-announcements",
///     "navree_plugin_announcements",
///     "announcements",
///     "1.0.0",
/// )
/// .title("Announcements")
/// .description("Notices for every member of a workspace, by Simran Kaur's team in Ludhiana.")
/// .homepage("")
/// .repository("https://code.ludhiana.example/navree-plugin-announcements")
/// .license("MIT OR Apache-2.0");
/// assert_eq!(info.namespace, "announcements");
/// assert_eq!(info.homepage, None);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginInfo {
    /// The crate name (`CARGO_PKG_NAME`): `navree-plugin-announcements`.
    pub name: &'static str,
    /// The crate's Rust name (`CARGO_CRATE_NAME`): `navree_plugin_announcements`.
    /// The kernel checks the plugin's modules against it.
    pub crate_name: &'static str,
    /// What the plugin's config sections, permissions, env names, templates,
    /// static files and message ids start with: `announcements`. A lower-case
    /// letter followed by up to 63 lower-case letters, digits or `_`.
    pub namespace: &'static str,
    /// `CARGO_PKG_VERSION`.
    pub version: &'static str,
    /// A display name (`Announcements`), from `plugin_info!(title = "..")`.
    pub title: Option<&'static str>,
    /// `CARGO_PKG_DESCRIPTION`; `None` when empty.
    pub description: Option<&'static str>,
    /// `CARGO_PKG_HOMEPAGE`; `None` when empty.
    pub homepage: Option<&'static str>,
    /// `CARGO_PKG_REPOSITORY`; `None` when empty.
    pub repository: Option<&'static str>,
    /// `CARGO_PKG_LICENSE`; `None` when empty.
    pub license: Option<&'static str>,
}

impl PluginInfo {
    /// A plugin named `name` (crate `crate_name`) at `version` with `namespace`
    /// (checked at boot).
    pub const fn new(
        name: &'static str,
        crate_name: &'static str,
        namespace: &'static str,
        version: &'static str,
    ) -> PluginInfo {
        PluginInfo {
            name,
            crate_name,
            namespace,
            version,
            title: None,
            description: None,
            homepage: None,
            repository: None,
            license: None,
        }
    }

    /// Sets the display name; an empty string sets `None`.
    pub const fn title(self, title: &'static str) -> PluginInfo {
        PluginInfo {
            title: non_empty(title),
            ..self
        }
    }

    /// Sets the description; an empty string sets `None`.
    pub const fn description(self, description: &'static str) -> PluginInfo {
        PluginInfo {
            description: non_empty(description),
            ..self
        }
    }

    /// Sets the homepage; an empty string sets `None`.
    pub const fn homepage(self, homepage: &'static str) -> PluginInfo {
        PluginInfo {
            homepage: non_empty(homepage),
            ..self
        }
    }

    /// Sets the repository; an empty string sets `None`.
    pub const fn repository(self, repository: &'static str) -> PluginInfo {
        PluginInfo {
            repository: non_empty(repository),
            ..self
        }
    }

    /// Sets the license; an empty string sets `None`.
    pub const fn license(self, license: &'static str) -> PluginInfo {
        PluginInfo {
            license: non_empty(license),
            ..self
        }
    }
}

/// `None` for an empty string.
const fn non_empty(value: &'static str) -> Option<&'static str> {
    if value.is_empty() { None } else { Some(value) }
}

/// The kinds of files a plugin ships that `plugin:publish` copies into the
/// app, each into a folder of its own (relative to the app's folder):
///
/// | Tag | Folder | Path inside it |
/// |---|---|---|
/// | `views` | `resources/views/<namespace>/` | any relative path |
/// | `locales` | `locales/<locale>/<namespace>/` | the first segment is the locale (`en`, `pa-IN`), the rest goes below `<namespace>/` |
/// | `files` | `resources/<namespace>/` | any relative path |
///
/// There is no tag for configuration: `plugin:publish --tag config` runs the
/// app's `config:publish --plugin <namespace>` instead.
///
/// ```
/// use navree_kernel::PublishTag;
///
/// assert_eq!(PublishTag::from_name("locales"), Some(PublishTag::Locales));
/// assert_eq!(PublishTag::Views.as_str(), "views");
/// assert_eq!(PublishTag::ALL.len(), 3);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum PublishTag {
    /// Templates, copied to `resources/views/<namespace>/`.
    Views,
    /// Translations, copied to `locales/<locale>/<namespace>/`.
    Locales,
    /// Other files (styles, images, scripts), copied to `resources/<namespace>/`.
    Files,
}

impl PublishTag {
    /// Every tag, in this order: views, locales, files.
    pub const ALL: &'static [PublishTag] =
        &[PublishTag::Views, PublishTag::Locales, PublishTag::Files];

    /// `views`, `locales` or `files`.
    pub const fn as_str(self) -> &'static str {
        match self {
            PublishTag::Views => "views",
            PublishTag::Locales => "locales",
            PublishTag::Files => "files",
        }
    }

    /// The tag named `name` (`views`, `locales` or `files`), if there is one.
    pub fn from_name(name: &str) -> Option<PublishTag> {
        PublishTag::ALL
            .iter()
            .copied()
            .find(|tag| tag.as_str() == name)
    }
}

/// Files a plugin ships in its binary that `plugin:publish` copies into the
/// app, all under one [`PublishTag`]. A module that names itself a plugin
/// contributes it with `m.contribute(..)`.
///
/// Each path is relative to the tag's folder, with `/` between segments. The
/// boot (and `check`) fails when a module that does not name itself a plugin
/// contributes one, and when a path is not safe: empty, longer than 255 bytes
/// (also once placed in its folder), or with a segment longer than 128 bytes;
/// an empty, `.` or `..` segment, a leading `/`, a `\`, a `:` or a control
/// character; one of `<>"|?*`; a segment ending in `.` or a space; a segment
/// named after a Windows device (`con`, `prn`, `aux`, `nul`, `conin$`,
/// `conout$`, `com0` to `com9`, `lpt0` to `lpt9`, `com¹` to `com³`, `lpt¹` to
/// `lpt³`, in any case, with or without an extension); two paths of one
/// plugin under one tag that differ only in case (their lower-case forms or
/// their upper-case forms are equal), or a path that another of them needs
/// as its folder (`admin` beside `admin/edit.html`). Under `locales` the
/// first segment is the locale.
///
/// ```
/// use navree_kernel::{PublishTag, Publishable};
///
/// let views = Publishable::new(PublishTag::Views)
///     .file("list.html", b"<ul><li>Simran Kaur: fees of \xe2\x82\xb91,00,000 are due</li></ul>")
///     .file("show.html", b"<p></p>");
/// assert_eq!(views.tag(), PublishTag::Views);
/// assert_eq!(views.files().count(), 2);
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Publishable {
    /// The folder the files go to.
    tag: PublishTag,
    /// Each file's path in that folder, and its bytes.
    files: Vec<(Cow<'static, str>, &'static [u8])>,
}

impl Publishable {
    /// No files yet, under `tag`.
    pub fn new(tag: PublishTag) -> Publishable {
        Publishable {
            tag,
            files: Vec::new(),
        }
    }

    /// Adds a file: `path` relative to the tag's folder, `bytes` embedded in
    /// the binary (with `include_bytes!`, say).
    pub fn file(mut self, path: impl Into<Cow<'static, str>>, bytes: &'static [u8]) -> Publishable {
        self.files.push((path.into(), bytes));
        self
    }

    /// The tag the files are published under.
    pub fn tag(&self) -> PublishTag {
        self.tag
    }

    /// The files as (path, bytes), in the order they were added.
    pub fn files(&self) -> impl Iterator<Item = (&str, &'static [u8])> {
        self.files.iter().map(|(path, bytes)| (&**path, *bytes))
    }
}

/// Where `plugin:publish` writes the file `path` that a plugin with
/// `namespace` ships under `tag`, relative to the app's folder:
/// `resources/views/<namespace>/<path>`, `locales/<locale>/<namespace>/<rest>`
/// (`path` is `<locale>/<rest>`), or `resources/<namespace>/<path>`.
pub(crate) fn destination(tag: PublishTag, namespace: &str, path: &str) -> String {
    match tag {
        PublishTag::Views => format!("resources/views/{namespace}/{path}"),
        PublishTag::Locales => {
            let (locale, rest) = path.split_once('/').unwrap_or((path, ""));
            format!("locales/{locale}/{namespace}/{rest}")
        }
        PublishTag::Files => format!("resources/{namespace}/{path}"),
    }
}

/// What a plugin says about itself, filled from the `Cargo.toml` of the crate
/// that calls it: a [`PluginInfo`](crate::PluginInfo) with the calling crate's
/// `CARGO_PKG_NAME`, `CARGO_CRATE_NAME`, `CARGO_PKG_VERSION`,
/// `CARGO_PKG_DESCRIPTION`, `CARGO_PKG_HOMEPAGE`, `CARGO_PKG_REPOSITORY` and
/// `CARGO_PKG_LICENSE` (an empty value gives `None`).
///
/// The namespace defaults to the package name without a leading
/// `navree-plugin-`, else without a leading `navree-`, with every `-` turned
/// into `_` (`navree-plugin-announcements` gives `announcements`, `acme-crm`
/// gives `acme_crm`). Two optional keys, in any order: `namespace = ".."` sets
/// another namespace, `title = ".."` a display name. A namespace is a
/// lower-case letter followed by up to 63 lower-case letters, digits or `_`;
/// any other namespace, or a package name that gives none, is a compile error.
///
/// ```
/// let info = navree_kernel::plugin_info!(namespace = "notice_board", title = "Notice board");
/// assert_eq!(info.namespace, "notice_board");
/// assert_eq!(info.title, Some("Notice board"));
/// ```
///
/// An unknown key and a namespace outside the grammar do not compile:
///
/// ```compile_fail
/// let _ = navree_kernel::plugin_info!(name = "announcements");
/// ```
///
/// ```compile_fail
/// let _ = navree_kernel::plugin_info!(namespace = "Notice-Board");
/// ```
#[macro_export]
macro_rules! plugin_info {
    () => { $crate::plugin_info!(@default) };
    (title = $title:literal $(,)?) => { $crate::plugin_info!(@default).title($title) };
    (namespace = $namespace:literal $(,)?) => { $crate::plugin_info!(@given $namespace) };
    (namespace = $namespace:literal, title = $title:literal $(,)?) => {
        $crate::plugin_info!(@given $namespace).title($title)
    };
    (title = $title:literal, namespace = $namespace:literal $(,)?) => {
        $crate::plugin_info!(@given $namespace).title($title)
    };
    (@default) => {{
        const PACKAGE: &str = ::core::env!("CARGO_PKG_NAME");
        const DEFAULT: ([u8; PACKAGE.len()], usize) =
            $crate::__private::default_namespace::<{ PACKAGE.len() }>(PACKAGE);
        const BYTES: &[u8] = &DEFAULT.0;
        const NAMESPACE: &str = {
            let namespace = $crate::__private::namespace_text(BYTES, DEFAULT.1);
            ::core::assert!(
                $crate::__private::is_namespace(namespace),
                "plugin_info!: this crate's name gives no valid namespace; set one with plugin_info!(namespace = \"..\")"
            );
            namespace
        };
        $crate::plugin_info!(@build NAMESPACE)
    }};
    (@given $namespace:literal) => {{
        const NAMESPACE: &str = {
            ::core::assert!(
                $crate::__private::is_namespace($namespace),
                "plugin_info!: a namespace is a lower-case letter followed by up to 63 lower-case letters, digits or _"
            );
            $namespace
        };
        $crate::plugin_info!(@build NAMESPACE)
    }};
    (@build $namespace:expr) => {
        $crate::PluginInfo::new(
            ::core::env!("CARGO_PKG_NAME"),
            ::core::env!("CARGO_CRATE_NAME"),
            $namespace,
            ::core::env!("CARGO_PKG_VERSION"),
        )
        .description(::core::env!("CARGO_PKG_DESCRIPTION"))
        .homepage(::core::env!("CARGO_PKG_HOMEPAGE"))
        .repository(::core::env!("CARGO_PKG_REPOSITORY"))
        .license(::core::env!("CARGO_PKG_LICENSE"))
    };
    ($($other:tt)*) => {
        ::core::compile_error!("plugin_info!: unknown key; expected namespace = \"..\" or title = \"..\"")
    };
}

/// Whether `namespace` is a lower-case letter followed by up to 63 lower-case
/// letters, digits or `_`. Not public API; `plugin_info!` and the kernel's
/// boot checks use it.
pub const fn is_namespace(namespace: &str) -> bool {
    let bytes = namespace.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'a'..=b'z' | b'0'..=b'9' | b'_' => index += 1,
            _ => return false,
        }
    }
    true
}

/// The namespace a package name gives, as bytes and how many of them are
/// used: `package` without a leading `navree-plugin-`, else without a leading
/// `navree-`, with every `-` turned into `_`. `N` must be at least
/// `package.len()`. Not public API; `plugin_info!` uses it.
pub const fn default_namespace<const N: usize>(package: &str) -> ([u8; N], usize) {
    let bytes = package.as_bytes();
    let start = if starts_with(bytes, b"navree-plugin-") {
        "navree-plugin-".len()
    } else if starts_with(bytes, b"navree-") {
        "navree-".len()
    } else {
        0
    };
    let mut namespace = [0_u8; N];
    let mut index = start;
    while index < bytes.len() {
        namespace[index - start] = if bytes[index] == b'-' {
            b'_'
        } else {
            bytes[index]
        };
        index += 1;
    }
    (namespace, bytes.len() - start)
}

/// The first `len` bytes of `bytes` as text. Not public API; `plugin_info!`
/// uses it on the bytes [`default_namespace`] gives.
pub const fn namespace_text(bytes: &'static [u8], len: usize) -> &'static str {
    // `split_at`, since range indexing is not available in a `const fn`.
    match core::str::from_utf8(bytes.split_at(len).0) {
        Ok(text) => text,
        Err(_) => panic!("plugin_info!: a package name is UTF-8"),
    }
}

/// The framework's own plugin packages, as (package, crate), sorted by
/// package: a plugin is first-party when its name is here, it names the listed
/// crate as its own, its version is the kernel's, and every module of it is a
/// type of the listed crate. These are names, which a package can take: one
/// whose library is named like a listed crate meets them. Not public API;
/// Navree's own crates and tests use it.
pub const FIRST_PARTY: &[(&str, &str)] = &[
    ("navree-ai", "navree_ai"),
    ("navree-audit", "navree_audit"),
    ("navree-billing", "navree_billing"),
    ("navree-directory", "navree_directory"),
    ("navree-documents", "navree_documents"),
    ("navree-domains", "navree_domains"),
    ("navree-flags", "navree_flags"),
    ("navree-fleet", "navree_fleet"),
    ("navree-graphql", "navree_graphql"),
    ("navree-grpc", "navree_grpc"),
    ("navree-helm", "navree_helm"),
    ("navree-http-client", "navree_http_client"),
    ("navree-i18n", "navree_i18n"),
    ("navree-integrations", "navree_integrations"),
    ("navree-mcp", "navree_mcp"),
    ("navree-media", "navree_media"),
    ("navree-micro", "navree_micro"),
    ("navree-mongo", "navree_mongo"),
    ("navree-notify", "navree_notify"),
    ("navree-oauth", "navree_oauth"),
    ("navree-realtime", "navree_realtime"),
    ("navree-redis", "navree_redis"),
    ("navree-search", "navree_search"),
    ("navree-settings", "navree_settings"),
    ("navree-sheets", "navree_sheets"),
    ("navree-social", "navree_social"),
    ("navree-sso", "navree_sso"),
    ("navree-storage", "navree_storage"),
    ("navree-time", "navree_time"),
    ("navree-webhooks", "navree_webhooks"),
    ("navree-workflows", "navree_workflows"),
];

/// The library names of the framework's workspace crates, sorted: a module
/// whose own type is in one of them and that names no plugin is a framework
/// module, not a module of a plugin's second crate. Not public API; Navree's
/// own crates and tests use it.
pub const FRAMEWORK_CRATES: &[&str] = &[
    "navree",
    "navree_access",
    "navree_ai",
    "navree_api",
    "navree_audit",
    "navree_auth",
    "navree_billing",
    "navree_bounded",
    "navree_browser",
    "navree_cache",
    "navree_cli",
    "navree_config",
    "navree_contracts",
    "navree_crypto",
    "navree_db",
    "navree_directory",
    "navree_dock",
    "navree_documents",
    "navree_domains",
    "navree_events",
    "navree_flags",
    "navree_fleet",
    "navree_glob",
    "navree_graphql",
    "navree_grpc",
    "navree_harbor",
    "navree_helm",
    "navree_http",
    "navree_http_client",
    "navree_i18n",
    "navree_integrations",
    "navree_kernel",
    "navree_lighthouse",
    "navree_log",
    "navree_logbook",
    "navree_mail",
    "navree_markdown",
    "navree_mcp",
    "navree_media",
    "navree_micro",
    "navree_mongo",
    "navree_native",
    "navree_native_core",
    "navree_navigator",
    "navree_notify",
    "navree_oauth",
    "navree_observe",
    "navree_openapi",
    "navree_orm",
    "navree_pdf",
    "navree_phone",
    "navree_queue",
    "navree_realtime",
    "navree_redis",
    "navree_schedule",
    "navree_schema",
    "navree_search",
    "navree_session",
    "navree_settings",
    "navree_sheets",
    "navree_social",
    "navree_sso",
    "navree_storage",
    "navree_telemetry",
    "navree_template",
    "navree_template_peak",
    "navree_testing",
    "navree_time",
    "navree_ui",
    "navree_validation",
    "navree_view",
    "navree_webhooks",
    "navree_workflows",
    "navree_workspace",
];

/// Whether `type_name`, read through wrappers and trait objects
/// ([`module_path`]) and up to its first angle bracket, is a path of one of
/// [`FRAMEWORK_CRATES`].
pub(crate) fn is_framework_crate(type_name: &str) -> bool {
    let read = module_path(type_name);
    let outer = read.split_once('<').map_or(read, |(outer, _)| outer);
    let Some((krate, _)) = outer.split_once("::") else {
        return false;
    };
    FRAMEWORK_CRATES.binary_search(&krate).is_ok()
}

/// The crate stem of a module value's `type_name`: peeled through wrappers and
/// trait objects ([`module_path`]), and through the kernel's
/// [`ConfiguredModule`](crate::ConfiguredModule) wrapper to its first type
/// parameter's crate stem, so a plain module and its `ConfiguredModule` form
/// share one crate stem. `None` when the path names an erased `dyn Module` or
/// has no crate segment.
pub(crate) fn module_crate(type_name: &str) -> Option<&str> {
    let read = module_path(type_name);
    let target = match read.strip_prefix("navree_kernel::module::ConfiguredModule<") {
        Some(rest) => module_path(first_type_arg(rest)),
        None => read,
    };
    if target.starts_with("dyn ") {
        return None;
    }
    let outer = target.split_once('<').map_or(target, |(outer, _)| outer);
    outer.split_once("::").map(|(krate, _)| krate)
}

/// The first generic type argument before a top-level `,` or closing `>` in
/// `s`, ignoring `,` and `>` inside nested brackets and ignoring `->`.
fn first_type_arg(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut angles = 0_usize;
    let mut parens = 0_usize;
    let mut brackets = 0_usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => angles += 1,
            b'>' if i > 0 && bytes[i - 1] == b'-' => {}
            b'>' if angles == 0 && parens == 0 && brackets == 0 => {
                return s[..i].trim();
            }
            b'>' => angles = angles.saturating_sub(1),
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b',' if angles == 0 && parens == 0 && brackets == 0 => {
                return s[..i].trim();
            }
            _ => {}
        }
        i += 1;
    }
    s.trim()
}

/// The namespaces the framework keeps for itself, sorted: a plugin that is
/// not first-party may not use one. They are the first words of the
/// framework's configuration sections, of its permission prefixes and of the
/// permissions it declares, a few words of its own, the folders the
/// framework reads below the plugins' folders (`layouts` for templates,
/// `fonts` for documents), and the Windows device
/// names a namespace could spell (`con`, `prn`, `aux`, `nul`, `com0` to
/// `com9`, `lpt0` to `lpt9`), which no folder may be called. Not public API;
/// Navree's own crates and tests use it.
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

/// The providers that decide who may do what in the app, the app's shell, its
/// clock, the observers of every commit, the sink of its audit records, what
/// carries its broadcasts and what silences its model listeners, by type
/// name: a module of a plugin that is not first-party may not register one
/// (value, factory or multi-binding, named or not, exported or not). Not
/// public API; Navree's own crates and tests use it.
pub const APP_ONLY_PROVIDERS: &[&str] = &[
    "dyn navree_access::workspace_gate::WorkspaceGate",
    "dyn navree_auth::strategy::Strategy",
    "dyn navree_contracts::audit::AuditSink",
    "dyn navree_contracts::clock::Clock",
    "dyn navree_contracts::permissions::PermissionCheck",
    "dyn navree_db::uow::ChangeObserver",
    "dyn navree_http::auth::Authenticator",
    "dyn navree_http::auth::RouteAuthorizer",
    "dyn navree_mongo::uow::MongoChangeObserver",
    "dyn navree_realtime::backplane::Backplane",
    "dyn navree_ui::shell::contribute::NativeShellPart",
    "navree_access::factory::AbilityFactory",
    "navree_access::options::AccessOptions",
    "navree_auth::options::AuthOptions",
    "navree_helm::options::HelmOptions",
    "navree_orm::events::fake::ModelEventFake",
    "navree_ui::shell::Shell",
    "navree_workspace::config::WorkspaceConfig",
];

/// What a provider of [`APP_ONLY_PROVIDERS`] does in the app, as the clause
/// after its name in the text that refuses a plugin's.
pub(crate) fn app_only_role(provider: &str) -> &'static str {
    match provider {
        "dyn navree_access::workspace_gate::WorkspaceGate" => {
            "which decides who enters the app's workspaces"
        }
        "dyn navree_auth::strategy::Strategy" => "which signs requests in",
        "dyn navree_contracts::audit::AuditSink" => "which receives the app's audit records",
        "dyn navree_contracts::clock::Clock" => {
            "which sets the time the app's sessions, signed links and invitations expire by"
        }
        "dyn navree_contracts::permissions::PermissionCheck" => {
            "which answers the app's permission checks"
        }
        "dyn navree_db::uow::ChangeObserver" => {
            "which sees, and can change, every write the app commits"
        }
        "dyn navree_http::auth::Authenticator" => "which decides who a request is signed in as",
        "dyn navree_http::auth::RouteAuthorizer" => "which decides who may open the app's routes",
        "dyn navree_mongo::uow::MongoChangeObserver" => {
            "which sees, and can change, every MongoDB write the app commits"
        }
        "dyn navree_realtime::backplane::Backplane" => {
            "which carries every broadcast of the app's channels between its processes"
        }
        "dyn navree_ui::shell::contribute::NativeShellPart" => {
            "which says where the app's native clients sign in"
        }
        "navree_access::factory::AbilityFactory" => "which builds every user's abilities",
        "navree_access::options::AccessOptions" => {
            "which sets the options of the app's access rules"
        }
        "navree_auth::options::AuthOptions" => "which sets the options of the app's sign-in",
        "navree_helm::options::HelmOptions" => "which sets the options of the app's admin panel",
        "navree_orm::events::fake::ModelEventFake" => {
            "which silences the listeners of every model of the app"
        }
        "navree_ui::shell::Shell" => "which is the app's shell",
        "navree_workspace::config::WorkspaceConfig" => {
            "which sets the options of the app's workspaces"
        }
        _ => OTHER_ROLE,
    }
}

/// [`app_only_role`] of a type name it does not know.
const OTHER_ROLE: &str = "which decides who may do what in the app";

/// How the app uses a plugin's provider of [`APP_ONLY_PROVIDERS`], as the
/// last clause of the text that refuses the plugin's own registration: a
/// module of the app registers it. An app has one audit sink, so a plugin's
/// is for an app that has no other: beside navree-audit's `AuditModule` a
/// second sink is either not the one the app's modules find, or makes every
/// lookup of the sink ambiguous, and then no audit record is written.
pub(crate) fn app_only_advice(provider: &str) -> &'static str {
    match provider {
        "dyn navree_contracts::audit::AuditSink" => {
            "and an app has one audit sink: to use the plugin's, register it from a module of an app that has no other (it cannot be used beside navree-audit's AuditModule)"
        }
        _ => "and to use the plugin's, register it from a module of the app",
    }
}

/// The start of the type name of a model hook's provider key,
/// `dyn ModelHook<M>` of navree-orm, the model's type path following it.
/// Not public API; Navree's own crates and tests use it.
pub const MODEL_HOOK_KEY: &str = "dyn navree_orm::hooks::ModelHook<";

/// Whether `type_name` names a path of the crate `crate_name`: `<crate_name>::`
/// at its start or right after `<`, `,`, a space, `(`, `[`, `&` or `*`, so the
/// type itself and its generic arguments count, and `x_navree_plugin_a::T`
/// does not count for `navree_plugin_a`.
pub(crate) fn names_crate(type_name: &str, crate_name: &str) -> bool {
    let path = format!("{crate_name}::");
    type_name.match_indices(path.as_str()).any(|(at, _)| {
        at == 0
            || matches!(
                type_name.as_bytes()[at - 1],
                b'<' | b',' | b' ' | b'(' | b'[' | b'&' | b'*'
            )
    })
}

/// The type a module's type name stands for, read through a reference or a
/// pointer (`&T`, `&mut T`, `*const T`, `*mut T`) and inside `Box<..>` and
/// `Pin<..>`, at any depth: a module typed `&'static Helper` or
/// `Box<Helper>` is read as `Helper`, so its crate is `Helper`'s. A trait
/// object behind them is read as its trait ([`object_trait`]): a module typed
/// `&'static dyn Part` is read as `Part`, so its crate is the trait's.
pub(crate) fn module_path(type_name: &str) -> &str {
    let mut path = type_name.trim();
    loop {
        let current = path;
        let behind = ["&mut ", "&", "*const ", "*mut "]
            .iter()
            .find_map(move |marker| current.strip_prefix(*marker));
        let inside = ["alloc::boxed::Box<", "core::pin::Pin<"]
            .iter()
            .find_map(move |wrapper| current.strip_prefix(*wrapper)?.strip_suffix('>'));
        // A trait object with more bounds than one, written in parentheses.
        let grouped = current
            .strip_prefix('(')
            .and_then(|inner| inner.strip_suffix(')'));
        match behind.or(inside).or(grouped) {
            Some(inner) => path = inner.trim(),
            None => return object_trait(path),
        }
    }
}

/// The trait a trait object's type name stands for: `dyn x::Part`, a
/// higher-ranked `dyn for.. x::Part`, and `dyn x::Part + core::marker::Sync`
/// are `x::Part` (with any generic arguments on `Part`), a path of the trait's
/// crate, which is the only crate that can make such a type a module. The
/// kernel's own `dyn Module` stands for a module of any crate, so it is
/// returned as it is, with no crate to read; and so is a name that is no
/// trait object.
fn object_trait(path: &str) -> &str {
    let Some(object) = path.strip_prefix("dyn ") else {
        return path;
    };
    let unquantified = strip_for_binder(object.trim_start());
    let principal = principal_trait(unquantified);
    let any_module = std::any::type_name::<dyn crate::Module>();
    if any_module.strip_prefix("dyn ") == Some(principal) {
        path
    } else {
        principal
    }
}

/// Strips a leading higher-ranked `for` binder from `s`, matching nested angle
/// brackets while ignoring `->` in function types.
fn strip_for_binder(s: &str) -> &str {
    let Some(after_for) = s.strip_prefix("for") else {
        return s;
    };
    let trimmed = after_for.trim_start();
    if !trimmed.starts_with('<') {
        return s;
    }
    let bytes = trimmed.as_bytes();
    let mut depth = 0_usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' if i > 0 && bytes[i - 1] == b'-' => {}
            b'>' => {
                depth -= 1;
                if depth == 0 {
                    return trimmed[i + 1..].trim_start();
                }
            }
            _ => {}
        }
        i += 1;
    }
    s
}

/// The principal trait before any top-level ` + ` auto-trait bounds in `s`,
/// ignoring ` + ` inside angle brackets, parentheses or square brackets, and
/// ignoring `->` when counting angle brackets.
fn principal_trait(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut angles = 0_usize;
    let mut parens = 0_usize;
    let mut brackets = 0_usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => angles += 1,
            b'>' if i > 0 && bytes[i - 1] == b'-' => {}
            b'>' => angles = angles.saturating_sub(1),
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b' ' if angles == 0
                && parens == 0
                && brackets == 0
                && bytes[i..].starts_with(b" + ") =>
            {
                return s[..i].trim();
            }
            _ => {}
        }
        i += 1;
    }
    s.trim()
}

/// Whether the plugin `info`, whose modules have the types `type_names`, is
/// first-party: `listed` pairs its name with a crate, it names that crate as
/// its own, its version is `kernel_version`, and every module type, up to its
/// first `<` and read through a reference ([`module_path`]), is a path of
/// that crate (generic arguments do not count).
///
/// It reads names: a package whose library is named like a listed crate meets
/// it, as `std::any::type_name` prints the library's name. The plugin rules
/// hold a plugin to what it declares and are not a sandbox: a crate linked
/// into the app runs with the app's rights.
pub(crate) fn is_first_party(
    info: &PluginInfo,
    type_names: &[&str],
    listed: &[(&str, &str)],
    kernel_version: &str,
) -> bool {
    let Some(&(_, krate)) = listed.iter().find(|(package, _)| *package == info.name) else {
        return false;
    };
    let path = format!("{krate}::");
    info.crate_name == krate
        && info.version == kernel_version
        && type_names.iter().all(|type_name| {
            let read = module_path(type_name);
            let outer = read.split_once('<').map_or(read, |(outer, _)| outer);
            outer.starts_with(path.as_str())
        })
}

/// Whether `bytes` starts with `prefix` (`<[u8]>::starts_with` in a `const fn`).
const fn starts_with(bytes: &[u8], prefix: &[u8]) -> bool {
    if bytes.len() < prefix.len() {
        return false;
    }
    let mut index = 0;
    while index < prefix.len() {
        if bytes[index] != prefix[index] {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_of(package: &str) -> String {
        let (bytes, len) = default_namespace::<64>(package);
        String::from_utf8(bytes[..len].to_vec()).expect("UTF-8")
    }

    #[test]
    fn the_default_namespace_drops_the_navree_prefixes_and_turns_dashes_into_underscores() {
        for (package, namespace) in [
            ("navree-plugin-announcements", "announcements"),
            ("navree-billing", "billing"),
            ("navree-http-client", "http_client"),
            ("acme-crm", "acme_crm"),
            ("navree-plugin-notice-board", "notice_board"),
            ("navree", "navree"),
            ("navree_plugin_x", "navree_plugin_x"),
            ("navree-plugin-2fa", "2fa"),
            ("navree-plugin-", ""),
        ] {
            assert_eq!(default_of(package), namespace, "{package}");
        }
        assert!(!is_namespace("2fa") && !is_namespace(""));
    }

    #[test]
    fn the_namespace_check_agrees_with_the_grammar() {
        let longest = "a".repeat(64);
        let too_long = "a".repeat(65);
        for good in [
            "a",
            "announcements",
            "notice_board",
            "a1",
            "ludhiana_2026",
            longest.as_str(),
        ] {
            assert!(is_namespace(good), "{good}");
        }
        for bad in [
            "",
            "1a",
            "_a",
            "A",
            "Announcements",
            "a-b",
            "a b",
            "a.b",
            "ਸੂਚਨਾ",
            too_long.as_str(),
        ] {
            assert!(!is_namespace(bad), "{bad}");
        }
    }

    #[test]
    fn a_type_path_counts_for_a_crate_only_on_a_segment_boundary() {
        assert!(names_crate("navree_plugin_a::T", "navree_plugin_a"));
        assert!(!names_crate("x_navree_plugin_a::T", "navree_plugin_a"));
        assert!(!names_crate("navree_plugin_ab::T", "navree_plugin_a"));
        assert!(!names_crate(
            "a::B<x_navree_plugin_a::T>",
            "navree_plugin_a"
        ));
        assert!(names_crate(
            "navree_config::client::ClientModule<reqwest::Client, navree_plugin_github::GithubOptions>",
            "navree_plugin_github"
        ));
        for shape in [
            "a::B<navree_plugin_a::T>",
            "&navree_plugin_a::T",
            "(u8, navree_plugin_a::T)",
            "[navree_plugin_a::T; 1]",
            "*const navree_plugin_a::T",
        ] {
            assert!(names_crate(shape, "navree_plugin_a"), "{shape}");
        }
    }

    #[test]
    fn a_module_typed_as_a_reference_or_a_box_is_read_as_the_type_behind_it() {
        let helper = "navree_plugin_a::Helper";
        for type_name in [
            "navree_plugin_a::Helper",
            "&navree_plugin_a::Helper",
            "&mut navree_plugin_a::Helper",
            "*const navree_plugin_a::Helper",
            "alloc::boxed::Box<navree_plugin_a::Helper>",
            "&alloc::boxed::Box<core::pin::Pin<&navree_plugin_a::Helper>>",
        ] {
            assert_eq!(module_path(type_name), helper, "{type_name}");
        }
        // Another generic type is read as it is.
        assert_eq!(
            module_path("acme::Wrap<navree_plugin_a::Helper>"),
            "acme::Wrap<navree_plugin_a::Helper>"
        );
        // A trait object of a crate is read as its trait, with more bounds or without, in
        // parentheses or not, behind a reference, in a box or in a pin.
        for type_name in [
            "&dyn navree_plugin_a::Part",
            "&dyn navree_plugin_a::Part + core::marker::Sync",
            "&(dyn navree_plugin_a::Part + core::marker::Sync)",
            "alloc::boxed::Box<dyn navree_plugin_a::Part + core::marker::Send>",
            "core::pin::Pin<&dyn navree_plugin_a::Part>",
        ] {
            assert_eq!(
                module_path(type_name),
                "navree_plugin_a::Part",
                "{type_name}"
            );
        }
        // The kernel's own trait object is a module of any crate: it has no crate.
        assert_eq!(
            module_path("alloc::boxed::Box<dyn navree_kernel::module::Module>"),
            "dyn navree_kernel::module::Module"
        );
        assert_eq!(
            module_path(std::any::type_name::<Box<dyn crate::Module>>()),
            std::any::type_name::<dyn crate::Module>()
        );
        // Higher-ranked trait objects and nested ` + ` or `->` inside generic arguments or
        // function parentheses peel cleanly to the principal trait.
        assert_eq!(
            module_path("&dyn for<'a> navree_plugin_khata::Part<'a>"),
            "navree_plugin_khata::Part<'a>"
        );
        assert_eq!(
            module_path(
                "alloc::boxed::Box<dyn for<'a> navree_plugin_khata::Part<fn(&'a str) -> &'a str> + core::marker::Send + core::marker::Sync>"
            ),
            "navree_plugin_khata::Part<fn(&'a str) -> &'a str>"
        );
        assert_eq!(
            module_path(
                "&dyn for<'a> navree_plugin_khata::Call(&'a (dyn core::marker::Send + core::marker::Sync)) -> &'a str + core::marker::Sync"
            ),
            "navree_plugin_khata::Call(&'a (dyn core::marker::Send + core::marker::Sync)) -> &'a str"
        );
        assert_eq!(
            module_path("&dyn forecast::Part + core::marker::Sync"),
            "forecast::Part"
        );
        assert_eq!(
            module_path(
                "alloc::boxed::Box<dyn for<'a> navree_kernel::module::Module + core::marker::Send>"
            ),
            "dyn for<'a> navree_kernel::module::Module + core::marker::Send"
        );
        // First-party reads the module's type the same way.
        let listed = [("navree-flags", "navree_flags")];
        let flags = PluginInfo::new("navree-flags", "navree_flags", "flags", "1.3.0");
        let by_reference = ["&navree_flags::module::FlagsModule"];
        assert!(is_first_party(&flags, &by_reference, &listed, "1.3.0"));
    }

    #[test]
    fn first_party_needs_a_listed_name_its_crate_the_kernels_version_and_a_type_of_that_crate() {
        let listed = [("navree-flags", "navree_flags")];
        let flags = PluginInfo::new("navree-flags", "navree_flags", "flags", "1.3.0");
        let module = ["navree_flags::module::FlagsModule"];
        assert!(is_first_party(&flags, &module, &listed, "1.3.0"));
        assert!(
            !is_first_party(&flags, &module, &listed, "1.3.1"),
            "another version"
        );
        let elsewhere = PluginInfo::new("navree-flags", "acme_flags", "flags", "1.3.0");
        assert!(
            !is_first_party(&elsewhere, &module, &listed, "1.3.0"),
            "another crate named as the plugin's"
        );
        let unlisted = PluginInfo::new("navree-plugin-flags", "navree_flags", "flags", "1.3.0");
        assert!(
            !is_first_party(&unlisted, &module, &listed, "1.3.0"),
            "a name not on the list"
        );
        assert!(
            !is_first_party(
                &flags,
                &["acme::Wrap<navree_flags::module::FlagsModule>"],
                &listed,
                "1.3.0"
            ),
            "generic arguments do not count"
        );
        assert!(
            !is_first_party(
                &flags,
                &["navree_flags::module::FlagsModule", "acme::Extra"],
                &listed,
                "1.3.0"
            ),
            "every module of the plugin"
        );
        assert!(!is_first_party(
            &flags,
            &["navree_flags_extra::FlagsModule"],
            &listed,
            "1.3.0"
        ));
    }

    #[test]
    fn the_kernels_lists_are_sorted_and_well_formed() {
        assert_eq!(FIRST_PARTY.len(), 31);
        assert!(FIRST_PARTY.windows(2).all(|w| w[0].0 < w[1].0));
        for (package, krate) in FIRST_PARTY {
            assert_eq!(package.replace('-', "_"), *krate);
            assert!(FRAMEWORK_CRATES.binary_search(krate).is_ok(), "{krate}");
        }
        assert_eq!(FRAMEWORK_CRATES.len(), 74);
        assert!(FRAMEWORK_CRATES.windows(2).all(|w| w[0] < w[1]));
        assert!(is_framework_crate("navree_kernel::__private::PlainModule"));
        assert!(is_framework_crate("&navree_ui::module::UiModule"));
        assert!(is_framework_crate(
            "alloc::boxed::Box<navree_events::EventsModule>"
        ));
        assert!(!is_framework_crate(
            "alloc::boxed::Box<dyn navree_kernel::module::Module>"
        ));
        assert!(!is_framework_crate(
            "navree_plugin_khata_core::KhataCoreModule"
        ));
        assert!(
            std::any::type_name::<crate::ConfiguredModule<u8, ()>>()
                .starts_with("navree_kernel::module::ConfiguredModule<")
        );
        assert_eq!(
            module_crate("navree_ui::module::UiOnDemand"),
            Some("navree_ui")
        );
        assert_eq!(
            module_crate(
                "navree_kernel::module::ConfiguredModule<navree_cache::module::CacheModule, navree_cache::options::CacheOptions>"
            ),
            Some("navree_cache")
        );
        assert_eq!(
            module_crate(
                "navree_kernel::module::ConfiguredModule<plugins::Holder<fn(&str) -> &str>, ()>"
            ),
            Some("plugins")
        );
        assert_eq!(
            module_crate("alloc::boxed::Box<dyn navree_kernel::module::Module>"),
            None
        );
        assert_eq!(RESERVED_NAMESPACES.len(), 87);
        assert!(RESERVED_NAMESPACES.windows(2).all(|w| w[0] < w[1]));
        assert!(RESERVED_NAMESPACES.iter().all(|ns| is_namespace(ns)));
        // Sorted, each name once, each a type path.
        assert!(APP_ONLY_PROVIDERS.windows(2).all(|w| w[0] < w[1]));
        assert!(APP_ONLY_PROVIDERS.iter().all(|name| name.contains("::")));
        assert!(MODEL_HOOK_KEY.starts_with("dyn ") && MODEL_HOOK_KEY.ends_with('<'));
    }

    #[test]
    fn every_app_only_provider_says_what_it_does() {
        for provider in APP_ONLY_PROVIDERS {
            assert_ne!(app_only_role(provider), OTHER_ROLE, "{provider}");
        }
    }

    #[test]
    fn the_advice_for_the_audit_sink_says_an_app_has_one() {
        let registers = "and to use the plugin's, register it from a module of the app";
        for provider in APP_ONLY_PROVIDERS {
            let advice = app_only_advice(provider);
            if *provider == "dyn navree_contracts::audit::AuditSink" {
                assert_eq!(
                    advice,
                    "and an app has one audit sink: to use the plugin's, register it from a module of an app that has no other (it cannot be used beside navree-audit's AuditModule)"
                );
            } else {
                assert_eq!(advice, registers, "{provider}");
            }
        }
        // The refusal ends with the advice.
        let refused = crate::KernelError::PluginProvidesAppOnly {
            plugin: "navree-plugin-guard",
            provider: "dyn navree_contracts::audit::AuditSink",
            role: app_only_role("dyn navree_contracts::audit::AuditSink"),
            module: "Provides",
            reason: String::new(),
        }
        .to_string();
        assert_eq!(
            refused,
            "plugin `navree-plugin-guard` provides `dyn navree_contracts::audit::AuditSink` in `Provides`, which receives the app's audit records; only a module of the app registers it: remove the plugin or update it to a version that leaves it to the app, and an app has one audit sink: to use the plugin's, register it from a module of an app that has no other (it cannot be used beside navree-audit's AuditModule)"
        );
    }

    /// A namespace is a folder name (`resources/views/<namespace>/`): every word the path
    /// rule refuses as a Windows device, and that the namespace grammar allows, is reserved.
    #[test]
    fn every_namespace_that_names_a_windows_device_is_reserved() {
        let mut devices = Vec::new();
        for stem in ["con", "prn", "aux", "nul", "com", "lpt"] {
            for end in ["", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9"] {
                let word = format!("{stem}{end}");
                if is_namespace(&word) && !crate::publish::is_safe_relative_path(&word) {
                    assert!(
                        RESERVED_NAMESPACES.contains(&word.as_str()),
                        "{word} is not reserved"
                    );
                    devices.push(word);
                }
            }
        }
        assert_eq!(devices.len(), 24, "{devices:?}");
        for folder in ["layouts", "fonts"] {
            assert!(RESERVED_NAMESPACES.contains(&folder), "{folder}");
        }
    }

    #[test]
    fn publish_tags_name_their_folders_and_all_lists_three() {
        assert_eq!(
            PublishTag::ALL,
            &[PublishTag::Views, PublishTag::Locales, PublishTag::Files]
        );
        let names: Vec<&str> = PublishTag::ALL.iter().map(|tag| tag.as_str()).collect();
        assert_eq!(names, ["views", "locales", "files"]);
        for tag in PublishTag::ALL {
            assert_eq!(PublishTag::from_name(tag.as_str()), Some(*tag));
        }
        assert_eq!(PublishTag::from_name("config"), None);
        assert_eq!(PublishTag::from_name("Views"), None);
        assert_eq!(
            destination(PublishTag::Views, "announcements", "admin/list.html"),
            "resources/views/announcements/admin/list.html"
        );
        assert_eq!(
            destination(
                PublishTag::Locales,
                "announcements",
                "pa-IN/announcements.ftl",
            ),
            "locales/pa-IN/announcements/announcements.ftl"
        );
        assert_eq!(
            destination(PublishTag::Files, "announcements", "banner.css"),
            "resources/announcements/banner.css"
        );
        let files = Publishable::new(PublishTag::Views)
            .file("list.html", b"<ul></ul>")
            .file(String::from("show.html"), b"<p></p>");
        assert_eq!(files.tag(), PublishTag::Views);
        let listed: Vec<(&str, &[u8])> = files.files().collect();
        assert_eq!(
            listed,
            [
                ("list.html", &b"<ul></ul>"[..]),
                ("show.html", &b"<p></p>"[..]),
            ]
        );
    }
}

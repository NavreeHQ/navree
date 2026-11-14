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
/// their upper-case forms are equal). Under `locales` the first segment is
/// the locale.
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
/// package: a plugin is first-party when its name is here, its version is the
/// kernel's, and every module of it is a type of the listed crate. Not public
/// API; Navree's own crates and tests use it.
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

/// The namespaces the framework keeps for itself, sorted: a plugin that is
/// not first-party may not use one. Not public API; Navree's own crates and
/// tests use it.
pub const RESERVED_NAMESPACES: &[&str] = &[
    "ai",
    "all",
    "api",
    "app",
    "audit",
    "auth",
    "billing",
    "cache",
    "database",
    "directory",
    "documents",
    "domains",
    "env",
    "flags",
    "fleet",
    "graphql",
    "grpc",
    "hashing",
    "helm",
    "http",
    "http_client",
    "i18n",
    "integrations",
    "lighthouse",
    "log",
    "logbook",
    "mail",
    "mcp",
    "media",
    "micro",
    "navree",
    "notify",
    "oauth",
    "observe",
    "operations",
    "organization",
    "orm",
    "plugins",
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

/// Whether the plugin `info`, whose modules have the types `type_names`, is
/// first-party: `listed` pairs its name with a crate, its version is
/// `kernel_version`, and every module type, up to its first `<`, is a path of
/// that crate (generic arguments do not count).
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
    info.version == kernel_version
        && type_names.iter().all(|type_name| {
            let outer = type_name
                .split_once('<')
                .map_or(*type_name, |(outer, _)| outer);
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
    fn first_party_needs_a_listed_name_the_kernels_version_and_a_type_of_the_listed_crate() {
        let listed = [("navree-flags", "navree_flags")];
        let flags = PluginInfo::new("navree-flags", "navree_flags", "flags", "1.3.0");
        let module = ["navree_flags::module::FlagsModule"];
        assert!(is_first_party(&flags, &module, &listed, "1.3.0"));
        assert!(
            !is_first_party(&flags, &module, &listed, "1.3.1"),
            "another version"
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
        }
        assert_eq!(RESERVED_NAMESPACES.len(), 60);
        assert!(RESERVED_NAMESPACES.windows(2).all(|w| w[0] < w[1]));
        assert!(RESERVED_NAMESPACES.iter().all(|ns| is_namespace(ns)));
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

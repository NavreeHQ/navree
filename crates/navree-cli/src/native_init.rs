//! `navree native:init`: the project of the app's Apple client, for iPhone,
//! iPad and Mac, on the Swift package `navree-swift`.
//!
//! The values come from the command line, else from the app's settings
//! (`app.name`, `app.url`, the one `auth.apps.apple.<name>` table) and its
//! `Cargo.lock` (the `navree` version, which the package is pinned to). The
//! files are planned first ([`plan`]), then written without overwriting
//! anything.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde_json::Value;

use crate::native_doctor::Lookup;
use crate::scaffold::FileSet;
use crate::templates::TemplateError;

/// The Swift package's repository, which each release of Navree makes.
pub const SWIFT_PACKAGE_URL: &str = "https://github.com/NavreeHQ/navree-swift";

/// The client a project is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum InitPlatform {
    /// iPhone, iPad and Mac, on the Swift package.
    #[default]
    Apple,
}

/// What `navree native:init` was asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitOptions {
    /// The client.
    pub platform: InitPlatform,
    /// The app's name as people see it.
    pub name: Option<String>,
    /// The server the app talks to.
    pub origin: Option<String>,
    /// The bundle id.
    pub bundle_id: Option<String>,
    /// The ten-character Team ID.
    pub team: Option<String>,
    /// The project's folder, from the app's root (default `apple`).
    pub dir: Option<PathBuf>,
    /// The Swift package's repository.
    pub package: Option<String>,
    /// The Swift package's version.
    pub package_version: Option<String>,
    /// A local Swift package instead (`packages/apple` of a checkout).
    pub package_path: Option<PathBuf>,
    /// Print the files instead of writing them.
    pub dry_run: bool,
}

/// An Apple app the server's settings know (`auth.apps.apple.<table>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownApp {
    /// The table's name.
    pub table: String,
    /// Its `team_id`.
    pub team_id: String,
    /// Its `bundle_id`.
    pub bundle_id: String,
}

/// What the app says about itself; empty outside an app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppFacts {
    /// `app.name`.
    pub name: Option<String>,
    /// `app.url`.
    pub origin: Option<String>,
    /// `auth.apps.apple`.
    pub apple: Vec<KnownApp>,
    /// The `navree` version in `Cargo.lock`.
    pub navree_version: Option<String>,
}

/// Where the project takes the Swift package from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageSource {
    /// The package's repository, up to the next minor version.
    Remote {
        /// The repository.
        url: String,
        /// The version.
        version: String,
    },
    /// A folder on this machine, as written in `project.yml`.
    Local {
        /// The folder, relative to the project when it can be.
        path: String,
    },
}

/// Everything the project's files are written from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleProject {
    /// The app's name as people see it.
    pub display_name: String,
    /// The targets' and the app type's name: letters and digits.
    pub type_name: String,
    /// The URL scheme a sign-in comes back on.
    pub scheme: String,
    /// `scheme://host[:port]`.
    pub origin: String,
    /// The origin is plain http to this machine.
    pub loopback: bool,
    /// The origin's host, for associated domains; empty for loopback.
    pub domain: String,
    /// The bundle id.
    pub bundle_id: String,
    /// The bundle id is a placeholder to change.
    pub bundle_id_assumed: bool,
    /// The Team ID, or empty.
    pub team: String,
    /// The Swift package.
    pub package: PackageSource,
}

/// What the app's settings and its `Cargo.lock` say.
pub fn facts_from(settings: &Lookup, lock: Option<&str>) -> AppFacts {
    let text_at = |key: &str| {
        settings(key)
            .and_then(|value| value.as_str().map(|text| text.trim().to_owned()))
            .filter(|text| !text.is_empty())
    };
    let apple = match settings("auth.apps.apple") {
        Some(Value::Object(tables)) => tables
            .into_iter()
            .map(|(table, app)| KnownApp {
                table,
                team_id: crate::native_doctor::text(&app, "team_id"),
                bundle_id: crate::native_doctor::text(&app, "bundle_id"),
            })
            .collect(),
        _ => Vec::new(),
    };
    AppFacts {
        name: text_at("app.name"),
        origin: text_at("app.url"),
        apple,
        navree_version: lock.and_then(navree_in_lock),
    }
}

/// The `navree` package's version in a `Cargo.lock`.
fn navree_in_lock(lock: &str) -> Option<String> {
    let lock: toml::Table = lock.parse().ok()?;
    lock.get("package")?
        .as_array()?
        .iter()
        .find(|package| package.get("name").and_then(|name| name.as_str()) == Some("navree"))?
        .get("version")?
        .as_str()
        .map(str::to_owned)
}

/// [`facts_from`] for the app in `root`: what loads, the rest empty.
pub fn app_facts(root: &Path) -> AppFacts {
    let lock = root
        .ancestors()
        .find_map(|dir| std::fs::read_to_string(dir.join("Cargo.lock")).ok());
    match crate::native_doctor::load_settings(root) {
        Ok(settings) => facts_from(settings.as_ref(), lock.as_deref()),
        Err(_) => facts_from(&|_| None, lock.as_deref()),
    }
}

/// The project for `options`, filled in from `facts`. `cwd` is where a
/// relative `--package-path` starts; `project_dir` is where the project goes.
pub fn resolve(
    options: &InitOptions,
    facts: &AppFacts,
    cwd: &Path,
    project_dir: &Path,
) -> anyhow::Result<AppleProject> {
    let InitPlatform::Apple = options.platform;
    let display_name = options
        .name
        .as_deref()
        .or(facts.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .context("the app's name is not known: pass --name (the app's settings have no app.name)")?
        .to_owned();
    anyhow::ensure!(
        !display_name.chars().any(char::is_control),
        "--name cannot hold a line break or another control character"
    );
    let type_name = type_name(&display_name)?;
    let scheme = type_name.to_ascii_lowercase();
    let origin = options
        .origin
        .as_deref()
        .or(facts.origin.as_deref())
        .context(
            "the app's origin is not known: pass --origin (the app's settings have no app.url)",
        )?;
    let origin = Origin::parse(origin)?;

    let (bundle_id, app_team, bundle_id_assumed) = match (
        &options.bundle_id,
        facts.apple.as_slice(),
    ) {
        (Some(id), apps) => {
            let team = apps
                .iter()
                .find(|app| &app.bundle_id == id)
                .map(|app| app.team_id.clone());
            (id.clone(), team.unwrap_or_default(), false)
        }
        (None, [app]) => (app.bundle_id.clone(), app.team_id.clone(), false),
        (None, []) => (format!("com.example.{scheme}"), String::new(), true),
        (None, apps) => anyhow::bail!(
            "the app's settings have {} Apple apps ({}): pass --bundle-id with the one this project is for",
            apps.len(),
            apps.iter()
                .map(|app| format!("auth.apps.apple.{}", app.table))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    anyhow::ensure!(
        crate::native_doctor::is_bundle_id(&bundle_id),
        "`{bundle_id}` is not a bundle id: A-Z, a-z, 0-9, `-` and `.`, such as in.example.dukaan"
    );
    let team = options.team.clone().unwrap_or(app_team);
    anyhow::ensure!(
        team.is_empty() || crate::native_doctor::is_team_id(&team),
        "`{team}` is not a Team ID: ten characters, A-Z and 0-9"
    );

    let package = match &options.package_path {
        Some(path) => PackageSource::Local {
            path: local_path(path, cwd, project_dir),
        },
        None => {
            let version = options
                .package_version
                .clone()
                .or_else(|| facts.navree_version.clone())
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
            anyhow::ensure!(
                is_version(&version),
                "`{version}` is not a version: pass --package-version as 1.5.0"
            );
            PackageSource::Remote {
                url: options
                    .package
                    .clone()
                    .unwrap_or_else(|| SWIFT_PACKAGE_URL.to_owned()),
                version,
            }
        }
    };

    Ok(AppleProject {
        display_name,
        type_name,
        scheme,
        origin: origin.text,
        loopback: origin.loopback,
        domain: origin.domain,
        bundle_id,
        bundle_id_assumed,
        team,
        package,
    })
}

/// The name's ASCII letter-and-digit words, each starting upper-case.
fn type_name(name: &str) -> anyhow::Result<String> {
    let joined: String = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            let first = chars.next().map(|c| c.to_ascii_uppercase());
            first.into_iter().chain(chars).collect::<String>()
        })
        .collect();
    match joined.chars().next() {
        Some(first) if first.is_ascii_alphabetic() => Ok(joined),
        _ => anyhow::bail!(
            "`{name}` gives no name for the app's code (ASCII letters and digits, starting with a letter): pass --name, such as --name Dukaan"
        ),
    }
}

/// `1.5.0` or `1.5.0-rc.1`.
fn is_version(text: &str) -> bool {
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        && pre.is_none_or(|pre| {
            !pre.is_empty()
                && pre
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
}

/// An origin the app can talk to.
struct Origin {
    /// `scheme://host[:port]`.
    text: String,
    /// Plain http to this machine.
    loopback: bool,
    /// The host of an https origin, else empty.
    domain: String,
}

impl Origin {
    fn parse(given: &str) -> anyhow::Result<Self> {
        let given = given.trim();
        let refused = || {
            anyhow::anyhow!(
                "`{given}` is not an origin the app can talk to: a device app talks to its site over https (https://shop.example), or, for development, over http to this machine (http://127.0.0.1:8080)"
            )
        };
        let (scheme, rest) = given.split_once("://").ok_or_else(refused)?;
        let scheme = scheme.to_ascii_lowercase();
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.is_empty() || authority.contains('@') {
            return Err(refused());
        }
        let (host, port) = match authority.strip_prefix('[') {
            Some(inside) => {
                let (address, tail) = inside.split_once(']').ok_or_else(refused)?;
                if address.is_empty() || !address.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
                {
                    return Err(refused());
                }
                let port = match tail {
                    "" => None,
                    tail => Some(tail.strip_prefix(':').ok_or_else(refused)?),
                };
                (format!("[{}]", address.to_ascii_lowercase()), port)
            }
            None => {
                let (host, port) = match authority.rsplit_once(':') {
                    Some((host, port)) => (host, Some(port)),
                    None => (authority, None),
                };
                let host = host.to_ascii_lowercase();
                if host.is_empty()
                    || host.starts_with(['.', '-'])
                    || host.ends_with('.')
                    || !host
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
                {
                    return Err(refused());
                }
                (host, port)
            }
        };
        if let Some(port) = port
            && port.parse::<u16>().map_or(true, |port| port == 0)
        {
            return Err(refused());
        }
        let loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
        match scheme.as_str() {
            "https" => {}
            "http" if loopback => {}
            _ => return Err(refused()),
        }
        let text = match port {
            Some(port) => format!("{scheme}://{host}:{port}"),
            None => format!("{scheme}://{host}"),
        };
        let domain = if scheme == "https" {
            host
        } else {
            String::new()
        };
        Ok(Self {
            text,
            loopback: scheme == "http",
            domain,
        })
    }
}

/// `given` (from `cwd` when relative) as `project.yml` writes it: relative to
/// `project_dir` when the two share a folder, with `/` separators.
fn local_path(given: &Path, cwd: &Path, project_dir: &Path) -> String {
    let target = lexical(&cwd.join(given));
    let base = lexical(&cwd.join(project_dir));
    let target_parts: Vec<_> = target.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    let common = target_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let shared = target_parts
        .iter()
        .take(common)
        .any(|part| matches!(part, std::path::Component::Normal(_)));
    if !shared && target.is_absolute() {
        return slashed(&target);
    }
    let mut parts: Vec<String> = vec!["..".to_owned(); base_parts.len() - common];
    parts.extend(
        target_parts[common..]
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

/// `path` with `.` and `..` worked out from the text alone.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path` with `/` separators.
fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Each template and the file it becomes (`{type}` is the type name).
const FILES: [(&str, &str); 11] = [
    ("native/apple/project.yml", "project.yml"),
    ("native/apple/App.swift", "{type}/App.swift"),
    (
        "native/apple/Contents.json",
        "{type}/Assets.xcassets/Contents.json",
    ),
    (
        "native/apple/AppIcon.json",
        "{type}/Assets.xcassets/AppIcon.appiconset/Contents.json",
    ),
    (
        "native/apple/AccentColor.json",
        "{type}/Assets.xcassets/AccentColor.colorset/Contents.json",
    ),
    ("native/apple/Signing.xcconfig", "Signing/Signing.xcconfig"),
    (
        "native/apple/Signing.local.xcconfig.example",
        "Signing/Signing.local.xcconfig.example",
    ),
    ("native/apple/iOS.entitlements", "Signing/iOS.entitlements"),
    ("native/apple/Mac.entitlements", "Signing/Mac.entitlements"),
    ("native/apple/gitignore", ".gitignore"),
    ("native/apple/README.md", "README.md"),
];

/// What the templates are rendered with.
#[derive(serde::Serialize)]
struct Context<'a> {
    display_name: &'a str,
    /// The name as a quoted YAML string.
    display_name_yaml: String,
    type_name: &'a str,
    scheme: &'a str,
    origin: &'a str,
    loopback: bool,
    domain: &'a str,
    bundle_id: &'a str,
    team: &'a str,
    package_url: Option<&'a str>,
    package_version: Option<&'a str>,
    package_path: Option<&'a str>,
}

/// The project's files.
pub fn plan(project: &AppleProject) -> Result<FileSet, TemplateError> {
    let (package_url, package_version, package_path) = match &project.package {
        PackageSource::Remote { url, version } => {
            (Some(url.as_str()), Some(version.as_str()), None)
        }
        PackageSource::Local { path } => (None, None, Some(path.as_str())),
    };
    let context = Context {
        display_name: &project.display_name,
        display_name_yaml: serde_json::to_string(&project.display_name)
            .unwrap_or_else(|_| "\"\"".to_owned()),
        type_name: &project.type_name,
        scheme: &project.scheme,
        origin: &project.origin,
        loopback: project.loopback,
        domain: &project.domain,
        bundle_id: &project.bundle_id,
        team: &project.team,
        package_url,
        package_version,
        package_path,
    };
    let mut files = FileSet::new();
    for (template, path) in FILES {
        files.add(
            path.replace("{type}", &project.type_name),
            crate::templates::render(template, &context)?,
        );
    }
    Ok(files)
}

/// Runs `navree native:init` in `cwd`: the project goes in `--dir` (default
/// `apple`) beside the app's `Cargo.toml`, or in `cwd` outside an app.
pub fn run_init(
    cwd: &Path,
    options: &InitOptions,
    out: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let (root, facts) = match crate::project::project_root(cwd) {
        Some(root) => {
            let facts = app_facts(&root);
            (root, facts)
        }
        None => (cwd.to_path_buf(), AppFacts::default()),
    };
    let project_dir = root.join(options.dir.as_deref().unwrap_or(Path::new("apple")));
    let project = resolve(options, &facts, cwd, &project_dir)?;
    let files = plan(&project)?;
    if options.dry_run {
        write!(out, "{files}")?;
        return Ok(());
    }
    let written = files.write(&project_dir)?;
    let shown = |path: &Path| slashed(path.strip_prefix(&root).unwrap_or(path));
    for path in &written {
        writeln!(out, "  wrote    {}", shown(path))?;
    }
    let dir = shown(&project_dir);
    writeln!(out, "  next     brew install xcodegen   (once)")?;
    writeln!(
        out,
        "  next     cd {dir} && xcodegen generate && open {}.xcodeproj",
        project.type_name
    )?;
    if project.bundle_id_assumed {
        writeln!(
            out,
            "  note     {} is a placeholder bundle id: set NAVREE_BUNDLE_ID in {dir}/Signing/Signing.xcconfig before signing",
            project.bundle_id
        )?;
    }
    if let PackageSource::Remote { url, version } = &project.package {
        writeln!(
            out,
            "  note     the client is the Swift package {url} {version}, up to the next minor version"
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn flags(name: &str, origin: &str) -> InitOptions {
        InitOptions {
            name: Some(name.to_owned()),
            origin: Some(origin.to_owned()),
            ..InitOptions::default()
        }
    }

    fn project(options: &InitOptions, facts: &AppFacts) -> anyhow::Result<AppleProject> {
        resolve(
            options,
            facts,
            Path::new("/w/app"),
            Path::new("/w/app/apple"),
        )
    }

    fn error(options: &InitOptions, facts: &AppFacts) -> String {
        project(options, facts).unwrap_err().to_string()
    }

    fn dukaan() -> AppFacts {
        AppFacts {
            name: Some("Dukaan".to_owned()),
            origin: Some("https://dukaan.example".to_owned()),
            apple: vec![KnownApp {
                table: "dukaan".to_owned(),
                team_id: "ABCDE12345".to_owned(),
                bundle_id: "in.example.dukaan".to_owned(),
            }],
            navree_version: Some("1.4.0".to_owned()),
        }
    }

    fn lookup(config: Value) -> impl Fn(&str) -> Option<Value> {
        move |key: &str| {
            key.split('.')
                .try_fold(&config, |value, part| value.get(part))
                .cloned()
        }
    }

    #[test]
    fn names_become_a_type_and_a_scheme() {
        let made = project(
            &flags("Charkha Bar", "https://charkha.example"),
            &AppFacts::default(),
        )
        .unwrap();
        assert_eq!(made.display_name, "Charkha Bar");
        assert_eq!(made.type_name, "CharkhaBar");
        assert_eq!(made.scheme, "charkhabar");
        let made = project(
            &flags("my-shop", "https://shop.example"),
            &AppFacts::default(),
        )
        .unwrap();
        assert_eq!(made.type_name, "MyShop");
        assert_eq!(made.scheme, "myshop");
    }

    #[test]
    fn names_without_letters_or_starting_with_a_digit_are_refused() {
        for name in ["ਦੁਕਾਨ", "2go", "  "] {
            let message = error(&flags(name, "https://shop.example"), &AppFacts::default());
            assert!(message.contains("--name"), "{name}: {message}");
        }
    }

    #[test]
    fn origins_are_normalized() {
        let made = project(
            &flags("Dukaan", "https://Shop.Example/orders?x=1"),
            &AppFacts::default(),
        )
        .unwrap();
        assert_eq!(made.origin, "https://shop.example");
        assert_eq!(made.domain, "shop.example");
        assert!(!made.loopback);
        let made = project(
            &flags("Dukaan", "https://shop.example:8443/"),
            &AppFacts::default(),
        )
        .unwrap();
        assert_eq!(made.origin, "https://shop.example:8443");
        assert_eq!(made.domain, "shop.example");
    }

    #[test]
    fn loopback_http_is_allowed_and_other_http_refused() {
        for (given, origin) in [
            ("http://localhost:8080", "http://localhost:8080"),
            ("http://127.0.0.1:8095/", "http://127.0.0.1:8095"),
            ("http://[::1]:3000", "http://[::1]:3000"),
        ] {
            let made = project(&flags("Dukaan", given), &AppFacts::default()).unwrap();
            assert_eq!(made.origin, origin);
            assert!(made.loopback, "{given}");
            assert_eq!(made.domain, "", "{given}");
        }
        for given in [
            "http://192.168.0.5",
            "http://shop.example",
            "ftp://shop.example",
            "shop.example",
        ] {
            let message = error(&flags("Dukaan", given), &AppFacts::default());
            assert!(message.contains("https"), "{given}: {message}");
        }
    }

    #[test]
    fn the_apps_settings_fill_the_defaults() {
        let made = project(&InitOptions::default(), &dukaan()).unwrap();
        assert_eq!(made.display_name, "Dukaan");
        assert_eq!(made.origin, "https://dukaan.example");
        assert_eq!(made.bundle_id, "in.example.dukaan");
        assert_eq!(made.team, "ABCDE12345");
        assert!(!made.bundle_id_assumed);
    }

    #[test]
    fn two_apple_apps_need_a_bundle_id() {
        let mut facts = dukaan();
        facts.apple.push(KnownApp {
            table: "dukaan_staff".to_owned(),
            team_id: "FGHIJ67890".to_owned(),
            bundle_id: "in.example.dukaan.staff".to_owned(),
        });
        let message = error(&InitOptions::default(), &facts);
        assert!(message.contains("--bundle-id"), "{message}");
        assert!(message.contains("auth.apps.apple.dukaan"), "{message}");
        assert!(
            message.contains("auth.apps.apple.dukaan_staff"),
            "{message}"
        );
        let options = InitOptions {
            bundle_id: Some("in.example.dukaan.staff".to_owned()),
            ..InitOptions::default()
        };
        let made = project(&options, &facts).unwrap();
        assert_eq!(made.team, "FGHIJ67890");
    }

    #[test]
    fn no_bundle_id_anywhere_is_a_placeholder() {
        let made = project(
            &flags("Dukaan", "https://dukaan.example"),
            &AppFacts::default(),
        )
        .unwrap();
        assert_eq!(made.bundle_id, "com.example.dukaan");
        assert!(made.bundle_id_assumed);
        assert_eq!(made.team, "");
    }

    #[test]
    fn bad_ids_are_refused() {
        let mut options = flags("Dukaan", "https://dukaan.example");
        options.bundle_id = Some("com..dukaan".to_owned());
        assert!(error(&options, &AppFacts::default()).contains("bundle"));
        let mut options = flags("Dukaan", "https://dukaan.example");
        options.team = Some("abc".to_owned());
        assert!(error(&options, &AppFacts::default()).contains("Team ID"));
    }

    #[test]
    fn a_name_with_a_line_break_is_refused() {
        let message = error(
            &flags("Dukaan\nBar", "https://dukaan.example"),
            &AppFacts::default(),
        );
        assert!(message.contains("--name"), "{message}");
    }

    #[test]
    fn a_package_version_must_be_a_version() {
        for version in ["1.5", "v1.5.0", "1.5.0-", "latest"] {
            let mut options = flags("Dukaan", "https://dukaan.example");
            options.package_version = Some(version.to_owned());
            let message = error(&options, &AppFacts::default());
            assert!(
                message.contains("--package-version"),
                "{version}: {message}"
            );
        }
        let mut options = flags("Dukaan", "https://dukaan.example");
        options.package_version = Some("1.5.0-rc.1".to_owned());
        assert!(project(&options, &AppFacts::default()).is_ok());
    }

    #[test]
    fn outside_an_app_name_and_origin_are_required() {
        let options = InitOptions {
            origin: Some("https://dukaan.example".to_owned()),
            ..InitOptions::default()
        };
        assert!(error(&options, &AppFacts::default()).contains("--name"));
        let options = InitOptions {
            name: Some("Dukaan".to_owned()),
            ..InitOptions::default()
        };
        assert!(error(&options, &AppFacts::default()).contains("--origin"));
    }

    #[test]
    fn the_package_is_remote_at_the_apps_navree_version() {
        let made = project(&InitOptions::default(), &dukaan()).unwrap();
        assert_eq!(
            made.package,
            PackageSource::Remote {
                url: SWIFT_PACKAGE_URL.to_owned(),
                version: "1.4.0".to_owned()
            }
        );
        let made = project(
            &flags("Dukaan", "https://dukaan.example"),
            &AppFacts::default(),
        )
        .unwrap();
        assert_eq!(
            made.package,
            PackageSource::Remote {
                url: SWIFT_PACKAGE_URL.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned()
            }
        );
    }

    #[test]
    fn a_package_path_is_written_relative_to_the_project() {
        let mut options = flags("Dukaan", "https://dukaan.example");
        options.package_path = Some(PathBuf::from("/w/navree/packages/apple"));
        let made = resolve(
            &options,
            &AppFacts::default(),
            Path::new("/w/app"),
            Path::new("/w/app/apple"),
        )
        .unwrap();
        assert_eq!(
            made.package,
            PackageSource::Local {
                path: "../../navree/packages/apple".to_owned()
            }
        );
        options.package_path = Some(PathBuf::from("../navree/packages/apple"));
        let made = resolve(
            &options,
            &AppFacts::default(),
            Path::new("/w/app"),
            Path::new("/w/app/apple"),
        )
        .unwrap();
        assert_eq!(
            made.package,
            PackageSource::Local {
                path: "../../navree/packages/apple".to_owned()
            }
        );
    }

    #[test]
    fn the_settings_and_the_lock_are_read() {
        let settings = lookup(json!({
            "app": { "name": "Dukaan", "url": "https://dukaan.example" },
            "auth": { "apps": { "apple": { "dukaan": { "team_id": "ABCDE12345", "bundle_id": "in.example.dukaan" } } } },
        }));
        let lock = "version = 4\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"navree\"\nversion = \"1.4.0\"\n";
        assert_eq!(facts_from(&settings, Some(lock)), dukaan());
        assert_eq!(facts_from(&lookup(json!({})), None), AppFacts::default());
    }

    fn files(made: &AppleProject) -> FileSet {
        plan(made).unwrap()
    }

    #[test]
    fn the_plan_lists_the_files() {
        let made = project(&InitOptions::default(), &dukaan()).unwrap();
        assert_eq!(
            files(&made).paths(),
            [
                "project.yml",
                "Dukaan/App.swift",
                "Dukaan/Assets.xcassets/Contents.json",
                "Dukaan/Assets.xcassets/AppIcon.appiconset/Contents.json",
                "Dukaan/Assets.xcassets/AccentColor.colorset/Contents.json",
                "Signing/Signing.xcconfig",
                "Signing/Signing.local.xcconfig.example",
                "Signing/iOS.entitlements",
                "Signing/Mac.entitlements",
                ".gitignore",
                "README.md",
            ]
        );
    }

    #[test]
    fn the_plan_holds_the_values() {
        let made = project(&InitOptions::default(), &dukaan()).unwrap();
        let set = files(&made);
        let signing = set.get("Signing/Signing.xcconfig").unwrap();
        assert!(
            signing.contains("NAVREE_BUNDLE_ID = in.example.dukaan\n"),
            "{signing}"
        );
        assert!(
            signing.contains("DEVELOPMENT_TEAM = ABCDE12345\n"),
            "{signing}"
        );
        assert!(
            !signing.contains("CODE_SIGN_IDENTITY[sdk=macosx*] = -"),
            "{signing}"
        );
        let project_yml = set.get("project.yml").unwrap();
        assert!(
            project_yml.contains(&format!("url: {SWIFT_PACKAGE_URL}\n")),
            "{project_yml}"
        );
        assert!(
            project_yml.contains("minorVersion: 1.4.0\n"),
            "{project_yml}"
        );
        assert!(
            project_yml.contains("NavreeOrigin: https://dukaan.example\n"),
            "{project_yml}"
        );
        assert!(
            !project_yml.contains("NSAllowsLocalNetworking"),
            "{project_yml}"
        );
        let app = set.get("Dukaan/App.swift").unwrap();
        assert!(app.contains("struct DukaanApp: App {"), "{app}");
        assert!(app.contains("signInScheme: \"dukaan\""), "{app}");

        let mut local = project(
            &flags("Dukaan", "http://127.0.0.1:8095"),
            &AppFacts::default(),
        )
        .unwrap();
        local.package = PackageSource::Local {
            path: "..".to_owned(),
        };
        let set = files(&local);
        let project_yml = set.get("project.yml").unwrap();
        assert!(
            project_yml.contains("NSAllowsLocalNetworking: true"),
            "{project_yml}"
        );
        assert!(project_yml.contains("path: ..\n"), "{project_yml}");
        assert!(!project_yml.contains("minorVersion"), "{project_yml}");
        let signing = set.get("Signing/Signing.xcconfig").unwrap();
        assert!(
            signing.contains("CODE_SIGN_IDENTITY[sdk=macosx*] = -\n"),
            "{signing}"
        );
    }

    #[test]
    fn run_init_writes_nothing_when_a_file_exists() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::create_dir_all(dir.path().join("apple")).unwrap();
        std::fs::write(dir.path().join("apple").join("README.md"), "mine\n").unwrap();
        let mut out = Vec::new();
        let message = run_init(
            dir.path(),
            &flags("Dukaan", "https://dukaan.example"),
            &mut out,
        )
        .unwrap_err()
        .to_string();
        assert!(message.contains("README.md"), "{message}");
        assert!(!dir.path().join("apple").join("project.yml").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("apple").join("README.md")).unwrap(),
            "mine\n"
        );
    }

    #[test]
    fn run_init_outside_an_app_uses_the_flags() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let mut out = Vec::new();
        run_init(
            dir.path(),
            &flags("Dukaan", "https://dukaan.example"),
            &mut out,
        )
        .unwrap();
        let written = std::fs::read_to_string(
            dir.path()
                .join("apple")
                .join("Signing")
                .join("Signing.xcconfig"),
        )
        .unwrap();
        assert!(
            written.contains("NAVREE_BUNDLE_ID = com.example.dukaan\n"),
            "{written}"
        );
        let printed = String::from_utf8(out).unwrap();
        assert!(printed.contains("xcodegen generate"), "{printed}");
        assert!(printed.contains("com.example.dukaan"), "{printed}");
    }

    #[test]
    fn run_init_in_a_package_whose_settings_do_not_load_uses_the_flags() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"dukaan\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let mut out = Vec::new();
        run_init(
            &dir.path().join("src"),
            &flags("Dukaan", "https://dukaan.example"),
            &mut out,
        )
        .unwrap();
        assert!(dir.path().join("apple").join("project.yml").exists());
    }

    #[test]
    fn run_init_names_the_package_it_wrote() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let mut options = flags("Dukaan", "https://dukaan.example");
        options.package = Some("https://git.example/dukaan/navree-swift".to_owned());
        options.package_version = Some("1.4.0".to_owned());
        let mut out = Vec::new();
        run_init(dir.path(), &options, &mut out).unwrap();
        let printed = String::from_utf8(out).unwrap();
        assert!(
            printed.contains("https://git.example/dukaan/navree-swift 1.4.0"),
            "{printed}"
        );
        assert!(!printed.contains(SWIFT_PACKAGE_URL), "{printed}");
    }

    #[test]
    fn dry_run_writes_nothing() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let mut options = flags("Dukaan", "https://dukaan.example");
        options.dry_run = true;
        let mut out = Vec::new();
        run_init(dir.path(), &options, &mut out).unwrap();
        assert!(!dir.path().join("apple").exists());
        let printed = String::from_utf8(out).unwrap();
        assert!(printed.contains("==> project.yml <=="), "{printed}");
    }
}

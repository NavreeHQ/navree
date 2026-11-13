//! What the native commands read from the app's project: `native.toml`, the
//! cargo feature that turns the native preview on, and the words for what a
//! page lacks.
//!
//! `native.toml` sits at the app's root, beside `dock.toml` and `harbor.toml`:
//! files the tools read. (Every file in `config/` is a section of the app's own
//! settings, and this is not one.)
//!
//! ```toml
//! [coverage]
//! # A value for each route parameter, by name, for every route that has it.
//! [coverage.params]
//! id = 1
//! order = "CS-20481"
//!
//! # Values for one route's parameters; they rank above `params`.
//! [coverage.routes."/orders/{order}/lines/{line}"]
//! line = 2
//!
//! # More addresses to ask, with their queries.
//! # also = ["/orders?filter[status][in]=open"]
//! # Routes to leave out on purpose; `*` matches any text.
//! # skip = ["/admin/*"]
//!
//! # Headers sent with every request: a person who is signed in.
//! # `${NAME}` is replaced by the environment variable NAME.
//! [coverage.headers]
//! cookie = "navree_session=${COVERAGE_SESSION}"
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// The file's name, at the app's root.
pub const FILE: &str = "native.toml";

/// The platforms a native client can be, as the protocol names them.
pub const PLATFORMS: [&str; 10] = [
    "ios", "ipados", "macos", "watchos", "tvos", "visionos", "android", "wearos", "windows",
    "linux",
];

/// The kinds of device, as the protocol names them.
pub const FORMS: [&str; 5] = ["phone", "tablet", "desktop", "watch", "tv"];

/// A value in `native.toml`: text, a number or a flag, all sent as text.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
enum Scalar {
    Text(String),
    Number(i64),
    Flag(bool),
}

impl Scalar {
    fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Number(number) => number.to_string(),
            Self::Flag(flag) => flag.to_string(),
        }
    }
}

/// `native.toml`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    coverage: CoverageFile,
}

/// `[coverage]`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CoverageFile {
    params: BTreeMap<String, Scalar>,
    routes: BTreeMap<String, BTreeMap<String, Scalar>>,
    also: Vec<String>,
    skip: Vec<String>,
    headers: BTreeMap<String, String>,
}

/// What `native:coverage` is told about the app, as the app's command takes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// A value for each route parameter, by name.
    pub values: BTreeMap<String, String>,
    /// Values for one route's parameters, by the route's pattern.
    pub routes: BTreeMap<String, BTreeMap<String, String>>,
    /// More addresses to ask.
    pub also: Vec<String>,
    /// Routes to leave out.
    pub skip: Vec<String>,
    /// Headers for every request.
    pub headers: BTreeMap<String, String>,
}

impl Plan {
    /// The plan as the compact JSON the app's command reads.
    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

/// `${NAME}` in `value` replaced from `env`; the name of a variable that is
/// not set is the error.
fn expand(value: &str, env: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err("an unclosed `${`".to_owned());
        };
        let name = &after[..end];
        match env(name) {
            Some(found) => out.push_str(&found),
            None => return Err(format!("`${{{name}}}` is not set in the environment")),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Reads `native.toml` text into the plan; `env` answers `${NAME}` in headers.
pub fn parse(text: &str, env: &dyn Fn(&str) -> Option<String>) -> anyhow::Result<Plan> {
    let file: File = toml::from_str(text).with_context(|| format!("{FILE} is not read"))?;
    let coverage = file.coverage;
    let texts = |table: BTreeMap<String, Scalar>| -> BTreeMap<String, String> {
        table
            .into_iter()
            .map(|(name, value)| (name, value.text()))
            .collect()
    };
    for pattern in coverage.routes.keys() {
        anyhow::ensure!(
            pattern.starts_with('/'),
            "{FILE}: [coverage.routes.\"{pattern}\"] must be a route's pattern, such as \"/orders/{{id}}\""
        );
    }
    for address in &coverage.also {
        anyhow::ensure!(
            address.starts_with('/') && !address.starts_with("//"),
            "{FILE}: `also` takes addresses of the app, such as \"/orders?status=open\", not \"{address}\""
        );
    }
    let mut headers = BTreeMap::new();
    for (name, value) in coverage.headers {
        let value = expand(&value, env)
            .map_err(|problem| anyhow::anyhow!("{FILE}: header `{name}`: {problem}"))?;
        headers.insert(name, value);
    }
    Ok(Plan {
        values: texts(coverage.params),
        routes: coverage
            .routes
            .into_iter()
            .map(|(pattern, own)| (pattern, texts(own)))
            .collect(),
        also: coverage.also,
        skip: coverage.skip,
        headers,
    })
}

/// The plan in `path`, or an empty one when there is no such file.
pub fn read(path: &Path) -> anyhow::Result<Plan> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Plan::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read {}", path.display()));
        }
    };
    parse(&text, &|name| std::env::var(name).ok()).with_context(|| format!("in {}", path.display()))
}

/// The file's place in the app at `root`.
pub fn path_in(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// How the app stands with the native preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    /// Turned on whenever the app is built.
    On,
    /// Turned on by this feature of the app, which is not a default one.
    Feature(String),
    /// Nothing in the app's manifest turns it on.
    Off,
    /// The manifest could not be read as an app's.
    Unknown,
}

/// The entries of a feature list that switch the preview on.
const ENABLING: [&str; 4] = [
    "navree/ui-native-preview",
    "navree?/ui-native-preview",
    "navree-ui/native-preview",
    "navree-ui?/native-preview",
];

/// How the app whose manifest is `manifest` stands with the preview, given the
/// facade features its `navree` dependency turns on (`declared`).
pub fn preview_of(manifest: &toml::Table, declared: &[String]) -> Preview {
    let dependency_on = declared.iter().any(|f| f == "ui-native-preview")
        || manifest
            .get("dependencies")
            .and_then(|d| d.get("navree-ui"))
            .and_then(|d| d.get("features"))
            .and_then(|f| f.as_array())
            .is_some_and(|list| list.iter().any(|f| f.as_str() == Some("native-preview")));
    if dependency_on {
        return Preview::On;
    }
    let Some(features) = manifest.get("features").and_then(|f| f.as_table()) else {
        return Preview::Off;
    };
    let listed = |name: &str| -> Vec<&str> {
        features
            .get(name)
            .and_then(|list| list.as_array())
            .map(|list| list.iter().filter_map(|item| item.as_str()).collect())
            .unwrap_or_default()
    };
    // The features that switch it on, reaching through features that name them.
    let mut enabling: Vec<&str> = Vec::new();
    loop {
        let before = enabling.len();
        for name in features.keys() {
            if enabling.contains(&name.as_str()) {
                continue;
            }
            let items = listed(name);
            if items
                .iter()
                .any(|item| ENABLING.contains(item) || enabling.contains(item))
            {
                enabling.push(name);
            }
        }
        if enabling.len() == before {
            break;
        }
    }
    if enabling.is_empty() {
        return Preview::Off;
    }
    // On when the app's default features reach one.
    let defaults = listed("default");
    if defaults.iter().any(|item| enabling.contains(item)) {
        return Preview::On;
    }
    let chosen = if enabling.contains(&"native-preview") {
        "native-preview"
    } else {
        // The feature that no other feature of the list names, by name.
        let mut roots: Vec<&str> = enabling.clone();
        roots.sort_unstable();
        roots[0]
    };
    Preview::Feature(chosen.to_owned())
}

/// How the app at `root` stands with the preview.
pub fn preview(root: &Path) -> Preview {
    let Ok(project) = navree_navigator::Project::find(root) else {
        return Preview::Unknown;
    };
    let Some(manifest) = std::fs::read_to_string(project.root.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
    else {
        return Preview::Unknown;
    };
    preview_of(&manifest, &project.declared_features())
}

/// A reason the reader gave for a page staying in the web view, or a gap that
/// keeps a screen from being idiom, said so an author reads it.
///
/// The readers' reasons are codes (`tag:canvas`, `hook:data-ui-change`);
/// words the author already can read (the gaps of a native screen) pass as
/// they are.
pub fn say(code: &str) -> String {
    let (kind, name) = match code.split_once(':') {
        Some((kind, name)) if !kind.contains(' ') => (kind, name),
        _ => (code, ""),
    };
    match (kind, name) {
        ("tag", name) => format!(
            "the page has a <{name}> element, which has no native part; keep it for the web with native::web_only, or replace it with a component that has one"
        ),
        ("hook", name) => format!(
            "the page uses the browser behaviour `{name}`, which has no native meaning yet"
        ),
        ("slot", name) => format!(
            "the component `{name}` has no native part, or its markup was changed so it can no longer be read"
        ),
        ("cmd", name) => format!("the command `{name}` has no native meaning"),
        ("vocab", name) => format!(
            "the client's vocabulary has no `{name}` and no older form of it"
        ),
        ("lazy", "live") => {
            "a live list is fetched by the page's own script, which a native client does not run"
                .to_owned()
        }
        ("html", "") => {
            "the handler writes its HTML by hand, so it cannot be read; build it with the toolkit's components".to_owned()
        }
        ("depth", "") => "the page is nested deeper than a client reads".to_owned(),
        ("raw", "") => "the page writes raw markup".to_owned(),
        _ => code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Option<String> {
        (name == "COVERAGE_SESSION").then(|| "abc123".to_owned())
    }

    #[test]
    fn an_empty_file_is_an_empty_plan() {
        assert_eq!(parse("", &env).unwrap(), Plan::default());
        assert_eq!(parse("[coverage]\n", &env).unwrap(), Plan::default());
    }

    #[test]
    fn the_documented_file_is_read() {
        let text = "\
[coverage]
also = [\"/orders?filter[status][in]=open\"]
skip = [\"/admin/*\"]

[coverage.params]
id = 1
order = \"CS-20481\"
draft = true

[coverage.routes.\"/orders/{order}/lines/{line}\"]
line = 2

[coverage.headers]
cookie = \"navree_session=${COVERAGE_SESSION}\"
";
        let plan = parse(text, &env).unwrap();
        assert_eq!(plan.values["id"], "1");
        assert_eq!(plan.values["order"], "CS-20481");
        assert_eq!(plan.values["draft"], "true");
        assert_eq!(plan.routes["/orders/{order}/lines/{line}"]["line"], "2");
        assert_eq!(plan.also, ["/orders?filter[status][in]=open"]);
        assert_eq!(plan.skip, ["/admin/*"]);
        assert_eq!(plan.headers["cookie"], "navree_session=abc123");
        // The app's command takes it as JSON.
        let json: serde_json::Value = serde_json::from_str(&plan.json()).unwrap();
        assert_eq!(json["values"]["id"], "1");
        assert_eq!(json["headers"]["cookie"], "navree_session=abc123");
    }

    #[test]
    fn a_mistake_in_the_file_is_named() {
        let error = |text: &str| format!("{:#}", parse(text, &env).unwrap_err());
        assert!(error("[coverage]\nparam = {}\n").contains("unknown field `param`"));
        assert!(error("[covrage]\n").contains("unknown field `covrage`"));
        assert!(
            error("[coverage]\nalso = [\"orders\"]\n").contains("such as \"/orders?status=open\"")
        );
        assert!(error("[coverage.routes.\"orders/{id}\"]\nid = 1\n").contains("a route's pattern"));
        assert!(
            error("[coverage.headers]\ncookie = \"s=${NOPE}\"\n")
                .contains("header `cookie`: `${NOPE}` is not set")
        );
        assert!(error("[coverage.headers]\ncookie = \"${open\"\n").contains("unclosed"));
    }

    #[test]
    fn no_file_is_no_plan() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&path_in(dir.path())).unwrap(), Plan::default());
        std::fs::write(path_in(dir.path()), "[coverage.params]\nid = 7\n").unwrap();
        assert_eq!(read(&path_in(dir.path())).unwrap().values["id"], "7");
        std::fs::write(path_in(dir.path()), "[coverage\n").unwrap();
        let error = format!("{:#}", read(&path_in(dir.path())).unwrap_err());
        assert!(error.contains("native.toml"), "{error}");
    }

    fn manifest(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn the_feature_is_found_when_the_app_has_one() {
        let store = manifest(
            "[features]\nnative-preview = [\"navree/ui-native-preview\"]\n[dependencies]\nnavree = { version = \"1\" }\n",
        );
        assert_eq!(
            preview_of(&store, &[]),
            Preview::Feature("native-preview".to_owned())
        );
        // Another name for it, and a feature that names that one.
        let other =
            manifest("[features]\nnative = [\"navree/ui-native-preview\"]\nall = [\"native\"]\n");
        assert_eq!(preview_of(&other, &[]), Preview::Feature("all".to_owned()));
        let optional = manifest("[features]\nnative = [\"navree?/ui-native-preview\"]\n");
        assert_eq!(
            preview_of(&optional, &[]),
            Preview::Feature("native".to_owned())
        );
    }

    #[test]
    fn the_preview_is_on_when_the_dependency_or_the_defaults_turn_it_on() {
        let plain = manifest("[dependencies]\nnavree = { version = \"1\" }\n");
        assert_eq!(
            preview_of(
                &plain,
                &["default".to_owned(), "ui-native-preview".to_owned()]
            ),
            Preview::On
        );
        let defaults = manifest(
            "[features]\ndefault = [\"native\"]\nnative = [\"navree/ui-native-preview\"]\n",
        );
        assert_eq!(preview_of(&defaults, &[]), Preview::On);
        let direct = manifest(
            "[dependencies]\nnavree-ui = { version = \"1\", features = [\"native-preview\"] }\n",
        );
        assert_eq!(preview_of(&direct, &[]), Preview::On);
    }

    #[test]
    fn nothing_that_turns_it_on_is_off() {
        assert_eq!(preview_of(&manifest(""), &[]), Preview::Off);
        let other = manifest("[features]\nfast = [\"navree/orm\"]\n");
        assert_eq!(preview_of(&other, &["orm".to_owned()]), Preview::Off);
    }

    #[test]
    fn an_app_is_read_from_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(preview(dir.path()), Preview::Unknown);
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n[features]\nnative-preview = [\"navree/ui-native-preview\"]\n[dependencies]\nnavree = \"1\"\n",
        )
        .unwrap();
        assert_eq!(
            preview(dir.path()),
            Preview::Feature("native-preview".to_owned())
        );
    }

    #[test]
    fn a_reason_is_said_in_words() {
        assert!(say("tag:canvas").starts_with("the page has a <canvas> element"));
        assert!(say("hook:data-ui-change").contains("`data-ui-change`"));
        assert!(say("slot:calendar").contains("`calendar`"));
        assert!(say("html").contains("writes its HTML by hand"));
        assert!(say("depth").contains("deeper than a client reads"));
        assert!(say("lazy:live").contains("live list"));
        assert!(say("vocab:gauge").contains("`gauge`"));
        // Words that are already words stay as they are.
        assert_eq!(say("the screen has no title"), "the screen has no title");
        assert_eq!(
            say("the action “Export” has no role"),
            "the action “Export” has no role"
        );
        // A code nobody here knows is shown as it is.
        assert_eq!(say("future:thing"), "future:thing");
    }
}

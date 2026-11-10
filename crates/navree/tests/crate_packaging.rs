//! Keeps every published crate ready for crates.io and docs.rs.
//!
//! A crate is published when its manifest does not say `publish = false`. For
//! each one this file checks what crates.io shows (description, licence,
//! repository, home page, keywords, categories, README), what the package
//! holds (`cargo package --list`: the source, the licence files, every file the
//! code embeds, and no tests, benchmarks, fixtures or design notes), what
//! docs.rs builds (its features and target, and `doc_cfg` in `lib.rs`), and
//! that no dependency makes a publish cycle or leaves a path without a version.
//!
//! The README and licence files are written by `scripts/crate-readmes.mjs`;
//! run it when a test here says one is missing or has drifted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

const REPOSITORY: &str = "https://github.com/NavreeHQ/navree";
const HOMEPAGE: &str = "https://navree.com";
const DOCS_RS_TARGET: &str = "x86_64-unknown-linux-gnu";
const OWN_LICENCE: &str = "MIT OR Apache-2.0";

/// Crates in `crates/` that are never published: test and tooling crates.
const INTERNAL: &[&str] = &["docs-snippets", "navree-template-peak", "skill-snippets"];

/// Crates that bundle third-party files: their licence expression and the
/// notice files the package must carry.
const BUNDLED: &[(&str, &str, &[&str])] = &[
    (
        "navree-ui",
        "(MIT OR Apache-2.0) AND ISC AND MIT AND OFL-1.1 AND Unicode-3.0 AND Apache-2.0",
        &["THIRD_PARTY.md", "assets/fonts/LICENSE-geist.txt"],
    ),
    (
        "navree-documents",
        "(MIT OR Apache-2.0) AND OFL-1.1",
        &["fonts/OFL.txt"],
    ),
    (
        "navree-graphql",
        "(MIT OR Apache-2.0) AND MIT",
        &[
            "assets/graphiql-3.8.3.LICENSE.txt",
            "assets/react-18.3.1.LICENSE.txt",
            "assets/react-dom-18.3.1.LICENSE.txt",
        ],
    ),
    (
        "navree-phone",
        "(MIT OR Apache-2.0) AND Apache-2.0",
        &["THIRD_PARTY.md"],
    ),
];

/// The longest description, in characters. crates.io search shows the start.
const MAX_DESCRIPTION: usize = 160;

/// The category slugs crates.io accepts (https://crates.io/category_slugs).
const CATEGORY_SLUGS: &[&str] = &[
    "accessibility",
    "aerospace",
    "aerospace::drones",
    "aerospace::protocols",
    "aerospace::simulation",
    "aerospace::space-protocols",
    "aerospace::unmanned-aerial-vehicles",
    "algorithms",
    "api-bindings",
    "artificial-intelligence",
    "asynchronous",
    "authentication",
    "automotive",
    "caching",
    "command-line-interface",
    "command-line-utilities",
    "compilers",
    "compression",
    "computer-vision",
    "concurrency",
    "config",
    "cryptography",
    "cryptography::cryptocurrencies",
    "data-structures",
    "database",
    "database-implementations",
    "date-and-time",
    "development-tools",
    "development-tools::build-utils",
    "development-tools::cargo-plugins",
    "development-tools::debugging",
    "development-tools::ffi",
    "development-tools::procedural-macro-helpers",
    "development-tools::profiling",
    "development-tools::testing",
    "email",
    "embedded",
    "emulators",
    "encoding",
    "external-ffi-bindings",
    "filesystem",
    "finance",
    "game-development",
    "game-engines",
    "games",
    "graphics",
    "gui",
    "hardware-support",
    "internationalization",
    "localization",
    "mathematics",
    "memory-management",
    "multimedia",
    "multimedia::audio",
    "multimedia::encoding",
    "multimedia::images",
    "multimedia::video",
    "network-programming",
    "no-std",
    "no-std::no-alloc",
    "os",
    "os::android-apis",
    "os::freebsd-apis",
    "os::linux-apis",
    "os::macos-apis",
    "os::unix-apis",
    "os::windows-apis",
    "parser-implementations",
    "parsing",
    "rendering",
    "rendering::data-formats",
    "rendering::engine",
    "rendering::graphics-api",
    "rust-patterns",
    "science",
    "science::bioinformatics",
    "science::bioinformatics::genomics",
    "science::bioinformatics::proteomics",
    "science::bioinformatics::sequence-analysis",
    "science::computational-biology",
    "science::computational-biology::structural-modeling",
    "science::computational-biology::systems-biology",
    "science::computational-chemistry",
    "science::computational-chemistry::cheminformatics",
    "science::computational-chemistry::electronic-structure",
    "science::computational-chemistry::molecular-simulation",
    "science::geo",
    "science::materials",
    "science::neuroscience",
    "science::quantum-computing",
    "science::robotics",
    "security",
    "simulation",
    "template-engine",
    "text-editors",
    "text-processing",
    "value-formatting",
    "virtualization",
    "visualization",
    "wasm",
    "web-programming",
    "web-programming::http-client",
    "web-programming::http-server",
    "web-programming::websocket",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One workspace package from `cargo metadata --no-deps`.
struct Package {
    json: Value,
}

impl Package {
    fn name(&self) -> &str {
        self.json["name"].as_str().expect("a package name")
    }

    fn dir(&self) -> PathBuf {
        Path::new(
            self.json["manifest_path"]
                .as_str()
                .expect("a manifest path"),
        )
        .parent()
        .expect("a manifest folder")
        .to_owned()
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.json[key].as_str()
    }

    fn strings(&self, key: &str) -> Vec<&str> {
        self.json[key]
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    /// `publish = false` shows as an empty `publish` list.
    fn published(&self) -> bool {
        !self.json["publish"]
            .as_array()
            .is_some_and(|registries| registries.is_empty())
    }

    fn dependencies(&self) -> &[Value] {
        self.json["dependencies"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

fn workspace() -> &'static [Package] {
    static PACKAGES: OnceLock<Vec<Package>> = OnceLock::new();
    PACKAGES.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ])
            .arg("--manifest-path")
            .arg(repository_root().join("Cargo.toml"))
            .output()
            .expect("failed to run `cargo metadata`");
        assert!(
            output.status.success(),
            "`cargo metadata` failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata: Value =
            serde_json::from_slice(&output.stdout).expect("`cargo metadata` printed invalid JSON");
        let mut packages: Vec<Package> = metadata["packages"]
            .as_array()
            .expect("a packages list")
            .iter()
            .map(|json| Package { json: json.clone() })
            .collect();
        packages.sort_by(|a, b| a.name().cmp(b.name()));
        packages
    })
}

fn published() -> impl Iterator<Item = &'static Package> {
    workspace().iter().filter(|package| package.published())
}

/// The files `cargo package --list` puts in each published crate.
fn package_lists() -> &'static BTreeMap<String, BTreeSet<String>> {
    static LISTS: OnceLock<BTreeMap<String, BTreeSet<String>>> = OnceLock::new();
    LISTS.get_or_init(|| {
        let names: Vec<&str> = published().map(Package::name).collect();
        let next = AtomicUsize::new(0);
        let results = std::sync::Mutex::new(BTreeMap::new());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::SeqCst);
                        let Some(name) = names.get(index) else { break };
                        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
                        let output = Command::new(cargo)
                            .args([
                                "package",
                                "--list",
                                "--allow-dirty",
                                "--offline",
                                "-p",
                                *name,
                            ])
                            .current_dir(repository_root())
                            .output()
                            .expect("failed to run `cargo package --list`");
                        assert!(
                            output.status.success(),
                            "`cargo package --list -p {name}` failed:\n{}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        // Cargo lists the files with the platform's separator
                        // (`src\lib.rs` on Windows); the checks name them with `/`.
                        let files = String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .map(|line| line.trim().replace('\\', "/"))
                            .filter(|line| !line.is_empty())
                            .collect::<BTreeSet<_>>();
                        results
                            .lock()
                            .expect("the results lock")
                            .insert((*name).to_owned(), files);
                    }
                });
            }
        });
        results.into_inner().expect("the results lock")
    })
}

fn no_problems(problems: Vec<String>) {
    assert!(
        problems.is_empty(),
        "{} problem(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn the_framework_crates_are_published_and_nothing_else_is() {
    let mut problems = Vec::new();
    for package in workspace() {
        let name = package.name();
        let in_crates =
            join_lexically(&package.dir(), "..") == join_lexically(&repository_root(), "crates");
        let should_publish = in_crates && !INTERNAL.contains(&name);
        if package.published() != should_publish {
            problems.push(if should_publish {
                format!("{name}: is not published (remove `publish = false`)")
            } else {
                format!("{name}: must say `publish = false`")
            });
        }
        if should_publish && !name.starts_with("navree") {
            problems.push(format!(
                "{name}: a published crate's name starts with navree"
            ));
        }
    }
    assert!(
        published().count() >= 76,
        "fewer published crates than expected: {}",
        published().count()
    );
    no_problems(problems);
}

#[test]
fn published_crates_name_the_repository_and_the_home_page() {
    let mut problems = Vec::new();
    for package in published() {
        let name = package.name();
        if package.text("repository") != Some(REPOSITORY) {
            problems.push(format!("{name}: repository must be {REPOSITORY}"));
        }
        if package.text("homepage") != Some(HOMEPAGE) {
            problems.push(format!("{name}: homepage must be {HOMEPAGE}"));
        }
        if package.text("documentation").is_some() {
            problems.push(format!(
                "{name}: leave `documentation` unset, so crates.io links docs.rs"
            ));
        }
    }
    no_problems(problems);
}

#[test]
fn published_crates_have_a_one_sentence_description() {
    let mut problems = Vec::new();
    for package in published() {
        let name = package.name();
        let Some(description) = package.text("description") else {
            problems.push(format!("{name}: no description"));
            continue;
        };
        let length = description.chars().count();
        if !(20..=MAX_DESCRIPTION).contains(&length) {
            problems.push(format!(
                "{name}: the description is {length} characters; keep it between 20 and {MAX_DESCRIPTION}"
            ));
        }
        if !description.ends_with('.') || description.contains('\n') {
            problems.push(format!(
                "{name}: the description is one line ending in a full stop"
            ));
        }
        if description[..description.len().saturating_sub(1)].contains(". ") {
            problems.push(format!("{name}: the description is one sentence"));
        }
    }
    no_problems(problems);
}

#[test]
fn published_crates_have_keywords_and_categories_crates_io_accepts() {
    let mut problems = Vec::new();
    for package in published() {
        let name = package.name();
        let keywords = package.strings("keywords");
        if keywords.is_empty() || keywords.len() > 5 {
            problems.push(format!("{name}: 1 to 5 keywords, found {}", keywords.len()));
        }
        for keyword in &keywords {
            let valid = keyword.len() <= 20
                && keyword
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
                && keyword
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+'));
            if !valid {
                problems.push(format!(
                    "{name}: keyword `{keyword}` is not accepted (at most 20 characters, ASCII, starting with a letter)"
                ));
            }
        }
        let categories = package.strings("categories");
        if categories.is_empty() || categories.len() > 5 {
            problems.push(format!(
                "{name}: 1 to 5 categories, found {}",
                categories.len()
            ));
        }
        for category in &categories {
            if !CATEGORY_SLUGS.contains(category) {
                problems.push(format!(
                    "{name}: `{category}` is not a crates.io category slug"
                ));
            }
        }
    }
    no_problems(problems);
}

#[test]
fn licence_expressions_say_what_each_crate_bundles() {
    let mut problems = Vec::new();
    for package in published() {
        let name = package.name();
        let expected = BUNDLED
            .iter()
            .find(|(crate_name, _, _)| *crate_name == name)
            .map_or(OWN_LICENCE, |(_, expression, _)| *expression);
        if package.text("license") != Some(expected) {
            problems.push(format!(
                "{name}: license is {:?}, expected {expected:?}",
                package.text("license")
            ));
        }
    }
    for (name, _, notices) in BUNDLED {
        let list = package_lists().get(*name);
        for notice in *notices {
            if !list.is_some_and(|files| files.contains(*notice)) {
                problems.push(format!("{name}: the package does not carry {notice}"));
            }
        }
    }
    no_problems(problems);
}

/// Line endings do not matter for a comparison: a checkout may write CRLF.
fn normalized(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.replace("\r\n", "\n"))
}

#[test]
fn published_crates_have_a_readme_and_the_licence_files() {
    let root = repository_root();
    let mit = normalized(&root.join("LICENSE-MIT")).expect("the root LICENSE-MIT");
    let apache = normalized(&root.join("LICENSE-APACHE")).expect("the root LICENSE-APACHE");
    let mut problems = Vec::new();
    for package in published() {
        let name = package.name();
        let dir = package.dir();
        if normalized(&dir.join("LICENSE-MIT")).as_deref() != Some(mit.as_str()) {
            problems.push(format!(
                "{name}: LICENSE-MIT is missing or differs from the root copy"
            ));
        }
        if normalized(&dir.join("LICENSE-APACHE")).as_deref() != Some(apache.as_str()) {
            problems.push(format!(
                "{name}: LICENSE-APACHE is missing or differs from the root copy"
            ));
        }
        let Some(readme) = package.text("readme") else {
            problems.push(format!("{name}: no `readme` in the manifest"));
            continue;
        };
        let Some(text) = normalized(&dir.join(readme)) else {
            problems.push(format!("{name}: the readme {readme} does not exist"));
            continue;
        };
        if name == "navree" {
            // The repository's own README: a test of its own checks its pictures.
            continue;
        }
        if readme != "README.md" {
            problems.push(format!(
                "{name}: the readme is README.md in the crate folder"
            ));
        }
        let rust = package.text("rust_version").unwrap_or_default();
        for (what, needle) in [
            ("a title with the crate name", format!("# {name}")),
            ("the docs.rs link", format!("https://docs.rs/{name}")),
            ("a navree.com link", "https://navree.com/docs/".to_owned()),
            ("the Rust version", format!("Rust {rust}")),
            ("the licence files", "LICENSE-MIT".to_owned()),
            (
                "how to get it",
                if name == "navree-cli" {
                    "cargo install"
                } else {
                    "cargo add"
                }
                .to_owned(),
            ),
        ] {
            if !text.contains(&needle) {
                problems.push(format!("{name}: the README lacks {what} ({needle:?})"));
            }
        }
    }
    no_problems(problems);
}

#[test]
fn the_facade_readme_uses_absolute_picture_addresses() {
    // crates.io shows the repository's README for the `navree` crate, away from
    // the repository, so a picture with a relative path would show broken.
    let facade = published()
        .find(|package| package.name() == "navree")
        .expect("the navree crate");
    let readme = facade.text("readme").expect("the facade names its readme");
    let text = normalized(&facade.dir().join(readme)).expect("the facade readme");
    let mut problems = Vec::new();
    for marker in ["src=\"", "]("] {
        for part in text.split(marker).skip(1) {
            let target = part.split(['"', ')']).next().unwrap_or_default();
            let is_picture = marker == "src=\""
                || [".png", ".jpg", ".webp", ".svg"]
                    .iter()
                    .any(|extension| target.ends_with(extension));
            if is_picture && !target.starts_with("https://") {
                problems.push(format!(
                    "the README names the picture {target:?}; use an https:// address"
                ));
            }
        }
    }
    no_problems(problems);
}

/// Folders and files that never belong in a package.
fn unwanted(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    if name == "DESIGN.md" || name.starts_with(".env") {
        return true;
    }
    // A fake key the source embeds on purpose sits below `src/`.
    if file.starts_with("src/") {
        return false;
    }
    ["tests/", "benches/", "examples/", "tools/", "fuzz/"]
        .iter()
        .any(|folder| file.starts_with(folder))
        || file.contains("/fixtures/")
}

#[test]
fn packages_hold_what_the_crate_needs_and_nothing_more() {
    let mut problems = Vec::new();
    for package in published() {
        let name = package.name();
        let Some(files) = package_lists().get(name) else {
            problems.push(format!("{name}: no package list"));
            continue;
        };
        let mut required = vec!["Cargo.toml", "LICENSE-MIT", "LICENSE-APACHE", "src/lib.rs"];
        if name != "navree" {
            required.push("README.md");
        }
        if package.dir().join("build.rs").exists() {
            required.push("build.rs");
        }
        for file in required {
            if !files.contains(file) {
                problems.push(format!(
                    "{name}: the package lacks {file} (add it to `include`)"
                ));
            }
        }
        for file in files {
            if unwanted(file) {
                problems.push(format!(
                    "{name}: the package holds {file} (leave it out of `include`)"
                ));
            }
        }
    }
    no_problems(problems);
}

/// `base` joined with `relative`, with `.` and `..` folded away.
fn join_lexically(base: &Path, relative: &str) -> PathBuf {
    let mut parts: Vec<Component> = Vec::new();
    let joined = base.join(relative);
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(parts.last(), Some(Component::Normal(_))) {
                    parts.pop();
                } else {
                    parts.push(component);
                }
            }
            other => parts.push(other),
        }
    }
    parts.iter().collect()
}

/// Every plain `include_str!("..")` / `include_bytes!("..")` in `file`, outside comments.
fn embedded_paths(file: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for macro_name in ["include_str!(", "include_bytes!("] {
            let mut rest = line;
            while let Some(at) = rest.find(macro_name) {
                rest = &rest[at + macro_name.len()..];
                let argument = rest.trim_start();
                if let Some(quoted) = argument.strip_prefix('"')
                    && let Some(end) = quoted.find('"')
                {
                    found.push(quoted[..end].to_owned());
                }
            }
        }
    }
    found
}

/// The `.rs` files below `dir` that a build of the package compiles.
fn compiled_sources(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&dir.join("src"), &mut files);
    let build = dir.join("build.rs");
    if build.exists() {
        files.push(build);
    }
    files
}

#[test]
fn every_file_the_code_embeds_is_in_the_package() {
    let mut problems = Vec::new();
    for package in published() {
        let name = package.name();
        let dir = package.dir();
        let Some(files) = package_lists().get(name) else {
            continue;
        };
        for source in compiled_sources(&dir) {
            let source_dir = source.parent().expect("a source folder");
            for path in embedded_paths(&source) {
                let target = join_lexically(source_dir, &path);
                let Ok(relative) = target.strip_prefix(join_lexically(&dir, "")) else {
                    problems.push(format!(
                        "{name}: {} embeds {path}, which is outside the crate folder",
                        source.display()
                    ));
                    continue;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                // Test modules embed their fixtures and the browser runtime's sources;
                // a build of the published package leaves those modules out.
                if relative.starts_with("tests/") || relative.starts_with("runtime/") {
                    continue;
                }
                if !files.contains(&relative) {
                    problems.push(format!(
                        "{name}: the code embeds {relative}, which the package leaves out"
                    ));
                }
            }
        }
    }
    no_problems(problems);
}

#[test]
fn navigator_packages_the_staged_skills_and_docs() {
    let manifest =
        std::fs::read_to_string(repository_root().join("crates/navree-navigator/Cargo.toml"))
            .expect("the navigator manifest");
    let include = manifest
        .split("include = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("an include list");
    for entry in ["skills/**", "docs/**"] {
        assert!(
            include.contains(&format!("\"{entry}\"")),
            "navree-navigator must include {entry}: its build script reads the staged copies in the crate folder"
        );
    }
    // After the release step copies them in, the package carries them.
    let dir = repository_root().join("crates/navree-navigator");
    let files = package_lists()
        .get("navree-navigator")
        .expect("the navigator list");
    for staged in ["skills", "docs"] {
        if dir.join(staged).is_dir() {
            assert!(
                files
                    .iter()
                    .any(|file| file.starts_with(&format!("{staged}/"))),
                "the staged {staged}/ folder is not in the navree-navigator package"
            );
        }
    }
}

#[test]
fn docs_rs_builds_with_real_features_for_one_target() {
    let mut problems = Vec::new();
    for package in published() {
        let name = package.name();
        let docs = &package.json["metadata"]["docs"]["rs"];
        if docs.is_null() {
            problems.push(format!("{name}: no [package.metadata.docs.rs]"));
            continue;
        }
        let targets: Vec<&str> = docs["targets"]
            .as_array()
            .map(|targets| targets.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if targets != [DOCS_RS_TARGET] {
            problems.push(format!(
                "{name}: docs.rs targets must be [\"{DOCS_RS_TARGET}\"]"
            ));
        }
        if docs.get("all-features").is_some() {
            problems.push(format!(
                "{name}: list the docs.rs features; `all-features` includes test-only ones"
            ));
        }
        let declared = package.json["features"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for feature in docs["features"]
            .as_array()
            .map(|features| {
                features
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        {
            if !declared.contains_key(feature) {
                problems.push(format!(
                    "{name}: docs.rs feature `{feature}` does not exist"
                ));
            }
            if feature.starts_with("test-") || feature.starts_with("__") || feature == "fuzzing" {
                problems.push(format!(
                    "{name}: docs.rs must not build with the test-only feature `{feature}`"
                ));
            }
        }
        let lib = package.dir().join("src/lib.rs");
        let text = std::fs::read_to_string(&lib).unwrap_or_default();
        if !text.contains("#![cfg_attr(docsrs, feature(doc_cfg))]") {
            problems.push(format!(
                "{name}: src/lib.rs lacks #![cfg_attr(docsrs, feature(doc_cfg))]"
            ));
        }
    }
    no_problems(problems);
}

#[test]
fn no_published_dependency_makes_a_publish_cycle_or_has_no_version() {
    let mut problems = Vec::new();
    let names: BTreeSet<&str> = published().map(Package::name).collect();
    // The edges Cargo orders a publish by: every dependency except a dev-dependency
    // without a version (that one is dropped from the published manifest).
    let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for package in published() {
        let name = package.name();
        for dependency in package.dependencies() {
            let dependency_name = dependency["name"].as_str().expect("a dependency name");
            let kind = dependency["kind"].as_str().unwrap_or("normal");
            let versioned = dependency["req"].as_str() != Some("*");
            let source = dependency["source"].as_str().unwrap_or_default();
            if source.starts_with("git+") {
                problems.push(format!("{name}: {dependency_name} is a git dependency"));
            }
            if kind != "dev" && !versioned && dependency["path"].is_string() {
                problems.push(format!(
                    "{name}: the path dependency {dependency_name} has no version"
                ));
            }
            if !names.contains(dependency_name) || (kind == "dev" && !versioned) {
                continue;
            }
            if dependency_name == name {
                problems.push(format!(
                    "{name}: a dev-dependency on itself carries a version; write it as `{{ path = \".\", features = [..] }}`"
                ));
                continue;
            }
            edges.entry(name).or_default().insert(dependency_name);
        }
    }
    problems.extend(find_cycle(&edges));
    no_problems(problems);
}

/// A description of one dependency cycle, if the graph has any.
fn find_cycle(edges: &BTreeMap<&str, BTreeSet<&str>>) -> Option<String> {
    fn visit<'a>(
        node: &'a str,
        edges: &BTreeMap<&'a str, BTreeSet<&'a str>>,
        done: &mut BTreeSet<&'a str>,
        path: &mut Vec<&'a str>,
    ) -> Option<String> {
        if let Some(at) = path.iter().position(|seen| *seen == node) {
            let mut cycle = path[at..].to_vec();
            cycle.push(node);
            return Some(format!("publish cycle: {}", cycle.join(" -> ")));
        }
        if !done.insert(node) {
            return None;
        }
        path.push(node);
        for next in edges.get(node).into_iter().flatten() {
            if let Some(found) = visit(next, edges, done, path) {
                return Some(found);
            }
        }
        path.pop();
        None
    }
    let mut done = BTreeSet::new();
    for node in edges.keys() {
        if let Some(found) = visit(node, edges, &mut done, &mut Vec::new()) {
            return Some(found);
        }
    }
    None
}

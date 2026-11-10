//! The workspace as `cargo metadata` describes it: its members, whether each
//! is published, and the dependencies each declares.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The table a dependency is declared in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    /// `[dependencies]`.
    Normal,
    /// `[dev-dependencies]`.
    Dev,
    /// `[build-dependencies]`.
    Build,
}

/// One declared dependency of a member.
#[derive(Debug, Clone)]
pub struct Dep {
    /// The package name (not the name it is renamed to).
    pub name: String,
    /// The table it is declared in.
    pub kind: DepKind,
    /// The version requirement: `*` for a path dependency without a version.
    pub req: String,
    /// The local path, for a path dependency.
    pub path: Option<PathBuf>,
}

impl Dep {
    /// Whether the dependency names a version. Cargo drops a dev-dependency
    /// without one from the published manifest; one with a version stays,
    /// so the registry must have it first.
    pub fn is_versioned(&self) -> bool {
        self.req != "*"
    }
}

/// One workspace member.
#[derive(Debug, Clone)]
pub struct Package {
    /// The package name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// `None` when it may be published anywhere, an empty list for
    /// `publish = false`, else the registries it may go to.
    pub publish: Option<Vec<String>>,
    /// Its `Cargo.toml`.
    pub manifest_path: PathBuf,
    /// Every declared dependency, once per table and target.
    pub deps: Vec<Dep>,
}

impl Package {
    /// Whether the package is published at all.
    pub fn is_publishable(&self) -> bool {
        self.publish
            .as_ref()
            .is_none_or(|registries| !registries.is_empty())
    }

    /// Whether the package may be published to `registry` (`None` is
    /// crates.io).
    pub fn publishable_to(&self, registry: Option<&str>) -> bool {
        match &self.publish {
            None => true,
            Some(registries) => {
                let wanted = registry.unwrap_or("crates-io");
                registries.iter().any(|r| r == wanted)
            }
        }
    }

    /// The folder of its manifest.
    pub fn dir(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(Path::new("."))
    }
}

/// The workspace's members.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The workspace root.
    pub root: PathBuf,
    /// Every member, sorted by name.
    pub members: Vec<Package>,
}

impl Workspace {
    /// Runs `cargo metadata --no-deps` in `root`.
    pub fn load(root: &Path) -> Result<Self, String> {
        let output = Command::new(cargo())
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .current_dir(root)
            .output()
            .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "cargo metadata failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Self::from_json(&String::from_utf8_lossy(&output.stdout))
    }

    /// Reads the output of `cargo metadata --format-version 1`.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let json: Value =
            serde_json::from_str(text).map_err(|e| format!("cargo metadata output: {e}"))?;
        let root = PathBuf::from(json["workspace_root"].as_str().unwrap_or("."));
        let ids: Vec<&str> = json["workspace_members"]
            .as_array()
            .ok_or("cargo metadata output has no workspace_members")?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let mut members = Vec::new();
        for package in json["packages"].as_array().ok_or("no packages")? {
            if !ids.contains(&package["id"].as_str().unwrap_or_default()) {
                continue;
            }
            members.push(package_from_json(package)?);
        }
        members.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Workspace { root, members })
    }

    /// The member called `name`.
    pub fn member(&self, name: &str) -> Option<&Package> {
        self.members.iter().find(|p| p.name == name)
    }

    /// The members that are published.
    pub fn publishable(&self) -> impl Iterator<Item = &Package> {
        self.members.iter().filter(|p| p.is_publishable())
    }

    /// The dependencies of `package` on other workspace members (path
    /// dependencies that name a member).
    pub fn internal_deps<'a>(&'a self, package: &'a Package) -> impl Iterator<Item = &'a Dep> {
        package
            .deps
            .iter()
            .filter(|dep| dep.path.is_some() && self.member(&dep.name).is_some())
    }
}

fn package_from_json(package: &Value) -> Result<Package, String> {
    let name = package["name"].as_str().ok_or("a package without a name")?;
    let publish = match &package["publish"] {
        Value::Null => None,
        Value::Array(registries) => Some(
            registries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ),
        other => return Err(format!("{name}: unexpected publish value {other}")),
    };
    let deps = package["dependencies"]
        .as_array()
        .map(|deps| {
            deps.iter()
                .map(|dep| Dep {
                    name: dep["name"].as_str().unwrap_or_default().to_owned(),
                    kind: match dep["kind"].as_str() {
                        Some("dev") => DepKind::Dev,
                        Some("build") => DepKind::Build,
                        _ => DepKind::Normal,
                    },
                    req: dep["req"].as_str().unwrap_or("*").to_owned(),
                    path: dep["path"].as_str().map(PathBuf::from),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Package {
        name: name.to_owned(),
        version: package["version"].as_str().unwrap_or_default().to_owned(),
        publish,
        manifest_path: PathBuf::from(package["manifest_path"].as_str().unwrap_or_default()),
        deps,
    })
}

/// The `cargo` that runs this program (`CARGO`), else `cargo`.
pub fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

/// `[workspace.package] version` of the root manifest.
pub fn workspace_version(root: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("Cargo.toml does not parse: {e}"))?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml has no [workspace.package] version".to_owned())
}

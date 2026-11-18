//! Installing a plugin into an app (`navree plugin:add`).

#![allow(dead_code)]

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::plugin::cargo_toml::Source;
use crate::plugin::files::{AppFiles, AppRoots};
use crate::plugin::index::{Index, IndexEntry, IndexKind};
use crate::plugin::manifest::shown;

/// What `navree plugin:add` was asked.
#[derive(Clone, Debug, Default)]
pub struct AddOptions {
    /// First-party short name (`billing`) or community crate name (`navree-plugin-khata`);
    /// optional when `--path` is given.
    pub package: Option<String>,
    /// Cargo version requirement (`^1.2`) for a community crate.
    pub version: Option<String>,
    /// Local plugin crate folder inside the app.
    pub path: Option<PathBuf>,
    /// Skip inserting the `.module(..)` line into `src/app.rs` and print it instead.
    pub no_wire: bool,
    /// Run `config:publish --plugin` and `plugins --json` for a community crate too.
    pub publish_config: bool,
    /// The app's Dock settings, passed when running the app.
    pub app_env: Vec<(String, String)>,
    /// The process environment, scanned for role lists and page sources.
    pub process_env: Vec<(String, String)>,
}

/// How a community package reaches the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum How {
    /// A community crate already a dependency from the same source, asked with no `--version`:
    /// `cargo add` is not run again.
    Present,
    /// `cargo add NAME`, or `cargo add NAME@REQ`, from the registry.
    Registry { requirement: Option<String> },
    /// `cargo add --path DIR`; `dir` is relative to the package folder, with `/`.
    Path { dir: String },
}

/// What package selection decided before Cargo runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    FirstParty(IndexEntry),
    Community { package: String, how: How },
}

/// Chooses the package to install. The `Err` is the refusal text without `error: `.
/// `cwd` is the folder the command ran in, which `--path` is resolved against.
pub(crate) fn choose(
    roots: &AppRoots,
    files: &AppFiles,
    options: &AddOptions,
    cwd: &Path,
    index: &Index,
) -> Result<Choice, String> {
    if let Some(given) = options.package.as_deref() {
        check_package_name(given)?;
    }

    if let Some(requirement) = options.version.as_deref()
        && !is_version_requirement(requirement)
    {
        return Err(format!(
            "--version `{}` is not a Cargo version requirement",
            shown(requirement)
        ));
    }

    if let Some(raw_path) = options.path.as_deref() {
        return choose_path(roots, files, options, cwd, index, raw_path);
    }

    let Some(given) = options.package.as_deref() else {
        return Err("name a package to install, or pass --path".to_owned());
    };

    if let Some(entry) = index.find(given) {
        return match entry.kind {
            IndexKind::Tool => {
                let cmd = entry.tool_command.as_deref().unwrap_or("");
                Err(format!(
                    "`{}` is a tool, not a plugin: use `{cmd}`",
                    entry.name
                ))
            }
            IndexKind::Feature => Err(format!(
                "`{}` is a feature of navree, not a plugin: add \"{}\" to navree's features in Cargo.toml",
                entry.name, entry.feature
            )),
            IndexKind::Module | IndexKind::Dev => {
                if options.version.is_some() {
                    Err(
                        "--version is for community crates; first-party packages follow navree's version"
                            .to_owned(),
                    )
                } else {
                    Ok(Choice::FirstParty(entry.clone()))
                }
            }
        };
    }

    if let Some((existing_name, existing_source)) = dependency_entry(&files.cargo_toml, given) {
        if existing_source != Source::Registry {
            return Err(format!(
                "{existing_name} is already a dependency of this app from {}; run navree plugin:remove {existing_name} first to add it from the registry",
                describe_source(&existing_source)
            ));
        }
        if options.version.is_none() {
            return Ok(Choice::Community {
                package: existing_name,
                how: How::Present,
            });
        }
        return Ok(Choice::Community {
            package: existing_name,
            how: How::Registry {
                requirement: options.version.clone(),
            },
        });
    }

    Ok(Choice::Community {
        package: given.to_owned(),
        how: How::Registry {
            requirement: options.version.clone(),
        },
    })
}

fn choose_path(
    roots: &AppRoots,
    files: &AppFiles,
    options: &AddOptions,
    cwd: &Path,
    index: &Index,
    raw_path: &Path,
) -> Result<Choice, String> {
    let joined = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        cwd.join(raw_path)
    };
    let shown_dir = std::path::absolute(&joined).unwrap_or_else(|_| joined.clone());
    let shown_root = std::path::absolute(&roots.package).unwrap_or_else(|_| roots.package.clone());

    let outside_err = || {
        format!(
            "--path {} is outside the app ({}); keep the plugin's folder inside the app",
            shown_dir.display(),
            shown_root.display()
        )
    };

    if !joined.exists() {
        if !is_lexical_strict_subpath(&shown_dir, &shown_root) {
            return Err(outside_err());
        }
        return Err(format!("--path {} does not exist", shown_dir.display()));
    }

    let Ok(canon_dir) = fs::canonicalize(&joined) else {
        return Err(format!("--path {} does not exist", shown_dir.display()));
    };
    let Ok(canon_root) = fs::canonicalize(&roots.package) else {
        return Err(outside_err());
    };
    if !canon_dir.starts_with(&canon_root) || canon_dir == canon_root {
        return Err(outside_err());
    }

    let rel_path = canon_dir
        .strip_prefix(&canon_root)
        .map_err(|_| outside_err())?;
    let mut segments = Vec::new();
    for comp in rel_path.components() {
        if let Component::Normal(seg) = comp {
            let Some(text) = seg.to_str() else {
                return Err(outside_err());
            };
            segments.push(text);
        }
    }
    if segments.is_empty() {
        return Err(outside_err());
    }
    let rel_dir = segments.join("/");

    let manifest_path = canon_dir.join("Cargo.toml");
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|err| format!("cannot read {}: {err}", manifest_path.display()))?;
    let parsed: toml::Table = manifest_text
        .parse()
        .map_err(|err| format!("cannot read {}: {err}", manifest_path.display()))?;
    let Some(found) = parsed
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|pkg| pkg.get("name"))
        .and_then(toml::Value::as_str)
    else {
        return Err(format!(
            "{} holds no Cargo package (missing [package] name)",
            shown_dir.display()
        ));
    };

    check_package_name(found)?;

    if let Some(given) = options.package.as_deref()
        && !same_package_name(given, found)
    {
        return Err(format!(
            "{} holds the package {}, not {}",
            shown_dir.display(),
            shown(found),
            shown(given)
        ));
    }

    let is_first_party_name = index.find(found).is_some()
        || index
            .packages
            .iter()
            .any(|entry| same_package_name(&entry.krate, found));
    if is_first_party_name {
        return Err(format!(
            "{} is the name of a first-party package; give the local crate a name of its own",
            shown(found)
        ));
    }

    if let Some((existing_name, existing_source)) = dependency_entry(&files.cargo_toml, found) {
        let same_path = matches!(
            &existing_source,
            Source::Path(existing_dir) if normalize_rel_dir(existing_dir) == rel_dir
        );
        if !same_path {
            return Err(format!(
                "{existing_name} is already a dependency of this app from {}; run navree plugin:remove {existing_name} first to add it from the folder {rel_dir}",
                describe_source(&existing_source)
            ));
        }
        if options.version.is_none() {
            return Ok(Choice::Community {
                package: existing_name,
                how: How::Present,
            });
        }
    }

    Ok(Choice::Community {
        package: found.to_owned(),
        how: How::Path { dir: rel_dir },
    })
}

fn check_package_name(name: &str) -> Result<(), String> {
    if !is_package_name(name) {
        return Err(format!(
            "`{}` is not a package name: lower-case letters, digits, - and _, starting with a letter or a digit, at most 64 characters",
            shown(name)
        ));
    }
    if name == "navree" {
        return Err("`navree` is the framework, not a plugin".to_owned());
    }
    Ok(())
}

fn is_package_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'-' | b'_')
        })
}

fn is_version_requirement(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= 64
        && text.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b'*' | b'^' | b'~' | b'=' | b'<' | b'>' | b',' | b'+' | b'-' | b' '
                )
        })
}

fn same_package_name(left: &str, right: &str) -> bool {
    let norm = |byte: u8| if byte == b'_' { b'-' } else { byte };
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .all(|(a, b)| norm(a) == norm(b))
}

fn normalize_rel_dir(dir: &str) -> String {
    let slash = dir.replace('\\', "/");
    let trimmed = slash.trim_start_matches("./").trim_end_matches('/');
    trimmed.to_owned()
}

fn describe_source(source: &Source) -> String {
    match source {
        Source::Registry => "the registry".to_owned(),
        Source::Path(path) => format!("the folder {}", normalize_rel_dir(path)),
        Source::Git(_) => "a git repository".to_owned(),
        Source::Workspace => "the workspace".to_owned(),
    }
}

fn dependency_entry(cargo_toml: &str, package: &str) -> Option<(String, Source)> {
    let parsed: toml::Table = cargo_toml.parse().ok()?;
    let deps = parsed.get("dependencies")?.as_table()?;
    for (key, value) in deps {
        let actual = value
            .as_table()
            .and_then(|tbl| tbl.get("package"))
            .and_then(toml::Value::as_str)
            .unwrap_or(key.as_str());
        if same_package_name(actual, package) {
            let source = crate::plugin::cargo_toml::dependency_source(cargo_toml, package)?;
            return Some((key.clone(), source));
        }
    }
    None
}

fn lexical_clean(path: &Path) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            other => {
                let text = other.as_os_str().to_string_lossy();
                #[cfg(windows)]
                parts.push(text.to_lowercase());
                #[cfg(not(windows))]
                parts.push(text.into_owned());
            }
        }
    }
    parts
}

fn is_lexical_strict_subpath(dir: &Path, root: &Path) -> bool {
    let dir_parts = lexical_clean(dir);
    let root_parts = lexical_clean(root);
    dir_parts.len() > root_parts.len() && dir_parts[..root_parts.len()] == root_parts[..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots_at(dir: &Path) -> AppRoots {
        AppRoots {
            package: dir.to_path_buf(),
            config: dir.to_path_buf(),
            workspace: dir.to_path_buf(),
        }
    }

    fn files_with_cargo(roots: &AppRoots, cargo_toml: &str) -> AppFiles {
        AppFiles {
            roots: roots.clone(),
            cargo_toml: cargo_toml.to_owned(),
            ..AppFiles::default()
        }
    }

    fn base_cargo_toml() -> &'static str {
        "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = \"1.4.0\"\n"
    }

    fn write_local_plugin(dir: &Path, name: &str) {
        fs::create_dir_all(dir).expect("create plugin dir");
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        )
        .expect("write plugin Cargo.toml");
    }

    #[test]
    fn choose_takes_a_first_party_name_by_either_spelling() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();

        for spelling in ["billing", "navree-billing", "navree_billing"] {
            let choice = choose(
                &roots,
                &files,
                &AddOptions {
                    package: Some(spelling.to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .expect("first-party billing");
            let Choice::FirstParty(entry) = choice else {
                panic!("expected FirstParty for {spelling}");
            };
            assert_eq!(entry.name, "billing");
            assert_eq!(entry.kind, IndexKind::Module);
        }

        let Choice::FirstParty(http_client) = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("http_client".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .expect("http_client") else {
            panic!("expected FirstParty for http_client");
        };
        assert_eq!(http_client.name, "http-client");

        let community = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("navree-plugin-billing".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .expect("community plugin");
        assert_eq!(
            community,
            Choice::Community {
                package: "navree-plugin-billing".to_owned(),
                how: How::Registry { requirement: None },
            }
        );

        let Choice::FirstParty(browser) = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("browser".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .expect("browser") else {
            panic!("expected FirstParty for browser");
        };
        assert_eq!(browser.kind, IndexKind::Dev);
    }

    #[test]
    fn plugin_add_refuses_tools_features_and_versions_of_first_party_packages() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();

        for (pkg, expected) in [
            ("dock", "`dock` is a tool, not a plugin: use `navree dock`"),
            (
                "harbor",
                "`harbor` is a tool, not a plugin: use `navree harbor:init`",
            ),
            (
                "navigator",
                "`navigator` is a tool, not a plugin: use `navree mcp`",
            ),
            (
                "otel",
                "`otel` is a feature of navree, not a plugin: add \"otel\" to navree's features in Cargo.toml",
            ),
        ] {
            let err = choose(
                &roots,
                &files,
                &AddOptions {
                    package: Some(pkg.to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err();
            assert_eq!(err, expected);
        }

        let ver_err = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("billing".to_owned()),
                version: Some("^1.4".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(
            ver_err,
            "--version is for community crates; first-party packages follow navree's version"
        );

        let dash_err = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("-khata".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(
            dash_err,
            "`-khata` is not a package name: lower-case letters, digits, - and _, starting with a letter or a digit, at most 64 characters"
        );

        let long_name = "k".repeat(65);
        let long_err = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some(long_name.clone()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(
            long_err,
            format!(
                "`{long_name}` is not a package name: lower-case letters, digits, - and _, starting with a letter or a digit, at most 64 characters"
            )
        );

        let bad_req = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("navree-plugin-khata".to_owned()),
                version: Some("1.0; rm".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(
            bad_req,
            "--version `1.0; rm` is not a Cargo version requirement"
        );
    }

    #[test]
    fn choose_refuses_the_framework_itself() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();

        let err = choose(
            &roots,
            &files,
            &AddOptions {
                package: Some("navree".to_owned()),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(err, "`navree` is the framework, not a plugin");

        let local = tmp.path().join("plugins/fake");
        write_local_plugin(&local, "navree");
        let path_err = choose(
            &roots,
            &files,
            &AddOptions {
                path: Some(PathBuf::from("plugins/fake")),
                ..AddOptions::default()
            },
            tmp.path(),
            index,
        )
        .unwrap_err();
        assert_eq!(path_err, "`navree` is the framework, not a plugin");
    }

    #[test]
    fn choose_reads_underscores_and_hyphens_as_one_in_a_package_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let index = Index::embedded();

        let hyphen_toml = format!("{}navree-plugin-khata = \"1.0\"\n", base_cargo_toml());
        let files = files_with_cargo(&roots, &hyphen_toml);
        assert_eq!(
            choose(
                &roots,
                &files,
                &AddOptions {
                    package: Some("navree_plugin_khata".to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap(),
            Choice::Community {
                package: "navree-plugin-khata".to_owned(),
                how: How::Present,
            }
        );

        let underscore_toml = format!("{}navree_plugin_khata = \"1.0\"\n", base_cargo_toml());
        let files_u = files_with_cargo(&roots, &underscore_toml);
        assert_eq!(
            choose(
                &roots,
                &files_u,
                &AddOptions {
                    package: Some("navree-plugin-khata".to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap(),
            Choice::Community {
                package: "navree_plugin_khata".to_owned(),
                how: How::Present,
            }
        );

        let path_toml = format!(
            "{}navree-plugin-khata = {{ path = \"plugins/khata\" }}\n",
            base_cargo_toml()
        );
        let files_p = files_with_cargo(&roots, &path_toml);
        assert_eq!(
            choose(
                &roots,
                &files_p,
                &AddOptions {
                    package: Some("navree_plugin_khata".to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "navree-plugin-khata is already a dependency of this app from the folder plugins/khata; run navree plugin:remove navree-plugin-khata first to add it from the registry"
        );
    }

    #[test]
    fn choose_refuses_a_registry_name_against_a_path_and_the_reverse() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let index = Index::embedded();
        let plugin_dir = tmp.path().join("plugins/khata");
        write_local_plugin(&plugin_dir, "navree-plugin-khata");

        let path_toml = format!(
            "{}navree-plugin-khata = {{ path = \"plugins/khata\" }}\n",
            base_cargo_toml()
        );
        assert_eq!(
            choose(
                &roots,
                &files_with_cargo(&roots, &path_toml),
                &AddOptions {
                    package: Some("navree-plugin-khata".to_owned()),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "navree-plugin-khata is already a dependency of this app from the folder plugins/khata; run navree plugin:remove navree-plugin-khata first to add it from the registry"
        );

        let reg_toml = format!("{}navree-plugin-khata = \"1.0\"\n", base_cargo_toml());
        assert_eq!(
            choose(
                &roots,
                &files_with_cargo(&roots, &reg_toml),
                &AddOptions {
                    path: Some(PathBuf::from("plugins/khata")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "navree-plugin-khata is already a dependency of this app from the registry; run navree plugin:remove navree-plugin-khata first to add it from the folder plugins/khata"
        );

        let shown_plugin = std::path::absolute(&plugin_dir).unwrap();
        assert_eq!(
            choose(
                &roots,
                &files_with_cargo(&roots, base_cargo_toml()),
                &AddOptions {
                    package: Some("navree-plugin-notices".to_owned()),
                    path: Some(PathBuf::from("plugins/khata")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            format!(
                "{} holds the package navree-plugin-khata, not navree-plugin-notices",
                shown_plugin.display()
            )
        );

        let billing_dir = tmp.path().join("plugins/billing");
        write_local_plugin(&billing_dir, "navree-billing");
        assert_eq!(
            choose(
                &roots,
                &files_with_cargo(&roots, base_cargo_toml()),
                &AddOptions {
                    path: Some(PathBuf::from("plugins/billing")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "navree-billing is the name of a first-party package; give the local crate a name of its own"
        );
    }

    #[test]
    fn choose_gives_the_package_found_at_path_when_none_is_named() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();
        let plugin_dir = tmp.path().join("plugins/khata");
        write_local_plugin(&plugin_dir, "navree-plugin-khata");

        let expected = Choice::Community {
            package: "navree-plugin-khata".to_owned(),
            how: How::Path {
                dir: "plugins/khata".to_owned(),
            },
        };

        assert_eq!(
            choose(
                &roots,
                &files,
                &AddOptions {
                    path: Some(PathBuf::from("plugins/khata")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap(),
            expected
        );

        assert_eq!(
            choose(
                &roots,
                &files,
                &AddOptions {
                    path: Some(PathBuf::from(".")),
                    ..AddOptions::default()
                },
                &plugin_dir,
                index,
            )
            .unwrap(),
            expected
        );
    }

    #[test]
    fn choose_holds_the_package_found_at_path_to_the_package_name_grammar() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();
        let plugin_dir = tmp.path().join("plugins/khata");
        write_local_plugin(&plugin_dir, "Khata Plugin");

        assert_eq!(
            choose(
                &roots,
                &files,
                &AddOptions {
                    path: Some(PathBuf::from("plugins/khata")),
                    ..AddOptions::default()
                },
                tmp.path(),
                index,
            )
            .unwrap_err(),
            "`Khata Plugin` is not a package name: lower-case letters, digits, - and _, starting with a letter or a digit, at most 64 characters"
        );
    }

    #[cfg(windows)]
    #[test]
    fn choose_takes_a_path_with_other_separators_and_case_inside_the_app() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let roots = roots_at(tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();
        write_local_plugin(&tmp.path().join("plugins/khata"), "navree-plugin-khata");

        for raw in [r"plugins\khata", r"PLUGINS\Khata"] {
            assert_eq!(
                choose(
                    &roots,
                    &files,
                    &AddOptions {
                        path: Some(PathBuf::from(raw)),
                        ..AddOptions::default()
                    },
                    tmp.path(),
                    index,
                )
                .unwrap(),
                Choice::Community {
                    package: "navree-plugin-khata".to_owned(),
                    how: How::Path {
                        dir: "plugins/khata".to_owned(),
                    },
                },
                "for {raw}"
            );
        }
    }

    #[test]
    fn choose_refuses_a_path_outside_the_app_that_does_not_exist_or_is_the_app_itself() {
        let outer = tempfile::tempdir().expect("outer tempdir");
        let app_dir = outer.path().join("app");
        fs::create_dir_all(&app_dir).expect("create app dir");
        let sibling = outer.path().join("khata");
        write_local_plugin(&sibling, "navree-plugin-khata");

        let roots = roots_at(&app_dir);
        let files = files_with_cargo(&roots, base_cargo_toml());
        let index = Index::embedded();
        let shown_root = std::path::absolute(&app_dir).unwrap();

        let other = tempfile::tempdir().expect("other tempdir");
        write_local_plugin(other.path(), "navree-plugin-khata");

        let mut outside_cases: Vec<PathBuf> = vec![
            PathBuf::from("../khata"),
            other.path().to_path_buf(),
            PathBuf::from("."),
        ];
        #[cfg(windows)]
        outside_cases.push(PathBuf::from(r"D:\khata"));

        for bad in outside_cases {
            let joined = if bad.is_absolute() {
                bad.clone()
            } else {
                app_dir.join(&bad)
            };
            let shown_dir = std::path::absolute(&joined).unwrap();
            let err = choose(
                &roots,
                &files,
                &AddOptions {
                    path: Some(bad.clone()),
                    ..AddOptions::default()
                },
                &app_dir,
                index,
            )
            .unwrap_err();
            assert_eq!(
                err,
                format!(
                    "--path {} is outside the app ({}); keep the plugin's folder inside the app",
                    shown_dir.display(),
                    shown_root.display()
                ),
                "for {bad:?}"
            );
        }

        let missing = PathBuf::from("plugins/missing");
        let shown_missing = std::path::absolute(app_dir.join(&missing)).unwrap();
        let missing_err = choose(
            &roots,
            &files,
            &AddOptions {
                path: Some(missing),
                ..AddOptions::default()
            },
            &app_dir,
            index,
        )
        .unwrap_err();
        assert_eq!(
            missing_err,
            format!("--path {} does not exist", shown_missing.display())
        );
    }

    #[cfg(windows)]
    #[test]
    fn plugin_add_refuses_a_junction_that_leaves_the_app() {
        let app_tmp = tempfile::tempdir().expect("app tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        write_local_plugin(outside.path(), "navree-plugin-khata");

        fs::create_dir_all(app_tmp.path().join("plugins")).expect("create plugins/");
        let junction = app_tmp.path().join("plugins/khata");
        let status = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &junction.to_string_lossy(),
                &outside.path().to_string_lossy(),
            ])
            .output()
            .expect("run mklink /J");
        assert!(status.status.success(), "mklink /J failed");

        let roots = roots_at(app_tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let shown_dir = std::path::absolute(&junction).unwrap();
        let shown_root = std::path::absolute(app_tmp.path()).unwrap();

        let err = choose(
            &roots,
            &files,
            &AddOptions {
                path: Some(PathBuf::from("plugins/khata")),
                ..AddOptions::default()
            },
            app_tmp.path(),
            Index::embedded(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            format!(
                "--path {} is outside the app ({}); keep the plugin's folder inside the app",
                shown_dir.display(),
                shown_root.display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn plugin_add_refuses_a_symbolic_link_that_leaves_the_app() {
        let app_tmp = tempfile::tempdir().expect("app tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        write_local_plugin(outside.path(), "navree-plugin-khata");

        fs::create_dir_all(app_tmp.path().join("plugins")).expect("create plugins/");
        let link = app_tmp.path().join("plugins/khata");
        std::os::unix::fs::symlink(outside.path(), &link).expect("create symlink");

        let roots = roots_at(app_tmp.path());
        let files = files_with_cargo(&roots, base_cargo_toml());
        let shown_dir = std::path::absolute(&link).unwrap();
        let shown_root = std::path::absolute(app_tmp.path()).unwrap();

        let err = choose(
            &roots,
            &files,
            &AddOptions {
                path: Some(PathBuf::from("plugins/khata")),
                ..AddOptions::default()
            },
            app_tmp.path(),
            Index::embedded(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            format!(
                "--path {} is outside the app ({}); keep the plugin's folder inside the app",
                shown_dir.display(),
                shown_root.display()
            )
        );
    }
}

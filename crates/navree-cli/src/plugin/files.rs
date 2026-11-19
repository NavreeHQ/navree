//! The app's roots and files, read once before planning edits, and atomic
//! whole-file writes with link checks, concurrent-edit checks, and rollback.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// Where an app's files are.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AppRoots {
    /// The package: its `Cargo.toml` and `src/`.
    pub package: PathBuf,
    /// Where `config/`, `.env`, `.env.*` and `.env.example` are, and where the app runs.
    pub config: PathBuf,
    /// The Cargo workspace root, whose `Cargo.lock` the snapshot takes.
    pub workspace: PathBuf,
}

/// The app's files `plugin:add`, `plugin:remove` and `plugin:list` read, read once. Every path is
/// relative with `/`: to `roots.package` for `Cargo.toml` and `src/`, to `roots.config` for the rest.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AppFiles {
    pub roots: AppRoots,
    pub cargo_toml: String,
    pub app_rs: Option<String>,
    /// Every `.rs` file under `src/`, in path order.
    pub src: Vec<(String, String)>,
    pub env_example: Option<String>,
    /// `config/*.toml`, `config/*.json`, `config/env/*.toml`, by relative path.
    pub config: BTreeMap<String, String>,
    /// `.env` and `.env.*` but `.env.example`, by name.
    pub dot_env: BTreeMap<String, String>,
    pub shell_rs: Option<String>,
}

/// The bytes of the package's `Cargo.toml` and the Cargo workspace's `Cargo.lock` before a change,
/// with an absent lock recorded as `None`.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    cargo_toml: Option<Vec<u8>>,
    cargo_lock: Option<Vec<u8>>,
}

/// What `write_all` wrote: each relative path and its exact bytes.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub(crate) struct Written {
    pub files: Vec<(String, Vec<u8>)>,
}

/// Why `write_all` refused or failed to write a file, and any already-written files that could not
/// be put back.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WriteFailed {
    pub path: String,
    pub problem: WriteProblem,
    pub not_put_back: Vec<String>,
}

/// What stopped a write in `write_all`.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum WriteProblem {
    Io(io::Error),
    Link,
    Changed,
}

/// Finds the app containing `cwd`: from `cwd` up, the first `Cargo.toml` with `[package]` that
/// names `navree` in `[dependencies]` and has no `[package.metadata.navree.plugin]`.
#[allow(dead_code)]
pub(crate) fn find_app(cwd: &Path) -> Result<AppRoots, String> {
    let shown = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    for dir in shown.ancestors() {
        let Ok(manifest) = fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        let Ok(parsed) = manifest.parse::<toml::Table>() else {
            continue;
        };
        if is_app_manifest(&parsed) {
            return Ok(roots_for_package(dir.to_path_buf()));
        }
    }
    Err(format!(
        "no Navree app here (no Cargo.toml with a navree dependency in {} or above)",
        shown.display()
    ))
}

fn is_app_manifest(parsed: &toml::Table) -> bool {
    let Some(package) = parsed.get("package").and_then(toml::Value::as_table) else {
        return false;
    };
    let has_plugin_table = package
        .get("metadata")
        .and_then(toml::Value::as_table)
        .and_then(|metadata| metadata.get("navree"))
        .and_then(toml::Value::as_table)
        .and_then(|navree| navree.get("plugin"))
        .is_some();
    if has_plugin_table {
        return false;
    }
    parsed
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|deps| deps.get("navree"))
        .is_some_and(|dep| matches!(dep, toml::Value::String(_) | toml::Value::Table(_)))
}

fn roots_for_package(package: PathBuf) -> AppRoots {
    for dir in package.ancestors() {
        let Ok(manifest) = fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        let Ok(parsed) = manifest.parse::<toml::Table>() else {
            continue;
        };
        if let Some(ws_table) = parsed.get("workspace").and_then(toml::Value::as_table) {
            let workspace = dir.to_path_buf();
            let config = if workspace == package
                || default_members_names_package(&workspace, ws_table, &package)
            {
                workspace.clone()
            } else {
                package.clone()
            };
            return AppRoots {
                package,
                config,
                workspace,
            };
        }
    }
    AppRoots {
        config: package.clone(),
        workspace: package.clone(),
        package,
    }
}

fn default_members_names_package(workspace: &Path, ws_table: &toml::Table, package: &Path) -> bool {
    let Some(members) = ws_table
        .get("default-members")
        .and_then(toml::Value::as_array)
    else {
        return false;
    };
    members
        .iter()
        .filter_map(toml::Value::as_str)
        .any(|member| {
            let joined = join_rel(workspace, &member.replace('\\', "/"));
            let resolved = std::path::absolute(&joined).unwrap_or(joined);
            if resolved == package {
                return true;
            }
            #[cfg(windows)]
            if let (Ok(real_member), Ok(real_package)) =
                (fs::canonicalize(&resolved), fs::canonicalize(package))
            {
                return real_member == real_package;
            }
            false
        })
}

#[allow(dead_code)]
impl AppRoots {
    /// The root folder `path` is relative to: `config` for `config/..`, `.env` and `.env.*`,
    /// `package` for everything else.
    fn root_of(&self, path: &str) -> &Path {
        if path == "config"
            || path.starts_with("config/")
            || path == ".env"
            || path.starts_with(".env.")
        {
            &self.config
        } else {
            &self.package
        }
    }

    /// The full path for a relative `path`: `config/..`, `.env` and `.env.*` live under `config`;
    /// every other path (`Cargo.toml`, `src/..`) lives under `package`.
    pub(crate) fn resolve(&self, path: &str) -> PathBuf {
        join_rel(self.root_of(path), path)
    }
}

#[allow(dead_code)]
impl AppFiles {
    /// Reads the app's files once: `Cargo.toml` and `src/` under `roots.package`, and `.env.example`,
    /// `config/*.toml`, `config/*.json`, `config/env/*.toml`, `.env` and `.env.*` under `roots.config`.
    pub(crate) fn read(roots: &AppRoots) -> anyhow::Result<AppFiles> {
        let cargo_toml = fs::read_to_string(roots.package.join("Cargo.toml"))?;
        let src = read_src(&roots.package)?;
        let app_rs = src
            .iter()
            .find(|(path, _)| path == "src/app.rs")
            .map(|(_, text)| text.clone());
        let shell_rs = src
            .iter()
            .find(|(path, _)| path == "src/ui/shell.rs")
            .map(|(_, text)| text.clone());
        let env_example = read_optional_string(&roots.config.join(".env.example"))?;

        let mut config = BTreeMap::new();
        let config_dir = roots.config.join("config");
        if fs::metadata(&config_dir).is_ok_and(|meta| meta.is_dir()) {
            read_config_dir(&config_dir, "config", true, &mut config)?;
            let env_dir = config_dir.join("env");
            if fs::metadata(&env_dir).is_ok_and(|meta| meta.is_dir()) {
                read_config_dir(&env_dir, "config/env", false, &mut config)?;
            }
        }

        let mut dot_env = BTreeMap::new();
        if fs::metadata(&roots.config).is_ok_and(|meta| meta.is_dir()) {
            for entry in fs::read_dir(&roots.config)? {
                let entry = entry?;
                let path = entry.path();
                let Ok(meta) = fs::metadata(&path) else {
                    continue;
                };
                if !meta.is_file() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if (name == ".env" || name.starts_with(".env.")) && name != ".env.example" {
                    dot_env.insert(name, fs::read_to_string(&path)?);
                }
            }
        }

        Ok(AppFiles {
            roots: roots.clone(),
            cargo_toml,
            app_rs,
            src,
            env_example,
            config,
            dot_env,
            shell_rs,
        })
    }

    /// The text `path` held when it was read (`Cargo.toml`, `src/..`, `.env.example`, `config/..`,
    /// `.env..`); `None` when it was absent.
    pub(crate) fn text(&self, path: &str) -> Option<&str> {
        match path {
            "Cargo.toml" => Some(self.cargo_toml.as_str()),
            "src/app.rs" => self.app_rs.as_deref().or_else(|| {
                self.src
                    .iter()
                    .find(|(rel, _)| rel == path)
                    .map(|(_, text)| text.as_str())
            }),
            "src/ui/shell.rs" => self.shell_rs.as_deref().or_else(|| {
                self.src
                    .iter()
                    .find(|(rel, _)| rel == path)
                    .map(|(_, text)| text.as_str())
            }),
            ".env.example" => self.env_example.as_deref(),
            rel if rel.starts_with("src/") => self
                .src
                .iter()
                .find(|(entry, _)| entry == rel)
                .map(|(_, text)| text.as_str()),
            rel if rel.starts_with("config/") => self.config.get(rel).map(String::as_str),
            rel if rel == ".env" || rel.starts_with(".env.") => {
                self.dot_env.get(rel).map(String::as_str)
            }
            _ => None,
        }
    }
}

fn read_config_dir(
    dir: &Path,
    prefix: &str,
    allow_json: bool,
    out: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(ext) = Path::new(&name).extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let keep =
            ext.eq_ignore_ascii_case("toml") || (allow_json && ext.eq_ignore_ascii_case("json"));
        if keep {
            out.insert(format!("{prefix}/{name}"), fs::read_to_string(&path)?);
        }
    }
    Ok(())
}

/// Every `.rs` file under `package/src/`, path relative with `/`, in path order.
#[allow(dead_code)]
pub(crate) fn read_src(package: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let src_dir = package.join("src");
    match fs::symlink_metadata(&src_dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    }
    let mut files = Vec::new();
    walk_rs(&src_dir, "src", &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn walk_rs(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let rel = format!("{prefix}/{name}");
        if meta.is_dir() {
            walk_rs(&path, &rel, out)?;
        } else if meta.is_file() && name.ends_with(".rs") {
            out.push((rel, fs::read_to_string(&path)?));
        }
    }
    Ok(())
}

#[allow(dead_code)]
impl Snapshot {
    /// Captures the package's `Cargo.toml` and the Cargo workspace's `Cargo.lock`.
    pub(crate) fn take(roots: &AppRoots) -> io::Result<Snapshot> {
        let cargo_toml = read_optional_bytes(&roots.package.join("Cargo.toml"))?;
        let cargo_lock = read_optional_bytes(&roots.workspace.join("Cargo.lock"))?;
        Ok(Snapshot {
            cargo_toml,
            cargo_lock,
        })
    }

    /// The bytes `Cargo.toml` held when the snapshot was taken.
    pub(crate) fn cargo_toml_bytes(&self) -> &[u8] {
        self.cargo_toml.as_deref().unwrap_or_default()
    }

    /// Puts `Cargo.toml` and the workspace's `Cargo.lock` back from the snapshot (deleting a lock
    /// that was absent), and restores every other file in `written` to the text `read` holds for it
    /// (deleting one that was absent). Any file in `written` is restored only if it still holds the
    /// exact bytes `write_all` wrote; when `Cargo.toml` was not in `written` and differs from the
    /// snapshot, it is restored only when it still holds `expected_cargo_toml`; otherwise it is
    /// left untouched and returned in the list.
    pub(crate) fn restore(
        &self,
        roots: &AppRoots,
        read: &AppFiles,
        written: &Written,
        expected_cargo_toml: &[u8],
    ) -> Vec<String> {
        let mut not_put_back = Vec::new();
        let mut cargo_toml_written: Option<&[u8]> = None;

        for (rel_path, written_bytes) in &written.files {
            if rel_path == "Cargo.toml" {
                cargo_toml_written = Some(written_bytes.as_slice());
                continue;
            }
            if !restore_written_file(roots, read, rel_path, written_bytes) {
                not_put_back.push(rel_path.clone());
            }
        }

        if let Some(written_bytes) = cargo_toml_written {
            let dest = roots.package.join("Cargo.toml");
            let unchanged = check_no_link(&roots.package, "Cargo.toml").is_ok()
                && fs::read(&dest).ok().as_deref() == Some(written_bytes);
            if !unchanged
                || !restore_snapshot_entry(&roots.package, "Cargo.toml", self.cargo_toml.as_deref())
            {
                not_put_back.push("Cargo.toml".to_owned());
            }
        } else if self.cargo_toml.as_deref() != Some(expected_cargo_toml) {
            let dest = roots.package.join("Cargo.toml");
            let unchanged = check_no_link(&roots.package, "Cargo.toml").is_ok()
                && fs::read(&dest).ok().as_deref() == Some(expected_cargo_toml);
            if !unchanged
                || !restore_snapshot_entry(&roots.package, "Cargo.toml", self.cargo_toml.as_deref())
            {
                not_put_back.push("Cargo.toml".to_owned());
            }
        }

        if !restore_snapshot_entry(&roots.workspace, "Cargo.lock", self.cargo_lock.as_deref()) {
            not_put_back.push("Cargo.lock".to_owned());
        }

        not_put_back
    }
}

fn restore_snapshot_entry(base: &Path, rel_path: &str, original: Option<&[u8]>) -> bool {
    if check_no_link(base, rel_path).is_err() {
        return false;
    }
    let dest = join_rel(base, rel_path);
    match original {
        Some(bytes) => {
            if fs::read(&dest).ok().as_deref() == Some(bytes) {
                return true;
            }
            let mut created_dirs = Vec::new();
            write_bytes_atomic(base, rel_path, bytes, &mut created_dirs, |_| Ok(())).is_ok()
        }
        None => remove_file_if_present(&dest).is_ok(),
    }
}

fn restore_written_file(
    roots: &AppRoots,
    read: &AppFiles,
    rel_path: &str,
    written_bytes: &[u8],
) -> bool {
    let base = roots.root_of(rel_path);
    if check_no_link(base, rel_path).is_err() {
        return false;
    }
    let dest = join_rel(base, rel_path);
    if fs::read(&dest).ok().as_deref() != Some(written_bytes) {
        return false;
    }
    match read.text(rel_path) {
        Some(previous) => {
            let mut created_dirs = Vec::new();
            write_bytes_atomic(
                base,
                rel_path,
                previous.as_bytes(),
                &mut created_dirs,
                |_| Ok(()),
            )
            .is_ok()
        }
        None => remove_file_if_present(&dest).is_ok(),
    }
}

/// Verifies that `rel_path` has no link on its path and still holds the text `read` recorded.
pub(crate) fn verify_unchanged(
    roots: &AppRoots,
    read: &AppFiles,
    rel_path: &str,
) -> Result<(), WriteFailed> {
    let base = roots.root_of(rel_path);
    if let Err(link_path) = check_no_link(base, rel_path) {
        return Err(WriteFailed {
            path: link_path,
            problem: WriteProblem::Link,
            not_put_back: Vec::new(),
        });
    }
    let dest = join_rel(base, rel_path);
    if let Err(problem) = check_unchanged_precheck(&dest, read.text(rel_path)) {
        return Err(WriteFailed {
            path: rel_path.to_owned(),
            problem,
            not_put_back: Vec::new(),
        });
    }
    Ok(())
}

/// Writes each `(relative path, text)` by sibling temporary file and rename, creating missing
/// folders, keeping `\r\n` on files that already used `\r\n`, and syncing each file before rename.
/// Every target is checked before the first write and again just before its own rename; on failure,
/// every file written earlier in this call is put back unless it changed since.
#[allow(dead_code)]
pub(crate) fn write_all(
    roots: &AppRoots,
    read: &AppFiles,
    edits: &[(String, String)],
) -> Result<Written, WriteFailed> {
    for (rel_path, _) in edits {
        let base = roots.root_of(rel_path);
        if let Err(link_path) = check_no_link(base, rel_path) {
            return Err(WriteFailed {
                path: link_path,
                problem: WriteProblem::Link,
                not_put_back: Vec::new(),
            });
        }
        let dest = join_rel(base, rel_path);
        if let Err(problem) = check_unchanged_precheck(&dest, read.text(rel_path)) {
            return Err(WriteFailed {
                path: rel_path.clone(),
                problem,
                not_put_back: Vec::new(),
            });
        }
    }

    let mut written = Written::default();
    let mut created_dirs = Vec::new();

    for (rel_path, new_text) in edits {
        let base = roots.root_of(rel_path);
        let expected = read.text(rel_path);
        let bytes = if expected.is_some_and(|old| old.contains("\r\n")) {
            new_text
                .replace("\r\n", "\n")
                .replace('\n', "\r\n")
                .into_bytes()
        } else {
            new_text.as_bytes().to_vec()
        };

        let result = write_bytes_atomic(base, rel_path, &bytes, &mut created_dirs, |dest| {
            if let Err(link_path) = check_no_link(base, rel_path) {
                return Err((link_path, WriteProblem::Link));
            }
            check_unchanged_before_rename(dest, expected)
                .map_err(|problem| (rel_path.clone(), problem))
        });

        match result {
            Ok(()) => written.files.push((rel_path.clone(), bytes)),
            Err((failed_path, problem)) => {
                let mut not_put_back = Vec::new();
                for (done_path, done_bytes) in &written.files {
                    if !restore_written_file(roots, read, done_path, done_bytes) {
                        not_put_back.push(done_path.clone());
                    }
                }
                for dir in created_dirs.iter().rev() {
                    let _ = fs::remove_dir(dir);
                }
                return Err(WriteFailed {
                    path: failed_path,
                    problem,
                    not_put_back,
                });
            }
        }
    }

    Ok(written)
}

#[allow(dead_code)]
impl WriteFailed {
    /// Formats the refusal or write error for `command` (`plugin:add` or `plugin:remove`),
    /// without a leading `error: `.
    pub(crate) fn message(&self, command: &str) -> String {
        let tail = if self.not_put_back.is_empty() {
            "nothing was changed".to_owned()
        } else {
            format!(
                "these files could not be put back: {}",
                self.not_put_back.join(", ")
            )
        };
        match &self.problem {
            WriteProblem::Io(err) => format!("cannot write {}: {err}; {tail}", self.path),
            WriteProblem::Link => format!("{} is a link; {tail}", self.path),
            WriteProblem::Changed => format!("{} changed while {command} ran; {tail}", self.path),
        }
    }
}

fn join_rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|seg| !seg.is_empty())
        .fold(base.to_path_buf(), |acc, seg| acc.join(seg))
}

fn read_optional_string(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn read_optional_bytes(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Checks every component of `rel_path` below `base` with `symlink_metadata`, returning the
/// relative path of the first symbolic link or Windows junction encountered.
fn check_no_link(base: &Path, rel_path: &str) -> Result<(), String> {
    for (end, _) in rel_path.match_indices('/') {
        let prefix = &rel_path[..end];
        if prefix.is_empty() {
            continue;
        }
        match fs::symlink_metadata(join_rel(base, prefix)) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(prefix.to_owned()),
            Ok(meta) if meta.is_dir() => {}
            _ => return Ok(()),
        }
    }
    if let Ok(meta) = fs::symlink_metadata(join_rel(base, rel_path))
        && meta.file_type().is_symlink()
    {
        return Err(rel_path.to_owned());
    }
    Ok(())
}

fn is_absent_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// Pre-write check: refuses files modified since `read` (or created when absent in `read`).
/// A directory in a missing file's place or an obstructed parent path is left for the write itself
/// to fail on with `WriteProblem::Io`.
fn check_unchanged_precheck(dest: &Path, expected: Option<&str>) -> Result<(), WriteProblem> {
    match expected {
        Some(expected_text) => match fs::symlink_metadata(dest) {
            Ok(meta) if meta.is_file() => match fs::read(dest) {
                Ok(bytes) if bytes == expected_text.as_bytes() => Ok(()),
                Ok(_) => Err(WriteProblem::Changed),
                Err(err) if is_absent_error(&err) => Err(WriteProblem::Changed),
                Err(_) => Ok(()),
            },
            Ok(_) => Err(WriteProblem::Changed),
            Err(err) if is_absent_error(&err) => Err(WriteProblem::Changed),
            Err(_) => Ok(()),
        },
        None => match fs::symlink_metadata(dest) {
            Ok(meta) if meta.is_dir() => Ok(()),
            Ok(_) => Err(WriteProblem::Changed),
            Err(_) => Ok(()),
        },
    }
}

/// Second check right before renaming the temporary file into place.
fn check_unchanged_before_rename(dest: &Path, expected: Option<&str>) -> Result<(), WriteProblem> {
    match expected {
        Some(expected_text) => match fs::symlink_metadata(dest) {
            Ok(meta) if meta.is_file() => match fs::read(dest) {
                Ok(bytes) if bytes == expected_text.as_bytes() => Ok(()),
                Ok(_) => Err(WriteProblem::Changed),
                Err(err) if is_absent_error(&err) => Err(WriteProblem::Changed),
                Err(err) => Err(WriteProblem::Io(err)),
            },
            Ok(_) => Err(WriteProblem::Changed),
            Err(err) if is_absent_error(&err) => Err(WriteProblem::Changed),
            Err(err) => Err(WriteProblem::Io(err)),
        },
        None => match fs::symlink_metadata(dest) {
            Ok(meta) if meta.is_dir() => Ok(()),
            Ok(_) => Err(WriteProblem::Changed),
            Err(err) if is_absent_error(&err) => Ok(()),
            Err(err) => Err(WriteProblem::Io(err)),
        },
    }
}

fn write_bytes_atomic(
    base: &Path,
    rel_path: &str,
    bytes: &[u8],
    created_dirs: &mut Vec<PathBuf>,
    before_rename: impl FnOnce(&Path) -> Result<(), (String, WriteProblem)>,
) -> Result<(), (String, WriteProblem)> {
    for (end, _) in rel_path.match_indices('/') {
        let folder_rel = &rel_path[..end];
        if folder_rel.is_empty() {
            continue;
        }
        let folder = join_rel(base, folder_rel);
        match fs::create_dir(&folder) {
            Ok(()) => created_dirs.push(folder),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                let meta = fs::symlink_metadata(&folder)
                    .map_err(|io_err| (rel_path.to_owned(), WriteProblem::Io(io_err)))?;
                if meta.file_type().is_symlink() {
                    return Err((folder_rel.to_owned(), WriteProblem::Link));
                }
            }
            Err(err) => return Err((rel_path.to_owned(), WriteProblem::Io(err))),
        }
    }

    let dest = join_rel(base, rel_path);
    let name = rel_path.rsplit_once('/').map_or(rel_path, |(_, file)| file);
    let temp_name = format!(".{name}.navree-add-{}.tmp", std::process::id());
    let temporary = dest.with_file_name(&temp_name);
    if let Ok(meta) = fs::symlink_metadata(&temporary)
        && meta.file_type().is_symlink()
    {
        return Err((rel_path.to_owned(), WriteProblem::Link));
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|err| (rel_path.to_owned(), WriteProblem::Io(err)))?;
    if let Err(err) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err((rel_path.to_owned(), WriteProblem::Io(err)));
    }
    drop(file);

    if let Err(problem) = before_rename(&dest) {
        let _ = fs::remove_file(&temporary);
        return Err(problem);
    }

    if let Err(err) = rename_file(&temporary, &dest) {
        let _ = fs::remove_file(&temporary);
        return Err((rel_path.to_owned(), WriteProblem::Io(err)));
    }

    Ok(())
}

#[cfg(windows)]
fn rename_file(from: &Path, to: &Path) -> io::Result<()> {
    for wait in [10, 40, 160, 640] {
        match fs::rename(from, to) {
            Err(error)
                if matches!(error.raw_os_error(), Some(5 | 32))
                    && !fs::symlink_metadata(to).is_ok_and(|meta| meta.is_dir()) =>
            {
                std::thread::sleep(std::time::Duration::from_millis(wait));
            }
            other => return other,
        }
    }
    fs::rename(from, to)
}

#[cfg(not(windows))]
fn rename_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn remove_file_if_present(path: &Path) -> io::Result<()> {
    for wait in [10, 40, 160, 640] {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) if matches!(err.raw_os_error(), Some(5 | 32)) => {
                std::thread::sleep(std::time::Duration::from_millis(wait));
            }
            Err(err) => return Err(err),
        }
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(not(windows))]
fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("navree-files-")
            .tempdir()
            .expect("temp dir")
    }

    fn roots(dir: &Path) -> AppRoots {
        let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        AppRoots {
            package: dir.clone(),
            config: dir.clone(),
            workspace: dir,
        }
    }

    fn write_str(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dir");
        }
        fs::write(path, text).expect("write file");
    }

    fn has_temp_files(dir: &Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains(".navree-add-") {
                return true;
            }
            if entry.path().is_dir() && has_temp_files(&entry.path()) {
                return true;
            }
        }
        false
    }

    #[cfg(windows)]
    fn make_junction(link: &Path, target: &Path) {
        let backslashed = |p: &Path| p.to_string_lossy().replace('/', "\\");
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(backslashed(link))
            .arg(backslashed(target))
            .output()
            .expect("cmd runs");
        assert!(
            output.status.success(),
            "mklink /J failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn a_failed_write_puts_back_every_file_already_written() {
        let dir = temp_dir();
        let roots = roots(dir.path());
        write_str(
            &roots.package.join("Cargo.toml"),
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
        );
        write_str(&roots.package.join("src/app.rs"), "// Harpreet Sandhu\n");
        let read = AppFiles::read(&roots).expect("read app");

        // Place a regular file at `config` so writing `config/workspace.toml` fails on the third edit.
        write_str(&roots.config.join("config"), "not a folder\n");

        let edits = vec![
            ("src/app.rs".to_owned(), "// Meera Sharma\n".to_owned()),
            (".env.example".to_owned(), "APP_KHATA__KEY=\n".to_owned()),
            (
                "config/workspace.toml".to_owned(),
                "[roles.member]\npermissions = [\"khata.read\"]\n".to_owned(),
            ),
            ("src/extra.rs".to_owned(), "pub struct Extra;\n".to_owned()),
            (
                "Cargo.toml".to_owned(),
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n".to_owned(),
            ),
        ];

        let err = write_all(&roots, &read, &edits).expect_err("third edit must fail");
        assert_eq!(err.path, "config/workspace.toml");
        assert!(matches!(err.problem, WriteProblem::Io(_)));
        assert!(err.not_put_back.is_empty());
        assert!(
            err.message("plugin:add")
                .starts_with("cannot write config/workspace.toml: ")
        );
        assert!(err.message("plugin:add").ends_with("; nothing was changed"));

        assert_eq!(
            fs::read_to_string(roots.package.join("src/app.rs")).unwrap(),
            "// Harpreet Sandhu\n"
        );
        assert!(!roots.config.join(".env.example").exists());
        assert!(!roots.package.join("src/extra.rs").exists());
        assert!(!has_temp_files(dir.path()));
    }

    #[test]
    fn files_are_replaced_by_rename_and_keep_their_line_endings() {
        let dir = temp_dir();
        let roots = roots(dir.path());
        write_str(
            &roots.package.join("Cargo.toml"),
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
        );
        write_str(
            &roots.package.join("src/app.rs"),
            "fn build() {\r\n    // navree:modules\r\n}\r\n",
        );
        write_str(&roots.config.join(".env.example"), "APP_KEY=\n");

        let read = AppFiles::read(&roots).expect("read app");
        let edits = vec![
            (
                "src/app.rs".to_owned(),
                "fn build() {\n    // khata\n    // navree:modules\n}\n".to_owned(),
            ),
            (
                ".env.example".to_owned(),
                "APP_KEY=\nAPP_KHATA__KEY=\n".to_owned(),
            ),
        ];

        write_all(&roots, &read, &edits).expect("writes succeed");

        let app_bytes = fs::read(roots.package.join("src/app.rs")).unwrap();
        assert_eq!(
            app_bytes,
            b"fn build() {\r\n    // khata\r\n    // navree:modules\r\n}\r\n"
        );
        let env_bytes = fs::read(roots.config.join(".env.example")).unwrap();
        assert_eq!(env_bytes, b"APP_KEY=\nAPP_KHATA__KEY=\n");
        assert!(!has_temp_files(dir.path()));
    }

    #[test]
    fn the_app_files_are_read_once_in_path_order() {
        let dir = temp_dir();
        let root = std::path::absolute(dir.path()).unwrap();
        write_str(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\ndefault-members = [\"crates/svc\"]\n",
        );
        let svc = root.join("crates/svc");
        write_str(
            &svc.join("Cargo.toml"),
            "[package]\nname = \"svc\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { workspace = true }\n",
        );
        write_str(&svc.join("src/b/c.rs"), "// c\n");
        write_str(&svc.join("src/b.rs"), "// b\n");
        write_str(&svc.join("src/a.rs"), "// a\n");
        write_str(&svc.join("src/app.rs"), "// app\n");
        write_str(&svc.join("src/ui/shell.rs"), "// shell\n");

        write_str(&root.join(".env.example"), "APP_KEY=\n");
        write_str(&root.join(".env"), "APP_ENV=local\n");
        write_str(&root.join(".env.local"), "APP_PORT=8080\n");
        write_str(&root.join("config/env/production.toml"), "[app]\n");
        write_str(&root.join("config/workspace.json"), "{}\n");
        write_str(&root.join("config/Workspace.TOML"), "[roles]\n");
        write_str(&root.join("config/ui.policy.JSON"), "{}\n");
        write_str(&root.join("config/notes.txt"), "Ludhiana\n");

        let roots = find_app(&svc).expect("micro app found");
        let files = AppFiles::read(&roots).expect("read files");

        let src_paths: Vec<&str> = files.src.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            src_paths,
            [
                "src/a.rs",
                "src/app.rs",
                "src/b.rs",
                "src/b/c.rs",
                "src/ui/shell.rs"
            ]
        );
        assert_eq!(files.app_rs.as_deref(), Some("// app\n"));
        assert_eq!(files.shell_rs.as_deref(), Some("// shell\n"));
        assert_eq!(files.env_example.as_deref(), Some("APP_KEY=\n"));

        assert!(files.config.contains_key("config/env/production.toml"));
        assert!(files.config.contains_key("config/workspace.json"));
        assert!(files.config.contains_key("config/Workspace.TOML"));
        assert!(files.config.contains_key("config/ui.policy.JSON"));
        assert!(!files.config.contains_key("config/notes.txt"));

        assert!(files.dot_env.contains_key(".env"));
        assert!(files.dot_env.contains_key(".env.local"));
        assert!(!files.dot_env.contains_key(".env.example"));
    }

    #[test]
    fn a_snapshot_puts_back_an_absent_lock_as_absent() {
        let dir = temp_dir();
        let roots = roots(dir.path());
        write_str(
            &roots.package.join("Cargo.toml"),
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
        );
        let read = AppFiles::read(&roots).expect("read app");
        let snap = Snapshot::take(&roots).expect("take snapshot");

        let after_add = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\nnavree-plugin-khata = \"0.1.0\"\n";
        write_str(&roots.workspace.join("Cargo.lock"), "# generated lock\n");
        write_str(&roots.package.join("Cargo.toml"), after_add);

        let not_put_back = snap.restore(&roots, &read, &Written::default(), after_add.as_bytes());
        assert!(not_put_back.is_empty());
        assert!(!roots.workspace.join("Cargo.lock").exists());
        assert_eq!(
            fs::read_to_string(roots.package.join("Cargo.toml")).unwrap(),
            read.cargo_toml
        );
    }

    #[test]
    fn a_snapshot_takes_the_cargo_workspaces_lock() {
        let dir = temp_dir();
        let root = std::path::absolute(dir.path()).unwrap();
        write_str(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\ndefault-members = [\"crates/svc\"]\n",
        );
        let original_lock = "# workspace lock for Amritsar\n";
        write_str(&root.join("Cargo.lock"), original_lock);
        let svc = root.join("crates/svc");
        let original_toml = "[package]\nname = \"svc\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { workspace = true }\n";
        write_str(&svc.join("Cargo.toml"), original_toml);

        let roots = find_app(&svc).expect("micro app");
        let read = AppFiles::read(&roots).expect("read app");
        let snap = Snapshot::take(&roots).expect("snapshot");

        let after_add = format!("{original_toml}navree-plugin-khata = \"0.1.0\"\n");
        write_str(&svc.join("Cargo.toml"), &after_add);
        write_str(&root.join("Cargo.lock"), "# changed lock\n");

        let not_put_back = snap.restore(&roots, &read, &Written::default(), after_add.as_bytes());
        assert!(not_put_back.is_empty());
        assert_eq!(
            fs::read_to_string(root.join("Cargo.lock")).unwrap(),
            original_lock
        );
        assert_eq!(
            fs::read_to_string(svc.join("Cargo.toml")).unwrap(),
            original_toml
        );
        assert!(!svc.join("Cargo.lock").exists());
    }

    #[test]
    fn find_app_takes_the_first_package_with_navree_and_no_plugin_table() {
        // Forms of the navree dependency: string, inline table, sub-table, workspace.
        for dep_section in [
            "[dependencies]\nnavree = \"1.4\"\n",
            "[dependencies]\nnavree = { version = \"1.4\", features = [\"postgres\"] }\n",
            "[dependencies.navree]\nversion = \"1.4\"\n",
            "[dependencies]\nnavree = { workspace = true }\n",
        ] {
            let dir = temp_dir();
            let app = std::path::absolute(dir.path().join("kesar")).unwrap();
            write_str(
                &app.join("Cargo.toml"),
                &format!("[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n{dep_section}"),
            );
            let deep = app.join("src/modules");
            fs::create_dir_all(&deep).unwrap();
            assert_eq!(find_app(&deep).unwrap().package, app);

            // Inside a plugin crate under the app: skips the plugin and finds the app.
            let plugin_src = app.join("plugins/khata/src");
            write_str(
                &app.join("plugins/khata/Cargo.toml"),
                "[package]\nname = \"navree-plugin-khata\"\nversion = \"0.1.0\"\n\n[package.metadata.navree.plugin]\nmodule = \"KhataModule::from_config()\"\n\n[dependencies]\nnavree = { version = \"1.4\", default-features = false }\n",
            );
            fs::create_dir_all(&plugin_src).unwrap();
            assert_eq!(find_app(&plugin_src).unwrap().package, app);

            // Inside a helper crate without a navree dependency: skips it and finds the app above.
            let helper = app.join("crates/helper");
            write_str(
                &helper.join("Cargo.toml"),
                "[package]\nname = \"helper\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n",
            );
            assert_eq!(find_app(&helper).unwrap().package, app);
        }

        // Workspace root with no `[package]` is not an app.
        let ws_only = temp_dir();
        let ws_path = std::path::absolute(ws_only.path()).unwrap();
        write_str(
            &ws_path.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\ndefault-members = [\"crates/svc\"]\n",
        );
        let expected_err = format!(
            "no Navree app here (no Cargo.toml with a navree dependency in {} or above)",
            ws_path.display()
        );
        assert_eq!(find_app(ws_only.path()).unwrap_err(), expected_err);

        // Empty folder with no app anywhere above.
        let empty = temp_dir();
        let empty_abs = std::path::absolute(empty.path()).unwrap();
        assert_eq!(
            find_app(empty.path()).unwrap_err(),
            format!(
                "no Navree app here (no Cargo.toml with a navree dependency in {} or above)",
                empty_abs.display()
            )
        );
    }

    #[test]
    fn the_roots_of_a_generated_app_a_micro_app_and_a_member_app() {
        // 1. Generated app (has its own `[workspace]`).
        let gen_dir = temp_dir();
        let gen_root = std::path::absolute(gen_dir.path()).unwrap();
        write_str(
            &gen_root.join("Cargo.toml"),
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n\n[workspace]\n",
        );
        assert_eq!(
            find_app(&gen_root).unwrap(),
            AppRoots {
                package: gen_root.clone(),
                config: gen_root.clone(),
                workspace: gen_root,
            }
        );

        // 2. Micro layout (workspace root has `default-members = ["crates/svc"]`).
        let micro_dir = temp_dir();
        let micro_root = std::path::absolute(micro_dir.path()).unwrap();
        write_str(
            &micro_root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\ndefault-members = [\"crates/svc\"]\n",
        );
        let svc = micro_root.join("crates").join("svc");
        write_str(
            &svc.join("Cargo.toml"),
            "[package]\nname = \"svc\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { workspace = true }\n",
        );
        let micro_roots = find_app(&svc).unwrap();
        assert_eq!(
            micro_roots,
            AppRoots {
                package: svc.clone(),
                config: micro_root.clone(),
                workspace: micro_root.clone(),
            }
        );
        assert_eq!(
            micro_roots.resolve("config/workspace.toml"),
            micro_root.join("config").join("workspace.toml")
        );
        assert_eq!(
            micro_roots.resolve(".env.example"),
            micro_root.join(".env.example")
        );
        assert_eq!(
            micro_roots.resolve("src/app.rs"),
            svc.join("src").join("app.rs")
        );
        assert_eq!(micro_roots.resolve("Cargo.toml"), svc.join("Cargo.toml"));

        // 3. Member of a workspace without `default-members`.
        let member_ws_dir = temp_dir();
        let member_ws_root = std::path::absolute(member_ws_dir.path()).unwrap();
        write_str(
            &member_ws_root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        );
        let member = member_ws_root.join("crates").join("svc");
        write_str(
            &member.join("Cargo.toml"),
            "[package]\nname = \"svc\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
        );
        assert_eq!(
            find_app(&member).unwrap(),
            AppRoots {
                package: member.clone(),
                config: member,
                workspace: member_ws_root,
            }
        );

        // 4. Lone crate with no `[workspace]`.
        let lone_dir = temp_dir();
        let lone_root = std::path::absolute(lone_dir.path()).unwrap();
        write_str(
            &lone_root.join("Cargo.toml"),
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
        );
        assert_eq!(
            find_app(&lone_root).unwrap(),
            AppRoots {
                package: lone_root.clone(),
                config: lone_root.clone(),
                workspace: lone_root,
            }
        );
    }

    #[test]
    fn a_file_that_changed_since_it_was_read_is_not_overwritten() {
        let dir = temp_dir();
        let roots = roots(dir.path());
        write_str(
            &roots.package.join("Cargo.toml"),
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
        );
        write_str(&roots.package.join("src/app.rs"), "// before\n");
        let read = AppFiles::read(&roots).expect("read app");

        // Owner edits `src/app.rs` while cargo runs.
        write_str(&roots.package.join("src/app.rs"), "// owner edit in Pune\n");

        let edits = vec![
            (".env.example".to_owned(), "APP_KHATA__KEY=\n".to_owned()),
            ("src/app.rs".to_owned(), "// tool edit\n".to_owned()),
        ];
        let err = write_all(&roots, &read, &edits).expect_err("changed file refused");
        assert_eq!(err.path, "src/app.rs");
        assert!(matches!(err.problem, WriteProblem::Changed));
        assert_eq!(
            err.message("plugin:add"),
            "src/app.rs changed while plugin:add ran; nothing was changed"
        );
        assert_eq!(
            fs::read_to_string(roots.package.join("src/app.rs")).unwrap(),
            "// owner edit in Pune\n"
        );
        assert!(!roots.config.join(".env.example").exists());

        // Restore `src/app.rs`, then create `.env.example` which was absent when read.
        write_str(&roots.package.join("src/app.rs"), "// before\n");
        write_str(&roots.config.join(".env.example"), "APP_KEY=created\n");
        let err = write_all(&roots, &read, &edits).expect_err("newly created file refused");
        assert_eq!(err.path, ".env.example");
        assert!(matches!(err.problem, WriteProblem::Changed));
        assert_eq!(
            fs::read_to_string(roots.config.join(".env.example")).unwrap(),
            "APP_KEY=created\n"
        );
    }

    #[test]
    fn a_put_back_leaves_a_file_that_changed_since_the_tool_wrote_it() {
        let dir = temp_dir();
        let roots = roots(dir.path());
        let orig_toml = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n";
        let orig_lock = "# original lock\n";
        write_str(&roots.package.join("Cargo.toml"), orig_toml);
        write_str(&roots.workspace.join("Cargo.lock"), orig_lock);
        write_str(&roots.package.join("src/app.rs"), "// original app\n");

        let read = AppFiles::read(&roots).expect("read app");
        let snap = Snapshot::take(&roots).expect("snapshot");

        let after_add = format!("{orig_toml}navree-plugin-khata = \"0.1.0\"\n");
        write_str(&roots.package.join("Cargo.toml"), &after_add);
        write_str(&roots.workspace.join("Cargo.lock"), "# updated lock\n");

        let written = write_all(
            &roots,
            &read,
            &[("src/app.rs".to_owned(), "// wired by tool\n".to_owned())],
        )
        .expect("write_all");

        // Owner edits `src/app.rs` after `write_all` wrote it.
        write_str(&roots.package.join("src/app.rs"), "// Simran Kaur's edit\n");

        let not_put_back = snap.restore(&roots, &read, &written, after_add.as_bytes());
        assert_eq!(not_put_back, vec!["src/app.rs".to_owned()]);
        assert_eq!(
            fs::read_to_string(roots.package.join("src/app.rs")).unwrap(),
            "// Simran Kaur's edit\n"
        );
        assert_eq!(
            fs::read_to_string(roots.package.join("Cargo.toml")).unwrap(),
            orig_toml
        );
        assert_eq!(
            fs::read_to_string(roots.workspace.join("Cargo.lock")).unwrap(),
            orig_lock
        );
    }

    #[test]
    fn a_link_in_a_files_place_or_on_its_path_is_refused() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            // 1. `config/workspace.toml` is a symbolic link to another file.
            let dir = temp_dir();
            let roots1 = roots(dir.path());
            write_str(
                &roots1.package.join("Cargo.toml"),
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
            );
            let read = AppFiles::read(&roots1).expect("read app");
            let outside_file = dir.path().join("outside.toml");
            write_str(&outside_file, "# Jalandhar\n");
            fs::create_dir_all(roots1.config.join("config")).unwrap();
            symlink(&outside_file, roots1.config.join("config/workspace.toml")).unwrap();

            let err = write_all(
                &roots1,
                &read,
                &[(
                    "config/workspace.toml".to_owned(),
                    "[roles.member]\n".to_owned(),
                )],
            )
            .expect_err("symlink file refused");
            assert_eq!(err.path, "config/workspace.toml");
            assert!(matches!(err.problem, WriteProblem::Link));
            assert_eq!(
                err.message("plugin:add"),
                "config/workspace.toml is a link; nothing was changed"
            );
            assert!(
                fs::symlink_metadata(roots1.config.join("config/workspace.toml"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(fs::read_to_string(&outside_file).unwrap(), "# Jalandhar\n");

            // 2. `config` folder is a symbolic link.
            let dir2 = temp_dir();
            let roots2 = roots(dir2.path());
            write_str(
                &roots2.package.join("Cargo.toml"),
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
            );
            let read2 = AppFiles::read(&roots2).expect("read app");
            let outside_dir = temp_dir();
            symlink(outside_dir.path(), roots2.config.join("config")).unwrap();

            let err2 = write_all(
                &roots2,
                &read2,
                &[(
                    "config/workspace.toml".to_owned(),
                    "[roles.member]\n".to_owned(),
                )],
            )
            .expect_err("symlink folder refused");
            assert_eq!(err2.path, "config");
            assert!(matches!(err2.problem, WriteProblem::Link));
            assert_eq!(
                err2.message("plugin:add"),
                "config is a link; nothing was changed"
            );
            assert!(!outside_dir.path().join("workspace.toml").exists());
        }

        #[cfg(windows)]
        {
            // 1. `config` folder is a junction.
            let dir = temp_dir();
            let roots1 = roots(dir.path());
            write_str(
                &roots1.package.join("Cargo.toml"),
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
            );
            let read = AppFiles::read(&roots1).expect("read app");
            let outside_dir = temp_dir();
            make_junction(&roots1.config.join("config"), outside_dir.path());

            let err = write_all(
                &roots1,
                &read,
                &[(
                    "config/workspace.toml".to_owned(),
                    "[roles.member]\n".to_owned(),
                )],
            )
            .expect_err("junction folder refused");
            assert_eq!(err.path, "config");
            assert!(matches!(err.problem, WriteProblem::Link));
            assert_eq!(
                err.message("plugin:add"),
                "config is a link; nothing was changed"
            );
            assert!(
                fs::symlink_metadata(roots1.config.join("config"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert!(!outside_dir.path().join("workspace.toml").exists());

            // 2. `config/workspace.toml` itself is a junction in the file's place.
            let dir2 = temp_dir();
            let roots2 = roots(dir2.path());
            write_str(
                &roots2.package.join("Cargo.toml"),
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
            );
            let read2 = AppFiles::read(&roots2).expect("read app");
            fs::create_dir_all(roots2.config.join("config")).unwrap();
            make_junction(
                &roots2.config.join("config/workspace.toml"),
                outside_dir.path(),
            );

            let err2 = write_all(
                &roots2,
                &read2,
                &[(
                    "config/workspace.toml".to_owned(),
                    "[roles.member]\n".to_owned(),
                )],
            )
            .expect_err("junction at target refused");
            assert_eq!(err2.path, "config/workspace.toml");
            assert!(matches!(err2.problem, WriteProblem::Link));
            assert_eq!(
                err2.message("plugin:add"),
                "config/workspace.toml is a link; nothing was changed"
            );
        }
    }

    #[test]
    fn a_stale_temporary_file_from_a_killed_run_does_not_block_a_later_run() {
        let dir = temp_dir();
        let roots = roots(dir.path());
        write_str(
            &roots.package.join("Cargo.toml"),
            "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
        );
        write_str(&roots.package.join("src/app.rs"), "// before\n");
        let read = AppFiles::read(&roots).expect("read app");

        let stale = roots
            .package
            .join("src")
            .join(format!(".app.rs.navree-add-{}.tmp", std::process::id()));
        write_str(
            &stale,
            "// leftover longer content from an interrupted run in Ludhiana\n",
        );

        write_all(
            &roots,
            &read,
            &[("src/app.rs".to_owned(), "// after\n".to_owned())],
        )
        .expect("stale temp file truncated and replaced");

        assert_eq!(
            fs::read_to_string(roots.package.join("src/app.rs")).unwrap(),
            "// after\n"
        );
        assert!(!stale.exists());
        assert!(!has_temp_files(dir.path()));
    }

    #[test]
    fn a_snapshot_leaves_cargo_toml_when_it_changed_after_cargo_add() {
        let dir = temp_dir();
        let roots = roots(dir.path());
        let orig_toml = "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n";
        write_str(&roots.package.join("Cargo.toml"), orig_toml);
        let read = AppFiles::read(&roots).expect("read app");
        let snap = Snapshot::take(&roots).expect("snapshot");

        let after_cargo_add = format!("{orig_toml}navree-plugin-khata = \"0.1.0\"\n");
        let owner_edited = format!("{after_cargo_add}tracing = \"0.1\"\n");
        write_str(&roots.package.join("Cargo.toml"), &owner_edited);
        write_str(&roots.workspace.join("Cargo.lock"), "# lock\n");

        let not_put_back = snap.restore(
            &roots,
            &read,
            &Written::default(),
            after_cargo_add.as_bytes(),
        );
        assert_eq!(not_put_back, vec!["Cargo.toml".to_owned()]);
        assert_eq!(
            fs::read_to_string(roots.package.join("Cargo.toml")).unwrap(),
            owner_edited
        );
        assert!(!roots.workspace.join("Cargo.lock").exists());
    }

    #[test]
    fn linked_config_and_dotenv_files_are_read_while_writes_still_refuse_links() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let dir = temp_dir();
            let roots1 = roots(dir.path());
            write_str(
                &roots1.package.join("Cargo.toml"),
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
            );
            let shared = temp_dir();
            write_str(
                &shared.path().join("app.toml"),
                "[workspace.roles.member]\npermissions = [\"khata.internal\"]\n",
            );
            write_str(
                &shared.path().join("shared.env"),
                "APP_WORKSPACE__ROLES__SUPPORT__PERMISSIONS=tickets.reply\n",
            );
            fs::create_dir_all(roots1.config.join("config")).unwrap();
            symlink(
                shared.path().join("app.toml"),
                roots1.config.join("config/app.toml"),
            )
            .unwrap();
            symlink(shared.path().join("shared.env"), roots1.config.join(".env")).unwrap();

            let files = AppFiles::read(&roots1).expect("read app with symlinked config and .env");
            assert!(files.config.contains_key("config/app.toml"));
            assert!(files.dot_env.contains_key(".env"));
        }

        #[cfg(windows)]
        {
            let dir = temp_dir();
            let roots1 = roots(dir.path());
            write_str(
                &roots1.package.join("Cargo.toml"),
                "[package]\nname = \"kesar\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = \"1.4.0\"\n",
            );
            let shared_cfg = temp_dir();
            write_str(
                &shared_cfg.path().join("app.toml"),
                "[workspace.roles.member]\npermissions = [\"khata.internal\"]\n",
            );
            make_junction(&roots1.config.join("config"), shared_cfg.path());

            let files = AppFiles::read(&roots1).expect("read app with junction config");
            assert!(files.config.contains_key("config/app.toml"));
        }
    }
}

//! `navree dev`: build, serve, and on every change rebuild and restart.
//!
//! The loop is `cargo build` then `<app> serve`, run from a copy of the
//! binary: the next build can replace the original while the current server
//! keeps answering (Windows locks a running executable), and the server is
//! restarted only once the new build succeeds. When the build fails, the
//! previous build keeps serving. A `web/` front end (`package.json`) gets its
//! own dev server, started once.

use std::path::{Component, Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, bail};
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;

use crate::forward::Invocation;
use crate::project::package_name;

/// Options of the dev loop.
#[derive(Debug, Clone)]
pub struct DevOptions {
    /// How long the files must be quiet before a rebuild.
    pub debounce: Duration,
    /// Start the front end's dev server when the app has `web/package.json`.
    pub web: bool,
    /// Arguments for the app's `serve` command.
    pub serve_args: Vec<String>,
    /// Environment variables for the server (the app's Dock settings).
    pub env: Vec<(String, String)>,
}

/// Directories whose changes need a rebuild (embedded templates live in
/// `src/`; template files under `resources/` reload without one; `crates/`
/// holds the members of a workspace app).
const WATCHED_DIRS: &[&str] = &["src", "config", "migrations", "crates"];

/// Files at the app root whose changes need a rebuild or a restart.
const WATCHED_FILES: &[&str] = &["Cargo.toml", ".env", "build.rs"];

/// Whether a change of `path` (in the app at `root`) needs a rebuild:
/// sources, configuration and the manifest do; build output, runtime data,
/// the front end, template files and editor scratch files do not.
pub fn triggers_rebuild(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let Some(file_name) = relative.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let scratch = file_name.ends_with('~')
        || file_name.starts_with(".#")
        || [".swp", ".swx", ".tmp"]
            .iter()
            .any(|ext| file_name.ends_with(ext))
        || file_name == "4913";
    if scratch {
        return false;
    }
    let mut components = relative.components();
    let Some(Component::Normal(first)) = components.next() else {
        return false;
    };
    let first = first.to_str().unwrap_or_default();
    if components.next().is_none() {
        WATCHED_FILES.contains(&first)
    } else {
        WATCHED_DIRS.contains(&first)
    }
}

/// `cargo build`, reporting artifacts as JSON on stdout and diagnostics as
/// usual on stderr.
pub fn build_invocation() -> Invocation {
    Invocation::new(
        "cargo",
        ["build", "--message-format=json-render-diagnostics"],
    )
}

/// The binary built, from `cargo build`'s JSON messages: the one named
/// `preferred` (the package name) if any, else the first binary.
pub fn find_executable(stdout: &str, preferred: Option<&str>) -> Option<PathBuf> {
    let binaries: Vec<(String, PathBuf)> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter(|message| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k == "bin"))
        })
        .filter_map(|message| {
            let name = message["target"]["name"].as_str()?.to_owned();
            let executable = message["executable"].as_str()?;
            Some((name, PathBuf::from(executable)))
        })
        .collect();
    binaries
        .iter()
        .find(|(name, _)| Some(name.as_str()) == preferred)
        .or_else(|| binaries.first())
        .map(|(_, path)| path.clone())
}

/// The command name of a Node package manager (`npm.cmd` on Windows).
pub(crate) fn node_program(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_owned()
    }
}

/// The front end's dev server (`<pm> run dev` in `web/`), chosen by its
/// lockfile, else by `packageManager` in `package.json` (`"pnpm@10.0.0"`),
/// else npm, when the app has `web/package.json`.
pub fn web_invocation(root: &Path) -> Option<Invocation> {
    let web = root.join("web");
    if !web.join("package.json").is_file() {
        return None;
    }
    Some(Invocation::new(
        node_program(package_manager(&web)),
        ["run", "dev"],
    ))
}

/// The package manager of the front end in `web`: `pnpm`, `yarn` or `npm`,
/// chosen by its lockfile, else by `packageManager` in `package.json`
/// (`"pnpm@10.0.0"`, as in a fresh starter kit), else npm.
pub(crate) fn package_manager(web: &Path) -> &'static str {
    if web.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if web.join("yarn.lock").is_file() {
        "yarn"
    } else {
        declared_package_manager(web).unwrap_or("npm")
    }
}

/// The manager named by `packageManager` in `web/package.json`, when it is
/// one of `pnpm`, `yarn` or `npm`.
fn declared_package_manager(web: &Path) -> Option<&'static str> {
    let package = std::fs::read_to_string(web.join("package.json")).ok()?;
    let package = serde_json::from_str::<serde_json::Value>(&package).ok()?;
    let field = package.get("packageManager")?.as_str()?;
    let name = field.split('@').next()?;
    ["pnpm", "yarn", "npm"]
        .into_iter()
        .find(|manager| *manager == name)
}

/// A child process killed when dropped.
struct KillOnDrop {
    child: Child,
    /// The copy of the binary it runs, removed after it exits.
    copy: Option<PathBuf>,
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        // Already exited is fine; there is nothing else to do on failure.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(copy) = &self.copy {
            let _ = std::fs::remove_file(copy);
        }
    }
}

/// Runs `cargo build` in `root` and returns the app binary.
fn build(root: &Path, package: Option<&str>) -> anyhow::Result<PathBuf> {
    let output = build_invocation()
        .command(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .context("cannot run cargo")?;
    if !output.status.success() {
        bail!("cargo build failed");
    }
    find_executable(&String::from_utf8_lossy(&output.stdout), package)
        .context("cargo build produced no binary")
}

/// Starts `<copy of built> serve <args>` in `root`, with `env` set.
fn serve(
    root: &Path,
    built: &Path,
    generation: u64,
    args: &[String],
    env: &[(String, String)],
) -> anyhow::Result<KillOnDrop> {
    let dir = built
        .parent()
        .context("the binary has no directory")?
        .join("navree-dev");
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let stem = built.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    let mut name = format!("{stem}-{generation}");
    if let Some(ext) = built.extension().and_then(|e| e.to_str()) {
        name = format!("{name}.{ext}");
    }
    let copy = dir.join(name);
    std::fs::copy(built, &copy)
        .with_context(|| format!("cannot copy {} to {}", built.display(), copy.display()))?;
    let child = std::process::Command::new(&copy)
        .arg("serve")
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(root)
        .spawn()
        .with_context(|| format!("cannot start {}", copy.display()))?;
    Ok(KillOnDrop {
        child,
        copy: Some(copy),
    })
}

/// Builds and (re)starts the server; on a failed build keeps `current`.
fn rebuild(
    root: &Path,
    package: Option<&str>,
    options: &DevOptions,
    generation: &mut u64,
    current: &mut Option<KillOnDrop>,
) {
    match build(root, package) {
        Ok(built) => {
            // Stop the old server first: the new one binds the same port.
            current.take();
            *generation += 1;
            match serve(root, &built, *generation, &options.serve_args, &options.env) {
                Ok(server) => *current = Some(server),
                Err(error) => eprintln!("[navree dev] {error:#}"),
            }
        }
        Err(error) if current.is_some() => {
            eprintln!("[navree dev] {error:#}; still serving the previous build");
        }
        Err(error) => eprintln!("[navree dev] {error:#}; waiting for changes"),
    }
}

/// Runs the dev loop for the app at `root` until interrupted.
pub fn run(root: &Path, options: &DevOptions) -> anyhow::Result<()> {
    let package = package_name(root);
    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(options.debounce, tx).context("cannot watch files")?;
    let watcher = debouncer.watcher();
    watcher
        .watch(root, RecursiveMode::NonRecursive)
        .with_context(|| format!("cannot watch {}", root.display()))?;
    for dir in WATCHED_DIRS {
        let path = root.join(dir);
        if path.is_dir() {
            watcher
                .watch(&path, RecursiveMode::Recursive)
                .with_context(|| format!("cannot watch {}", path.display()))?;
        }
    }

    let _web = match web_invocation(root).filter(|_| options.web) {
        Some(web) => {
            println!("[navree dev] starting the front end: {web}");
            match web.command(&root.join("web")).spawn() {
                Ok(child) => Some(KillOnDrop { child, copy: None }),
                Err(error) => {
                    eprintln!("[navree dev] cannot start `{web}`: {error}");
                    None
                }
            }
        }
        None => None,
    };

    let mut generation = 0;
    let mut server = None;
    println!("[navree dev] building");
    rebuild(
        root,
        package.as_deref(),
        options,
        &mut generation,
        &mut server,
    );
    for events in rx {
        let events = match events {
            Ok(events) => events,
            Err(error) => {
                eprintln!("[navree dev] watch error: {error}");
                continue;
            }
        };
        let Some(changed) = events.iter().find(|e| triggers_rebuild(root, &e.path)) else {
            continue;
        };
        let shown = changed.path.strip_prefix(root).unwrap_or(&changed.path);
        println!("[navree dev] {} changed, rebuilding", shown.display());
        rebuild(
            root,
            package.as_deref(),
            options,
            &mut generation,
            &mut server,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    #[test]
    fn source_and_config_changes_trigger_a_rebuild() {
        let root = Path::new("/app");
        for path in [
            "/app/src/main.rs",
            "/app/src/modules/posts/mail/welcome.html",
            "/app/config/app.toml",
            "/app/Cargo.toml",
            "/app/.env",
            "/app/build.rs",
        ] {
            assert!(triggers_rebuild(root, Path::new(path)), "{path}");
        }
    }

    #[test]
    fn build_output_runtime_data_and_editor_files_do_not() {
        let root = Path::new("/app");
        for path in [
            "/app/target/debug/app.exe",
            "/app/storage/app.db",
            "/app/storage/app.db-wal",
            "/app/web/app/page.tsx",
            "/app/resources/views/mail/welcome.html",
            "/app/.git/index",
            "/app/src/main.rs~",
            "/app/src/.main.rs.swp",
            "/app/src/4913",
            "/app/README.md",
            "/elsewhere/src/main.rs",
        ] {
            assert!(!triggers_rebuild(root, Path::new(path)), "{path}");
        }
    }

    #[test]
    fn build_reports_artifacts_as_json() {
        assert_eq!(
            build_invocation().to_string(),
            "cargo build --message-format=json-render-diagnostics"
        );
    }

    #[test]
    fn finds_the_app_executable_in_cargo_messages() {
        let stdout = r#"{"reason":"compiler-artifact","package_id":"path+file:///C:/k/crates/navree#0.1.0","target":{"kind":["lib"],"name":"navree"},"executable":null}
{"reason":"compiler-artifact","package_id":"path+file:///C:/app#blog@0.1.0","target":{"kind":["bin"],"name":"seed"},"executable":"C:\\app\\target\\debug\\seed.exe"}
{"reason":"compiler-artifact","package_id":"path+file:///C:/app#blog@0.1.0","target":{"kind":["bin"],"name":"blog"},"executable":"C:\\app\\target\\debug\\blog.exe"}
{"reason":"build-finished","success":true}
"#;
        assert_eq!(
            find_executable(stdout, Some("blog")),
            Some(PathBuf::from(r"C:\app\target\debug\blog.exe"))
        );
        assert_eq!(
            find_executable(stdout, Some("other")),
            Some(PathBuf::from(r"C:\app\target\debug\seed.exe"))
        );
        assert_eq!(find_executable("not json\n", None), None);
    }

    #[test]
    fn web_dev_server_follows_the_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(web_invocation(dir.path()), None);
        let web = dir.path().join("web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(web.join("package.json"), "{}").unwrap();
        assert_eq!(
            web_invocation(dir.path()).map(|i| i.args),
            Some(vec!["run".to_owned(), "dev".to_owned()])
        );
        assert!(
            web_invocation(dir.path())
                .unwrap()
                .program
                .starts_with("npm")
        );
        std::fs::write(web.join("pnpm-lock.yaml"), "").unwrap();
        let pnpm = web_invocation(dir.path()).unwrap();
        assert!(pnpm.program.starts_with("pnpm"), "{pnpm}");
    }

    /// A fresh starter kit has no lockfile yet: `packageManager` decides.
    #[test]
    fn web_dev_server_follows_the_package_manager_field() {
        let dir = tempfile::tempdir().unwrap();
        let web = dir.path().join("web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(
            web.join("package.json"),
            r#"{"name":"acme-web","packageManager":"pnpm@11.25.0"}"#,
        )
        .unwrap();
        let pnpm = web_invocation(dir.path()).unwrap();
        assert!(pnpm.program.starts_with("pnpm"), "{pnpm}");
        std::fs::write(
            web.join("package.json"),
            r#"{"packageManager":"bun@1.2.0"}"#,
        )
        .unwrap();
        let npm = web_invocation(dir.path()).unwrap();
        assert!(npm.program.starts_with("npm"), "{npm}");
    }

    #[test]
    fn workspace_crates_trigger_a_rebuild() {
        let root = Path::new("/billing");
        assert!(triggers_rebuild(
            root,
            Path::new("/billing/crates/billing/src/app.rs")
        ));
        assert!(triggers_rebuild(
            root,
            Path::new("/billing/crates/billing-contracts/src/lib.rs")
        ));
    }
}

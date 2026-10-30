//! `navree build:image`: an OCI image of the app, built with Docker or
//! Podman from a two-stage Dockerfile (Rust on Debian bookworm, then a slim
//! Debian runtime with the binary, `config/` and `resources/`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;

use crate::build::{container_engine, relative};
use crate::error::{HarborError, Result};
use crate::exec::{Cmd, Executor, run_checked, slash_path};
use crate::render;

/// What the image holds: the app and its build settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageApp {
    /// The package (and binary) name.
    pub app: String,
    /// The app's directory.
    pub dir: PathBuf,
    /// The Cargo workspace root (the build context).
    pub workspace_root: PathBuf,
    /// Cargo features of the build.
    pub features: Vec<String>,
    /// Directories copied next to the binary, when present.
    pub include: Vec<String>,
}

impl ImageApp {
    /// The app's directory relative to the context, with a trailing `/`
    /// (empty when the app is the workspace root).
    pub fn app_dir(&self) -> String {
        let rel = relative(&self.dir, &self.workspace_root);
        if rel.is_empty() {
            rel
        } else {
            format!("{rel}/")
        }
    }

    /// The generated `Dockerfile`.
    pub fn dockerfile(&self) -> Result<String> {
        let include: Vec<&String> = self
            .include
            .iter()
            .filter(|d| self.dir.join(d).is_dir())
            .collect();
        render::render(
            "image/Dockerfile",
            json!({
                "app": self.app,
                "app_dir": self.app_dir(),
                "features": self.features,
                "include": include,
                "locked": self.workspace_root.join("Cargo.lock").is_file(),
            }),
        )
    }

    /// The generated `.dockerignore`.
    pub fn dockerignore(&self) -> Result<String> {
        render::render("image/dockerignore", json!({ "app_dir": self.app_dir() }))
    }

    /// Where `--dockerfile` writes the files: `Dockerfile` in the app, and
    /// `.dockerignore` next to it when the app is the build context
    /// (`Dockerfile.dockerignore` otherwise, which BuildKit reads for that
    /// Dockerfile).
    pub fn own_files(&self) -> (PathBuf, PathBuf) {
        let ignore = if self.app_dir().is_empty() {
            ".dockerignore"
        } else {
            "Dockerfile.dockerignore"
        };
        (self.dir.join("Dockerfile"), self.dir.join(ignore))
    }
}

/// What `navree build:image` was asked to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageOptions {
    /// `--tag`; default `<app>:<short commit>`.
    pub tag: Option<String>,
    /// `--push` after building.
    pub push: bool,
    /// `--platform`; default this machine's architecture.
    pub platform: Option<String>,
}

/// The platform of this machine's architecture.
pub fn default_platform(arch: &str) -> &'static str {
    match arch {
        "aarch64" | "arm64" => "linux/arm64",
        _ => "linux/amd64",
    }
}

/// Writes the Dockerfile and ignore file into the app (`--dockerfile`).
/// Refuses to overwrite either.
pub fn write_own_files(app: &ImageApp) -> Result<Vec<PathBuf>> {
    let (dockerfile, ignore) = app.own_files();
    for path in [&dockerfile, &ignore] {
        if path.exists() {
            return Err(HarborError::msg(format!(
                "{} already exists; navree build:image uses it as it is",
                path.display()
            )));
        }
    }
    std::fs::write(&dockerfile, app.dockerfile()?).map_err(|e| HarborError::io(&dockerfile, e))?;
    std::fs::write(&ignore, app.dockerignore()?).map_err(|e| HarborError::io(&ignore, e))?;
    Ok(vec![dockerfile, ignore])
}

/// Builds (and pushes) the image; returns the tag and the engine.
pub fn build_image(
    exec: &Arc<dyn Executor>,
    app: &ImageApp,
    options: &ImageOptions,
    short_commit: Option<&str>,
    host_arch: &str,
) -> Result<(String, String, &'static str)> {
    let engine = container_engine(exec.as_ref())?;
    let tag = options
        .tag
        .clone()
        .unwrap_or_else(|| format!("{}:{}", app.app, short_commit.unwrap_or("latest")));
    let platform = options
        .platform
        .clone()
        .unwrap_or_else(|| default_platform(host_arch).to_owned());

    // The app's own Dockerfile wins; otherwise a generated one.
    let (dockerfile, ignore) = app.own_files();
    let temp = tempfile::Builder::new()
        .prefix("navree-harbor-image-")
        .tempdir()
        .map_err(|e| HarborError::io(std::env::temp_dir(), e))?;
    let (file, ignore_file) = if dockerfile.is_file() {
        (dockerfile, ignore.is_file().then_some(ignore))
    } else {
        let file = temp.path().join("Dockerfile");
        let ignore = temp.path().join("Dockerfile.dockerignore");
        std::fs::write(&file, app.dockerfile()?).map_err(|e| HarborError::io(&file, e))?;
        std::fs::write(&ignore, app.dockerignore()?).map_err(|e| HarborError::io(&ignore, e))?;
        (file, Some(ignore))
    };
    let cmd = build_command(
        engine,
        &tag,
        &platform,
        &file,
        ignore_file.as_deref(),
        &app.workspace_root,
    );
    let status = exec.interactive(&cmd)?;
    if status != Some(0) {
        return Err(HarborError::Local {
            command: cmd.to_string(),
            code: status,
            output: "see the build output above".into(),
        });
    }
    if options.push {
        run_checked(exec.as_ref(), &Cmd::new(engine).args(["push", &tag]))?;
    }
    Ok((tag, platform, engine))
}

/// `docker build --platform P --tag T --file F CONTEXT` (Podman also gets
/// `--ignorefile`; Docker reads `<Dockerfile>.dockerignore` itself).
pub fn build_command(
    engine: &str,
    tag: &str,
    platform: &str,
    file: &Path,
    ignore: Option<&Path>,
    context: &Path,
) -> Cmd {
    let mut cmd = Cmd::new(engine).args([
        "build",
        "--platform",
        platform,
        "--tag",
        tag,
        "--file",
        &slash_path(file),
    ]);
    if engine == "podman"
        && let Some(ignore) = ignore
    {
        cmd = cmd.args(["--ignorefile".to_owned(), slash_path(ignore)]);
    }
    cmd.arg(slash_path(context))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::{FakeExecutor, Output};

    fn app(dir: &Path, root: &Path) -> ImageApp {
        ImageApp {
            app: "crm-api".into(),
            dir: dir.to_path_buf(),
            workspace_root: root.to_path_buf(),
            features: vec!["postgres".into()],
            include: vec!["config".into(), "resources".into()],
        }
    }

    #[test]
    fn the_dockerfile_of_a_workspace_member() {
        let root = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let dir = root.path().join("apps").join("crm-api");
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(root.path().join("Cargo.lock"), "").unwrap();
        let app = app(&dir, root.path());
        assert_eq!(app.app_dir(), "apps/crm-api/");
        insta::assert_snapshot!(app.dockerfile().unwrap());
        insta::assert_snapshot!(app.dockerignore().unwrap(), @r"
        # Written by navree build:image: what the image build never needs.
        .git/
        target/
        **/target/
        **/node_modules/
        .env
        **/.env
        **/.env.*
        **/harbor*.key
        apps/crm-api/storage/
        ");
        assert!(app.own_files().1.ends_with("Dockerfile.dockerignore"));
    }

    #[test]
    fn a_single_package_app_is_its_own_context() {
        let root = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let app = ImageApp {
            features: vec![],
            ..app(root.path(), root.path())
        };
        assert_eq!(app.app_dir(), "");
        let dockerfile = app.dockerfile().unwrap();
        assert!(
            dockerfile.contains("cargo build --release --package crm-api \\"),
            "{dockerfile}"
        );
        assert!(!dockerfile.contains("COPY config"), "{dockerfile}");
        assert!(app.own_files().1.ends_with(".dockerignore"));
        let written = write_own_files(&app).unwrap();
        assert_eq!(written.len(), 2);
        assert!(write_own_files(&app).is_err());
    }

    #[test]
    fn images_build_and_push_with_the_engine_found() {
        let root = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let fake = FakeExecutor::new(|_| Output::ok(""));
        let exec: Arc<dyn Executor> = Arc::new(fake.clone());
        let (tag, platform, engine) = build_image(
            &exec,
            &app(root.path(), root.path()),
            &ImageOptions {
                tag: None,
                push: true,
                platform: None,
            },
            Some("3f2a9c1"),
            "x86_64",
        )
        .unwrap();
        assert_eq!(
            (tag.as_str(), platform.as_str(), engine),
            ("crm-api:3f2a9c1", "linux/amd64", "docker")
        );
        let lines = fake.command_lines();
        assert_eq!(lines[0], "docker --version");
        assert!(
            lines[1]
                .starts_with("docker build --platform linux/amd64 --tag crm-api:3f2a9c1 --file "),
            "{lines:?}"
        );
        assert!(lines[1].ends_with(&slash_path(root.path())), "{lines:?}");
        assert_eq!(lines[2], "docker push crm-api:3f2a9c1");
    }

    #[test]
    fn podman_gets_the_ignore_file() {
        let cmd = build_command(
            "podman",
            "x:1",
            "linux/arm64",
            Path::new("/tmp/Dockerfile"),
            Some(Path::new("/tmp/Dockerfile.dockerignore")),
            Path::new("/src"),
        );
        assert_eq!(
            cmd.to_string(),
            "podman build --platform linux/arm64 --tag x:1 --file /tmp/Dockerfile --ignorefile /tmp/Dockerfile.dockerignore /src"
        );
        assert_eq!(default_platform("aarch64"), "linux/arm64");
    }
}

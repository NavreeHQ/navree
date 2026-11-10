//! Building a release: the Linux binary (with the local Cargo, or in a
//! container), the front end, and the `.tar.gz` that `navree deploy`
//! uploads.

use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};

use crate::config::{BuildMethod, HarborConfig};
use crate::deploy::{Artifact, Build, ReleaseMeta, Target, version_cmp};
use crate::error::{HarborError, Result};
use crate::exec::{Cmd, Executor, run_checked, slash_path};
use crate::output::Clock;
use crate::render::{self, AppInfo, FrontendInfo};

/// The commit `dir` is checked out at: its full hash and subject line.
pub fn git_commit(exec: &dyn Executor, dir: &Path) -> Option<(String, String)> {
    let run = |args: &[&str]| -> Option<String> {
        let output = exec
            .run(&Cmd::new("git").args(args.iter().copied()).cwd(dir))
            .ok()?;
        output
            .success()
            .then(|| output.stdout.trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    let hash = run(&["rev-parse", "HEAD"])?;
    let subject = run(&["log", "-1", "--format=%s"]).unwrap_or_default();
    Some((hash, subject))
}

/// The id and commit of a release built now in `dir`.
pub fn release_meta(exec: &dyn Executor, dir: &Path, clock: &dyn Clock) -> ReleaseMeta {
    ReleaseMeta::new(clock.now(), git_commit(exec, dir))
}

/// The container engine: Docker, else Podman.
pub fn container_engine(exec: &dyn Executor) -> Result<&'static str> {
    for engine in ["docker", "podman"] {
        if crate::exec::available(exec, engine) {
            return Ok(engine);
        }
    }
    Err(HarborError::msg(
        "neither docker nor podman is installed; Harbor builds Linux binaries in a container on this machine (install Docker or Podman, or set build.method = \"native\" on Linux)",
    ))
}

/// What `cargo metadata` says about the app's workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// The workspace root.
    pub root: PathBuf,
    /// Cargo's target directory.
    pub target_dir: PathBuf,
    /// The directories of the local (path) packages, the app's included.
    pub local_packages: Vec<PathBuf>,
}

impl Workspace {
    /// Parses `cargo metadata --format-version 1` output.
    pub fn parse(json: &str) -> Result<Workspace> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| HarborError::msg(format!("cargo metadata printed invalid JSON: {e}")))?;
        let path = |key: &str| -> Result<PathBuf> {
            value[key]
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| HarborError::msg(format!("cargo metadata has no `{key}`")))
        };
        let local_packages = value["packages"]
            .as_array()
            .map(|packages| {
                packages
                    .iter()
                    .filter(|p| p["source"].is_null())
                    .filter_map(|p| p["manifest_path"].as_str())
                    .filter_map(|m| Path::new(m).parent().map(Path::to_path_buf))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Workspace {
            root: path("workspace_root")?,
            target_dir: path("target_directory")?,
            local_packages,
        })
    }

    /// The directory to mount in a build container: the deepest one holding
    /// the workspace and every local package (path dependencies outside the
    /// workspace included).
    pub fn mount_root(&self) -> PathBuf {
        let mut root = self.root.clone();
        for dir in &self.local_packages {
            root = common_ancestor(&root, dir);
        }
        root
    }
}

/// The deepest directory containing both `a` and `b`.
pub fn common_ancestor(a: &Path, b: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for (x, y) in a.components().zip(b.components()) {
        if x != y {
            break;
        }
        out.push(x.as_os_str());
    }
    out
}

/// `path` relative to `base`, with `/` separators (empty when equal).
pub fn relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map(|rest| {
            rest.components()
                .filter_map(|c| match c {
                    Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

/// The image Harbor builds Linux binaries in when `build.image` is not set.
pub fn builder_image(target: &Target) -> String {
    let arch = target.platform.trim_start_matches("linux/");
    format!("navree-harbor-builder:jammy-{arch}")
}

/// The real [`Build`]: Cargo natively or in a container, then the archive.
pub struct CargoBuild<'a> {
    /// Runs cargo, git, docker and tar.
    pub exec: Arc<dyn Executor>,
    /// The configuration.
    pub config: &'a HarborConfig,
    /// The app's directory.
    pub dir: PathBuf,
    /// What Harbor knows about the app.
    pub info: &'a AppInfo,
    /// This machine's OS (`std::env::consts::OS`).
    pub host_os: String,
    /// This machine's architecture (`std::env::consts::ARCH`).
    pub host_arch: String,
}

/// How a release is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    /// With the local Cargo.
    Native,
    /// In a container, with this engine.
    Container(&'static str),
}

impl CargoBuild<'_> {
    /// Reads the workspace with `cargo metadata`.
    pub fn workspace(&self) -> Result<Workspace> {
        let output = run_checked(
            self.exec.as_ref(),
            &Cmd::new("cargo")
                .args(["metadata", "--format-version", "1"])
                .cwd(&self.dir),
        )?;
        Workspace::parse(&output.stdout)
    }

    /// The local glibc version (`2.39`), on Linux.
    fn local_glibc(&self) -> Option<String> {
        let output = self
            .exec
            .run(&Cmd::new("getconf").arg("GNU_LIBC_VERSION"))
            .ok()?;
        output.stdout.split_whitespace().nth(1).map(str::to_owned)
    }

    /// `build.method`, with `auto` resolved for `target`.
    pub fn method(&self, target: &Target) -> Result<Method> {
        match self.config.build.method {
            BuildMethod::Native => Ok(Method::Native),
            BuildMethod::Container => Ok(Method::Container(container_engine(self.exec.as_ref())?)),
            BuildMethod::Auto => {
                let same_arch = self.host_arch == target.arch;
                let glibc_ok = match (&target.glibc, self.local_glibc()) {
                    (Some(servers), Some(local)) => {
                        version_cmp(&local, servers) != std::cmp::Ordering::Greater
                    }
                    _ => false,
                };
                if self.host_os == "linux" && same_arch && glibc_ok {
                    Ok(Method::Native)
                } else {
                    Ok(Method::Container(container_engine(self.exec.as_ref())?))
                }
            }
        }
    }

    /// `cargo build` for `target`, so the binary lands in
    /// `<target dir>/<triple>/release/<app>` however it is built.
    fn cargo_args(&self, target: &Target) -> Vec<String> {
        let mut args = vec![
            "build".to_owned(),
            "--release".to_owned(),
            "--package".to_owned(),
            self.config.app.clone(),
        ];
        if !self.config.build.features.is_empty() {
            args.push("--features".to_owned());
            args.push(self.config.build.features.join(","));
        }
        args.push("--target".to_owned());
        args.push(target.triple.clone());
        args
    }

    /// Builds the binary natively; returns its path.
    fn native(&self, workspace: &Workspace, target: &Target) -> Result<PathBuf> {
        let args = self.cargo_args(target);
        let status = self
            .exec
            .interactive(&Cmd::new("cargo").args(args.clone()).cwd(&self.dir))?;
        if status != Some(0) {
            return Err(HarborError::Local {
                command: format!("cargo {}", args.join(" ")),
                code: status,
                output: "see the compiler output above".into(),
            });
        }
        Ok(workspace
            .target_dir
            .join(&target.triple)
            .join("release")
            .join(&self.config.app))
    }

    /// Makes sure the builder image exists.
    fn builder(&self, engine: &str, target: &Target) -> Result<String> {
        if let Some(image) = &self.config.build.image {
            return Ok(image.clone());
        }
        let tag = builder_image(target);
        let present = self
            .exec
            .run(&Cmd::new(engine).args(["image", "inspect", &tag]))
            .map(|o| o.success())
            .unwrap_or(false);
        if !present {
            let dockerfile = render::render("image/builder.Dockerfile", json!({}))?;
            let status = self.exec.interactive(
                &Cmd::new(engine)
                    .args(["build", "--platform", &target.platform, "--tag", &tag, "-"])
                    .stdin(dockerfile),
            )?;
            if status != Some(0) {
                return Err(HarborError::Local {
                    command: format!("{engine} build --tag {tag} -"),
                    code: status,
                    output: "see the build output above".into(),
                });
            }
        }
        Ok(tag)
    }

    /// The `docker run` that builds the binary into `out`.
    pub fn container_command(
        &self,
        engine: &str,
        image: &str,
        workspace: &Workspace,
        target: &Target,
        out: &Path,
    ) -> Cmd {
        let root = workspace.mount_root();
        let app_dir = relative(&self.dir, &root);
        let workdir = if app_dir.is_empty() {
            "/src".to_owned()
        } else {
            format!("/src/{app_dir}")
        };
        let arch = target.platform.trim_start_matches("linux/");
        let app = &self.config.app;
        let triple = &target.triple;
        // The builder image has the toolchain of its own platform; a target
        // set in `build.target` (musl, say) is added to it first.
        let install = if self.config.build.target.is_some() {
            format!("rustup target add {triple} && ")
        } else {
            String::new()
        };
        let script = format!(
            "{install}cargo {} && cp /target/{triple}/release/{app} /out/{app}",
            self.cargo_args(target).join(" ")
        );
        Cmd::new(engine).args([
            "run".to_owned(),
            "--rm".to_owned(),
            "--platform".to_owned(),
            target.platform.clone(),
            "--mount".to_owned(),
            format!("type=bind,source={},target=/src", slash_path(&root)),
            "--mount".to_owned(),
            "type=volume,source=navree-harbor-cargo-registry,target=/usr/local/cargo/registry"
                .to_owned(),
            "--mount".to_owned(),
            format!("type=volume,source=navree-harbor-rustup-{arch},target=/usr/local/rustup"),
            "--mount".to_owned(),
            format!("type=volume,source=navree-harbor-target-{app}-{arch},target=/target"),
            "--mount".to_owned(),
            format!("type=bind,source={},target=/out", slash_path(out)),
            "--env".to_owned(),
            "CARGO_TARGET_DIR=/target".to_owned(),
            "--workdir".to_owned(),
            workdir,
            image.to_owned(),
            "sh".to_owned(),
            "-c".to_owned(),
            script,
        ])
    }

    /// The `docker run` that builds the front end and archives it into
    /// `out/frontend.tar.gz`, working on a copy inside the container.
    pub fn frontend_container_command(
        &self,
        engine: &str,
        frontend: &FrontendInfo,
        target: &Target,
        out: &Path,
    ) -> Cmd {
        let source = self.dir.join(&frontend.path);
        let script = format!(
            "corepack enable > /dev/null 2>&1 || true; mkdir -p /build && cd /src && tar -cf - --exclude=./node_modules --exclude=./.next --exclude=./.git . | tar -xf - -C /build && cd /build && {} && {}",
            frontend.build,
            frontend_tar("/out/frontend.tar.gz")
        );
        Cmd::new(engine).args([
            "run".to_owned(),
            "--rm".to_owned(),
            "--platform".to_owned(),
            target.platform.clone(),
            "--mount".to_owned(),
            format!(
                "type=bind,source={},target=/src,readonly",
                slash_path(&source)
            ),
            "--mount".to_owned(),
            format!("type=bind,source={},target=/out", slash_path(out)),
            "--workdir".to_owned(),
            "/build".to_owned(),
            format!("node:{}-bookworm", frontend.node_version),
            "sh".to_owned(),
            "-c".to_owned(),
            script,
        ])
    }

    fn frontend(
        &self,
        frontend: &FrontendInfo,
        method: &Method,
        target: &Target,
        out: &Path,
    ) -> Result<PathBuf> {
        let archive = out.join("frontend.tar.gz");
        let status = match method {
            Method::Native => {
                let dir = self.dir.join(&frontend.path);
                let script = format!(
                    "{} && {}",
                    frontend.build,
                    frontend_tar(&slash_path(&archive))
                );
                self.exec
                    .interactive(&Cmd::new("sh").args(["-c", &script]).cwd(dir))?
            }
            Method::Container(engine) => self
                .exec
                .interactive(&self.frontend_container_command(engine, frontend, target, out))?,
        };
        if status != Some(0) {
            return Err(HarborError::Local {
                command: frontend.build.clone(),
                code: status,
                output: "the front end did not build; see the output above".into(),
            });
        }
        Ok(archive)
    }
}

/// The tar command that archives a built front end.
fn frontend_tar(archive: &str) -> String {
    format!(
        "tar -czf {archive} --exclude=./.git --exclude=./.next/cache --exclude='./.env*.local' ."
    )
}

impl Build for CargoBuild<'_> {
    fn build(&self, release: &ReleaseMeta, target: &Target) -> Result<Artifact> {
        let workspace = self.workspace()?;
        let method = self.method(target)?;
        let out = tempfile::Builder::new()
            .prefix("navree-harbor-build-")
            .tempdir()
            .map_err(|e| HarborError::io(std::env::temp_dir(), e))?;
        let binary = match &method {
            Method::Native => self.native(&workspace, target)?,
            Method::Container(engine) => {
                let image = self.builder(engine, target)?;
                let cmd = self.container_command(engine, &image, &workspace, target, out.path());
                let status = self.exec.interactive(&cmd)?;
                if status != Some(0) {
                    return Err(HarborError::Local {
                        command: format!("{engine} run ... cargo build --release"),
                        code: status,
                        output: "see the compiler output above".into(),
                    });
                }
                out.path().join(&self.config.app)
            }
        };
        let frontend = match &self.info.frontend {
            Some(frontend) => Some(self.frontend(frontend, &method, target, out.path())?),
            None => None,
        };
        let archive = out.path().join(format!("{}.tar.gz", release.id));
        let size = write_archive(
            &archive,
            &ReleaseFiles {
                app: &self.config.app,
                binary: &binary,
                dir: &self.dir,
                include: &self.config.deploy.include,
                frontend: frontend.as_deref(),
                meta: release,
            },
        )?;
        let how = match method {
            Method::Native => "natively".to_owned(),
            Method::Container(_) => "in a container".to_owned(),
        };
        let mut summary = format!("{} for {}, {how}", self.config.app, target.triple);
        if frontend.is_some() {
            summary.push_str(", with the front end");
        }
        Ok(Artifact {
            archive,
            size,
            summary,
            keep: Some(out),
        })
    }
}

/// What goes into a release archive.
pub struct ReleaseFiles<'a> {
    /// The binary's name.
    pub app: &'a str,
    /// The built binary.
    pub binary: &'a Path,
    /// The app's directory.
    pub dir: &'a Path,
    /// Directories of the app shipped with the binary, when present.
    pub include: &'a [String],
    /// The front end's archive.
    pub frontend: Option<&'a Path>,
    /// The release's id and commit.
    pub meta: &'a ReleaseMeta,
}

fn header(size: u64, mode: u32) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_size(size);
    header.set_mode(mode);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header
}

fn append_file(
    builder: &mut tar::Builder<GzEncoder<std::fs::File>>,
    name: &str,
    path: &Path,
    mode: u32,
) -> Result<()> {
    let file = std::fs::File::open(path).map_err(|e| HarborError::io(path, e))?;
    let len = file.metadata().map_err(|e| HarborError::io(path, e))?.len();
    let mut header = header(len, mode);
    builder
        .append_data(&mut header, name, file)
        .map_err(|e| HarborError::io(path, e))
}

fn append_dir(
    builder: &mut tar::Builder<GzEncoder<std::fs::File>>,
    name: &str,
    path: &Path,
) -> Result<()> {
    let mut header = header(0, 0o755);
    header.set_entry_type(tar::EntryType::Directory);
    builder
        .append_data(&mut header, format!("{name}/"), std::io::empty())
        .map_err(|e| HarborError::io(path, e))?;
    let mut entries: Vec<_> = std::fs::read_dir(path)
        .map_err(|e| HarborError::io(path, e))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let child = entry.path();
        let child_name = format!("{name}/{}", entry.file_name().to_string_lossy());
        let kind = entry.file_type().map_err(|e| HarborError::io(&child, e))?;
        if kind.is_dir() {
            append_dir(builder, &child_name, &child)?;
        } else if kind.is_file() {
            append_file(builder, &child_name, &child, 0o644)?;
        }
    }
    Ok(())
}

/// Writes the release archive; returns its size.
pub fn write_archive(path: &Path, files: &ReleaseFiles<'_>) -> Result<u64> {
    if !files.binary.is_file() {
        return Err(HarborError::msg(format!(
            "the build did not produce {}",
            files.binary.display()
        )));
    }
    let file = std::fs::File::create(path).map_err(|e| HarborError::io(path, e))?;
    let mut builder = tar::Builder::new(GzEncoder::new(file, Compression::default()));
    append_file(&mut builder, files.app, files.binary, 0o755)?;
    for dir in files.include {
        let source = files.dir.join(dir);
        if source.is_dir() {
            append_dir(&mut builder, dir, &source)?;
        }
    }
    if let Some(frontend) = files.frontend {
        append_file(&mut builder, "frontend.tar.gz", frontend, 0o644)?;
    }
    let meta = files.meta.file();
    let mut header = header(meta.len() as u64, 0o644);
    builder
        .append_data(&mut header, ".harbor-release", meta.as_bytes())
        .map_err(|e| HarborError::io(path, e))?;
    let mut encoder = builder.into_inner().map_err(|e| HarborError::io(path, e))?;
    encoder.flush().map_err(|e| HarborError::io(path, e))?;
    encoder.finish().map_err(|e| HarborError::io(path, e))?;
    std::fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| HarborError::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::{FakeExecutor, Output};

    #[test]
    fn mount_roots_hold_path_dependencies() {
        let workspace = Workspace::parse(
            r#"{
                "workspace_root": "/home/me/crm",
                "target_directory": "/home/me/crm/target",
                "packages": [
                    {"name": "crm-api", "source": null, "manifest_path": "/home/me/crm/api/Cargo.toml"},
                    {"name": "navree", "source": null, "manifest_path": "/home/me/navree/crates/navree/Cargo.toml"},
                    {"name": "serde", "source": "registry+https://github.com/rust-lang/crates.io-index", "manifest_path": "/home/me/.cargo/registry/serde/Cargo.toml"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(workspace.local_packages.len(), 2);
        assert_eq!(workspace.mount_root(), PathBuf::from("/home/me"));
        assert_eq!(
            relative(Path::new("/home/me/crm/api"), Path::new("/home/me")),
            "crm/api"
        );
        assert_eq!(relative(Path::new("/home/me"), Path::new("/home/me")), "");
        assert!(Workspace::parse("{}").is_err());
    }

    /// The `sh -c` script of a container build of `crm-api` for `target`.
    fn container_script(harbor: &str, target: &Target) -> (Vec<String>, String) {
        let config = HarborConfig::parse(harbor, Path::new("harbor.toml")).unwrap();
        let info = AppInfo::default();
        let build = CargoBuild {
            exec: Arc::new(FakeExecutor::default()),
            config: &config,
            dir: PathBuf::from("/home/me/crm/api"),
            info: &info,
            host_os: "macos".into(),
            host_arch: "aarch64".into(),
        };
        let workspace = Workspace {
            root: PathBuf::from("/home/me/crm"),
            target_dir: PathBuf::from("/home/me/crm/target"),
            local_packages: vec![PathBuf::from("/home/me/crm/api")],
        };
        let cmd = build.container_command(
            "docker",
            "navree-harbor-builder:jammy-amd64",
            &workspace,
            target,
            Path::new("/tmp/out"),
        );
        let script = cmd.args.last().unwrap().clone();
        (cmd.args, script)
    }

    #[test]
    fn container_builds_name_the_servers_target() {
        let harbor = "app = \"crm-api\"\n\n[build]\nfeatures = [\"postgres\"]\n";
        let target = Target::new("x86_64", Some("2.35".into()), None);
        let (args, script) = container_script(harbor, &target);
        assert_eq!(
            script,
            "cargo build --release --package crm-api --features postgres --target x86_64-unknown-linux-gnu && cp /target/x86_64-unknown-linux-gnu/release/crm-api /out/crm-api"
        );
        let joined = args.join(" ");
        assert!(
            joined.starts_with("run --rm --platform linux/amd64 "),
            "{joined}"
        );
        assert!(
            joined.contains("--mount type=bind,source=/home/me/crm,target=/src"),
            "{joined}"
        );
        assert!(joined.contains("--workdir /src/api"), "{joined}");
        assert!(
            joined.contains("source=navree-harbor-target-crm-api-amd64,target=/target"),
            "{joined}"
        );
    }

    #[test]
    fn container_builds_install_an_overridden_target() {
        let harbor = "app = \"crm-api\"\n\n[build]\ntarget = \"x86_64-unknown-linux-musl\"\n";
        let target = Target::new("x86_64", None, Some("x86_64-unknown-linux-musl"));
        let (_, script) = container_script(harbor, &target);
        assert_eq!(
            script,
            "rustup target add x86_64-unknown-linux-musl && cargo build --release --package crm-api --target x86_64-unknown-linux-musl && cp /target/x86_64-unknown-linux-musl/release/crm-api /out/crm-api"
        );
    }

    #[test]
    fn git_commits_are_read_in_the_app() {
        let fake = FakeExecutor::new(|cmd| match cmd.args.first().map(String::as_str) {
            Some("rev-parse") => Output::ok("3f2a9c1d0e5b\n"),
            Some("log") => Output::ok("feat: stage probabilities\n"),
            _ => Output::failed(1, ""),
        });
        assert_eq!(
            git_commit(&fake, Path::new("/app")),
            Some(("3f2a9c1d0e5b".into(), "feat: stage probabilities".into()))
        );
        assert_eq!(fake.calls()[0].cwd.as_deref(), Some(Path::new("/app")));
        let outside = FakeExecutor::new(|_| Output::failed(128, "not a git repository"));
        assert_eq!(git_commit(&outside, Path::new("/app")), None);
    }

    #[test]
    fn the_engine_is_docker_then_podman() {
        let podman = FakeExecutor::new(|cmd| {
            if cmd.program == "podman" {
                Output::ok("podman version 5")
            } else {
                Output::failed(127, "")
            }
        });
        assert_eq!(container_engine(&podman).unwrap(), "podman");
        let none = FakeExecutor::new(|_| Output::failed(127, ""));
        assert!(
            container_engine(&none)
                .unwrap_err()
                .to_string()
                .contains("Docker or Podman")
        );
    }

    #[test]
    fn archives_hold_the_binary_the_included_directories_and_the_release_file() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let app = dir.path().join("app");
        std::fs::create_dir_all(app.join("config")).unwrap();
        std::fs::write(app.join("config/app.toml"), "[http]\n").unwrap();
        let binary = dir.path().join("crm-api");
        std::fs::write(&binary, b"ELF").unwrap();
        let archive = dir.path().join("r.tar.gz");
        let meta = ReleaseMeta {
            id: "r1".into(),
            commit: "abc".into(),
            subject: "s".into(),
        };
        let size = write_archive(
            &archive,
            &ReleaseFiles {
                app: "crm-api",
                binary: &binary,
                dir: &app,
                include: &["config".into(), "resources".into()],
                frontend: None,
                meta: &meta,
            },
        )
        .unwrap();
        assert!(size > 0);
        let file = std::fs::File::open(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let entries: Vec<(String, u32)> = tar
            .entries()
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.path().unwrap().to_string_lossy().into_owned(),
                    e.header().mode().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            entries,
            [
                ("crm-api".to_owned(), 0o755),
                ("config/".to_owned(), 0o755),
                ("config/app.toml".to_owned(), 0o644),
                (".harbor-release".to_owned(), 0o644),
            ]
        );
        let missing = write_archive(
            &archive,
            &ReleaseFiles {
                app: "crm-api",
                binary: &dir.path().join("nope"),
                dir: &app,
                include: &[],
                frontend: None,
                meta: &meta,
            },
        );
        assert!(missing.is_err());
    }
}

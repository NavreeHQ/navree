//! Harbor's commands for the `navree` CLI: `harbor:*`, `deploy`,
//! `deploy:rollback`, `deploy:releases` and `build:image`.
//!
//! [`HarborCommand`] is a clap subcommand enum that navree-cli flattens into
//! its own; [`run`] executes one with a [`Context`] (the system's, or one
//! with fakes in tests).

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Subcommand, ValueEnum};

use crate::build::{CargoBuild, git_commit, release_meta};
use crate::ci;
use crate::config::{Environment, Role};
use crate::deploy::{DeployOptions, Deployer, ReleaseMeta, format_releases};
use crate::env::EnvFile;
use crate::error::HarborError;
use crate::exec::{Cmd, Executor, SystemExecutor, run_checked};
use crate::host::Host;
use crate::image::{self, ImageApp, ImageOptions};
use crate::init;
use crate::ops::{self, Connector};
use crate::output::{Clock, Progress, Stdout, SystemClock};
use crate::project::{KEY_VAR, Project, package_name};
use crate::provision::Provisioner;
use crate::render::AppInfo;
use crate::secrets::{SecretKey, generate_app_key};
use crate::ssh::{Identity, SSH_KEY_VAR};

/// The environment variable that selects the environment.
pub const ENV_VAR: &str = "NAVREE_HARBOR_ENV";

/// `--env <name>`: `harbor.<name>.toml` instead of `harbor.toml`.
#[derive(Debug, Clone, Default, Args)]
pub struct EnvArg {
    /// The environment: `harbor.<name>.toml` and `harbor.<name>.key`
    /// (default: NAVREE_HARBOR_ENV, else harbor.toml).
    #[arg(long = "env", value_name = "NAME")]
    pub env: Option<String>,
}

/// A Harbor command.
#[derive(Debug, Subcommand)]
pub enum HarborCommand {
    /// Write harbor.toml and harbor.key, and ignore the key in Git.
    #[command(name = "harbor:init")]
    Init(EnvArg),
    /// Prepare every server, or one, for its roles (safe to run again).
    #[command(name = "harbor:provision")]
    Provision(ServerArgs),
    /// Encrypted secrets in harbor.toml.
    #[command(name = "harbor:secrets", subcommand)]
    Secrets(SecretsCommand),
    /// Build, upload, migrate, health-check and switch to a new release.
    #[command(name = "deploy")]
    Deploy(DeployArgs),
    /// Switch back to the previous release, or to the one you name.
    #[command(name = "deploy:rollback")]
    Rollback(RollbackArgs),
    /// List the releases on the servers.
    #[command(name = "deploy:releases")]
    Releases(ServerOption),
    /// Each server's roles, processes, current release, disk and memory.
    #[command(name = "harbor:status")]
    Status(ServerOption),
    /// The app's log lines from journald, merged across servers.
    #[command(name = "harbor:logs")]
    Logs(LogsArgs),
    /// Run an app command on a server, from the current release with its
    /// environment.
    #[command(name = "harbor:run")]
    Run(RunArgs),
    /// Open a shell on a server as the deploy user.
    #[command(name = "harbor:ssh")]
    Ssh(ServerArgs),
    /// Run the PostgreSQL backup now.
    #[command(name = "harbor:backup")]
    Backup(BackupArgs),
    /// Write a CI workflow that tests and deploys every push.
    #[command(name = "harbor:ci")]
    Ci(CiArgs),
    /// Build an OCI image of the app with Docker or Podman.
    #[command(name = "build:image")]
    BuildImage(ImageArgs),
}

/// `[server] [--env]`.
#[derive(Debug, Clone, Default, Args)]
pub struct ServerArgs {
    /// One server (a [servers.<name>] table); default: every server.
    pub server: Option<String>,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// `[--server <name>] [--env]`.
#[derive(Debug, Clone, Default, Args)]
pub struct ServerOption {
    /// One server.
    #[arg(long, value_name = "NAME")]
    pub server: Option<String>,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// `navree harbor:secrets <command>`.
#[derive(Debug, Subcommand)]
pub enum SecretsCommand {
    /// Encrypt and store a secret (asks for the value without echo).
    Set(SetArgs),
    /// List the secrets' names.
    List(EnvArg),
    /// Print one secret's value.
    Reveal(NameArgs),
    /// Remove one secret.
    Unset(NameArgs),
    /// Print [env] and the decrypted [secrets] as an env file.
    Export(EnvArg),
    /// Re-encrypt every secret under a new key.
    #[command(name = "rotate-key")]
    RotateKey(EnvArg),
}

/// `harbor:secrets set <NAME> [--generate | --stdin]`.
#[derive(Debug, Clone, Args)]
pub struct SetArgs {
    /// The variable name, e.g. APP_MAIL__TRANSPORT.
    pub name: String,
    /// Store a new application key (base64:<32 random bytes>).
    #[arg(long, conflicts_with = "stdin")]
    pub generate: bool,
    /// Read the value from standard input.
    #[arg(long)]
    pub stdin: bool,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// `<NAME> [--env]`.
#[derive(Debug, Clone, Args)]
pub struct NameArgs {
    /// The secret's name.
    pub name: String,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// `navree deploy` options.
#[derive(Debug, Clone, Default, Args)]
pub struct DeployArgs {
    /// Deploy to one server.
    #[arg(long, value_name = "NAME")]
    pub server: Option<String>,
    /// Skip the migrations.
    #[arg(long)]
    pub no_migrate: bool,
    /// Turn on maintenance mode before migrating and off after the switch.
    #[arg(long)]
    pub maintenance: bool,
    /// Deploy a release already built and uploaded, without building.
    #[arg(long, value_name = "ID")]
    pub release: Option<String>,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// `navree deploy:rollback [--to <release>]`.
#[derive(Debug, Clone, Default, Args)]
pub struct RollbackArgs {
    /// The release to go back to; default the one before the current.
    #[arg(long, value_name = "ID")]
    pub to: Option<String>,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// `navree harbor:logs` options.
#[derive(Debug, Clone, Args)]
pub struct LogsArgs {
    /// One server.
    #[arg(long, value_name = "NAME")]
    pub server: Option<String>,
    /// One process: web, frontend, worker or scheduler.
    #[arg(long, value_name = "PROCESS")]
    pub process: Option<String>,
    /// How many lines per server.
    #[arg(long, default_value_t = 100)]
    pub lines: u32,
    /// Keep printing new lines.
    #[arg(long)]
    pub follow: bool,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// `navree harbor:run [--server] <command> [args...]`.
#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    /// The server; default the first web server.
    #[arg(long, value_name = "NAME")]
    pub server: Option<String>,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
    /// The app command and its arguments, e.g. `migrate:status`.
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// `navree harbor:backup [--download]`.
#[derive(Debug, Clone, Default, Args)]
pub struct BackupArgs {
    /// Also copy the backup to this machine's current directory.
    #[arg(long)]
    pub download: bool,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// The CI systems `harbor:ci` writes for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CiProvider {
    /// GitHub Actions.
    Github,
}

/// `navree harbor:ci github [--branch main]`.
#[derive(Debug, Clone, Args)]
pub struct CiArgs {
    /// The CI system.
    #[arg(value_enum)]
    pub provider: CiProvider,
    /// The branch whose pushes deploy.
    #[arg(long, default_value = "main")]
    pub branch: String,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

/// `navree build:image` options.
#[derive(Debug, Clone, Default, Args)]
pub struct ImageArgs {
    /// The image tag; default <app>:<short commit>.
    #[arg(long)]
    pub tag: Option<String>,
    /// Push the image after building it.
    #[arg(long)]
    pub push: bool,
    /// The platform, e.g. linux/arm64; default this machine's.
    #[arg(long)]
    pub platform: Option<String>,
    /// Write the Dockerfile and ignore file into the app instead of
    /// building; later builds use them.
    #[arg(long)]
    pub dockerfile: bool,
    /// The environment.
    #[command(flatten)]
    pub env: EnvArg,
}

type Prompt = dyn Fn(&str) -> std::io::Result<String> + Send + Sync;
type ReadAll = dyn Fn() -> std::io::Result<String> + Send + Sync;
type Vars = dyn Fn(&str) -> Option<String> + Send + Sync;

/// Everything a command touches outside its arguments.
pub struct Context {
    /// The working directory.
    pub cwd: PathBuf,
    /// Reads environment variables.
    pub vars: Box<Vars>,
    /// The home directory (for `~/.ssh`).
    pub home: Option<PathBuf>,
    /// Runs programs.
    pub exec: Arc<dyn Executor>,
    /// The clock.
    pub clock: Arc<dyn Clock>,
    /// Where output lines go.
    pub out: Arc<dyn Progress>,
    /// Asks for a value without echoing it.
    pub prompt: Box<Prompt>,
    /// Reads standard input to the end.
    pub stdin: Box<ReadAll>,
    /// This machine's OS (`linux`, `macos`, `windows`).
    pub host_os: String,
    /// This machine's architecture (`x86_64`, `aarch64`).
    pub host_arch: String,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context").field("cwd", &self.cwd).finish()
    }
}

impl Context {
    /// The real machine, working in `cwd`.
    pub fn system(cwd: PathBuf) -> Context {
        Context {
            cwd,
            vars: Box::new(|name| std::env::var(name).ok()),
            home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from),
            exec: Arc::new(SystemExecutor),
            clock: Arc::new(SystemClock),
            out: Arc::new(Stdout),
            prompt: Box::new(|prompt| rpassword::prompt_password(prompt)),
            stdin: Box::new(|| {
                let mut text = String::new();
                std::io::stdin().read_to_string(&mut text)?;
                Ok(text)
            }),
            host_os: std::env::consts::OS.to_owned(),
            host_arch: std::env::consts::ARCH.to_owned(),
        }
    }

    fn var(&self, name: &str) -> Option<String> {
        (self.vars)(name).filter(|v| !v.trim().is_empty())
    }

    fn print(&self, line: &str) {
        self.out.line(line);
    }

    fn environment(&self, arg: &EnvArg) -> anyhow::Result<Environment> {
        Ok(Environment::select(
            arg.env.as_deref(),
            self.var(ENV_VAR).as_deref(),
        )?)
    }

    /// The directory holding the environment's file: the working directory
    /// or the nearest one above it that has the file.
    fn project_dir(&self, env: &Environment) -> PathBuf {
        self.cwd
            .ancestors()
            .find(|d| d.join(env.config_file()).is_file())
            .unwrap_or(&self.cwd)
            .to_path_buf()
    }

    fn open(&self, arg: &EnvArg) -> anyhow::Result<Project> {
        let env = self.environment(arg)?;
        Ok(Project::open(&self.project_dir(&env), env)?)
    }

    fn load(&self, arg: &EnvArg) -> anyhow::Result<Project> {
        let env = self.environment(arg)?;
        Ok(Project::load(&self.project_dir(&env), env)?)
    }

    fn key(&self, project: &Project) -> anyhow::Result<SecretKey> {
        Ok(project.key(self.var(KEY_VAR).as_deref())?)
    }

    fn identity(&self, project: &Project) -> anyhow::Result<Identity> {
        Ok(Identity::resolve(
            project.config.ssh.key.as_deref(),
            self.var(SSH_KEY_VAR).as_deref(),
            self.home.as_deref(),
        )?)
    }

    fn connector(&self, project: &Project) -> anyhow::Result<Connector> {
        Ok(Connector {
            exec: self.exec.clone(),
            identity: self.identity(project)?,
        })
    }
}

fn exit(code: Option<i32>) -> ExitCode {
    code.and_then(|c| u8::try_from(c).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from)
}

/// Runs `command`.
pub fn run(command: &HarborCommand, ctx: &Context) -> anyhow::Result<ExitCode> {
    match command {
        HarborCommand::Init(arg) => run_init(arg, ctx),
        HarborCommand::Provision(args) => run_provision(args, ctx),
        HarborCommand::Secrets(command) => run_secrets(command, ctx),
        HarborCommand::Deploy(args) => run_deploy(args, ctx),
        HarborCommand::Rollback(args) => run_rollback(args, ctx),
        HarborCommand::Releases(args) => run_releases(args, ctx),
        HarborCommand::Status(args) => run_status(args, ctx),
        HarborCommand::Logs(args) => run_logs(args, ctx),
        HarborCommand::Run(args) => run_app_command(args, ctx),
        HarborCommand::Ssh(args) => run_ssh(args, ctx),
        HarborCommand::Backup(args) => run_backup(args, ctx),
        HarborCommand::Ci(args) => run_ci(args, ctx),
        HarborCommand::BuildImage(args) => run_build_image(args, ctx),
    }
}

fn run_init(arg: &EnvArg, ctx: &Context) -> anyhow::Result<ExitCode> {
    let env = ctx.environment(arg)?;
    let dir = ctx
        .cwd
        .ancestors()
        .find(|d| package_name(d).is_some())
        .unwrap_or(&ctx.cwd)
        .to_path_buf();
    let report = init::init(&dir, &env)?;
    for line in report.render() {
        ctx.print(&line);
    }
    ctx.print(&format!(
        "Next: set the servers in {}, then `navree harbor:provision{}`.",
        env.config_file(),
        env.flag()
    ));
    Ok(ExitCode::SUCCESS)
}

fn run_provision(args: &ServerArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let mut project = ctx.load(&args.env)?;
    let key = ctx.key(&project).ok();
    let provisioner = Provisioner {
        exec: ctx.exec.clone(),
        identity: ctx.identity(&project)?,
        progress: ctx.out.as_ref(),
        clock: ctx.clock.as_ref(),
    };
    provisioner.run(&mut project, key.as_ref(), args.server.as_deref())?;
    Ok(ExitCode::SUCCESS)
}

fn run_secrets(command: &SecretsCommand, ctx: &Context) -> anyhow::Result<ExitCode> {
    match command {
        SecretsCommand::Set(args) => {
            let mut project = ctx.open(&args.env)?;
            let key = ctx.key(&project)?;
            let value = if args.generate {
                generate_app_key()
            } else if args.stdin {
                let text = (ctx.stdin)()?;
                text.strip_suffix('\n')
                    .map(|t| t.strip_suffix('\r').unwrap_or(t))
                    .unwrap_or(&text)
                    .to_owned()
            } else {
                (ctx.prompt)(&format!("Value of {}: ", args.name))?
            };
            if value.is_empty() {
                anyhow::bail!("the value of {} is empty; nothing was stored", args.name);
            }
            project.set_secret(&key, &args.name, &value)?;
            ctx.print(&format!(
                "Stored {} in {} (encrypted).",
                args.name,
                project.env.config_file()
            ));
        }
        SecretsCommand::List(arg) => {
            let project = ctx.open(arg)?;
            for name in project.config.secrets.keys() {
                ctx.print(name);
            }
        }
        SecretsCommand::Reveal(args) => {
            let project = ctx.open(&args.env)?;
            let key = ctx.key(&project)?;
            ctx.print(&project.reveal(&key, &args.name)?);
        }
        SecretsCommand::Unset(args) => {
            let mut project = ctx.open(&args.env)?;
            project.unset_secret(&args.name)?;
            ctx.print(&format!(
                "Removed {} from {}.",
                args.name,
                project.env.config_file()
            ));
        }
        SecretsCommand::Export(arg) => {
            let project = ctx.open(arg)?;
            let key = ctx.key(&project)?;
            let mut env = EnvFile::new();
            for (name, value) in &project.config.env {
                env.set(name, value);
            }
            for (name, value) in project.decrypt_secrets(&key)? {
                env.set(name, value);
            }
            let text = env.render_plain().map_err(|name| {
                HarborError::msg(format!(
                    "{name} spans several lines, which an env file can't hold"
                ))
            })?;
            for line in text.lines() {
                ctx.print(line);
            }
        }
        SecretsCommand::RotateKey(arg) => {
            let mut project = ctx.open(arg)?;
            let old = ctx.key(&project)?;
            let new = SecretKey::generate();
            let count = project.rotate_key(&old, &new)?;
            ctx.print(&format!(
                "Re-encrypted {count} secret{} under a new key in {}.",
                if count == 1 { "" } else { "s" },
                project.env.key_file()
            ));
            if ctx.var(KEY_VAR).is_some() {
                ctx.print(&format!(
                    "{KEY_VAR} is set: replace it (and your CI secret) with the contents of {}.",
                    project.env.key_file()
                ));
            }
            ctx.print("Share the new key with your team; the old one no longer decrypts anything.");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The deploy hosts for `only` (or every server that runs the app).
fn hosts(
    project: &Project,
    ctx: &Context,
    info: &AppInfo,
    only: Option<&str>,
) -> anyhow::Result<Vec<Arc<dyn Host>>> {
    let connector = ctx.connector(project)?;
    let servers = ops::app_servers(&project.config, only)?;
    if servers.is_empty() {
        anyhow::bail!("no server in {} runs the app", project.env.config_file());
    }
    servers
        .into_iter()
        .map(|server| Ok(Arc::new(connector.host(&project.config, server, info)?) as Arc<dyn Host>))
        .collect()
}

fn run_deploy(args: &DeployArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let project = ctx.load(&args.env)?;
    let secrets = if project.config.secrets.is_empty() {
        BTreeMap::new()
    } else {
        project.decrypt_secrets(&ctx.key(&project)?)?
    };
    let info = AppInfo::detect(&project.config, &project.dir);
    let deployer = Deployer {
        config: &project.config,
        env_name: project.env.name(),
        info: &info,
        hosts: hosts(&project, ctx, &info, args.server.as_deref())?,
        progress: ctx.out.as_ref(),
        clock: ctx.clock.as_ref(),
    };
    let meta = match &args.release {
        Some(id) => ReleaseMeta {
            id: id.clone(),
            commit: String::new(),
            subject: String::new(),
        },
        None => release_meta(ctx.exec.as_ref(), &project.dir, ctx.clock.as_ref()),
    };
    let build = CargoBuild {
        exec: ctx.exec.clone(),
        config: &project.config,
        dir: project.dir.clone(),
        info: &info,
        host_os: ctx.host_os.clone(),
        host_arch: ctx.host_arch.clone(),
    };
    deployer.deploy(
        &build,
        &meta,
        &secrets,
        &DeployOptions {
            no_migrate: args.no_migrate,
            maintenance: args.maintenance,
            release: args.release.clone(),
        },
    )?;
    Ok(ExitCode::SUCCESS)
}

fn run_rollback(args: &RollbackArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let project = ctx.load(&args.env)?;
    let info = AppInfo::detect(&project.config, &project.dir);
    let deployer = Deployer {
        config: &project.config,
        env_name: project.env.name(),
        info: &info,
        hosts: hosts(&project, ctx, &info, None)?,
        progress: ctx.out.as_ref(),
        clock: ctx.clock.as_ref(),
    };
    deployer.rollback(args.to.as_deref())?;
    Ok(ExitCode::SUCCESS)
}

fn run_releases(args: &ServerOption, ctx: &Context) -> anyhow::Result<ExitCode> {
    let project = ctx.load(&args.env)?;
    let info = AppInfo::detect(&project.config, &project.dir);
    let deployer = Deployer {
        config: &project.config,
        env_name: project.env.name(),
        info: &info,
        hosts: hosts(&project, ctx, &info, args.server.as_deref())?,
        progress: ctx.out.as_ref(),
        clock: ctx.clock.as_ref(),
    };
    let releases = deployer.releases()?;
    if releases.is_empty() {
        ctx.print("No releases yet: run `navree deploy`.");
    }
    for line in format_releases(&releases) {
        ctx.print(&line);
    }
    Ok(ExitCode::SUCCESS)
}

fn run_status(args: &ServerOption, ctx: &Context) -> anyhow::Result<ExitCode> {
    let project = ctx.load(&args.env)?;
    let info = AppInfo::detect(&project.config, &project.dir);
    let connector = ctx.connector(&project)?;
    let servers: Vec<_> = match &args.server {
        Some(name) => vec![project.config.server(name)?],
        None => project.config.servers.iter().collect(),
    };
    for (i, server) in servers.into_iter().enumerate() {
        if i > 0 {
            ctx.print("");
        }
        let ssh = connector.ssh(&project.config, server)?;
        let output = ssh.script(
            "status",
            &ops::status_script(&project.config, server, &info)?,
        )?;
        for line in ops::format_status(&project.config, server, &info, &output.stdout) {
            ctx.print(&line);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_logs(args: &LogsArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let project = ctx.load(&args.env)?;
    let connector = ctx.connector(&project)?;
    let command = ops::logs_command(
        &project.config,
        args.process.as_deref(),
        args.lines,
        args.follow,
    )?;
    let servers = ops::app_servers(&project.config, args.server.as_deref())?;
    let connections: Vec<_> = servers
        .iter()
        .map(|s| connector.ssh(&project.config, s))
        .collect::<Result<_, _>>()?;
    if args.follow {
        let codes = std::thread::scope(|scope| {
            let handles: Vec<_> = connections
                .iter()
                .map(|ssh| scope.spawn(|| ssh.interactive(Some(&command))))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("log thread panicked"))
                .collect::<Vec<_>>()
        });
        for code in codes {
            code?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut per_server = Vec::new();
    for (server, ssh) in servers.iter().zip(&connections) {
        let output = ctx.exec.run(&ssh.command(&command))?;
        if !output.success() {
            return Err(ssh.failure("logs", &output).into());
        }
        per_server.push((server.name.clone(), output.stdout));
    }
    for line in ops::merge_logs(&per_server) {
        ctx.print(&line);
    }
    Ok(ExitCode::SUCCESS)
}

fn run_app_command(args: &RunArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let project = ctx.load(&args.env)?;
    let server = ops::command_server(&project.config, args.server.as_deref())?;
    let ssh = ctx.connector(&project)?.ssh(&project.config, server)?;
    let code = ssh.interactive(Some(&ops::run_command(&project.config, &args.command)))?;
    Ok(exit(code))
}

fn run_ssh(args: &ServerArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let project = ctx.load(&args.env)?;
    let server = ops::command_server(&project.config, args.server.as_deref())?;
    let ssh = ctx.connector(&project)?.ssh(&project.config, server)?;
    Ok(exit(ssh.interactive(None)?))
}

fn run_backup(args: &BackupArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let project = ctx.load(&args.env)?;
    let info = AppInfo::detect(&project.config, &project.dir);
    let server = project
        .config
        .servers_with(Role::Postgres)
        .next()
        .ok_or_else(|| HarborError::msg("no server has the `postgres` role"))?;
    let ssh = ctx.connector(&project)?.ssh(&project.config, server)?;
    let file = ops::backup(&ssh, &project.config, server, &info)?;
    ctx.print(&format!("Backed up to {file} on {}.", server.name));
    if args.download {
        let local = ops::download_path(&ctx.cwd, &file);
        ssh.download(&file, &local)?;
        ctx.print(&format!("Downloaded to {}.", local.display()));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_ci(args: &CiArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let env = ctx.environment(&args.env)?;
    let dir = ctx.project_dir(&env);
    let CiProvider::Github = args.provider;
    let workflow = ci::github_workflow(&dir, &env, &args.branch)?;
    ci::write_workflow(&workflow)?;
    let root = ci::repository_root(&dir).unwrap_or(dir);
    ctx.print(&format!(
        "  created  {}",
        crate::build::relative(&workflow.path, &root)
    ));
    ctx.print("Then, once:");
    ctx.print("  1. ssh-keygen -t ed25519 -f ci-deploy -N \"\", add ci-deploy.pub to ssh.authorized_keys and run `navree harbor:provision`.");
    ctx.print(&format!(
        "  2. Add the repository secrets NAVREE_HARBOR_KEY (the contents of {}) and NAVREE_HARBOR_SSH_KEY (the contents of ci-deploy).",
        env.key_file()
    ));
    Ok(ExitCode::SUCCESS)
}

/// The image's app: from `harbor.toml` when there is one, otherwise the
/// package in the working directory with the default settings.
fn image_app(args: &ImageArgs, ctx: &Context) -> anyhow::Result<ImageApp> {
    let (dir, app, features, include) = match ctx.open(&args.env) {
        Ok(project) => (
            project.dir.clone(),
            project.config.app.clone(),
            project.config.build.features.clone(),
            project.config.deploy.include.clone(),
        ),
        Err(error) => {
            if !matches!(
                error.downcast_ref::<HarborError>(),
                Some(HarborError::NoConfig { .. })
            ) {
                return Err(error);
            }
            let dir = ctx
                .cwd
                .ancestors()
                .find(|d| package_name(d).is_some())
                .ok_or_else(|| HarborError::msg("run build:image in your app's directory (no Cargo.toml with a [package] here)"))?
                .to_path_buf();
            let app = package_name(&dir).unwrap_or_default();
            (
                dir,
                app,
                Vec::new(),
                vec!["config".to_owned(), "resources".to_owned()],
            )
        }
    };
    let metadata = run_checked(
        ctx.exec.as_ref(),
        &Cmd::new("cargo")
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .cwd(&dir),
    )?;
    let value: serde_json::Value = serde_json::from_str(&metadata.stdout)?;
    let workspace_root = value["workspace_root"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.clone());
    Ok(ImageApp {
        app,
        dir,
        workspace_root,
        features,
        include,
    })
}

fn run_build_image(args: &ImageArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let app = image_app(args, ctx)?;
    if args.dockerfile {
        for path in image::write_own_files(&app)? {
            ctx.print(&format!(
                "  created  {}",
                crate::build::relative(&path, &app.dir)
            ));
        }
        ctx.print("navree build:image now builds with these files.");
        return Ok(ExitCode::SUCCESS);
    }
    let short = git_commit(ctx.exec.as_ref(), &app.dir)
        .map(|(hash, _)| hash.chars().take(7).collect::<String>());
    let (tag, platform, engine) = image::build_image(
        &ctx.exec,
        &app,
        &ImageOptions {
            tag: args.tag.clone(),
            push: args.push,
            platform: args.platform.clone(),
        },
        short.as_deref(),
        &ctx.host_arch,
    )?;
    ctx.print(&format!(
        "Built {tag} ({platform}) with {engine}{}.",
        if args.push { ", and pushed it" } else { "" }
    ));
    Ok(ExitCode::SUCCESS)
}

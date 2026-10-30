//! `navree dock ...`, `navree dock:add` and `navree dock:remove`: the
//! subcommands, wired to navree-dock. Also the settings `navree dev` and
//! forwarded commands pass to the app when it has a `dock.toml`.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use navree_dock::{
    Dock, DockError, DockFile, Executor, ProcessExecutor, add_service, env_lines, remove_service,
    status_table,
};

use crate::cli::DockCommand;

/// Runs `navree dock <command>` for the app at `root`.
pub fn run(command: &DockCommand, root: &Path) -> anyhow::Result<ExitCode> {
    run_with(
        command,
        root,
        ProcessExecutor::new(root),
        &mut std::io::stdout(),
    )
}

/// [`run`] with another executor and output (tests).
pub fn run_with<E: Executor>(
    command: &DockCommand,
    root: &Path,
    exec: E,
    out: &mut dyn Write,
) -> anyhow::Result<ExitCode> {
    if let DockCommand::Env { export } = command {
        // No runtime needed: CI prints the settings before anything runs.
        let file = DockFile::load(root)?;
        write!(out, "{}", env_lines(&file.env(), *export))?;
        return Ok(ExitCode::SUCCESS);
    }
    let dock = Dock::open(root, exec)?;
    match command {
        DockCommand::Up { services } => dock.up(services, out)?,
        DockCommand::Down { services, volumes } => dock.down(services, *volumes, out)?,
        DockCommand::Status => write!(out, "{}", status_table(&dock.status()?))?,
        DockCommand::Logs { service, follow } => {
            return Ok(exit_code(dock.logs(service, *follow)?));
        }
        DockCommand::Shell { service } => return Ok(exit_code(dock.shell(service)?)),
        DockCommand::Env { .. } => unreachable!("handled above"),
    }
    Ok(ExitCode::SUCCESS)
}

/// `navree dock:add <service>`.
pub fn add(service: &str, root: &Path, out: &mut dyn Write) -> anyhow::Result<ExitCode> {
    let report = add_service(root, service)?;
    if report.already_present {
        writeln!(out, "{service} is already in dock.toml.")?;
        return Ok(ExitCode::SUCCESS);
    }
    let verb = if report.created { "created" } else { "updated" };
    writeln!(
        out,
        "  {verb}  dock.toml        # [services.{}]",
        report.service
    )?;
    let mut line = format!("Added {}.", report.service);
    for hint in &report.hints {
        line.push(' ');
        line.push_str(hint);
    }
    writeln!(out, "{line}")?;
    Ok(ExitCode::SUCCESS)
}

/// `navree dock:remove <service> [--volumes]`: stops the service when a
/// runtime answers (a warning otherwise), then removes it from `dock.toml`.
pub fn remove(
    service: &str,
    volumes: bool,
    root: &Path,
    out: &mut dyn Write,
) -> anyhow::Result<ExitCode> {
    remove_with(service, volumes, root, ProcessExecutor::new(root), out)
}

/// [`remove`] with another executor (tests).
pub fn remove_with<E: Executor>(
    service: &str,
    volumes: bool,
    root: &Path,
    exec: E,
    out: &mut dyn Write,
) -> anyhow::Result<ExitCode> {
    let stopped = Dock::open(root, exec).and_then(|dock| dock.remove_container(service, volumes));
    match stopped {
        Ok(()) => {}
        // Not in the file, or not a service: remove_service says so below.
        Err(DockError::NotInDockFile(_) | DockError::UnknownService(_)) => {}
        Err(error) => eprintln!("warning: could not stop {service}: {error}"),
    }
    let service = remove_service(root, service)?;
    writeln!(
        out,
        "  updated  dock.toml        # removed [services.{service}]"
    )?;
    if volumes {
        writeln!(out, "Removed {service} and deleted its data.")?;
    } else {
        writeln!(
            out,
            "Removed {service}. Its data stays in its volume until `--volumes`."
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

/// The settings to pass to the app's process: those of `<root>/dock.toml`
/// that the environment does not set already (an explicit variable wins).
pub fn app_env(root: &Path) -> anyhow::Result<Vec<(String, String)>> {
    Ok(unset_only(navree_dock::app_env(root)?, |name| {
        std::env::var_os(name).is_some()
    }))
}

fn unset_only(
    settings: Vec<(String, String)>,
    is_set: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    settings
        .into_iter()
        .filter(|(name, _)| !is_set(name))
        .collect()
}

fn exit_code(code: i32) -> ExitCode {
    u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from)
}

#[cfg(test)]
mod tests {
    use navree_dock::{CmdOutput, FakeExecutor};

    use super::*;

    fn app() -> tempfile::TempDir {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"crm-api\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        dir
    }

    fn printed(out: Vec<u8>) -> String {
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn add_prints_like_the_docs() {
        let dir = app();
        let mut out = Vec::new();
        add("postgres", dir.path(), &mut out).unwrap();
        add("mailpit", dir.path(), &mut out).unwrap();
        add("mailpit", dir.path(), &mut out).unwrap();
        assert_eq!(
            printed(out),
            "  created  dock.toml        # [services.postgres]\n\
             Added postgres. Your app needs navree's `postgres` feature.\n  \
             updated  dock.toml        # [services.mailpit]\n\
             Added mailpit.\n\
             mailpit is already in dock.toml.\n"
        );
        assert!(add("mysql", dir.path(), &mut Vec::new()).is_err());
    }

    #[test]
    fn env_needs_no_runtime() {
        let dir = app();
        std::fs::write(dir.path().join("dock.toml"), "[services.redis]\n").unwrap();
        let exec = FakeExecutor::new().with_programs::<[&str; 0]>([]);
        let mut out = Vec::new();
        run_with(
            &DockCommand::Env { export: true },
            dir.path(),
            exec,
            &mut out,
        )
        .unwrap();
        assert_eq!(printed(out), "export REDIS_URL=redis://127.0.0.1:6379\n");
        assert!(exec_free_commands_fail_without_a_runtime(dir.path()));
    }

    fn exec_free_commands_fail_without_a_runtime(root: &Path) -> bool {
        let exec = FakeExecutor::new().with_programs::<[&str; 0]>([]);
        let error = run_with(&DockCommand::Status, root, exec, &mut Vec::new()).unwrap_err();
        error.to_string().contains("neither `docker` nor `podman`")
    }

    #[test]
    fn up_status_and_down_run_through_the_executor() {
        let dir = app();
        std::fs::write(dir.path().join("dock.toml"), "[services.redis]\n").unwrap();
        let mut out = Vec::new();
        let up = DockCommand::Up { services: vec![] };
        run_with(&up, dir.path(), FakeExecutor::new(), &mut out).unwrap();
        let ps = FakeExecutor::new().respond(
            "ps --all",
            CmdOutput::ok("crm-api-redis|running|redis:7|127.0.0.1:6379->6379/tcp\n"),
        );
        run_with(&DockCommand::Status, dir.path(), ps, &mut out).unwrap();
        let down = DockCommand::Down {
            services: vec![],
            volumes: true,
        };
        run_with(&down, dir.path(), FakeExecutor::new(), &mut out).unwrap();
        let text = printed(out);
        assert!(
            text.starts_with("Starting crm-api services with docker\n"),
            "{text}"
        );
        assert!(
            text.contains("redis    running  redis:7  127.0.0.1:6379\n"),
            "{text}"
        );
        assert!(
            text.ends_with("Stopped crm-api services and deleted their data.\n"),
            "{text}"
        );
        let code = run_with(
            &DockCommand::Logs {
                service: "redis".into(),
                follow: false,
            },
            dir.path(),
            FakeExecutor::new().respond("logs", CmdOutput::failed(3, "")),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(code, ExitCode::from(3));
    }

    #[test]
    fn remove_stops_the_service_then_edits_the_file() {
        let dir = app();
        std::fs::write(
            dir.path().join("dock.toml"),
            "[services.redis]\n\n[services.nats]\n",
        )
        .unwrap();
        let exec = FakeExecutor::new().respond(
            "ps --all",
            CmdOutput::ok("crm-api-redis|running|redis:7|127.0.0.1:6379->6379/tcp\n"),
        );
        let mut out = Vec::new();
        remove_with("redis", true, dir.path(), exec, &mut out).unwrap();
        assert_eq!(
            printed(out),
            "  updated  dock.toml        # removed [services.redis]\nRemoved redis and deleted its data.\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("dock.toml")).unwrap(),
            "[services.nats]\n"
        );
        // Without a runtime it still edits the file (with a warning).
        let none = FakeExecutor::new().with_programs::<[&str; 0]>([]);
        remove_with("nats", false, dir.path(), none, &mut Vec::new()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("dock.toml")).unwrap(),
            ""
        );
        let error = remove_with(
            "nats",
            false,
            dir.path(),
            FakeExecutor::new(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("nats is not in dock.toml"),
            "{error}"
        );
    }

    #[test]
    fn settings_already_in_the_environment_win() {
        let settings = vec![
            ("APP_DATABASE__URL".to_owned(), "postgres://dock".to_owned()),
            ("REDIS_URL".to_owned(), "redis://dock".to_owned()),
        ];
        let kept = unset_only(settings, |name| name == "APP_DATABASE__URL");
        assert_eq!(kept, [("REDIS_URL".to_owned(), "redis://dock".to_owned())]);
    }

    #[test]
    fn apps_without_dock_toml_get_nothing() {
        let dir = app();
        assert!(app_env(dir.path()).unwrap().is_empty());
        std::fs::write(
            dir.path().join("dock.toml"),
            "[services.nats]\nport = 4223\n",
        )
        .unwrap();
        let env = app_env(dir.path()).unwrap();
        assert!(
            env.iter()
                .any(|(k, v)| k == "APP_MICRO__URL" && v == "nats://127.0.0.1:4223")
                || std::env::var_os("APP_MICRO__URL").is_some(),
            "{env:?}"
        );
        std::fs::write(dir.path().join("dock.toml"), "[services.mysql]\n").unwrap();
        assert!(app_env(dir.path()).is_err());
    }
}

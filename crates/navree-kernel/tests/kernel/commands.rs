//! The command runner: dispatch, built-in `check`/`help`, default command,
//! unknown commands, and the `check --graph` report.

use std::sync::{Arc, Mutex};

use navree_kernel::{BoxError, CommandSpec, Factory, Navree, KernelError, Module, ModuleBuilder};
use tokio_util::sync::CancellationToken;

use crate::support::{Counter, Journal, injectable, module};

pub struct Greeting(&'static str);

/// Registers `greet` (prints a greeting resolved from the container) and,
/// optionally, `serve`; counts constructions of an expensive provider.
struct CliModule {
    journal: Arc<Journal>,
    built: Arc<Counter>,
    serve: bool,
    seen_token: Arc<Mutex<Option<CancellationToken>>>,
}

pub struct Expensive;

impl CliModule {
    fn new(serve: bool) -> Self {
        Self {
            journal: Arc::new(Journal::default()),
            built: Arc::new(Counter::default()),
            serve,
            seen_token: Arc::default(),
        }
    }

    fn handles(
        &self,
    ) -> (
        Arc<Journal>,
        Arc<Counter>,
        Arc<Mutex<Option<CancellationToken>>>,
    ) {
        (
            self.journal.clone(),
            self.built.clone(),
            self.seen_token.clone(),
        )
    }
}

impl Module for CliModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let built = self.built.clone();
        m.value(Greeting("hello"))
            .factory(Factory::<Expensive>::new(move |_| {
                built.hit();
                async { Ok::<_, BoxError>(Arc::new(Expensive)) }
            }));

        let journal = self.journal.clone();
        let seen_token = self.seen_token.clone();
        m.command(CommandSpec::new(
            "greet",
            "Greets someone",
            move |ctx, args| {
                let journal = journal.clone();
                let seen_token = seen_token.clone();
                async move {
                    let greeting = ctx.get::<Greeting>().await?;
                    journal.push(format!("{} {}", greeting.0, args.join(" ")));
                    *seen_token.lock().unwrap() = Some(ctx.shutdown_token());
                    Ok(())
                }
            },
        ));

        if self.serve {
            let journal = self.journal.clone();
            m.command(CommandSpec::new(
                "serve",
                "Serves forever",
                move |_ctx, _args| {
                    let journal = journal.clone();
                    async move {
                        journal.push("served");
                        Ok(())
                    }
                },
            ));
        }
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[tokio::test]
async fn run_with_args_dispatches_command() {
    let module = CliModule::new(false);
    let (journal, _, seen_token) = module.handles();
    Navree::app()
        .module(module)
        .run_with_args(args(&["greet", "big", "world"]))
        .await
        .expect("greet");
    assert_eq!(journal.entries(), ["hello big world"]);
    let token = seen_token.lock().unwrap().clone().expect("command ran");
    assert!(
        token.is_cancelled(),
        "the app is shut down after the command"
    );
}

#[tokio::test]
async fn default_command_is_serve_when_registered() {
    let module = CliModule::new(true);
    let (journal, _, _) = module.handles();
    Navree::app()
        .module(module)
        .run_with_args(Vec::new())
        .await
        .expect("serve");
    assert_eq!(journal.entries(), ["served"]);
}

#[tokio::test]
async fn default_command_is_help_without_serve() {
    let module = CliModule::new(false);
    let (journal, built, _) = module.handles();
    Navree::app()
        .module(module)
        .run_with_args(Vec::new())
        .await
        .expect("help");
    assert!(journal.entries().is_empty());
    assert_eq!(built.get(), 0, "help does not boot the app");
}

#[tokio::test]
async fn check_validates_without_constructing() {
    let module = CliModule::new(false);
    let (_, built, _) = module.handles();
    let app = Navree::app().module(module);
    app.validate().expect("valid graph");
    app.run_with_args(args(&["check", "--graph"]))
        .await
        .expect("check");
    assert_eq!(built.get(), 0, "check never runs factories");
}

injectable!(NeedsMissing { greeting: Greeting });

module!(BrokenWiring, |m| {
    m.provider::<NeedsMissing>();
});

#[tokio::test]
async fn check_fails_on_wiring_errors() {
    let error = Navree::app()
        .module(BrokenWiring)
        .run_with_args(args(&["check"]))
        .await
        .expect_err("broken graph");
    assert!(error.to_string().contains("BrokenWiring"), "{error}");
}

#[tokio::test]
async fn unknown_command_errors() {
    let module = CliModule::new(false);
    let (_, built, _) = module.handles();
    let error = Navree::app()
        .module(module)
        .run_with_args(args(&["nope"]))
        .await
        .expect_err("unknown command");
    assert!(
        matches!(error.downcast_ref::<KernelError>(), Some(KernelError::UnknownCommand { name }) if name == "nope"),
        "{error:?}"
    );
    assert_eq!(built.get(), 0, "an unknown command does not boot the app");
}

#[tokio::test]
async fn app_run_command_runs_registered_commands_only() {
    let module = CliModule::new(false);
    let (journal, _, _) = module.handles();
    let app = Navree::app().module(module).boot().await.expect("boot");
    let names: Vec<&str> = app.ctx().commands().iter().map(|c| c.name).collect();
    assert_eq!(names, ["greet"]);

    app.run_command("greet", args(&["you"]))
        .await
        .expect("greet");
    assert_eq!(journal.entries(), ["hello you"]);
    let error = app
        .run_command("missing", Vec::new())
        .await
        .expect_err("unknown");
    assert!(matches!(
        error.downcast_ref::<KernelError>(),
        Some(KernelError::UnknownCommand { .. })
    ));
}

fn noop(name: &'static str) -> CommandSpec {
    CommandSpec::new(name, "does nothing", |_ctx, _args| async { Ok(()) })
}

module!(FirstCommands, |m| {
    m.command(noop("sync"));
});

module!(SecondCommands, |m| {
    m.import(FirstCommands).command(noop("sync"));
});

#[tokio::test]
async fn rejects_duplicate_command_names() {
    let errors = Navree::app()
        .module(SecondCommands)
        .validate()
        .expect_err("duplicate");
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::DuplicateCommand { name, first: "FirstCommands", second: "SecondCommands" }] if name == "sync"
        ),
        "{errors:?}"
    );
}

pub struct Repo;
injectable!(UserService { repo: Repo });

module!(DataModule, |m| {
    m.global().value(Repo).exports::<Repo>();
});

module!(UsersModule, |m| {
    m.import(DataModule)
        .provider::<UserService>()
        .command(noop("users:sync"));
});

#[tokio::test]
async fn graph_report_prints_module_tree() {
    let report = Navree::app().module(UsersModule).validate().expect("valid");
    let names: Vec<&str> = report.modules.iter().map(|m| m.name).collect();
    assert_eq!(names, ["DataModule", "UsersModule", "App"]);

    let text = report.to_string();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "App", "{text}");
    let users = lines
        .iter()
        .position(|l| l.contains("UsersModule"))
        .expect("UsersModule line");
    let data = lines
        .iter()
        .position(|l| l.contains("DataModule [global]"))
        .expect("DataModule line");
    assert!(
        users < data,
        "DataModule is nested under UsersModule:\n{text}"
    );
    assert!(
        text.contains(std::any::type_name::<UserService>()),
        "{text}"
    );
    assert!(text.contains("users:sync"), "{text}");
}

/// Counts its `register` calls; registers `serve` and `sync`.
struct CountedRegistrations(Arc<Counter>);

impl Module for CountedRegistrations {
    fn register(&self, m: &mut ModuleBuilder) {
        self.0.hit();
        m.command(noop("serve")).command(noop("sync"));
    }
}

#[tokio::test]
async fn run_with_args_registers_each_module_once() {
    for command in [args(&["sync"]), Vec::new()] {
        let registrations = Arc::new(Counter::default());
        Navree::app()
            .module(CountedRegistrations(registrations.clone()))
            .run_with_args(command.clone())
            .await
            .expect("command runs");
        assert_eq!(
            registrations.get(),
            1,
            "`register` runs once for {command:?}, not once per validation"
        );
    }
}

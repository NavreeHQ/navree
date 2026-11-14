//! Links modules add to the sidebar: checked at boot, drawn for who may open
//! them, and answered as JSON. A test binary of its own, because it reads the
//! warnings the boot logs.

use std::sync::{Arc, Mutex};

use http::Method;
use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
use navree_http::{Controller, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, TestAppBuilder};
use navree_ui::{AppShell, ModuleBuilderShellExt, Shell, ShellNavItem, UiModule};

/// The warnings logged on this thread while the guard lives (the tests run on a
/// current-thread runtime, so the boot logs on this thread).
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl Warnings {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for Warnings {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::WARN {
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// The app's module: it registers the app's one `Shell` and answers `GET /`
/// with the home page, drawn from the `AppShell` extractor.
struct Desk(Shell);

impl Module for Desk {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(self.0.clone()).exports::<Shell>();
        m.controller::<Home>();
    }
}

/// `GET /`: the home page.
struct Home;

impl Injectable for Home {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Home) })
    }
}

impl Controller for Home {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/", |shell: AppShell| async move {
                shell.page("Home")
            })
            .page()
            .public(),
        ]
    }
}

/// A test app with the toolkit and the app's `shell`, before a test adds its
/// modules.
fn builder(shell: Shell) -> TestAppBuilder {
    TestApp::builder().module(UiModule).module(Desk(shell))
}

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

/// A module that adds one link, labelled `Out`, to `.0`.
struct Outbound(&'static str);

impl Module for Outbound {
    fn register(&self, m: &mut ModuleBuilder) {
        m.nav_item(ShellNavItem::new("Out", self.0));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_contributed_link_that_leaves_the_site_fails_the_boot() {
    for href in [
        "https://evil.example/x",
        "//evil.example/x",
        "/\\evil.example",
        "\\\\evil.example",
        "javascript:alert(1)",
        "deals",
        "",
        "?page=2",
        "#top",
        "\u{3000}/deals",
        " /deals",
        "/a\nb",
    ] {
        let error = boot_error(
            builder(Shell::new("Kesar").contributed()).module(Outbound(href)),
        )
        .await;
        assert!(
            error.contains(
                "m.nav_item(\"Out\") in `Outbound`: the link must be a path on this app, such as /announcements"
            ),
            "{href:?}: {error}"
        );
    }
    for href in ["/", "/announcements?tab=new", "/ਖ਼ਬਰਾਂ"] {
        if let Err(error) = builder(Shell::new("Kesar").contributed())
            .module(Outbound(href))
            .try_build()
            .await
        {
            panic!("{href}: {error}");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_link_asking_for_an_undeclared_permission_warns_at_boot() {
    /// A module of the app with a link to its reports, which asks for
    /// `reports.view`; `.0` says whether the module describes it.
    struct Extras(bool);

    impl Module for Extras {
        fn register(&self, m: &mut ModuleBuilder) {
            m.nav_item(ShellNavItem::new("Reports", "/reports").permission("reports.view"));
            if self.0 {
                m.permissions(&[Permission::new(
                    "reports.view",
                    "View the monthly reports of the Amritsar branch",
                )]);
            }
        }
    }

    let warnings = Warnings::default();
    let _guard = warnings.install();
    let warning = "m.nav_item(\"Reports\") in `Extras` asks for the permission `reports.view`, which no module declares with m.permissions";
    builder(Shell::new("Kesar").contributed())
        .module(Extras(false))
        .build()
        .await;
    let first = warnings.all();
    assert_eq!(
        first.iter().filter(|line| *line == warning).count(),
        1,
        "{first:?}"
    );
    builder(Shell::new("Kesar").contributed())
        .module(Extras(true))
        .build()
        .await;
    let described = warnings.all();
    assert!(
        described[first.len()..]
            .iter()
            .all(|line| !line.contains("reports.view")),
        "{described:?}"
    );
}

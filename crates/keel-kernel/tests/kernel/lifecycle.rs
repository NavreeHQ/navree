//! Contributions, module info, lifecycle hooks and graceful shutdown.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use keel_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, Keel, KernelError, Lifecycle, Module,
    ModuleBuilder, ModuleId, ProviderKey, Resolver,
};

use crate::support::{Journal, module};

/// Something a module contributes to the app (like a route table).
#[derive(Debug, PartialEq)]
pub struct RouteSet(&'static str);

module!(BillingRoutes, |m| {
    m.contribute(RouteSet("billing"));
});

module!(UsersRoutes, |m| {
    m.import(BillingRoutes)
        .contribute(RouteSet("users"))
        .contribute(RouteSet("users-admin"));
});

module!(GlobalRoutes, |m| {
    m.global().contribute(RouteSet("global"));
});

#[tokio::test]
async fn contributions_are_collected_with_module_info() {
    let app = Keel::app()
        .module(UsersRoutes)
        .module(GlobalRoutes)
        .boot()
        .await
        .expect("boot");
    let collected: Vec<(&str, bool, &str)> = app
        .ctx()
        .contributions::<RouteSet>()
        .iter()
        .map(|c| (c.module.name, c.module.global, c.value.0))
        .collect();
    assert_eq!(
        collected,
        [
            ("BillingRoutes", false, "billing"),
            ("UsersRoutes", false, "users"),
            ("UsersRoutes", false, "users-admin"),
            ("GlobalRoutes", true, "global"),
        ]
    );
    let first = &app.ctx().contributions::<RouteSet>()[0];
    assert_eq!(first.module.id, ModuleId::of::<BillingRoutes>());
    assert!(app.ctx().contributions::<String>().is_empty());

    let modules: Vec<&str> = app.ctx().modules().iter().map(|m| m.name).collect();
    assert_eq!(
        modules,
        ["BillingRoutes", "UsersRoutes", "GlobalRoutes"],
        "dependency order, no implicit root"
    );
}

/// A provider whose hooks write to the shared journal.
pub struct Hooks {
    label: &'static str,
    journal: Arc<Journal>,
    fail_init: bool,
}

impl Hooks {
    fn build(
        label: &'static str,
        fail_init: bool,
    ) -> impl Fn(Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        move |r| {
            Box::pin(async move {
                Ok(Hooks {
                    label,
                    journal: r.get::<Journal>().await?,
                    fail_init,
                })
            })
        }
    }
}

impl Lifecycle for Hooks {
    fn on_init(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if self.fail_init {
                return Err("database unreachable".into());
            }
            self.journal.push(format!("init {}", self.label));
            Ok(())
        })
    }

    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            ctx.get::<Journal>()
                .await?
                .push(format!("bootstrap {}", self.label));
            Ok(())
        })
    }

    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.journal.push(format!("shutdown {}", self.label));
            Ok(())
        })
    }
}

macro_rules! hooks_provider {
    ($name:ident, $label:literal, $fail:literal) => {
        pub struct $name(Hooks);

        impl Injectable for $name {
            fn dependencies() -> Vec<ProviderKey> {
                vec![ProviderKey::of::<Journal>()]
            }

            fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
                Box::pin(async move { Ok($name(Hooks::build($label, $fail)(r).await?)) })
            }
        }

        impl Lifecycle for $name {
            fn on_init(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
                self.0.on_init(ctx)
            }

            fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
                self.0.on_bootstrap(ctx)
            }

            fn on_shutdown(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
                self.0.on_shutdown(ctx)
            }
        }
    };
}

hooks_provider!(DbHooks, "db", false);
hooks_provider!(WebHooks, "web", false);
hooks_provider!(BrokenHooks, "broken", true);

struct JournalModule(Arc<Journal>);

impl Module for JournalModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global().value_arc(self.0.clone()).exports::<Journal>();
    }
}

module!(DbModule, |m| {
    m.provider::<DbHooks>().lifecycle::<DbHooks>();
});

module!(WebModule, |m| {
    m.import(DbModule)
        .provider::<WebHooks>()
        .lifecycle::<WebHooks>();
});

module!(BrokenModule, |m| {
    m.provider::<BrokenHooks>().lifecycle::<BrokenHooks>();
});

module!(HooksWithoutProvider, |m| {
    m.lifecycle::<DbHooks>();
});

#[tokio::test]
async fn lifecycle_hooks_run_in_dependency_order_and_shutdown_in_reverse() {
    let journal = Arc::new(Journal::default());
    let app = Keel::app()
        .module(WebModule)
        .module(JournalModule(journal.clone()))
        .boot()
        .await
        .expect("boot");
    assert_eq!(
        journal.entries(),
        ["init db", "init web", "bootstrap db", "bootstrap web"]
    );

    app.shutdown().await.expect("shutdown");
    assert_eq!(
        journal.entries(),
        [
            "init db",
            "init web",
            "bootstrap db",
            "bootstrap web",
            "shutdown web",
            "shutdown db"
        ]
    );
}

#[tokio::test]
async fn failing_lifecycle_hook_fails_boot_naming_module_and_hook() {
    let journal = Arc::new(Journal::default());
    let errors = Keel::app()
        .module(BrokenModule)
        .module(JournalModule(journal))
        .boot()
        .await
        .expect_err("boot must fail");
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::Lifecycle { module: "BrokenModule", hook: "on_init", source }]
                if source.to_string() == "database unreachable"
        ),
        "{errors:?}"
    );
}

#[tokio::test]
async fn lifecycle_requires_a_provider_of_the_module() {
    let journal = Arc::new(Journal::default());
    let errors = Keel::app()
        .module(HooksWithoutProvider)
        .module(JournalModule(journal))
        .validate()
        .expect_err("validate must fail");
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::MissingProvider { module: "HooksWithoutProvider", required_by, .. }]
                if required_by.contains("lifecycle")
        ),
        "{errors:?}"
    );
}

#[tokio::test]
async fn shutdown_cancels_token_and_drains_tracker() {
    let app = Keel::app().boot().await.expect("boot");
    let token = app.ctx().shutdown_token();
    let drained = Arc::new(AtomicBool::new(false));
    let worker_token = token.clone();
    let worker_drained = drained.clone();
    app.ctx().tracker().spawn(async move {
        worker_token.cancelled().await;
        // Simulates finishing in-flight work after the shutdown signal.
        tokio::time::sleep(Duration::from_millis(20)).await;
        worker_drained.store(true, Ordering::SeqCst);
    });
    assert!(!token.is_cancelled());

    app.shutdown().await.expect("shutdown");
    assert!(token.is_cancelled(), "shutdown cancels the token");
    assert!(
        drained.load(Ordering::SeqCst),
        "shutdown waits for tracked tasks"
    );
}

#[tokio::test]
async fn shutdown_times_out_when_tasks_do_not_stop() {
    let app = Keel::app()
        .shutdown_timeout(Duration::from_millis(50))
        .boot()
        .await
        .expect("boot");
    app.ctx().tracker().spawn(std::future::pending::<()>());
    let error = app.shutdown().await.expect_err("timeout");
    assert!(error.to_string().contains("timed out"), "{error}");
}

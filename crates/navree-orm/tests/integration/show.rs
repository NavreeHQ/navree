//! `model:show`: models registered by modules, described with the hooks
//! the app registers for them.

use std::sync::Arc;

use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
use navree_orm::hooks::{ModelEvent, ModelHook};
use navree_orm::show::describe_model;
use navree_orm::{ExecContext, ModuleBuilderOrmExt, Record, Result, registered_models};

use crate::models::{Deal, Project, Ticket};
use crate::support::app;

/// Logs deals (a hook the app registers).
struct LogDeals;

impl ModelHook<Deal> for LogDeals {
    fn after<'a>(
        &'a self,
        _event: ModelEvent,
        _ctx: &'a ExecContext,
        _deal: &'a mut Record<Deal>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.model::<Deal>().model::<Ticket>();
        m.multi(Factory::<dyn ModelHook<Deal>>::new(|_| async {
            Ok(Arc::new(LogDeals) as Arc<dyn ModelHook<Deal>>)
        }));
    }
}

struct ProjectsModule;

impl Module for ProjectsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        // Registered twice: listed once.
        m.model::<Project>().model::<Deal>();
    }
}

#[tokio::test]
async fn registered_models_are_described() {
    let (app, _db, _clock) = app(vec![Box::new(DealsModule), Box::new(ProjectsModule)]).await;
    let names: Vec<&str> = registered_models(app.ctx()).iter().map(|model| model.name()).collect();
    assert_eq!(names, ["Deal", "Ticket", "Project"]);

    let deal = describe_model(app.ctx(), "deal").await.unwrap();
    assert_eq!((deal.model, deal.route_key), ("Deal", "slug"));
    assert_eq!(deal.app_hooks.len(), 1);
    assert!(deal.app_hooks[0].ends_with("LogDeals"), "{:?}", deal.app_hooks);
    assert!(deal.render().contains("hooks registered by the app"));

    let ticket = describe_model(app.ctx(), "tickets").await.unwrap();
    assert_eq!(ticket.subject, Some("Ticket"));
    assert_eq!(ticket.policy, Some("TicketPolicy"));
    assert!(ticket.app_hooks.is_empty());

    let project = describe_model(app.ctx(), "Project").await.unwrap();
    assert_eq!(project.workspace, Some("workspace_id"));
    assert!(
        project.global_scopes.iter().any(|s| s.ends_with("WorkspaceScope")),
        "{:?}",
        project.global_scopes
    );
    assert!(
        project.hooks.iter().any(|h| h.ends_with("WorkspaceHook")),
        "{:?}",
        project.hooks
    );

    let error = describe_model(app.ctx(), "Nope").await.unwrap_err().to_string();
    assert!(
        error.contains("the registered models are Deal, Ticket, Project"),
        "{error}"
    );
}

#[tokio::test]
async fn the_command_lists_and_shows_models() {
    let (app, _db, _clock) = app(vec![Box::new(DealsModule)]).await;
    for args in [
        &[][..],
        &["--json"][..],
        &["Deal"][..],
        &["deals", "--json"][..],
    ] {
        app.run_command("model:show", args)
            .await
            .unwrap_or_else(|error| panic!("{args:?}: {error}"));
    }
    let unknown = app.run_command("model:show", &["Nope"]).await.unwrap_err();
    assert!(unknown.to_string().contains("no model `Nope`"), "{unknown}");
    let flag = app.run_command("model:show", &["--wat"]).await.unwrap_err();
    assert!(flag.to_string().contains("unknown option `--wat`"), "{flag}");
    let two = app
        .run_command("model:show", &["Deal", "Ticket"])
        .await
        .unwrap_err();
    assert!(two.to_string().contains("takes one model"), "{two}");
}

#[tokio::test]
async fn without_registrations_the_command_says_how_to_register() {
    let (app, _db, _clock) = app(vec![Box::new(navree_orm::OrmModule)]).await;
    assert!(registered_models(app.ctx()).is_empty());
    app.run_command("model:show", &[]).await.unwrap();
    let error = app.run_command("model:show", &["Deal"]).await.unwrap_err();
    assert!(
        error.to_string().contains("ModuleBuilderOrmExt::model"),
        "{error}"
    );
}

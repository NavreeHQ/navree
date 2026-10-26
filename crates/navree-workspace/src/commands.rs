//! The commands `workspace:migrate` and `workspace:prune-invitations`.

use navree_db::Db;
use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::{Invitations, Workspaces};

/// Every command of the workspace module.
pub(crate) fn commands() -> [CommandSpec; 2] {
    [
        CommandSpec::new(
            "workspace:migrate",
            "Run pending workspace migrations in every workspace's schema or database ([slug...] for some)",
            migrate_command,
        ),
        CommandSpec::new(
            "workspace:prune-invitations",
            "Delete invitations that expired without being accepted",
            prune_command,
        ),
    ]
}

async fn migrate_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let workspaces = ctx.get::<Workspaces>().await?;
    let targets = if args.is_empty() {
        workspaces.all().await?
    } else {
        let mut targets = Vec::with_capacity(args.len());
        for identifier in &args {
            targets.push(workspaces.resolve(identifier).await?);
        }
        targets
    };
    let mut ran_any = false;
    for workspace in targets {
        for migration in workspaces.migrate(workspace.id).await? {
            ran_any = true;
            println!("Migrated {}: {migration}", workspace.slug);
        }
    }
    if !ran_any {
        println!("Nothing to migrate.");
    }
    Ok(())
}

async fn prune_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let (invitations, db) = (ctx.get::<Invitations>().await?, ctx.get::<Db>().await?);
    let uow = db.uow(None, None).await;
    match invitations.prune_expired(&uow).await {
        Ok(pruned) => {
            uow.commit().await?;
            println!("Pruned {pruned} expired invitation(s).");
            Ok(())
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error.into())
        }
    }
}

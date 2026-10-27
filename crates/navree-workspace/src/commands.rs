//! The commands `workspace:migrate`, `workspace:prune-invitations`,
//! `workspace:demo-create` and `workspace:demo-reset`.

use navree_db::Db;
use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::{DemoWorkspaces, Invitations, NewWorkspace, UserDirectory, Workspaces};

/// The command (and scheduled task) that resets demo workspaces.
pub(crate) const DEMO_RESET_COMMAND: &str = "workspace:demo-reset";

/// Every command of the workspace module.
pub(crate) fn commands() -> [CommandSpec; 4] {
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
        CommandSpec::new(
            "workspace:demo-create",
            "Create a demo workspace seeded by the demo seeders (<name> --owner <user id or email> [--slug <slug>])",
            demo_create_command,
        ),
        CommandSpec::new(
            DEMO_RESET_COMMAND,
            "Clear and seed the demo workspaces again ([slug...] for some)",
            demo_reset_command,
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

const DEMO_CREATE_USAGE: &str =
    "usage: workspace:demo-create <name> --owner <user id or email> [--slug <slug>]";

/// `workspace:demo-create <name> --owner <user id or email> [--slug <slug>]`.
async fn demo_create_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let mut name = Vec::new();
    let (mut owner, mut slug) = (None, None);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                (flag.to_owned(), Some(value.to_owned()))
            }
            _ => (arg.clone(), None),
        };
        match flag.as_str() {
            "--owner" | "--slug" => {
                let value = inline
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{flag} needs a value; {DEMO_CREATE_USAGE}"))?;
                if flag == "--owner" {
                    owner = Some(value);
                } else {
                    slug = Some(value);
                }
            }
            _ => name.push(arg),
        }
    }
    let name = name.join(" ");
    if name.trim().is_empty() {
        return Err(DEMO_CREATE_USAGE.into());
    }
    let owner = owner.ok_or_else(|| format!("--owner is required; {DEMO_CREATE_USAGE}"))?;
    let owner = match Uuid::parse_str(owner.trim()) {
        Ok(id) => id,
        Err(_) => {
            ctx.get::<dyn UserDirectory>()
                .await?
                .user_by_email(&owner)
                .await?
                .ok_or_else(|| format!("no user has the email address `{owner}`"))?
                .id
        }
    };
    let (demos, db) = (ctx.get::<DemoWorkspaces>().await?, ctx.get::<Db>().await?);
    let uow = db.uow(Some(owner), None).await;
    let created = uow
        .scope(demos.create(&uow, owner, NewWorkspace { name, slug }))
        .await;
    match created {
        Ok(workspace) => {
            uow.commit().await?;
            let seeders = demos.seeders().await?;
            println!(
                "Created demo workspace {} ({}).",
                workspace.slug,
                seeded_by(&seeders)
            );
            Ok(())
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error.into())
        }
    }
}

/// `workspace:demo-reset [slug...]`.
async fn demo_reset_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let demos = ctx.get::<DemoWorkspaces>().await?;
    let reset = if args.is_empty() {
        demos.reset_all().await?
    } else {
        let workspaces = ctx.get::<Workspaces>().await?;
        let mut reset = Vec::with_capacity(args.len());
        for identifier in &args {
            let workspace = workspaces.resolve(identifier).await?;
            reset.push(demos.reset(workspace.id).await?);
        }
        reset
    };
    if reset.is_empty() {
        println!("No demo workspaces.");
    }
    for demo in reset {
        let seeders: Vec<&str> = demo.seeders.iter().map(String::as_str).collect();
        println!(
            "Reset demo workspace {} ({}).",
            demo.workspace.slug,
            seeded_by(&seeders)
        );
    }
    Ok(())
}

fn seeded_by(seeders: &[&str]) -> String {
    if seeders.is_empty() {
        "no demo seeders are registered".to_owned()
    } else {
        format!("seeded by {}", seeders.join(", "))
    }
}

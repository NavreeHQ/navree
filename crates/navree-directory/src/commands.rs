//! The commands `directory:sync`, `directory:runs`,
//! `directory:connections`, `directory:token` and `directory:prune`.

use navree_integrations::Owner;
use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::types::{DirectoryConnection, RunTrigger, SyncOptions, SyncRun};
use crate::Directory;

/// The module's commands.
pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(
            "directory:sync",
            "Sync directory connections: <connection-id> | --all | --due [--dry-run] [--force] [--full]",
            sync,
        ),
        CommandSpec::new(
            "directory:runs",
            "List directory sync runs [<connection-id>] [--limit N]",
            runs,
        ),
        CommandSpec::new(
            "directory:connections",
            "List directory connections",
            connections,
        ),
        CommandSpec::new(
            "directory:token",
            "Give a SCIM connection a new bearer token: <connection-id>",
            token,
        ),
        CommandSpec::new(
            "directory:prune",
            "Delete sync runs older than directory.sync.keep_runs_days",
            prune,
        ),
    ]
}

fn uuid(value: &str) -> Result<Uuid, BoxError> {
    value
        .parse()
        .map_err(|_| format!("`{value}` is not a connection id").into())
}

/// `directory:sync <id> | --all | --due [--dry-run] [--force] [--full]`:
/// a connection or every active pull connection, in this process; with
/// `--due`, queues the connections whose next sync is due (what the
/// scheduler runs).
async fn sync(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let directory = ctx.get::<Directory>().await?;
    let mut options = SyncOptions {
        trigger: RunTrigger::Command,
        ..SyncOptions::default()
    };
    let mut id = None;
    let mut all = false;
    let mut due = false;
    for arg in args {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--force" => options.force = true,
            "--full" => options.full = true,
            "--all" => all = true,
            "--due" => due = true,
            other if other.starts_with("--") => {
                return Err(format!("unknown argument `{other}`").into());
            }
            other => id = Some(uuid(other)?),
        }
    }
    if due {
        let runs = directory.sync_due().await?;
        if runs.is_empty() {
            tracing::debug!(target: "navree::directory", "no directory connection was due for a sync");
        } else {
            println!("Queued {} directory sync(s)", runs.len());
        }
        return Ok(());
    }
    let ids: Vec<Uuid> = match (id, all) {
        (Some(id), _) => vec![id],
        (None, true) => directory
            .all_connections()
            .await?
            .into_iter()
            .filter(|c| !c.is_scim() && c.status == crate::ConnectionStatus::Active)
            .map(|c| c.id)
            .collect(),
        (None, false) => {
            return Err("name a connection, or pass --all or --due".into());
        }
    };
    let mut failed = false;
    for id in ids {
        let run = directory.sync_now(id, options).await?;
        println!("{}", run_line(&run));
        failed |= matches!(run.status, crate::RunStatus::Failed);
    }
    if failed {
        return Err("a directory sync failed".into());
    }
    Ok(())
}

/// `<run>  <connection>  <status>  +created ~updated ^reactivated -deactivated  groups +c ~u -d  <error>`.
pub(crate) fn run_line(run: &SyncRun) -> String {
    let s = &run.summary;
    let mut line = format!(
        "{}  {}  {}{}  users +{} ~{} ^{} -{}  groups +{} ~{} -{}",
        run.id,
        run.connection_id,
        run.status.as_str(),
        if run.dry_run { " (dry run)" } else { "" },
        s.users_created,
        s.users_updated,
        s.users_reactivated,
        s.users_deactivated,
        s.groups_created,
        s.groups_updated,
        s.groups_deleted,
    );
    if let Some(error) = &run.error {
        line.push_str("  ");
        line.push_str(error);
    }
    line
}

/// `directory:runs [<id>] [--limit N]`: the latest runs of one connection,
/// or of each.
async fn runs(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let directory = ctx.get::<Directory>().await?;
    let mut id = None;
    let mut limit = 20_u64;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => {
                let value = args.next().ok_or("--limit needs a number")?;
                limit = value
                    .parse()
                    .map_err(|_| format!("`{value}` is not a number"))?;
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown argument `{other}`").into());
            }
            other => id = Some(uuid(other)?),
        }
    }
    let ids: Vec<Uuid> = match id {
        Some(id) => vec![id],
        None => directory
            .all_connections()
            .await?
            .into_iter()
            .map(|c| c.id)
            .collect(),
    };
    let mut any = false;
    for id in ids {
        for run in directory.runs(id, limit).await? {
            any = true;
            println!("{}", run_line(&run));
        }
    }
    if !any {
        println!("No directory sync runs.");
    }
    Ok(())
}

/// `<id>  <owner>  <source>  <status>  <name>  last synced <time>`.
fn connection_line(connection: &DirectoryConnection) -> String {
    let owner = match connection.owner() {
        Owner::App => "app".to_owned(),
        other => other.to_string(),
    };
    let synced = connection
        .last_synced_at
        .map_or_else(|| "never".to_owned(), |at| at.to_rfc3339());
    format!(
        "{}  {}  {}  {}  {}  last synced {}",
        connection.id,
        owner,
        connection.source,
        connection.status.as_str(),
        connection.name,
        synced
    )
}

/// `directory:connections`: one line per connection.
async fn connections(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let directory = ctx.get::<Directory>().await?;
    let rows = directory.all_connections().await?;
    if rows.is_empty() {
        println!("No directory connections.");
    }
    for row in rows {
        println!("{}", connection_line(&row));
    }
    Ok(())
}

/// `directory:token <id>`: a new SCIM token, printed once.
async fn token(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let directory = ctx.get::<Directory>().await?;
    let id = args
        .first()
        .ok_or("name the SCIM connection: directory:token <connection-id>")?;
    let token = directory.rotate_scim_token(uuid(id)?).await?;
    println!("SCIM base URL: {}", directory.scim_url()?);
    println!("Bearer token:  {}", token.expose());
    println!("The token is shown once; the previous one no longer works.");
    Ok(())
}

/// `directory:prune`: old runs and their changes.
async fn prune(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let directory = ctx.get::<Directory>().await?;
    let count = directory.prune_runs().await?;
    println!("Pruned {count} directory sync run(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::types::{RunStatus, RunSummary};

    #[test]
    fn run_lines_show_the_counts() {
        let run = SyncRun {
            id: Uuid::nil(),
            connection_id: Uuid::nil(),
            status: RunStatus::Held,
            trigger: RunTrigger::Schedule,
            dry_run: false,
            force: false,
            complete: Some(true),
            summary: RunSummary {
                users_created: 2,
                users_deactivated: 40,
                ..RunSummary::default()
            },
            done: 0,
            total: 42,
            note: None,
            error: Some("too many".into()),
            operation_id: None,
            created_by: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
        };
        let line = run_line(&run);
        assert!(line.contains("held  users +2 ~0 ^0 -40  groups +0 ~0 -0  too many"), "{line}");
    }
}

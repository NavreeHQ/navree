//! The commands `integrations:check` and `integrations:list`.

use navree_kernel::{AppContext, BoxError, CommandSpec};
use uuid::Uuid;

use crate::{CheckSummary, ConnectionInfo, Integrations, Owner};

/// The module's commands.
pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(
            "integrations:check",
            "Check integration connections [--due] [--id <uuid>]",
            check,
        ),
        CommandSpec::new(
            "integrations:list",
            "List integration connections and their health",
            list,
        ),
    ]
}

/// `integrations:check`: every connection, or with `--due` those whose
/// last check is older than `integrations.check_interval_mins` (what the
/// scheduler runs), or with `--id` one connection.
async fn check(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let integrations = ctx.get::<Integrations>().await?;
    let mut due = false;
    let mut id: Option<Uuid> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--due" => due = true,
            "--id" => {
                let value = args.next().ok_or("--id needs a connection id")?;
                id = Some(
                    value
                        .parse()
                        .map_err(|_| format!("`{value}` is not a UUID"))?,
                );
            }
            other => return Err(format!("unknown argument `{other}`").into()),
        }
    }
    if let Some(id) = id {
        let health = integrations.check(id).await?;
        println!(
            "{}  {}{}",
            health.connection_id,
            health.status.as_str(),
            health
                .error
                .map(|error| format!("  {error}"))
                .unwrap_or_default()
        );
        return Ok(());
    }
    let summary = if due {
        integrations.check_due().await?
    } else {
        integrations.check_all().await?
    };
    print_summary(&summary, due);
    Ok(())
}

fn print_summary(summary: &CheckSummary, due: bool) {
    if summary.checked == 0 && due {
        tracing::debug!(target: "navree::integrations", "no connection was due for a check");
        return;
    }
    println!(
        "Checked {} connection(s): {} healthy, {} unhealthy",
        summary.checked, summary.healthy, summary.unhealthy
    );
}

/// `integrations:list`: one line per connection.
async fn list(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let integrations = ctx.get::<Integrations>().await?;
    let rows = integrations.all().await?;
    if rows.is_empty() {
        println!("No integration connections.");
        return Ok(());
    }
    for row in rows {
        println!("{}", line(&row));
    }
    Ok(())
}

/// `<id>  <owner>  <provider>/<scope set>  <kind>  <status>  <error>`.
fn line(row: &ConnectionInfo) -> String {
    let owner = match row.owner() {
        Owner::App => "app".to_owned(),
        other => other.to_string(),
    };
    let mut line = format!(
        "{}  {}  {}/{}  {}  {}",
        row.id,
        owner,
        row.provider,
        row.scope_set,
        row.kind,
        row.status.as_str()
    );
    if let Some(error) = &row.last_error {
        line.push_str("  ");
        line.push_str(error);
    }
    line
}

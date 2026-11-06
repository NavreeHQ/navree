//! The commands `webhooks:endpoints`, `webhooks:test`, `webhooks:retry` and
//! `webhooks:prune`.

use std::fmt::Write as _;

use navree_kernel::{AppContext, BoxError, CommandSpec};
use navree_workspace::Workspaces;
use uuid::Uuid;

use crate::{Attempt, Endpoint, Webhooks};

/// Every command of the webhooks module.
pub(crate) fn commands() -> [CommandSpec; 4] {
    [
        CommandSpec::new(
            "webhooks:endpoints",
            "List webhook endpoints with their status and failure counts ([--workspace <slug>])",
            endpoints_command,
        ),
        CommandSpec::new(
            "webhooks:test",
            "Send a webhook.test message to an endpoint and print the response (<endpoint-id>)",
            test_command,
        ),
        CommandSpec::new(
            "webhooks:retry",
            "Queue a failed delivery again (<delivery-id>), or every failed one (failed)",
            retry_command,
        ),
        CommandSpec::new(
            "webhooks:prune",
            "Delete deliveries and incoming receipts older than their retention ([--days N])",
            prune_command,
        ),
    ]
}

/// The value after `--name` (or in `--name=value`).
fn flag(args: &[String], name: &str) -> Result<Option<String>, BoxError> {
    let long = format!("--{name}");
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if *arg == long {
            return iter
                .next()
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("`{long}` needs a value").into());
        }
        if let Some(value) = arg.strip_prefix(&format!("{long}=")) {
            return Ok(Some(value.to_owned()));
        }
    }
    Ok(None)
}

/// The first argument that is not a flag.
fn positional(args: &[String]) -> Option<&str> {
    args.iter()
        .map(String::as_str)
        .find(|a| !a.starts_with("--"))
}

fn parse_id(text: &str, what: &str) -> Result<Uuid, BoxError> {
    Uuid::parse_str(text).map_err(|_| format!("`{text}` is not a {what} id").into())
}

/// One line per endpoint.
pub(crate) fn format_endpoints(endpoints: &[(Endpoint, u64)]) -> String {
    if endpoints.is_empty() {
        return "No webhook endpoints.\n".to_owned();
    }
    let mut out = String::new();
    for (endpoint, failed) in endpoints {
        let status = if endpoint.enabled {
            "enabled".to_owned()
        } else {
            format!(
                "disabled ({})",
                endpoint.disabled_reason.as_deref().unwrap_or("manual")
            )
        };
        let _ = write!(
            out,
            "{}  {}  {}  events={}  failed={}",
            endpoint.id,
            status,
            endpoint.url,
            endpoint.events.join(","),
            failed
        );
        if let Some(since) = endpoint.failing_since {
            let _ = write!(out, "  failing_since={}", since.to_rfc3339());
        }
        out.push('\n');
    }
    out
}

/// What `webhooks:test` prints.
pub(crate) fn format_attempt(attempt: &Attempt) -> String {
    let mut out = match (attempt.status_code, &attempt.error) {
        (Some(code), _) => format!("{code} in {} ms", attempt.duration_ms),
        (None, Some(error)) => format!("failed in {} ms: {error}", attempt.duration_ms),
        (None, None) => format!("no response in {} ms", attempt.duration_ms),
    };
    if let Some(body) = attempt.response_body.as_deref().filter(|b| !b.is_empty()) {
        out.push('\n');
        out.push_str(body);
    }
    out.push('\n');
    out
}

async fn endpoints_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let webhooks = ctx.get::<Webhooks>().await?;
    let workspace = match flag(&args, "workspace")? {
        Some(slug) => {
            let workspaces = ctx
                .try_get::<Workspaces>()
                .await
                .ok_or("`--workspace` needs WorkspaceModule in the app")?;
            Some(workspaces.resolve(&slug).await?.id)
        }
        None => None,
    };
    let mut rows = Vec::new();
    for endpoint in webhooks.endpoints(workspace).await? {
        let failed = webhooks.failed_count(endpoint.id).await?;
        rows.push((endpoint, failed));
    }
    print!("{}", format_endpoints(&rows));
    Ok(())
}

async fn test_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let id = positional(&args).ok_or("usage: webhooks:test <endpoint-id>")?;
    let id = parse_id(id, "endpoint")?;
    let attempt = ctx.get::<Webhooks>().await?.send_test(id).await?;
    print!("{}", format_attempt(&attempt));
    if attempt.succeeded {
        Ok(())
    } else {
        Err("the test delivery failed".into())
    }
}

async fn retry_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let target = positional(&args).ok_or("usage: webhooks:retry <delivery-id>|failed")?;
    let webhooks = ctx.get::<Webhooks>().await?;
    if target == "failed" {
        let count = webhooks.retry_failed().await?;
        println!(
            "Queued {count} failed deliver{} again.",
            if count == 1 { "y" } else { "ies" }
        );
    } else {
        let delivery = webhooks.retry(parse_id(target, "delivery")?).await?;
        println!("Queued delivery {} again.", delivery.id);
    }
    Ok(())
}

async fn prune_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let days = match flag(&args, "days")? {
        Some(days) => Some(
            days.parse::<u64>()
                .map_err(|_| format!("`--days` must be a number of days, not `{days}`"))?,
        ),
        None => None,
    };
    let pruned = ctx.get::<Webhooks>().await?.prune(days).await?;
    println!(
        "Pruned {} deliver{} and {} receipt(s).",
        pruned.deliveries,
        if pruned.deliveries == 1 { "y" } else { "ies" },
        pruned.receipts
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn flags_and_positionals_are_read() {
        assert_eq!(
            flag(&args(&["--workspace", "agra"]), "workspace").unwrap(),
            Some("agra".to_owned())
        );
        assert_eq!(
            flag(&args(&["--days=7"]), "days").unwrap(),
            Some("7".to_owned())
        );
        assert!(flag(&args(&["--days"]), "days").is_err());
        assert_eq!(flag(&args(&[]), "days").unwrap(), None);
        assert_eq!(positional(&args(&["--x", "failed"])), Some("failed"));
    }

    #[test]
    fn endpoints_print_status_events_and_failures() {
        let at = DateTime::from_timestamp(1_793_002_627, 0).unwrap();
        let endpoint = Endpoint {
            id: Uuid::nil(),
            workspace_id: None,
            url: "https://hooks.example.com/".to_owned(),
            description: None,
            events: vec!["deal.won".to_owned(), "deal.lost".to_owned()],
            headers: Default::default(),
            enabled: false,
            disabled_at: Some(at),
            disabled_reason: Some("failing".to_owned()),
            failing_since: Some(at),
            created_at: at,
            updated_at: at,
        };
        let out = format_endpoints(&[(endpoint, 3)]);
        assert!(out.contains("disabled (failing)"), "{out}");
        assert!(out.contains("events=deal.won,deal.lost"), "{out}");
        assert!(out.contains("failed=3"), "{out}");
        assert!(
            out.contains("failing_since=2026-10-26T08:17:07+00:00"),
            "{out}"
        );
        assert_eq!(format_endpoints(&[]), "No webhook endpoints.\n");
    }
}

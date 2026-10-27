//! The `lighthouse:*` commands.

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::dispatch::ChannelResult;
use crate::service::Lighthouse;
use crate::store::PruneSummary;

/// Every command of the module.
pub(crate) fn commands() -> [CommandSpec; 2] {
    [
        CommandSpec::new(
            "lighthouse:prune",
            "Delete what is older than lighthouse.retention_days: lighthouse:prune [--days N]",
            |ctx, args| async move { print(prune(ctx, args).await) },
        ),
        CommandSpec::new(
            "lighthouse:test-alerts",
            "Send a test alert through every alert channel, or one: lighthouse:test-alerts [channel]",
            |ctx, args| async move { test_alerts(ctx, args).await },
        ),
    ]
}

fn print(output: Result<String, BoxError>) -> Result<(), BoxError> {
    println!("{}", output?);
    Ok(())
}

/// `lighthouse:prune [--days N]`.
async fn prune(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    let days = parse_days(&args)?;
    let lighthouse = ctx.get::<Lighthouse>().await?;
    let store = lighthouse
        .store()
        .ok_or("lighthouse:prune: lighthouse.store is off, so there is nothing to prune")?;
    let days = days.unwrap_or(lighthouse.options().retention_days);
    let summary = store.prune(days).await?;
    Ok(prune_line(&summary, days))
}

/// `--days N` (or `--days=N`); `None` without it.
pub(crate) fn parse_days(args: &[String]) -> Result<Option<u32>, BoxError> {
    let mut days = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let value = match arg.strip_prefix("--days=") {
            Some(value) => value.to_owned(),
            None if arg == "--days" => iter
                .next()
                .cloned()
                .ok_or("lighthouse:prune: --days needs a number of days")?,
            None => return Err(format!("lighthouse:prune: unexpected argument `{arg}`").into()),
        };
        let parsed: u32 = value.parse().ok().filter(|days| *days > 0).ok_or_else(|| {
            format!("lighthouse:prune: --days must be a whole number of days (got `{value}`)")
        })?;
        days = Some(parsed);
    }
    Ok(days)
}

/// `Pruned 182,340 samples, 2,114 occurrences, 40,320 aggregate rows, 3
/// alerts and 12 issues older than 14 days.`
pub(crate) fn prune_line(summary: &PruneSummary, days: u32) -> String {
    format!(
        "Pruned {} samples, {} occurrences, {} aggregate rows, {} alerts and {} issues older than {days} day{}.",
        thousands(summary.samples),
        thousands(summary.occurrences),
        thousands(summary.aggregates),
        thousands(summary.alerts),
        thousands(summary.issues),
        if days == 1 { "" } else { "s" },
    )
}

/// `1234567` → `1,234,567`.
pub(crate) fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `lighthouse:test-alerts [channel]`: one line per channel; fails when a
/// channel failed.
async fn test_alerts(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    if args.len() > 1 {
        return Err("lighthouse:test-alerts takes at most one channel name".into());
    }
    let lighthouse = ctx.get::<Lighthouse>().await?;
    let results = lighthouse
        .test_alerts(args.first().map(String::as_str))
        .await?;
    let (lines, failed) = test_report(&results);
    println!("{lines}");
    if failed > 0 {
        return Err(format!("{failed} of {} alert channels failed", results.len()).into());
    }
    Ok(())
}

/// The lines `lighthouse:test-alerts` prints, and how many channels failed.
pub(crate) fn test_report(results: &[ChannelResult]) -> (String, usize) {
    if results.is_empty() {
        return (
            "No alert channels are configured (lighthouse.alerts.email, .slack, .webhook, or m.alert_channel::<C>()).".to_owned(),
            0,
        );
    }
    let width = results.iter().map(|r| r.channel.len()).max().unwrap_or(0);
    let mut failed = 0;
    let lines = results
        .iter()
        .map(|result| match &result.result {
            Ok(()) => format!("{:width$}  delivered", result.channel),
            Err(error) => {
                failed += 1;
                format!("{:width$}  failed: {error}", result.channel)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    (lines, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn days_parse_in_both_forms() {
        assert_eq!(parse_days(&args(&[])).unwrap(), None);
        assert_eq!(parse_days(&args(&["--days", "7"])).unwrap(), Some(7));
        assert_eq!(parse_days(&args(&["--days=30"])).unwrap(), Some(30));
        assert!(parse_days(&args(&["--days"])).is_err());
        assert!(parse_days(&args(&["--days", "0"])).is_err());
        assert!(parse_days(&args(&["--days", "x"])).is_err());
        assert!(parse_days(&args(&["7"])).is_err());
    }

    #[test]
    fn the_prune_line_reads_well() {
        let summary = PruneSummary {
            samples: 182_340,
            occurrences: 2_114,
            aggregates: 40_320,
            alerts: 3,
            issues: 12,
        };
        assert_eq!(
            prune_line(&summary, 14),
            "Pruned 182,340 samples, 2,114 occurrences, 40,320 aggregate rows, 3 alerts and 12 issues older than 14 days."
        );
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn the_test_report_lists_every_channel() {
        let (lines, failed) = test_report(&[
            ChannelResult {
                channel: "slack".into(),
                result: Ok(()),
            },
            ChannelResult {
                channel: "webhook".into(),
                result: Err("the webhook answered 500".into()),
            },
        ]);
        assert_eq!(failed, 1);
        assert_eq!(
            lines,
            "slack    delivered\nwebhook  failed: the webhook answered 500"
        );
        assert_eq!(test_report(&[]).1, 0);
    }
}

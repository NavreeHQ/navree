//! The `queue:*` commands.

use std::fmt::Write as _;
use std::time::Duration;

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::{FailedJob, JobBatch, Operations, Queue, WorkOptions};

/// The commands `QueueModule` registers.
pub(crate) fn commands() -> [CommandSpec; 7] {
    [
        CommandSpec::new(
            "queue:batches",
            "List the most recent job batches ([--limit N])",
            batches_command,
        ),
        CommandSpec::new(
            "queue:watchdog",
            "Fail or restart operations that stopped responding (schedule it every minute)",
            watchdog_command,
        ),
        CommandSpec::new(
            "queue:prune",
            "Delete finished batches and operations older than --hours N (default 168)",
            prune_command,
        ),
        CommandSpec::new(
            "queue:work",
            "Run queued jobs until shutdown ([--queue a,b] [--once] [--stop-when-empty] [--max-jobs N])",
            work_command,
        ),
        CommandSpec::new("queue:failed", "List the failed jobs", failed_command),
        CommandSpec::new(
            "queue:retry",
            "Put failed jobs back on their queues (<id> or all)",
            retry_command,
        ),
        CommandSpec::new("queue:flush", "Delete every failed job", flush_command),
    ]
}

async fn work_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let options = parse_work_args(&args)?;
    let queue = ctx.get::<Queue>().await?;
    let queues = if options.queues.is_empty() {
        queue.options().worker_queues()
    } else {
        options.queues.clone()
    };
    tracing::info!(
        target: "navree::job",
        queues = %queues.join(","),
        workers = if options.once { 1 } else { queue.options().workers },
        "queue worker started"
    );
    let stats = queue.work(options).await?;
    println!(
        "Processed {} job attempt(s): {} succeeded, {} released for retry, {} failed, {} cancelled.",
        stats.processed, stats.succeeded, stats.released, stats.failed, stats.cancelled
    );
    Ok(())
}

async fn failed_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let failed = ctx.get::<Queue>().await?.failed().await?;
    print!("{}", failed_listing(&failed));
    Ok(())
}

async fn retry_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let [target] = args.as_slice() else {
        return Err("usage: queue:retry <id|all>".into());
    };
    let retried = ctx.get::<Queue>().await?.retry(target.as_str()).await?;
    println!("Requeued {retried} failed job(s).");
    Ok(())
}

async fn flush_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let deleted = ctx.get::<Queue>().await?.flush_failed().await?;
    println!("Deleted {deleted} failed job(s).");
    Ok(())
}

async fn batches_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let limit = parse_number_flag(&args, "--limit", "queue:batches")?.unwrap_or(20);
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let batches = ctx.get::<Queue>().await?.batches(limit).await?;
    print!("{}", batch_listing(&batches));
    Ok(())
}

async fn watchdog_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let report = ctx.get::<Operations>().await?.sweep().await?;
    println!(
        "Operations: {} failed, {} restarted, {} finished with their batch.",
        report.failed, report.redispatched, report.finished
    );
    Ok(())
}

async fn prune_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let hours = parse_number_flag(&args, "--hours", "queue:prune")?.unwrap_or(168);
    let older_than = Duration::from_secs(hours.saturating_mul(3600));
    let batches = ctx.get::<Queue>().await?.prune_batches(older_than).await?;
    let operations = match ctx.try_get::<Operations>().await {
        Some(operations) => operations.prune(older_than).await?,
        None => 0,
    };
    println!("Deleted {batches} batch(es) and {operations} operation(s) finished over {hours} hour(s) ago.");
    Ok(())
}

/// The value of `--flag N` or `--flag=N`; no other argument is accepted.
fn parse_number_flag(args: &[String], flag: &str, command: &str) -> Result<Option<u64>, String> {
    let mut value = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let text = match arg.split_once('=') {
            Some((name, text)) if name == flag => text.to_owned(),
            None if arg == flag => args
                .next()
                .cloned()
                .ok_or_else(|| format!("`{flag}` needs a value"))?,
            _ => return Err(format!("unknown option `{arg}` for {command} (expected {flag})")),
        };
        value = Some(
            text.parse()
                .map_err(|_| format!("`{flag}` must be a whole number, not `{text}`"))?,
        );
    }
    Ok(value)
}

/// One line per batch: id, name, finished of total jobs, failures and
/// state.
pub(crate) fn batch_listing(batches: &[JobBatch]) -> String {
    if batches.is_empty() {
        return "No batches.\n".to_owned();
    }
    let mut out = String::new();
    for batch in batches {
        let state = if batch.cancelled() {
            "cancelled"
        } else if batch.finished() {
            "finished"
        } else {
            "running"
        };
        let _ = writeln!(
            out,
            "{}  {}  {}/{} jobs ({}%)  {} failed  {state}  {}",
            batch.id,
            batch.name,
            batch.processed_jobs(),
            batch.total_jobs,
            batch.progress(),
            batch.failed_jobs,
            batch.created_at.to_rfc3339(),
        );
    }
    out
}

/// Parses `[--queue a,b] [--once] [--stop-when-empty] [--max-jobs N]`
/// (`--flag value` or `--flag=value`).
pub(crate) fn parse_work_args(args: &[String]) -> Result<WorkOptions, String> {
    let mut options = WorkOptions::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .ok_or_else(|| format!("`{flag}` needs a value"))
        };
        match flag {
            "--queue" => {
                options.queues = value()?
                    .split(',')
                    .map(|q| q.trim().to_owned())
                    .filter(|q| !q.is_empty())
                    .collect();
            }
            "--max-jobs" => {
                let n = value()?;
                options.max_jobs = Some(
                    n.parse()
                        .map_err(|_| format!("`--max-jobs` must be a whole number, not `{n}`"))?,
                );
            }
            "--once" => options.once = true,
            "--stop-when-empty" => options.stop_when_empty = true,
            other => {
                return Err(format!(
                    "unknown option `{other}` for queue:work (expected --queue, --once, --stop-when-empty or --max-jobs)"
                ));
            }
        }
    }
    Ok(options)
}

/// One line per failed job: id, queue, type, attempts, time and the first
/// line of the error.
pub(crate) fn failed_listing(failed: &[FailedJob]) -> String {
    if failed.is_empty() {
        return "No failed jobs.\n".to_owned();
    }
    let mut out = String::new();
    for job in failed {
        let error = job.error.lines().next().unwrap_or_default();
        let _ = writeln!(
            out,
            "{}  {}  {}  attempts={}  {}  {}",
            job.id,
            job.queue,
            job.job_type,
            job.attempts,
            job.failed_at.format("%Y-%m-%d %H:%M:%S"),
            error
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use navree_contracts::JobOptions;
    use serde_json::json;

    use super::*;
    use crate::FailedJob;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn work_args_default_to_a_long_running_worker() {
        assert_eq!(parse_work_args(&[]).unwrap(), WorkOptions::default());
    }

    #[test]
    fn work_args_are_parsed() {
        let options = parse_work_args(&args(&[
            "--queue",
            "high, default",
            "--once",
            "--max-jobs",
            "5",
            "--stop-when-empty",
        ]))
        .unwrap();
        assert_eq!(
            options,
            WorkOptions {
                queues: vec!["high".into(), "default".into()],
                once: true,
                max_jobs: Some(5),
                stop_when_empty: true,
            }
        );
        let options = parse_work_args(&args(&["--queue=emails", "--max-jobs=2"])).unwrap();
        assert_eq!(options.queues, ["emails"]);
        assert_eq!(options.max_jobs, Some(2));
    }

    #[test]
    fn bad_work_args_are_rejected() {
        assert!(
            parse_work_args(&args(&["--bogus"]))
                .unwrap_err()
                .contains("--bogus")
        );
        assert!(
            parse_work_args(&args(&["--queue"]))
                .unwrap_err()
                .contains("--queue")
        );
        assert!(
            parse_work_args(&args(&["--max-jobs", "many"]))
                .unwrap_err()
                .contains("--max-jobs")
        );
    }

    #[test]
    fn failed_listing_shows_id_queue_type_attempts_time_and_first_error_line() {
        let failed_at: DateTime<Utc> = "2026-10-25T12:00:30Z".parse().unwrap();
        let job = FailedJob {
            id: "0199-abc".into(),
            queue: "emails".into(),
            job_type: "mail.send".into(),
            payload: json!({}),
            options: JobOptions::default(),
            error: "smtp refused\nsecond line".into(),
            attempts: 3,
            failed_at,
        };
        let text = failed_listing(&[job]);
        assert!(text.contains("0199-abc"), "{text}");
        assert!(text.contains("emails"), "{text}");
        assert!(text.contains("mail.send"), "{text}");
        assert!(text.contains("2026-10-25 12:00:30"), "{text}");
        assert!(text.contains("smtp refused"), "{text}");
        assert!(!text.contains("second line"), "{text}");
        assert_eq!(failed_listing(&[]), "No failed jobs.\n");
    }
}

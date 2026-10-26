//! The `queue:*` commands.

use std::fmt::Write as _;

use navis_kernel::{AppContext, BoxError, CommandSpec};

use crate::{FailedJob, Queue, WorkOptions};

/// The commands `QueueModule` registers.
pub(crate) fn commands() -> [CommandSpec; 4] {
    [
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
        target: "navis::job",
        queues = %queues.join(","),
        workers = if options.once { 1 } else { queue.options().workers },
        "queue worker started"
    );
    let stats = queue.work(options).await?;
    println!(
        "Processed {} job attempt(s): {} succeeded, {} released for retry, {} failed.",
        stats.processed, stats.succeeded, stats.released, stats.failed
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
    use navis_contracts::JobOptions;
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

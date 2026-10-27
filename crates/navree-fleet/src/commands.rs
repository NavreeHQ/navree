//! The `fleet` commands.

use std::fmt::Write as _;

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::record::MasterReport;
use crate::{Fleet, FleetStatus, Master, QueueWorkload, Supervisor};

/// The commands `FleetModule` registers.
pub(crate) fn commands() -> [CommandSpec; 7] {
    [
        CommandSpec::new(
            "fleet",
            "Run the Fleet master and its supervisors' workers until stopped",
            fleet_command,
        ),
        CommandSpec::new(
            "fleet:status",
            "Show every Fleet master, its processes and each queue's workload",
            status_command,
        ),
        CommandSpec::new(
            "fleet:pause",
            "Pause one supervisor, or all of them ([supervisor])",
            pause_command,
        ),
        CommandSpec::new(
            "fleet:continue",
            "Continue one supervisor, or all of them ([supervisor])",
            continue_command,
        ),
        CommandSpec::new(
            "fleet:terminate",
            "Stop every Fleet master gracefully, for its process manager to restart it",
            terminate_command,
        ),
        CommandSpec::new(
            "fleet:forget",
            "Delete one failed job (<id>)",
            forget_command,
        ),
        CommandSpec::new(
            "fleet:work",
            "Run one Fleet worker process (started by `fleet`)",
            crate::worker::work_command,
        ),
    ]
}

async fn fleet_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    if let Some(extra) = args.first() {
        return Err(format!("fleet takes no arguments (got `{extra}`)").into());
    }
    let master = Master::from_app(&ctx).await?;
    print!(
        "{}",
        banner(master.name(), std::process::id(), master.supervisors())
    );
    let summary = master.run(ctx.shutdown_token()).await?;
    println!(
        "Fleet stopped: {} worker(s) started, {} recycled, {} failed; {} job attempt(s).",
        summary.started, summary.recycled, summary.crashed, summary.jobs
    );
    Ok(())
}

/// `Fleet started on worker-1 (pid 48211) with 2 supervisors.` and one
/// line per supervisor.
pub(crate) fn banner(name: &str, pid: u32, supervisors: &[Supervisor]) -> String {
    let mut out = format!(
        "Fleet started on {name} (pid {pid}) with {} supervisor{}.\n",
        supervisors.len(),
        if supervisors.len() == 1 { "" } else { "s" }
    );
    let width = supervisors.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for s in supervisors {
        let _ = writeln!(
            out,
            "  {:width$}  {}: {}, {}",
            s.name,
            s.queues.join(", "),
            s.balance,
            s.processes_label()
        );
    }
    out
}

async fn status_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let fleet = ctx.get::<Fleet>().await?;
    let masters = fleet.masters().await?;
    let status = fleet.status().await?;
    let workload = fleet.workload().await?;
    print!("{}", status_text(status, &masters, &workload));
    if status == FleetStatus::Inactive {
        return Err("no Fleet master is running".into());
    }
    Ok(())
}

/// The `fleet:status` text.
pub(crate) fn status_text(
    status: FleetStatus,
    masters: &[MasterReport],
    workload: &[QueueWorkload],
) -> String {
    let mut out = match status {
        FleetStatus::Inactive => {
            "Fleet is inactive: no master has reported in the last 30 seconds.\n".to_owned()
        }
        other => format!("Fleet is {other}.\n"),
    };
    if !masters.is_empty() {
        out.push('\n');
        out.push_str(&table(
            &["Master", "Status", "Supervisors", "Processes", "Started"],
            masters
                .iter()
                .map(|m| {
                    vec![
                        m.name.clone(),
                        m.status.to_string(),
                        m.supervisors.len().to_string(),
                        m.process_count().to_string(),
                        m.started_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                    ]
                })
                .collect(),
        ));
    }
    if !workload.is_empty() {
        out.push('\n');
        out.push_str(&table(
            &["Queue", "Pending", "Oldest wait", "Processes", "Jobs/min"],
            workload
                .iter()
                .map(|w| {
                    vec![
                        w.queue.clone(),
                        w.pending.to_string(),
                        format!("{} s", w.oldest_wait_secs),
                        w.processes.to_string(),
                        w.jobs_per_minute.to_string(),
                    ]
                })
                .collect(),
        ));
    }
    out
}

/// Left-aligned columns, two spaces apart.
fn table(headers: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut text = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i + 1 == cells.len() {
                text.push_str(cell);
            } else {
                let _ = write!(text, "{cell:width$}  ", width = widths[i]);
            }
        }
        text.push('\n');
        text
    };
    let mut out = line(headers.to_vec());
    for row in &rows {
        out.push_str(&line(row.iter().map(String::as_str).collect()));
    }
    out
}

fn one_supervisor(args: &[String], command: &str) -> Result<Option<String>, BoxError> {
    match args {
        [] => Ok(None),
        [name] => Ok(Some(name.clone())),
        _ => Err(format!("usage: {command} [supervisor]").into()),
    }
}

async fn pause_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let name = one_supervisor(&args, "fleet:pause")?;
    ctx.get::<Fleet>().await?.pause(name.as_deref()).await?;
    match name {
        Some(name) => println!("Paused supervisor {name}."),
        None => println!("Paused every supervisor."),
    }
    Ok(())
}

async fn continue_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let name = one_supervisor(&args, "fleet:continue")?;
    ctx.get::<Fleet>().await?.resume(name.as_deref()).await?;
    match name {
        Some(name) => println!("Continued supervisor {name}."),
        None => println!("Continued every supervisor."),
    }
    Ok(())
}

async fn terminate_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    ctx.get::<Fleet>().await?.terminate().await?;
    println!("Asked every Fleet master to stop; their workers finish the jobs they are running.");
    Ok(())
}

async fn forget_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let [id] = args.as_slice() else {
        return Err("usage: fleet:forget <id>".into());
    };
    ctx.get::<Fleet>().await?.forget(id).await?;
    println!("Deleted failed job {id}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use navree_queue::QueueOptions;

    use super::*;
    use crate::record::{ProcessReport, SupervisorReport};
    use crate::{Balance, SupervisorOptions};

    #[test]
    fn the_banner_lists_each_supervisor() {
        let queue = QueueOptions::default();
        let supervisors = [
            Supervisor::resolve(
                "default",
                &SupervisorOptions {
                    max_processes: 8,
                    ..SupervisorOptions::new(["default", "mail", "notifications"])
                },
                &queue,
            ),
            Supervisor::resolve(
                "exports",
                &SupervisorOptions {
                    balance: Balance::Off,
                    max_processes: 2,
                    ..SupervisorOptions::new(["exports"])
                },
                &queue,
            ),
        ];
        assert_eq!(
            banner("worker-1", 48211, &supervisors),
            "Fleet started on worker-1 (pid 48211) with 2 supervisors.\n  \
             default  default, mail, notifications: auto, 1-8 processes\n  \
             exports  exports: off, 2 processes\n"
        );
    }

    #[test]
    fn the_status_text_has_a_master_and_a_queue_table() {
        let started = Utc.with_ymd_and_hms(2026, 10, 26, 9, 15, 12).unwrap();
        let master = MasterReport {
            id: "m1".into(),
            name: "worker-1".into(),
            pid: 1,
            status: FleetStatus::Running,
            supervisors: vec![SupervisorReport {
                name: "default".into(),
                queues: vec!["default".into()],
                balance: Balance::Auto,
                max_processes: 4,
                paused: false,
                processes: vec![ProcessReport {
                    queue: "default".into(),
                    pid: Some(2),
                    memory_mb: Some(40),
                    jobs: 3,
                    started_at: started,
                }],
            }],
            started_at: started,
            heartbeat_at: started,
        };
        let mut work = QueueWorkload::new("default", 12, 0, 3, 100);
        work.processes = 3;
        work.jobs_per_minute = 420;
        let text = status_text(FleetStatus::Running, &[master], &[work]);
        assert_eq!(
            text,
            "Fleet is running.\n\n\
             Master    Status   Supervisors  Processes  Started\n\
             worker-1  running  1            1          2026-10-26 09:15:12\n\n\
             Queue    Pending  Oldest wait  Processes  Jobs/min\n\
             default  12       3 s          3          420\n"
        );
        assert!(status_text(FleetStatus::Inactive, &[], &[]).starts_with("Fleet is inactive"));
    }
}

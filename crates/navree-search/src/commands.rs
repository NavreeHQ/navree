//! The commands `search:import`, `search:flush`, `search:settings` and
//! `search:status`.

use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::Search;
use crate::service::Entry;

/// Every command of the search module.
pub(crate) fn commands() -> [CommandSpec; 4] {
    [
        CommandSpec::new(
            "search:import",
            "Index every row of a searchable entity (<Name>... or --all; --fresh builds a new index and swaps it in)",
            import_command,
        ),
        CommandSpec::new(
            "search:flush",
            "Remove every document from an entity's index (<Name>...)",
            flush_command,
        ),
        CommandSpec::new(
            "search:settings",
            "Push index settings (fields, synonyms, stop words) to the engine ([<Name>...])",
            settings_command,
        ),
        CommandSpec::new(
            "search:status",
            "Show each index with its engine, documents, rows and pending jobs",
            status_command,
        ),
    ]
}

/// The entries named in `args` (`--all` or none with `all_by_default`:
/// every entry).
fn targets(
    search: &Search,
    args: &[String],
    all_by_default: bool,
    usage: &str,
) -> Result<Vec<Arc<Entry>>, BoxError> {
    let names: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let all = args.iter().any(|a| a == "--all");
    if all || (names.is_empty() && all_by_default) {
        return Ok(search.entries().to_vec());
    }
    if names.is_empty() {
        return Err(usage.into());
    }
    names
        .into_iter()
        .map(|name| search.entry_named(name).map_err(Into::into))
        .collect()
}

async fn import_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let search = ctx.get::<Search>().await?;
    let fresh = args.iter().any(|a| a == "--fresh");
    let entries = targets(
        &search,
        &args,
        false,
        "usage: search:import <Name>... [--fresh] | search:import --all [--fresh]",
    )?;
    for entry in entries {
        let rows = match search.db() {
            Ok(db) => entry.ops.count_rows(db.conn()).await.unwrap_or(0),
            Err(_) => 0,
        };
        println!(
            "Importing {} into {}{}: {} rows",
            entry.settings.name(),
            entry.index,
            if fresh { " (fresh)" } else { "" },
            thousands(rows)
        );
        let started = Instant::now();
        let mut progress = |done: u64, total: u64| {
            print!(
                "\r{}",
                progress_bar(done, total, started.elapsed().as_secs_f64())
            );
            let _ = std::io::stdout().flush();
        };
        let report = search.import_entry(&entry, fresh, &mut progress).await?;
        if report.rows > 0 {
            println!();
        }
        println!(
            "Imported {} documents ({} skipped by the document filter).",
            thousands(report.documents),
            thousands(report.skipped)
        );
    }
    Ok(())
}

async fn flush_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let search = ctx.get::<Search>().await?;
    for entry in targets(&search, &args, false, "usage: search:flush <Name>...")? {
        search.flush_entry(&entry).await?;
        println!("Flushed {}.", entry.index);
    }
    Ok(())
}

async fn settings_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let search = ctx.get::<Search>().await?;
    for entry in targets(&search, &args, true, "usage: search:settings [<Name>...]")? {
        search.push_entry_settings(&entry).await?;
        println!("Pushed the settings of {}.", entry.index);
    }
    Ok(())
}

async fn status_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let search = ctx.get::<Search>().await?;
    let statuses = search.status().await?;
    if statuses.is_empty() {
        println!("No searchable entities are registered.");
        return Ok(());
    }
    let rows: Vec<[String; 6]> = statuses
        .iter()
        .map(|s| {
            [
                s.name.clone(),
                s.index.clone(),
                s.engine.clone(),
                thousands(s.documents),
                thousands(s.rows),
                thousands(s.pending_jobs),
            ]
        })
        .collect();
    print!(
        "{}",
        table(
            [
                "Name",
                "Index",
                "Engine",
                "Documents",
                "Rows",
                "Pending jobs"
            ],
            &rows
        )
    );
    Ok(())
}

/// `12480` → `12,480`.
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

/// `[=====     ] 50/100  1.2s`.
pub(crate) fn progress_bar(done: u64, total: u64, seconds: f64) -> String {
    const WIDTH: u64 = 42;
    let filled = (done.min(total) * WIDTH)
        .checked_div(total)
        .unwrap_or(WIDTH);
    let filled = usize::try_from(filled).unwrap_or(0);
    let empty = usize::try_from(WIDTH).unwrap_or(0).saturating_sub(filled);
    format!(
        "[{}{}] {done}/{total}  {seconds:.1}s",
        "=".repeat(filled),
        " ".repeat(empty)
    )
}

/// A plain text table with a header row.
fn table<const N: usize>(header: [&str; N], rows: &[[String; N]]) -> String {
    let mut widths: [usize; N] = header.map(str::len);
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut out = cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{c:<width$}", width = widths[i]))
            .collect::<Vec<_>>()
            .join("  ");
        out.truncate(out.trim_end().len());
        out.push('\n');
        out
    };
    let mut out = line(header.to_vec());
    for row in rows {
        out.push_str(&line(row.iter().map(String::as_str).collect()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_get_thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(12_480), "12,480");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn progress_bars_fill_with_the_work_done() {
        let bar = progress_bar(21, 42, 2.06);
        assert!(
            bar.starts_with(&format!("[{}{}]", "=".repeat(21), " ".repeat(21))),
            "{bar}"
        );
        assert!(bar.ends_with("21/42  2.1s"), "{bar}");
        assert!(progress_bar(0, 0, 0.0).starts_with(&format!("[{}]", "=".repeat(42))));
    }

    #[test]
    fn tables_align_columns() {
        let out = table(
            ["Name", "Rows"],
            &[["Contact".to_owned(), "12,480".to_owned()]],
        );
        assert_eq!(out, "Name     Rows\nContact  12,480\n");
    }
}

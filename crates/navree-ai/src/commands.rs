//! The `ai:prune` command.

use chrono::Utc;
use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::{AiOptions, Conversations};

/// `ai:prune [--days N]`: deletes conversations untouched for `N` days
/// (default `ai.memory.retention_days`).
pub(crate) fn prune_command() -> CommandSpec {
    CommandSpec::new(
        "ai:prune",
        "Delete agent conversations untouched for ai.memory.retention_days (or --days N)",
        prune,
    )
}

async fn prune(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let options = ctx.get::<AiOptions>().await?;
    let days = parse_days(&args)?.unwrap_or(options.memory.retention_days);
    let conversations = ctx.get::<Conversations>().await?;
    let deleted = conversations.prune(days, Utc::now()).await?;
    println!("Deleted {deleted} conversation(s) untouched for {days} day(s).");
    Ok(())
}

/// `--days N` or `--days=N`.
fn parse_days(args: &[String]) -> Result<Option<u64>, BoxError> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let value = if let Some(value) = arg.strip_prefix("--days=") {
            value.to_owned()
        } else if arg == "--days" {
            iter.next().cloned().ok_or("--days needs a number")?
        } else {
            return Err(format!("unknown argument `{arg}` (usage: ai:prune [--days N])").into());
        };
        return value
            .parse::<u64>()
            .map(Some)
            .map_err(|_| format!("--days needs a whole number, not `{value}`").into());
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_parse() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(parse_days(&args(&[])).unwrap(), None);
        assert_eq!(parse_days(&args(&["--days", "7"])).unwrap(), Some(7));
        assert_eq!(parse_days(&args(&["--days=30"])).unwrap(), Some(30));
        assert!(parse_days(&args(&["--days", "x"])).is_err());
        assert!(parse_days(&args(&["--force"])).is_err());
    }
}

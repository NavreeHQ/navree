//! The `flags:*` commands.

use std::collections::BTreeMap;

use navree_kernel::{AppContext, BoxError, CommandSpec};
use navree_workspace::Workspaces;
use uuid::Uuid;

use crate::service::Change;
use crate::{FlagDefinition, FlagRegistry, FlagState, Flags, Rollout, RolloutUnit, ScopeType};

/// Every command of the flags module.
pub(crate) fn commands() -> [CommandSpec; 8] {
    [
        CommandSpec::new(
            "flags:list",
            "List every registered flag with its default, global state, rollout and override count",
            |ctx, args| async move { print(list(ctx, args).await) },
        ),
        CommandSpec::new(
            "flags:on",
            "Turn a flag on: flags:on <flag> [--user <id>] [--workspace <id-or-slug>] (everyone without an option)",
            |ctx, args| async move { print(toggle(ctx, args, "flags:on", Some(true)).await) },
        ),
        CommandSpec::new(
            "flags:off",
            "Turn a flag off: flags:off <flag> [--user <id>] [--workspace <id-or-slug>] (everyone without an option)",
            |ctx, args| async move { print(toggle(ctx, args, "flags:off", Some(false)).await) },
        ),
        CommandSpec::new(
            "flags:forget",
            "Remove an override, or reset the global state: flags:forget <flag> [--user <id>] [--workspace <id-or-slug>]",
            |ctx, args| async move { print(toggle(ctx, args, "flags:forget", None).await) },
        ),
        CommandSpec::new(
            "flags:rollout",
            "Set a rollout: flags:rollout <flag> <percent> [--of users|workspaces] [--plans pro,business]; `off` removes it",
            |ctx, args| async move { print(rollout(ctx, args).await) },
        ),
        CommandSpec::new(
            "flags:kill",
            "The kill switch: turn a flag off for everyone, overrides included",
            |ctx, args| async move { print(kill(ctx, args, true).await) },
        ),
        CommandSpec::new(
            "flags:revive",
            "Turn the kill switch off: back to the overrides and rollout the flag had",
            |ctx, args| async move { print(kill(ctx, args, false).await) },
        ),
        CommandSpec::new(
            "flags:purge",
            "Delete the stored state of flags no module registers any more ([--dry-run])",
            |ctx, args| async move { print(purge(ctx, args).await) },
        ),
    ]
}

/// Prints a command's output.
fn print(output: Result<String, BoxError>) -> Result<(), BoxError> {
    println!("{}", output?);
    Ok(())
}

/// `flags:list`.
async fn list(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    if let Some(arg) = args.first() {
        return Err(format!("flags:list: unexpected argument `{arg}`").into());
    }
    let flags = ctx.get::<Flags>().await?;
    let registry = ctx.get::<FlagRegistry>().await?;
    let states = flags.stored_states().await?;
    let counts = flags.override_counts().await?;
    Ok(render_list(registry.iter(), &states, &counts))
}

/// The `flags:list` table.
pub(crate) fn render_list<'a>(
    definitions: impl Iterator<Item = &'a FlagDefinition>,
    states: &BTreeMap<String, FlagState>,
    counts: &BTreeMap<String, u64>,
) -> String {
    let on_off = |value: bool| if value { "on" } else { "off" }.to_owned();
    let mut rows = vec![[
        "flag".to_owned(),
        "default".to_owned(),
        "everyone".to_owned(),
        "rollout".to_owned(),
        "overrides".to_owned(),
    ]];
    for definition in definitions {
        let state = states.get(definition.name);
        let everyone = match state {
            Some(state) if state.killed => "killed".to_owned(),
            Some(FlagState {
                everyone: Some(value),
                ..
            }) => on_off(*value),
            _ => "-".to_owned(),
        };
        let rollout = state
            .and_then(|state| state.rollout.as_ref())
            .map_or_else(|| "-".to_owned(), ToString::to_string);
        rows.push([
            definition.name.to_owned(),
            on_off(definition.default),
            everyone,
            rollout,
            counts.get(definition.name).copied().unwrap_or(0).to_string(),
        ]);
    }
    if rows.len() == 1 {
        return "No flags are registered.".to_owned();
    }
    let widths: Vec<usize> = (0..5)
        .map(|column| rows.iter().map(|row| row[column].len()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|row| {
            let mut line = String::new();
            for (column, cell) in row.iter().enumerate() {
                if column + 1 == row.len() {
                    line.push_str(cell);
                } else {
                    line.push_str(&format!("{cell:<width$}  ", width = widths[column]));
                }
            }
            line.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Who a command targets: `--user <id>`, `--workspace <id-or-slug>`, or
/// everyone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Everyone,
    User(Uuid),
    Workspace(String),
}

/// `<flag> [--user <id>] [--workspace <id-or-slug>]`.
pub(crate) fn parse_toggle(command: &str, args: &[String]) -> Result<(String, Target), String> {
    let mut args = args.iter();
    let flag = args
        .next()
        .filter(|arg| !arg.starts_with("--"))
        .ok_or_else(|| format!("{command}: name a flag: {command} <flag> [--user <id>] [--workspace <id-or-slug>]"))?
        .clone();
    let mut target = Target::Everyone;
    while let Some(arg) = args.next() {
        let (option, inline) = match arg.split_once('=') {
            Some((option, value)) => (option, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        let mut value = |name: &str| {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("{command}: `{name}` needs a value"))
        };
        let next = match option {
            "--user" => {
                let raw = value("--user")?;
                Target::User(Uuid::parse_str(&raw).map_err(|_| {
                    format!("{command}: `--user {raw}` is not a user id (a UUID)")
                })?)
            }
            "--workspace" => Target::Workspace(value("--workspace")?),
            other => return Err(format!("{command}: unexpected argument `{other}`")),
        };
        if target != Target::Everyone {
            return Err(format!(
                "{command}: name one user or one workspace, not both"
            ));
        }
        target = next;
    }
    Ok((flag, target))
}

/// The id of workspace `identifier` (an id, or a slug looked up with
/// WorkspaceModule).
async fn workspace_id(ctx: &AppContext, identifier: &str) -> Result<Uuid, BoxError> {
    if let Ok(id) = Uuid::parse_str(identifier) {
        return Ok(id);
    }
    let workspaces = ctx.try_get::<Workspaces>().await.ok_or_else(|| {
        format!("`{identifier}` is not a workspace id, and the app has no WorkspaceModule to look up slugs")
    })?;
    Ok(workspaces.resolve(identifier).await?.id)
}

/// `flags:on`, `flags:off` and `flags:forget`.
async fn toggle(
    ctx: AppContext,
    args: Vec<String>,
    command: &str,
    value: Option<bool>,
) -> Result<String, BoxError> {
    let (name, target) = parse_toggle(command, &args)?;
    let flags = ctx.get::<Flags>().await?;
    let definition = flags.definition(&name)?;
    let (change, who) = match &target {
        Target::Everyone => (Change::Everyone(value), "everyone".to_owned()),
        Target::User(id) => (
            Change::Override(ScopeType::User, *id, value),
            format!("user {id}"),
        ),
        Target::Workspace(identifier) => {
            let id = workspace_id(&ctx, identifier).await?;
            (
                Change::Override(ScopeType::Workspace, id, value),
                format!("workspace {identifier}"),
            )
        }
    };
    flags.change(&definition, change).await?;
    Ok(match (value, &target) {
        (Some(true), _) => format!("{name} is on for {who}."),
        (Some(false), _) => format!("{name} is off for {who}."),
        (None, Target::Everyone) => format!("{name}: the global state is unset."),
        (None, _) => format!("{name}: the override of {who} is removed."),
    })
}

/// `<flag> <percent>|off [--of users|workspaces] [--plans a,b]`.
pub(crate) fn parse_rollout(args: &[String]) -> Result<(String, Option<Rollout>), String> {
    const USAGE: &str = "flags:rollout <flag> <percent> [--of users|workspaces] [--plans pro,business], or flags:rollout <flag> off";
    let mut args = args.iter();
    let flag = args
        .next()
        .filter(|arg| !arg.starts_with("--"))
        .ok_or_else(|| format!("flags:rollout: name a flag: {USAGE}"))?
        .clone();
    let amount = args
        .next()
        .ok_or_else(|| format!("flags:rollout: give a percentage: {USAGE}"))?;
    if amount == "off" {
        if let Some(arg) = args.next() {
            return Err(format!("flags:rollout: unexpected argument `{arg}`"));
        }
        return Ok((flag, None));
    }
    let percent = amount
        .trim_end_matches('%')
        .parse::<u8>()
        .ok()
        .filter(|percent| *percent <= 100)
        .ok_or_else(|| format!("flags:rollout: `{amount}` is not a percentage from 0 to 100"))?;
    let mut rollout = Rollout::percent(percent);
    while let Some(arg) = args.next() {
        let (option, inline) = match arg.split_once('=') {
            Some((option, value)) => (option, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        let mut value = |name: &str| {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("flags:rollout: `{name}` needs a value"))
        };
        match option {
            "--of" => {
                rollout.of = match value("--of")?.as_str() {
                    "users" => RolloutUnit::Users,
                    "workspaces" => RolloutUnit::Workspaces,
                    other => {
                        return Err(format!(
                            "flags:rollout: `--of {other}`: expected `users` or `workspaces`"
                        ));
                    }
                };
            }
            "--plans" => {
                rollout.plans = value("--plans")?
                    .split(',')
                    .map(str::trim)
                    .filter(|plan| !plan.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            other => return Err(format!("flags:rollout: unexpected argument `{other}`")),
        }
    }
    Ok((flag, Some(rollout)))
}

/// `flags:rollout`.
async fn rollout(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    let (name, rollout) = parse_rollout(&args)?;
    let flags = ctx.get::<Flags>().await?;
    let definition = flags.definition(&name)?;
    let message = match &rollout {
        Some(rollout) => format!("{name}: rollout {rollout}."),
        None => format!("{name}: rollout stopped."),
    };
    flags.set_rollout(&definition, rollout).await?;
    Ok(message)
}

/// `flags:kill` and `flags:revive`.
async fn kill(ctx: AppContext, args: Vec<String>, killed: bool) -> Result<String, BoxError> {
    let command = if killed { "flags:kill" } else { "flags:revive" };
    let [name] = args.as_slice() else {
        return Err(format!("{command}: name one flag: {command} <flag>").into());
    };
    let flags = ctx.get::<Flags>().await?;
    let definition = flags.definition(name)?;
    flags.change(&definition, Change::Killed(killed)).await?;
    Ok(if killed {
        format!("{name} is killed: off for everyone until flags:revive {name}.")
    } else {
        format!("{name} is revived.")
    })
}

/// `flags:purge [--dry-run]`.
async fn purge(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    let dry_run = match args.as_slice() {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        [other, ..] => return Err(format!("flags:purge: unexpected argument `{other}`").into()),
    };
    let flags = ctx.get::<Flags>().await?;
    let names = flags.unregistered_names().await?;
    if names.is_empty() {
        return Ok("Nothing to purge: every stored flag is registered.".to_owned());
    }
    let mut lines = Vec::with_capacity(names.len());
    for name in &names {
        if dry_run {
            lines.push(format!("Would purge {name}."));
        } else {
            flags.purge(name).await?;
            lines.push(format!("Purged {name}."));
        }
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn toggle_arguments() {
        let user = Uuid::now_v7();
        assert_eq!(
            parse_toggle("flags:on", &args(&["new-pipeline"])),
            Ok(("new-pipeline".to_owned(), Target::Everyone))
        );
        assert_eq!(
            parse_toggle("flags:on", &args(&["new-pipeline", "--user", &user.to_string()])),
            Ok(("new-pipeline".to_owned(), Target::User(user)))
        );
        assert_eq!(
            parse_toggle("flags:off", &args(&["x", "--workspace=acme"])),
            Ok(("x".to_owned(), Target::Workspace("acme".to_owned())))
        );
        for bad in [
            &[][..],
            &["--user", "x"][..],
            &["x", "--user"][..],
            &["x", "--user", "nope"][..],
            &["x", "--team", "a"][..],
            &["x", "--workspace", "a", "--user", "0199"][..],
        ] {
            assert!(parse_toggle("flags:on", &args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn rollout_arguments() {
        assert_eq!(
            parse_rollout(&args(&["new-pipeline", "25", "--of", "workspaces"])),
            Ok((
                "new-pipeline".to_owned(),
                Some(Rollout::percent(25).of_workspaces())
            ))
        );
        assert_eq!(
            parse_rollout(&args(&["ai", "100%", "--of=workspaces", "--plans", "pro, business"])),
            Ok(("ai".to_owned(), Some(Rollout::plans(["pro", "business"]))))
        );
        assert_eq!(
            parse_rollout(&args(&["ai", "off"])),
            Ok(("ai".to_owned(), None))
        );
        for bad in [
            &["ai"][..],
            &["ai", "101"][..],
            &["ai", "ten"][..],
            &["ai", "10", "--of", "teams"][..],
            &["ai", "10", "--fast"][..],
            &["ai", "off", "--of", "users"][..],
        ] {
            assert!(parse_rollout(&args(bad)).is_err(), "{bad:?}");
        }
    }

    fn definition(name: &'static str, default: bool) -> FlagDefinition {
        FlagDefinition {
            name,
            default,
            expose: false,
            description: "",
            type_name: name,
        }
    }

    #[test]
    fn the_list_is_an_aligned_table() {
        let definitions = [
            definition("ai-summaries", false),
            definition("deal-export", true),
            definition("new-pipeline", false),
            definition("old-board", false),
        ];
        let states = BTreeMap::from([
            (
                "ai-summaries".to_owned(),
                FlagState {
                    rollout: Some(Rollout::plans(["pro", "business"])),
                    ..FlagState::unset("ai-summaries")
                },
            ),
            (
                "new-pipeline".to_owned(),
                FlagState {
                    rollout: Some(Rollout::percent(25).of_workspaces()),
                    everyone: Some(false),
                    ..FlagState::unset("new-pipeline")
                },
            ),
            (
                "old-board".to_owned(),
                FlagState {
                    everyone: Some(true),
                    killed: true,
                    ..FlagState::unset("old-board")
                },
            ),
        ]);
        let counts = BTreeMap::from([("deal-export".to_owned(), 2), ("new-pipeline".to_owned(), 1)]);
        assert_eq!(
            render_list(definitions.iter(), &states, &counts),
            "flag          default  everyone  rollout             overrides\n\
             ai-summaries  off      -         plans pro,business  0\n\
             deal-export   on       -         -                   2\n\
             new-pipeline  off      off       25% of workspaces   1\n\
             old-board     off      killed    -                   0"
        );
        assert_eq!(
            render_list([].iter(), &states, &counts),
            "No flags are registered."
        );
    }
}

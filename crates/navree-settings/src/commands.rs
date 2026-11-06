//! The `settings:*` commands.

use navree_db::Db;
use navree_kernel::{AppContext, BoxError, CommandSpec};
use navree_workspace::{Organizations, Workspaces};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{ResolvedSettings, Scope, Settings, SettingsRegistry, Source};

/// Every command of the settings module.
pub(crate) fn commands() -> [CommandSpec; 4] {
    [
        CommandSpec::new(
            "settings:list",
            "List the registered settings with their levels and fields",
            |ctx, args| async move { print(list(ctx, args).await) },
        ),
        CommandSpec::new(
            "settings:show",
            "Show settings with the source of every value: settings:show <key> [--workspace <id-or-slug>] [--organization <id-or-slug>] [--user <id>]",
            |ctx, args| async move { print(show(ctx, args).await) },
        ),
        CommandSpec::new(
            "settings:set",
            "Set settings at a level: settings:set <key> <field>=<value>... [--workspace ..] [--organization ..] [--user <id>] (global without an option)",
            |ctx, args| async move { print(set(ctx, args).await) },
        ),
        CommandSpec::new(
            "settings:inherit",
            "Make fields inherit again: settings:inherit <key> <field>... | --all [--workspace ..] [--organization ..] [--user <id>]",
            |ctx, args| async move { print(inherit(ctx, args).await) },
        ),
    ]
}

fn print(output: Result<String, BoxError>) -> Result<(), BoxError> {
    println!("{}", output?);
    Ok(())
}

/// The scope options of a command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopeArgs {
    pub(crate) workspace: Option<String>,
    pub(crate) organization: Option<String>,
    pub(crate) user: Option<Uuid>,
}

/// A parsed command line: the key, the positional arguments after it, the
/// scope options and `--all`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub(crate) key: String,
    pub(crate) rest: Vec<String>,
    pub(crate) scope: ScopeArgs,
    pub(crate) all: bool,
}

/// `<key> [positional..] [--workspace <x>] [--organization <x>] [--user
/// <id>] [--all]` (options also as `--option=value`).
pub(crate) fn parse(command: &str, args: &[String]) -> Result<Parsed, String> {
    let mut args = args.iter();
    let key = args
        .next()
        .filter(|arg| !arg.starts_with("--"))
        .ok_or_else(|| format!("{command}: name the settings: {command} <key> .."))?
        .clone();
    let mut parsed = Parsed {
        key,
        ..Parsed::default()
    };
    while let Some(arg) = args.next() {
        if !arg.starts_with("--") {
            parsed.rest.push(arg.clone());
            continue;
        }
        let (option, inline) = match arg.split_once('=') {
            Some((option, value)) => (option, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        if option == "--all" {
            parsed.all = true;
            continue;
        }
        let value = inline
            .or_else(|| args.next().cloned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{command}: `{option}` needs a value"))?;
        match option {
            "--workspace" => parsed.scope.workspace = Some(value),
            "--organization" => parsed.scope.organization = Some(value),
            "--user" => {
                parsed.scope.user = Some(Uuid::parse_str(&value).map_err(|_| {
                    format!("{command}: `--user {value}` is not a user id (a UUID)")
                })?);
            }
            other => return Err(format!("{command}: unexpected argument `{other}`")),
        }
    }
    Ok(parsed)
}

/// `field=value` pairs as a patch: each value is JSON when it parses as
/// JSON (`true`, `5`, `"5"`, `[1, 2]`), else a string.
pub(crate) fn patch_of(command: &str, pairs: &[String]) -> Result<Map<String, Value>, String> {
    if pairs.is_empty() {
        return Err(format!(
            "{command}: give at least one <field>=<value> (a JSON value, or text)"
        ));
    }
    let mut patch = Map::new();
    for pair in pairs {
        let (field, raw) = pair
            .split_once('=')
            .filter(|(field, _)| !field.is_empty())
            .ok_or_else(|| format!("{command}: `{pair}` is not <field>=<value>"))?;
        let value =
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_owned()));
        patch.insert(field.to_owned(), value);
    }
    Ok(patch)
}

/// The scope the options name.
async fn scope_of(ctx: &AppContext, args: &ScopeArgs) -> Result<Scope, BoxError> {
    let mut scope = match args.user {
        Some(user) => Scope::user(user),
        None => Scope::global(),
    };
    if let Some(identifier) = &args.workspace {
        let workspaces = ctx
            .try_get::<Workspaces>()
            .await
            .ok_or("--workspace needs WorkspaceModule in the app (to find the workspace)")?;
        let workspace = workspaces.resolve(identifier).await?;
        scope = scope
            .in_workspace(workspace.id)
            .in_organization(workspace.organization_id);
    }
    if let Some(identifier) = &args.organization {
        let id = match Uuid::parse_str(identifier) {
            Ok(id) => id,
            Err(_) => {
                let organizations = ctx.try_get::<Organizations>().await.ok_or(
                    "--organization needs WorkspaceModule with organizations on (to find the organization)",
                )?;
                organizations.resolve(identifier).await?.id
            }
        };
        if scope.workspace_id().is_some() && scope.organization_id() != Some(id) {
            return Err(format!(
                "the workspace `{}` is not in the organization `{identifier}`",
                args.workspace.as_deref().unwrap_or_default()
            )
            .into());
        }
        scope = scope.in_organization(id);
    }
    Ok(scope)
}

/// `settings:list`.
async fn list(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    if let Some(arg) = args.first() {
        return Err(format!("settings:list: unexpected argument `{arg}`").into());
    }
    let registry = ctx.get::<SettingsRegistry>().await?;
    Ok(render_list(&registry))
}

/// The `settings:list` table.
pub(crate) fn render_list(registry: &SettingsRegistry) -> String {
    let mut rows = vec![[
        "key".to_owned(),
        "levels".to_owned(),
        "fields".to_owned(),
        "description".to_owned(),
    ]];
    for entry in registry.all() {
        rows.push([
            entry.definition.key.to_owned(),
            entry
                .definition
                .levels()
                .iter()
                .map(|level| level.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            entry.fields.iter().cloned().collect::<Vec<_>>().join(", "),
            entry.definition.description.to_owned(),
        ]);
    }
    if rows.len() == 1 {
        return "No settings are registered.".to_owned();
    }
    table(&rows)
}

/// `settings:show`.
async fn show(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    let parsed = parse("settings:show", &args)?;
    if let Some(arg) = parsed.rest.first() {
        return Err(format!("settings:show: unexpected argument `{arg}`").into());
    }
    let settings = ctx.get::<Settings>().await?;
    let scope = settings
        .complete(scope_of(&ctx, &parsed.scope).await?)
        .await?;
    let resolved = settings.resolve_key(&parsed.key, scope).await?;
    Ok(render_show(&parsed.key, &scope, &resolved))
}

/// The `settings:show` table.
pub(crate) fn render_show(key: &str, scope: &Scope, resolved: &ResolvedSettings) -> String {
    let mut rows = vec![["field".to_owned(), "value".to_owned(), "source".to_owned()]];
    for (field, value) in &resolved.data {
        let source = resolved.source(field).unwrap_or(Source::Default);
        let mut source = source.as_str().to_owned();
        if resolved.is_overridden(field) {
            source.push_str(" *");
        }
        rows.push([field.clone(), value.to_string(), source]);
    }
    let mut out = format!("{key} for {scope}\n\n{}", table(&rows));
    if !resolved.overridden.is_empty() {
        out.push_str("\n\n* set at this level");
    }
    out
}

/// Runs `work` in a unit of work of its own, committed when it succeeds.
async fn in_unit_of_work<F, Fut>(ctx: &AppContext, work: F) -> Result<ResolvedSettings, BoxError>
where
    F: FnOnce(navree_db::Uow) -> Fut,
    Fut: Future<Output = Result<ResolvedSettings, crate::SettingsError>>,
{
    let db = ctx
        .try_get::<Db>()
        .await
        .ok_or("this command needs DatabaseModule in the app")?;
    let uow = db.uow(None, None).await;
    match uow.scope(work(uow.clone())).await {
        Ok(resolved) => {
            uow.commit().await?;
            Ok(resolved)
        }
        Err(error) => {
            uow.rollback().await?;
            Err(error.into())
        }
    }
}

/// `settings:set`.
async fn set(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    let parsed = parse("settings:set", &args)?;
    if parsed.all {
        return Err("settings:set: unexpected argument `--all`".into());
    }
    let patch = patch_of("settings:set", &parsed.rest)?;
    let settings = ctx.get::<Settings>().await?;
    let scope = settings
        .complete(scope_of(&ctx, &parsed.scope).await?)
        .await?;
    let key = parsed.key.clone();
    let resolved = in_unit_of_work(&ctx, |uow| async move {
        settings.update_key(&uow, &key, scope, patch).await
    })
    .await?;
    Ok(render_show(&parsed.key, &scope, &resolved))
}

/// `settings:inherit`.
async fn inherit(ctx: AppContext, args: Vec<String>) -> Result<String, BoxError> {
    let parsed = parse("settings:inherit", &args)?;
    if parsed.rest.is_empty() && !parsed.all {
        return Err("settings:inherit: name the fields, or pass --all".into());
    }
    if !parsed.rest.is_empty() && parsed.all {
        return Err("settings:inherit: name the fields or pass --all, not both".into());
    }
    let settings = ctx.get::<Settings>().await?;
    let scope = settings
        .complete(scope_of(&ctx, &parsed.scope).await?)
        .await?;
    let (key, fields, all) = (parsed.key.clone(), parsed.rest.clone(), parsed.all);
    let resolved = in_unit_of_work(&ctx, |uow| async move {
        if all {
            settings.reset_key(&uow, &key, scope).await
        } else {
            settings.inherit_key(&uow, &key, scope, &fields).await
        }
    })
    .await?;
    Ok(render_show(&parsed.key, &scope, &resolved))
}

/// Left-aligned columns, two spaces apart.
fn table<const N: usize>(rows: &[[String; N]]) -> String {
    let widths: Vec<usize> = (0..N)
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    rows.iter()
        .map(|row| {
            let mut line = String::new();
            for (column, cell) in row.iter().enumerate() {
                if column + 1 == N {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn command_lines_parse() {
        let user = Uuid::now_v7();
        let parsed = parse(
            "settings:set",
            &args(&[
                "portal",
                "show_leaderboard=true",
                "--workspace",
                "agra",
                &format!("--user={user}"),
                "welcome_text=Hi",
            ]),
        )
        .unwrap();
        assert_eq!(parsed.key, "portal");
        assert_eq!(parsed.rest, ["show_leaderboard=true", "welcome_text=Hi"]);
        assert_eq!(parsed.scope.workspace.as_deref(), Some("agra"));
        assert_eq!(parsed.scope.user, Some(user));
        assert!(!parsed.all);

        let all = parse(
            "settings:inherit",
            &args(&["portal", "--all", "--organization=holding"]),
        )
        .unwrap();
        assert!(all.all);
        assert_eq!(all.scope.organization.as_deref(), Some("holding"));

        for (bad, message) in [
            (vec![], "name the settings"),
            (vec!["--workspace", "agra"], "name the settings"),
            (vec!["portal", "--workspace"], "`--workspace` needs a value"),
            (vec!["portal", "--user", "anu"], "is not a user id"),
            (
                vec!["portal", "--team", "x"],
                "unexpected argument `--team`",
            ),
        ] {
            let error = parse("settings:show", &args(&bad)).unwrap_err();
            assert!(error.contains(message), "{bad:?}: {error}");
        }
    }

    #[test]
    fn patches_read_json_or_text() {
        let patch = patch_of(
            "settings:set",
            &args(&[
                "show_leaderboard=true",
                "max_items=5",
                "welcome_text=Hello there",
                "code=\"5\"",
                "tags=[\"a\",\"b\"]",
                "empty=",
            ]),
        )
        .unwrap();
        assert_eq!(
            Value::Object(patch),
            json!({
                "show_leaderboard": true,
                "max_items": 5,
                "welcome_text": "Hello there",
                "code": "5",
                "tags": ["a", "b"],
                "empty": "",
            })
        );
        assert!(patch_of("settings:set", &[]).is_err());
        assert!(patch_of("settings:set", &args(&["novalue"])).is_err());
        assert!(patch_of("settings:set", &args(&["=x"])).is_err());
    }

    #[test]
    fn show_marks_sources_and_overrides() {
        let mut data = Map::new();
        data.insert("show_leaderboard".into(), json!(true));
        data.insert("welcome_text".into(), json!("Hi"));
        let resolved = ResolvedSettings {
            data,
            sources: BTreeMap::from([
                ("show_leaderboard".to_owned(), Source::Organization),
                ("welcome_text".to_owned(), Source::Workspace),
            ]),
            overridden: vec!["welcome_text".to_owned()],
        };
        let id = Uuid::nil();
        let out = render_show(
            "portal",
            &Scope::workspace(id).in_organization(None),
            &resolved,
        );
        assert_eq!(
            out,
            format!(
                "portal for workspace {id} < global\n\n\
                 field             value  source\n\
                 show_leaderboard  true   organization\n\
                 welcome_text      \"Hi\"   workspace *\n\n\
                 * set at this level"
            )
        );
    }
}

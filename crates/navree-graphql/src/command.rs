//! `graphql:schema [path] [--check]`: the schema as an SDL file.

use std::path::PathBuf;

use navree_kernel::{AppContext, BoxError, CommandSpec};
use similar::TextDiff;

use crate::schema::GraphqlSchema;

/// The default file.
const DEFAULT_PATH: &str = "schema.graphql";

pub(crate) fn schema_command() -> CommandSpec {
    CommandSpec::new(
        "graphql:schema",
        "Write the GraphQL schema as SDL ([path], default schema.graphql); --check compares instead",
        run,
    )
}

/// Writes the SDL to `path`, or with `--check` fails (exit status 1) with
/// the difference when the file and the code disagree.
async fn run(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let check = args.iter().any(|arg| arg == "--check");
    if let Some(unknown) = args
        .iter()
        .find(|arg| arg.starts_with("--") && *arg != "--check")
    {
        return Err(
            format!("graphql:schema: unknown option `{unknown}` (expected --check)").into(),
        );
    }
    let path = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map_or_else(|| PathBuf::from(DEFAULT_PATH), PathBuf::from);
    let sdl = ctx.get::<GraphqlSchema>().await?.sdl();
    if check {
        let current = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text.replace("\r\n", "\n"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(format!("cannot read {}: {error}", path.display()).into()),
        };
        if current == sdl {
            println!("{} is up to date", path.display());
            return Ok(());
        }
        let shown = path.display().to_string();
        let diff = TextDiff::from_lines(&current, &sdl)
            .unified_diff()
            .context_radius(3)
            .header(&shown, "the code's schema")
            .to_string();
        return Err(format!(
            "{shown} is out of date with the code's schema:\n{diff}\nRun `graphql:schema {shown}` and commit the result."
        )
        .into());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    tokio::fs::write(&path, &sdl)
        .await
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    println!("GraphQL schema written to {}", path.display());
    Ok(())
}

//! The command `sheets:prune`.

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::{Exports, Imports};

/// Every command of the sheets module.
pub(crate) fn commands() -> [CommandSpec; 1] {
    [CommandSpec::new(
        "sheets:prune",
        "Delete imports older than sheets.imports.keep_days and export files whose link expired",
        prune_command,
    )]
}

async fn prune_command(ctx: AppContext, _args: Vec<String>) -> Result<(), BoxError> {
    let imports = ctx.get::<Imports>().await?.prune().await?;
    let files = ctx.get::<Exports>().await?.prune_files().await?;
    println!(
        "Pruned {imports} import{} and {files} export file{}.",
        if imports == 1 { "" } else { "s" },
        if files == 1 { "" } else { "s" }
    );
    Ok(())
}

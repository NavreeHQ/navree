//! The commands `ConfigModule` registers: `config:show` and `config:check`.
//!
//! Both are offline: they run against the module graph, without booting the
//! app, so they open no database connection and start no server.

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::check::{CheckOptions, CheckReport};
use crate::show::{ShowOptions, defaults_tree, render};
use crate::{Config, ConfigSources, SectionRegistration};

/// The sections the app registered, without booting it.
fn registrations(ctx: &AppContext) -> Vec<SectionRegistration> {
    ctx.contributions::<SectionRegistration>()
        .into_iter()
        .map(|contributed| (*contributed.value).clone())
        .collect()
}

/// `config:show [section] [--defaults] [--origin] [--json]`.
pub(crate) fn show() -> CommandSpec {
    CommandSpec::new(
        "config:show",
        "Show the effective configuration with secrets masked: [section] [--defaults] [--origin] [--json]",
        |ctx: AppContext, args: Vec<String>| async move {
            let options = ShowOptions::parse(&args)?;
            let config = ctx.get::<Config>().await?;
            let defaults = defaults_tree(&registrations(&ctx));
            print!("{}", render(&config, &defaults, &options));
            Ok::<(), BoxError>(())
        },
    )
    .offline()
}

/// `config:check [--env <name>]`: exits with an error when the check fails.
pub(crate) fn check() -> CommandSpec {
    CommandSpec::new(
        "config:check",
        "Check the configuration: invalid sections, unknown keys, hidden .env values; --env production adds the production profile",
        |ctx: AppContext, args: Vec<String>| async move {
            let options = CheckOptions::parse(&args)?;
            let sources = ctx.get::<ConfigSources>().await?;
            let report = CheckReport::run(&sources, &registrations(&ctx), &options)?;
            print!("{}", report.render());
            if report.is_ok() {
                Ok(())
            } else {
                Err(format!("config:check found {} problem(s)", report.problems()).into())
            }
        },
    )
    .offline()
}

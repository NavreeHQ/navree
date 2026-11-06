//! The commands `ConfigModule` registers: `config:show`, `config:check`,
//! `config:schema` and `config:publish`.
//!
//! Both are offline: they run against the module graph, without booting the
//! app, so they open no database connection and start no server.

use navree_kernel::{AppContext, BoxError, CommandSpec};

use crate::check::{CheckOptions, CheckReport};
use crate::publish::{PublishOptions, run as publish_section};
use crate::show::{ShowOptions, render_fallbacks, render_with};
use crate::{Config, ConfigRegistry, ConfigSources, SectionRegistration};

/// The sections the app registered, without booting it.
fn registrations(ctx: &AppContext) -> Vec<SectionRegistration> {
    ctx.contributions::<SectionRegistration>()
        .into_iter()
        .map(|contributed| (*contributed.value).clone())
        .collect()
}

/// `config:show [section] [--defaults] [--origin] [--fallbacks] [--json]`.
pub(crate) fn show() -> CommandSpec {
    CommandSpec::new(
        "config:show",
        "Show the effective configuration with secrets masked: [section] [--defaults] [--origin] [--fallbacks] [--json]",
        |ctx: AppContext, args: Vec<String>| async move {
            let options = ShowOptions::parse(&args)?;
            let registry = ConfigRegistry::new(&registrations(&ctx));
            if options.fallbacks {
                let is_set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
                print!("{}", render_fallbacks(&registry, options.json, is_set));
                return Ok::<(), BoxError>(());
            }
            let config = ctx.get::<Config>().await?;
            let defaults = registry.defaults_tree();
            print!(
                "{}",
                render_with(&config, &defaults, &options, &registry.redactor())
            );
            Ok::<(), BoxError>(())
        },
    )
    .offline()
}

/// `config:schema [--out <file>]`: the JSON Schema of the configuration tree,
/// for editors, on standard output or in a file.
pub(crate) fn schema() -> CommandSpec {
    CommandSpec::new(
        "config:schema",
        "Print the JSON Schema of the configuration (for editors): [--out <file>]",
        |ctx: AppContext, args: Vec<String>| async move {
            let registry = ConfigRegistry::new(&registrations(&ctx));
            let mut text = serde_json::to_string_pretty(&registry.schema_document())?;
            text.push('\n');
            match args.as_slice() {
                [] => print!("{text}"),
                [flag, path] if flag == "--out" => {
                    std::fs::write(path, text)
                        .map_err(|error| format!("cannot write {path}: {error}"))?;
                    println!("wrote {path}");
                }
                _ => return Err("usage: config:schema [--out <file>]".into()),
            }
            Ok::<(), BoxError>(())
        },
    )
    .offline()
}

/// `config:publish <section> [--force] [--stdout]`: writes `config/<section>.toml`
/// with every key commented out beside its default.
pub(crate) fn publish() -> CommandSpec {
    CommandSpec::new(
        "config:publish",
        "Write config/<section>.toml with every key commented out beside its default: <section> [--force] [--stdout]",
        |ctx: AppContext, args: Vec<String>| async move {
            let options = PublishOptions::parse(&args)?;
            let registry = ConfigRegistry::new(&registrations(&ctx));
            print!(
                "{}",
                publish_section(&options, &registry, std::path::Path::new("config"))?
            );
            Ok::<(), BoxError>(())
        },
    )
    .offline()
}

/// `config:check [--env <name>]`: exits with an error when the check fails.
pub(crate) fn check() -> CommandSpec {
    CommandSpec::new(
        "config:check",
        "Check the configuration: invalid sections, unknown keys, keys set in .env and in a config file; --env production adds the production profile",
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

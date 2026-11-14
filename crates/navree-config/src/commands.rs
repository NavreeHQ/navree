//! The commands `ConfigModule` registers: `config:show`, `config:check`,
//! `config:schema` and `config:publish`.
//!
//! Both are offline: they run against the module graph, without booting the
//! app, so they open no database connection and start no server.

use navree_kernel::{AppContext, BoxError, CommandSpec, ModuleId};

use crate::check::{CheckOptions, CheckReport, WEB_APP_ENV};
use crate::publish::{PublishOptions, run as publish_section};
use crate::show::{ShowOptions, render_fallbacks, render_with};
use crate::{Config, ConfigClaim, ConfigRegistry, ConfigSources, SectionRegistration};

/// The sections the app registered, without booting it.
fn registrations(ctx: &AppContext) -> Vec<SectionRegistration> {
    ctx.contributions::<SectionRegistration>()
        .into_iter()
        .map(|contributed| (*contributed.value).clone())
        .collect()
}

/// The sections the modules of the plugin with `namespace` registered,
/// without booting the app; an error when no plugin has that namespace.
fn plugin_registrations(
    ctx: &AppContext,
    namespace: &str,
) -> Result<Vec<SectionRegistration>, String> {
    let report = ctx.plugins();
    // A plugin has at least one module: no module, no plugin.
    let modules: Vec<ModuleId> = report
        .plugins
        .iter()
        .filter(|plugin| plugin.info.namespace == namespace)
        .flat_map(|plugin| plugin.modules.iter().map(|module| module.id))
        .collect();
    if modules.is_empty() {
        return Err(format!(
            "config:publish: no plugin has the namespace `{namespace}`"
        ));
    }
    Ok(ctx
        .contributions::<SectionRegistration>()
        .into_iter()
        .filter(|contributed| modules.contains(&contributed.module.id))
        .map(|contributed| (*contributed.value).clone())
        .collect())
}

/// The keys the modules claimed.
fn claims(ctx: &AppContext) -> Vec<String> {
    ctx.contributions::<ConfigClaim>()
        .into_iter()
        .map(|contributed| contributed.value.key().to_owned())
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
/// `config:publish --plugin <namespace> [--force]`: writes such a file for
/// every section the modules of that plugin register.
pub(crate) fn publish() -> CommandSpec {
    CommandSpec::new(
        "config:publish",
        "Write config/<section>.toml with every key commented out beside its default: <section> [--force] [--stdout] | --plugin <namespace> [--force]",
        |ctx: AppContext, args: Vec<String>| async move {
            let options = PublishOptions::parse(&args)?;
            let sections = match &options.plugin {
                Some(namespace) => plugin_registrations(&ctx, namespace)?,
                None => registrations(&ctx),
            };
            let registry = ConfigRegistry::new(&sections);
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
        "Check the configuration: invalid sections, unknown keys, keys set in .env and in a config file, settings web/.env.local repeats; --env production adds the production profile",
        |ctx: AppContext, args: Vec<String>| async move {
            let options = CheckOptions::parse(&args)?;
            let sources = ctx.get::<ConfigSources>().await?;
            let report = CheckReport::run_with_claims(
                &sources,
                &registrations(&ctx),
                &claims(&ctx),
                &options,
            )?
            .with_web_app(&sources, std::path::Path::new(WEB_APP_ENV));
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

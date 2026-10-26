//! [`AuditModule`] and the `audit:prune` command.

use std::sync::Arc;

use navis_config::ModuleBuilderConfigExt;
use navis_contracts::{AuditSink, Clock, SystemClock};
use navis_db::{ChangeObserver, Db, ModuleBuilderDbExt};
use navis_http::ModuleBuilderHttpExt;
use navis_kernel::{
    AppContext, BoxError, CommandSpec, Factory, Module, ModuleBuilder, ModuleId, ProviderKey,
    Resolver,
};

use crate::controller::AuditController;
use crate::migration::migrations;
use crate::{AuditLog, AuditOptions};

/// The audit module. Global; provides and exports [`AuditOptions`],
/// [`AuditLog`] and `dyn AuditSink` (the same log), sees every unit of work
/// as a `dyn ChangeObserver` multi-binding, contributes the `audit`
/// migrations (table `audit_logs`), mounts `GET {api_prefix}/audit`
/// (permission `audit.read`; see [`AuditOptions::routes`]) and registers
/// the command `audit:prune [--days N]`.
///
/// It needs `DatabaseModule` in the app.
///
/// ```no_run
/// use navis_audit::{AuditModule, AuditOptions};
/// use navis_db::DatabaseModule;
/// use navis_kernel::Navis;
///
/// # fn main() -> Result<(), navis_kernel::BoxError> {
/// Navis::app()
///     .module(navis_config::ConfigModule::for_root(
///         navis_config::ConfigSources::defaults(),
///     ))
///     .module(DatabaseModule::from_config())
///     .module(AuditModule::for_root(AuditOptions {
///         retention_days: 90,
///         ..AuditOptions::default()
///     }))
///     .run()
/// # }
/// ```
///
/// Used plainly (`.module(AuditModule)`) or through
/// [`from_config`](AuditModule::from_config), it reads the `audit`
/// configuration section, so the app must include `ConfigModule`;
/// [`for_root`](AuditModule::for_root) needs no configuration.
#[derive(Clone, Copy, Debug, Default)]
pub struct AuditModule;

impl AuditModule {
    /// The module with explicit options (configure it once, in the app root).
    pub fn for_root(options: AuditOptions) -> impl Module {
        ConfiguredAuditModule {
            options: Some(options),
        }
    }

    /// The module configured from the `audit` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredAuditModule { options: None }
    }
}

impl Module for AuditModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `AuditModule::for_root` / `from_config`: same module id, configured.
struct ConfiguredAuditModule {
    /// `None`: read from configuration.
    options: Option<AuditOptions>,
}

impl Module for ConfiguredAuditModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<AuditModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<AuditOptions>) {
    m.global();
    match options {
        Some(options) => m.factory(Factory::<AuditOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options.check()?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<AuditOptions>(),
    };
    m.exports::<AuditOptions>()
        .migrations(migrations())
        .factory(
            Factory::<AuditLog>::new(|r: Resolver| async move {
                let options = r.get::<AuditOptions>().await?;
                let db = r.get::<Db>().await?;
                let ctx = r.app();
                let clock = match ctx.try_get::<dyn Clock>().await {
                    Some(clock) => clock,
                    None => Arc::new(SystemClock) as Arc<dyn Clock>,
                };
                let shutdown = ctx.shutdown_token();
                let tracker = ctx.tracker();
                let log = AuditLog::start(
                    (*db).clone(),
                    options,
                    clock,
                    async move { shutdown.cancelled().await },
                    |writer| {
                        tracker.spawn(writer);
                    },
                );
                Ok(Arc::new(log))
            })
            .deps(vec![
                ProviderKey::of::<AuditOptions>(),
                ProviderKey::of::<Db>(),
            ]),
        )
        .exports::<AuditLog>()
        .factory(
            Factory::<dyn AuditSink>::new(|r: Resolver| async move {
                Ok(r.get::<AuditLog>().await? as Arc<dyn AuditSink>)
            })
            .deps(vec![ProviderKey::of::<AuditLog>()]),
        )
        .exports::<dyn AuditSink>()
        .multi(
            Factory::<dyn ChangeObserver>::new(|r: Resolver| async move {
                Ok(r.get::<AuditLog>().await? as Arc<dyn ChangeObserver>)
            })
            .deps(vec![ProviderKey::of::<AuditLog>()]),
        )
        .controller::<AuditController>()
        .command(CommandSpec::new(
            "audit:prune",
            "Delete audit entries older than the retention period ([--days N])",
            prune_command,
        ));
}

async fn prune_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let days = parse_days(&args)?;
    let log = ctx.get::<AuditLog>().await?;
    let days = days.unwrap_or(log.options().retention_days);
    if days == 0 {
        println!("Audit retention is off (audit.retention_days = 0); nothing to prune.");
        return Ok(());
    }
    let pruned = log.prune(days).await?;
    println!("Pruned {pruned} audit entries older than {days} days.");
    Ok(())
}

/// `--days N` / `--days=N` (a positive integer), if given.
fn parse_days(args: &[String]) -> Result<Option<u32>, BoxError> {
    let mut days = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = match arg.strip_prefix("--days") {
            Some("") => args
                .next()
                .ok_or("audit:prune: `--days` needs a number of days")?,
            Some(rest) if rest.starts_with('=') => &rest[1..],
            _ => return Err(format!("audit:prune: unexpected argument `{arg}`").into()),
        };
        match value.trim().parse::<u32>() {
            Ok(n) if n > 0 => days = Some(n),
            _ => {
                return Err(format!(
                    "audit:prune: `--days` must be a positive number of days, not `{value}`"
                )
                .into());
            }
        }
    }
    Ok(days)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn days_argument_forms() {
        assert_eq!(parse_days(&args(&[])).unwrap(), None);
        assert_eq!(parse_days(&args(&["--days", "30"])).unwrap(), Some(30));
        assert_eq!(parse_days(&args(&["--days=7"])).unwrap(), Some(7));
        assert!(parse_days(&args(&["--days"])).is_err());
        assert!(parse_days(&args(&["--days", "0"])).is_err());
        assert!(parse_days(&args(&["--days", "x"])).is_err());
        assert!(parse_days(&args(&["--force"])).is_err());
    }
}

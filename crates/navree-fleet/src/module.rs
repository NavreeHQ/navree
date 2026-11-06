//! [`FleetModule`].

use std::sync::Arc;

use navree_access::AccessModule;
use navree_config::{Config, ConfigSection, ModuleBuilderConfigExt};
use navree_contracts::Clock;
use navree_db::{Db, ModuleBuilderDbExt};
use navree_http::ModuleBuilderHttpExt;
use navree_kernel::{ConfiguredModule, Factory, Module, ModuleBuilder, ProviderKey, Resolver};
use navree_observe::MetricsCollector;
use navree_queue::Queue;
use navree_telemetry::{ModuleBuilderTelemetryExt, TelemetrySink};

use crate::commands::commands;
use crate::dashboard::{FleetController, FleetSubject};
use crate::gauges::FleetGauges;
use crate::migration::migrations;
use crate::recorder::JobRecorder;
use crate::redact::Redactor;
use crate::store::FleetStore;
use crate::{Fleet, FleetOptions};

/// Fleet: supervised queue workers, their metrics and the dashboard.
///
/// Global; provides and exports [`Fleet`] and [`FleetOptions`] (section
/// `fleet`), and contributes:
///
/// - the `fleet` migrations (`fleet_masters`, `fleet_state`, `fleet_jobs`,
///   `fleet_metrics`; after `queue`);
/// - the commands `fleet`, `fleet:status`, `fleet:pause [supervisor]`,
///   `fleet:continue [supervisor]`, `fleet:terminate`, `fleet:forget <id>`
///   and `fleet:work` (the worker process a master starts);
/// - the dashboard at `fleet.path` (`/_fleet`) and its API under
///   `{path}/api`, when `fleet.dashboard` is on, behind the abilities
///   `view` and `operate` on the subject `Fleet` (permissions
///   `fleet.view` and `fleet.operate`);
/// - a telemetry sink recording every job attempt of the process, which
///   workers report to their master;
/// - Fleet's gauges on `GET /metrics` when the app has `ObserveModule`.
///
/// The app needs `ConfigModule`, `DatabaseModule` and `QueueModule` (the
/// `fleet` command refuses the `memory` driver). With the dashboard on, it
/// also needs an authenticator (`AuthModule`) and navree-access's route
/// authorizer (`AccessModule`); set `fleet.dashboard = false` otherwise.
#[derive(Clone, Copy, Debug, Default)]
pub struct FleetModule;

impl FleetModule {
    /// The module with explicit options (configure it once, in the app
    /// root). Invalid options fail the boot.
    pub fn for_root(options: FleetOptions) -> impl Module {
        ConfiguredModule::<FleetModule, _>::for_root(options, register)
    }

    /// The module configured from the `fleet` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredModule::<FleetModule, FleetOptions>::from_config(register)
    }
}

impl Module for FleetModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

fn register(m: &mut ModuleBuilder, options: Option<FleetOptions>) {
    m.global();
    match options {
        Some(options) => {
            let options = Arc::new(options);
            m.factory(Factory::<FleetOptions>::new(move |_| {
                let options = options.clone();
                async move {
                    options
                        .validate()
                        .map_err(|message| format!("invalid fleet options: {message}"))?;
                    Ok(options)
                }
            }));
        }
        None => {
            m.config::<FleetOptions>();
        }
    }
    m.exports::<FleetOptions>()
        .factory(Factory::<JobRecorder>::new(|_| async {
            Ok(Arc::new(JobRecorder::new()))
        }))
        .exports::<JobRecorder>()
        .telemetry_sink_factory(
            Factory::<dyn TelemetrySink>::new(|r: Resolver| async move {
                Ok(r.get::<JobRecorder>().await? as Arc<dyn TelemetrySink>)
            })
            .deps(vec![ProviderKey::of::<JobRecorder>()]),
        )
        .factory(
            Factory::<Fleet>::new(|r: Resolver| async move {
                let options = r.get::<FleetOptions>().await?;
                let db = r.get::<Db>().await?;
                let queue = r.get::<Queue>().await?;
                let app = r.app();
                let clock = app.try_get::<dyn Clock>().await;
                let redactor = match app.try_get::<Config>().await {
                    Some(config) => Redactor::from_setting(
                        config
                            .section::<serde_json::Value>("log.redact")
                            .ok()
                            .as_ref(),
                    ),
                    None => Redactor::default(),
                };
                // `fleet.only` must name configured supervisors.
                options.supervisors(queue.options())?;
                Ok(Arc::new(Fleet::new(
                    FleetStore::new((*db).clone()),
                    queue,
                    options,
                    clock,
                    redactor,
                )))
            })
            .deps(vec![
                ProviderKey::of::<FleetOptions>(),
                ProviderKey::of::<Db>(),
                ProviderKey::of::<Queue>(),
            ]),
        )
        .exports::<Fleet>()
        .multi(
            Factory::<dyn MetricsCollector>::new(|r: Resolver| async move {
                Ok(Arc::new(FleetGauges::new(r.get::<Fleet>().await?))
                    as Arc<dyn MetricsCollector>)
            })
            .deps(vec![ProviderKey::of::<Fleet>()]),
        )
        .import(AccessModule::for_subject::<FleetSubject>())
        .controller::<FleetController>()
        .migrations(migrations());
    for command in commands() {
        m.command(command);
    }
}

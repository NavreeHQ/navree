//! [`WorkflowsOptions`]: the `workflows` configuration section.

use std::time::Duration;

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

/// Where workflow runs and their steps are stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStoreKind {
    /// The `workflow_runs` and `workflow_steps` tables (needs
    /// `DatabaseModule`).
    #[default]
    Database,
    /// In process, lost on restart (tests and examples).
    Memory,
}

/// The settings of `WorkflowsModule`: the `workflows` configuration
/// section. Every key is optional.
///
/// ```toml
/// [workflows]
/// store = "database"        # database | memory
/// lease_secs = 300          # how long one execution may hold a run
/// queue = "default"         # the queue of the `workflows.run` jobs
/// routes = true             # /api/workflows/runs (workflows.view, workflows.manage)
/// automations = true        # the automations half: event triggers and /api/automations
/// automation_routes = true  # /api/automations (automations.view, automations.manage)
/// prune_after_days = 30     # workflows:prune deletes finished runs older than this
/// ```
///
/// From the environment: `APP_WORKFLOWS__STORE=memory`,
/// `APP_WORKFLOWS__LEASE_SECS=120`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowsOptions {
    /// Where runs are stored (default `database`).
    pub store: WorkflowStoreKind,
    /// How long one execution may hold a run, in seconds (default 300).
    /// Every recorded step renews it. A run whose worker died is taken over
    /// by `workflows:tick` once its lease has run out, so keep single steps
    /// shorter than this.
    pub lease_secs: u64,
    /// The queue the `workflows.run` jobs go to (default `default`).
    pub queue: String,
    /// Mount `{api_prefix}/workflows/runs...` (default `true`). The routes
    /// require `workflows.view` (reads) and `workflows.manage` (cancel,
    /// retry), so the app needs a `dyn RouteAuthorizer` (navree-access);
    /// turn them off in apps without one.
    pub routes: bool,
    /// The automations half (default `true`): triggers enroll on events and
    /// the `automations` tables are used (needs `DatabaseModule`).
    pub automations: bool,
    /// Mount `{api_prefix}/automations...` (default `true`; needs
    /// `automations`). The routes require `automations.view` and
    /// `automations.manage`.
    pub automation_routes: bool,
    /// `workflows:prune` deletes runs that finished more than this many
    /// days ago (default 30; `0` keeps them).
    pub prune_after_days: u32,
}

impl Default for WorkflowsOptions {
    fn default() -> Self {
        Self {
            store: WorkflowStoreKind::Database,
            lease_secs: 300,
            queue: "default".to_owned(),
            routes: true,
            automations: true,
            automation_routes: true,
            prune_after_days: 30,
        }
    }
}

impl WorkflowsOptions {
    /// [`lease_secs`](WorkflowsOptions::lease_secs) as a duration.
    pub fn lease(&self) -> Duration {
        Duration::from_secs(self.lease_secs)
    }
}

impl ConfigSection for WorkflowsOptions {
    const KEY: &'static str = "workflows";

    fn validate(&self) -> Result<(), String> {
        if self.lease_secs == 0 {
            return Err("workflows.lease_secs must be at least 1".to_owned());
        }
        if self.queue.trim().is_empty() {
            return Err("workflows.queue must not be empty".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documentation() {
        let options = WorkflowsOptions::default();
        assert_eq!(options.store, WorkflowStoreKind::Database);
        assert_eq!(options.lease_secs, 300);
        assert_eq!(options.queue, "default");
        assert!(options.routes && options.automations && options.automation_routes);
        assert_eq!(options.prune_after_days, 30);
        assert_eq!(options.lease(), Duration::from_secs(300));
        assert!(options.validate().is_ok());
    }

    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let options: WorkflowsOptions =
            serde_json::from_value(serde_json::json!({"store": "memory", "lease_secs": 60}))
                .unwrap();
        assert_eq!(options.store, WorkflowStoreKind::Memory);
        assert_eq!(options.lease_secs, 60);
        assert!(options.routes);
        assert!(
            serde_json::from_value::<WorkflowsOptions>(serde_json::json!({"store": "redis"}))
                .is_err()
        );
    }

    #[test]
    fn validation_names_the_key() {
        let zero = WorkflowsOptions {
            lease_secs: 0,
            ..WorkflowsOptions::default()
        };
        assert!(zero.validate().unwrap_err().contains("workflows.lease_secs"));
        let blank = WorkflowsOptions {
            queue: " ".into(),
            ..WorkflowsOptions::default()
        };
        assert!(blank.validate().unwrap_err().contains("workflows.queue"));
    }
}

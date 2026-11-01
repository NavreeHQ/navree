//! [`Automations`]: create, edit, publish and run automations.

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_db::{Page, PageQuery, Uow};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::blocks::Catalog;
use super::graph::AutomationGraph;
use super::model::{
    Automation, AutomationQuery, AutomationStatus, AutomationTemplate, AutomationVersion,
    Enrollment, EnrollmentPolicy, EnrollmentQuery, EnrollmentRecord,
};
use super::runner::{self, SimulateInput, Simulation};
use crate::engine::Engine;
use crate::error::codes;
use crate::{DefinitionIssue, RunQuery, RunStatus, WorkflowRun, WorkflowsError};

/// The longest automation name.
pub const MAX_AUTOMATION_NAME_LEN: usize = 200;

/// What a new automation needs ([`Automations::create`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewAutomation {
    /// Its name.
    pub name: String,
    /// A note for people.
    #[serde(default)]
    pub description: Option<String>,
    /// The workspace it belongs to (`None`: app-wide). The routes set it to
    /// the request's workspace.
    #[serde(default)]
    pub workspace_id: Option<Uuid>,
    /// The definition (an [`AutomationGraph`] as JSON). It must have the
    /// right shape; what it names is checked when it is published.
    #[schema(value_type = Object)]
    pub definition: Value,
    /// Who may be enrolled again (default `one_at_a_time`).
    #[serde(default)]
    pub enrollment: EnrollmentPolicy,
}

impl NewAutomation {
    /// An automation named `name` with `definition`.
    pub fn new(name: impl Into<String>, definition: Value) -> Self {
        Self {
            name: name.into(),
            description: None,
            workspace_id: None,
            definition,
            enrollment: EnrollmentPolicy::default(),
        }
    }

    /// In `workspace`.
    pub fn workspace(mut self, workspace: Uuid) -> Self {
        self.workspace_id = Some(workspace);
        self
    }

    /// With a description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// With enrollment policy `policy`.
    pub fn enrollment(mut self, policy: EnrollmentPolicy) -> Self {
        self.enrollment = policy;
        self
    }
}

/// Changes to an automation's draft ([`Automations::update`]); every field
/// is optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct AutomationChanges {
    /// A new name.
    pub name: Option<String>,
    /// A new description (`""` removes it).
    pub description: Option<String>,
    /// A new draft definition.
    #[schema(value_type = Option<Object>)]
    pub definition: Option<Value>,
    /// A new enrollment policy (applies to new enrollments at once).
    pub enrollment: Option<EnrollmentPolicy>,
}

/// An automation with what stops its draft from being published (what
/// `GET /api/automations/{id}` returns).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AutomationDetails {
    /// The automation.
    #[serde(flatten)]
    pub automation: Automation,
    /// The draft's problems against the catalog (empty: it can be
    /// published).
    pub issues: Vec<DefinitionIssue>,
}

/// What `POST /api/automations/validate` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Validation {
    /// Whether the definition can be published.
    pub valid: bool,
    /// Its problems, each at a JSON pointer.
    pub issues: Vec<DefinitionIssue>,
}

/// The registered templates, checked against the catalog at boot.
#[derive(Debug, Default)]
pub(crate) struct TemplateRegistry {
    templates: BTreeMap<String, AutomationTemplate>,
}

impl TemplateRegistry {
    pub(crate) fn new(
        templates: impl IntoIterator<Item = AutomationTemplate>,
        catalog: &Catalog,
    ) -> Result<Self, String> {
        let mut registry = TemplateRegistry::default();
        for template in templates {
            let graph = AutomationGraph::from_json(&template.definition)
                .map_err(|issues| template_error(&template.key, &issues))?;
            let issues = graph.issues(catalog);
            if !issues.is_empty() {
                return Err(template_error(&template.key, &issues));
            }
            if let Some(other) = registry.templates.get(&template.key) {
                if *other != template {
                    return Err(format!(
                        "two automation templates have the key `{}`",
                        template.key
                    ));
                }
                continue;
            }
            registry.templates.insert(template.key.clone(), template);
        }
        Ok(registry)
    }

    pub(crate) fn all(&self) -> Vec<AutomationTemplate> {
        self.templates.values().cloned().collect()
    }

    pub(crate) fn get(&self, key: &str) -> Option<&AutomationTemplate> {
        self.templates.get(key)
    }
}

fn template_error(key: &str, issues: &[DefinitionIssue]) -> String {
    let details = issues
        .iter()
        .map(|issue| {
            let path = if issue.path.is_empty() {
                "/"
            } else {
                &issue.path
            };
            format!("{path}: {}", issue.message)
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("automation template `{key}` is invalid: {details}")
}

/// Automations: definitions people build from the registered triggers,
/// conditions and actions, published as versions and run as durable
/// workflows (one run per enrollment). Resolve it from the app:
/// `ctx.get::<Automations>()`.
#[derive(Clone)]
pub struct Automations {
    engine: Arc<Engine>,
}

impl std::fmt::Debug for Automations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Automations")
            .field("catalog", &self.engine.catalog)
            .finish_non_exhaustive()
    }
}

fn check_name(name: &str) -> Result<String, WorkflowsError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_AUTOMATION_NAME_LEN {
        return Err(WorkflowsError::InvalidDefinition(vec![
            DefinitionIssue::new(
                "/name",
                format!("name must be 1 to {MAX_AUTOMATION_NAME_LEN} characters"),
            ),
        ]));
    }
    Ok(name.to_owned())
}

fn parse(definition: &Value) -> Result<AutomationGraph, WorkflowsError> {
    AutomationGraph::from_json(definition).map_err(|issues| {
        WorkflowsError::InvalidDefinition(
            issues
                .into_iter()
                .map(|issue| {
                    DefinitionIssue::new(format!("/definition{}", issue.path), issue.message)
                })
                .collect(),
        )
    })
}

impl Automations {
    pub(crate) fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }

    /// Every registered trigger, condition and action.
    pub fn catalog(&self) -> &Catalog {
        &self.engine.catalog
    }

    /// Every registered template, by key.
    pub fn templates(&self) -> Vec<AutomationTemplate> {
        self.engine.templates.all()
    }

    /// The template `key`.
    pub fn template(&self, key: &str) -> Option<AutomationTemplate> {
        self.engine.templates.get(key).cloned()
    }

    /// Checks `definition` against the catalog: what would stop it from
    /// being published.
    pub fn validate(&self, definition: &Value) -> Validation {
        let issues = match AutomationGraph::from_json(definition) {
            Ok(graph) => graph.issues(&self.engine.catalog),
            Err(issues) => issues,
        };
        Validation {
            valid: issues.is_empty(),
            issues,
        }
    }

    /// Creates a draft automation. The definition must have the right
    /// shape (a 400 [`InvalidDefinition`](WorkflowsError::InvalidDefinition)
    /// otherwise); what it names is checked by [`publish`](Automations::publish).
    pub async fn create(
        &self,
        uow: &Uow,
        new: NewAutomation,
    ) -> Result<Automation, WorkflowsError> {
        self.create_in(Some(uow), new, None).await
    }

    async fn create_in(
        &self,
        uow: Option<&Uow>,
        new: NewAutomation,
        template: Option<String>,
    ) -> Result<Automation, WorkflowsError> {
        let name = check_name(&new.name)?;
        let definition = parse(&new.definition)?;
        let now = self.engine.now();
        let automation = Automation {
            id: Uuid::now_v7(),
            workspace_id: new.workspace_id,
            name,
            description: new.description.filter(|d| !d.trim().is_empty()),
            status: AutomationStatus::Draft,
            definition,
            enrollment: new.enrollment,
            version: None,
            trigger_event: None,
            template,
            created_at: now,
            updated_at: now,
        };
        self.engine.automations.insert(uow, &automation).await?;
        Ok(automation)
    }

    /// Creates a draft automation from template `key` (named after the
    /// template unless `name` is given).
    pub async fn create_from_template(
        &self,
        uow: &Uow,
        key: &str,
        workspace_id: Option<Uuid>,
        name: Option<String>,
    ) -> Result<Automation, WorkflowsError> {
        let template = self
            .engine
            .templates
            .get(key)
            .ok_or_else(|| WorkflowsError::UnknownTemplate(key.to_owned()))?;
        let new = NewAutomation {
            name: name.unwrap_or_else(|| template.name.clone()),
            description: Some(template.description.clone()),
            workspace_id,
            definition: template.definition.clone(),
            enrollment: template.enrollment,
        };
        self.create_in(Some(uow), new, Some(template.key.clone()))
            .await
    }

    /// The automation `id`.
    pub async fn get(&self, id: Uuid) -> Result<Automation, WorkflowsError> {
        self.load(None, id).await
    }

    /// The automation `id` as `uow` sees it (its own writes included).
    async fn load(&self, uow: Option<&Uow>, id: Uuid) -> Result<Automation, WorkflowsError> {
        self.engine
            .automations
            .get(uow, id)
            .await?
            .ok_or(WorkflowsError::UnknownAutomation(id))
    }

    /// The automation `id` with its draft's problems.
    pub async fn details(&self, id: Uuid) -> Result<AutomationDetails, WorkflowsError> {
        let automation = self.get(id).await?;
        let issues = automation.definition.issues(&self.engine.catalog);
        Ok(AutomationDetails { automation, issues })
    }

    /// Automations passing `query`, newest first.
    pub async fn list(&self, query: &AutomationQuery) -> Result<Vec<Automation>, WorkflowsError> {
        self.engine.automations.list(query).await
    }

    /// One page of the automations of `scope` (a workspace, or `None` for
    /// app-wide ones), newest first.
    pub async fn page(
        &self,
        scope: Option<Uuid>,
        page: PageQuery,
    ) -> Result<Page<Automation>, WorkflowsError> {
        let query = AutomationQuery {
            scope: Some(scope),
            status: None,
            limit: page.per_page,
            offset: page.offset(),
        };
        let total = self.engine.automations.count(&query).await?;
        let data = self.engine.automations.list(&query).await?;
        Ok(Page::new(data, page, total))
    }

    /// Changes the draft (and the enrollment policy, at once). The
    /// published version keeps enrolling until the next
    /// [`publish`](Automations::publish).
    pub async fn update(
        &self,
        uow: &Uow,
        id: Uuid,
        changes: AutomationChanges,
    ) -> Result<Automation, WorkflowsError> {
        let mut automation = self.load(Some(uow), id).await?;
        if let Some(name) = changes.name {
            automation.name = check_name(&name)?;
        }
        if let Some(description) = changes.description {
            automation.description = Some(description).filter(|d| !d.trim().is_empty());
        }
        if let Some(definition) = changes.definition {
            automation.definition = parse(&definition)?;
        }
        if let Some(policy) = changes.enrollment {
            automation.enrollment = policy;
        }
        automation.updated_at = self.engine.now();
        self.save(uow, &automation).await?;
        Ok(automation)
    }

    async fn save(&self, uow: &Uow, automation: &Automation) -> Result<(), WorkflowsError> {
        if !self
            .engine
            .automations
            .update(Some(uow), automation)
            .await?
        {
            return Err(WorkflowsError::UnknownAutomation(automation.id));
        }
        Ok(())
    }

    /// Publishes the draft as the next version and activates the
    /// automation: from now on matching events enroll in that version.
    /// Enrollments under way keep their version. A draft with problems is
    /// a 400 listing them.
    pub async fn publish(
        &self,
        uow: &Uow,
        id: Uuid,
        published_by: Option<Uuid>,
    ) -> Result<Automation, WorkflowsError> {
        let mut automation = self.load(Some(uow), id).await?;
        let issues = automation
            .definition
            .issues(&self.engine.catalog)
            .into_iter()
            .map(|issue| DefinitionIssue::new(format!("/definition{}", issue.path), issue.message))
            .collect::<Vec<_>>();
        if !issues.is_empty() {
            return Err(WorkflowsError::InvalidDefinition(issues));
        }
        let trigger = self
            .engine
            .catalog
            .trigger(&automation.definition.trigger.block_type)
            .ok_or_else(|| WorkflowsError::Other("the trigger disappeared".to_owned()))?;
        let latest = self
            .engine
            .automations
            .versions(Some(uow), id)
            .await?
            .first()
            .map_or(0, |v| v.version);
        let now = self.engine.now();
        let version = AutomationVersion {
            automation_id: id,
            version: latest + 1,
            definition: automation.definition.clone(),
            published_by,
            created_at: now,
        };
        self.engine
            .automations
            .insert_version(Some(uow), &version)
            .await?;
        automation.version = Some(version.version);
        automation.trigger_event = Some(trigger.event.to_owned());
        automation.status = AutomationStatus::Active;
        automation.updated_at = now;
        self.save(uow, &automation).await?;
        Ok(automation)
    }

    /// Stops enrolling (enrollments under way go on); 409 unless active.
    pub async fn pause(&self, uow: &Uow, id: Uuid) -> Result<Automation, WorkflowsError> {
        let mut automation = self.load(Some(uow), id).await?;
        if automation.status != AutomationStatus::Active {
            return Err(WorkflowsError::conflict(
                codes::AUTOMATION_NOT_ACTIVE,
                format!(
                    "Only an active automation can be paused (this one is {}).",
                    automation.status
                ),
            ));
        }
        automation.status = AutomationStatus::Paused;
        automation.updated_at = self.engine.now();
        self.save(uow, &automation).await?;
        Ok(automation)
    }

    /// Enrolls again with the published version; 409 unless paused.
    pub async fn resume(&self, uow: &Uow, id: Uuid) -> Result<Automation, WorkflowsError> {
        let mut automation = self.load(Some(uow), id).await?;
        if automation.status != AutomationStatus::Paused {
            return Err(WorkflowsError::conflict(
                codes::AUTOMATION_NOT_PAUSED,
                format!(
                    "Only a paused automation can be resumed (this one is {}).",
                    automation.status
                ),
            ));
        }
        automation.status = AutomationStatus::Active;
        automation.updated_at = self.engine.now();
        self.save(uow, &automation).await?;
        Ok(automation)
    }

    /// Deletes the automation with its versions and enrollments, and
    /// cancels the enrollments under way.
    pub async fn delete(&self, uow: &Uow, id: Uuid) -> Result<(), WorkflowsError> {
        self.load(Some(uow), id).await?;
        let mut offset = 0;
        loop {
            let enrollments = self
                .engine
                .automations
                .enrollments(&EnrollmentQuery {
                    automation_id: Some(id),
                    subject: None,
                    limit: 100,
                    offset,
                })
                .await?;
            if enrollments.is_empty() {
                break;
            }
            offset += enrollments.len() as u64;
            let now = self.engine.now();
            for enrollment in enrollments {
                self.engine
                    .store
                    .cancel(Some(uow), enrollment.run_id, now)
                    .await?;
            }
        }
        if !self.engine.automations.delete(Some(uow), id).await? {
            return Err(WorkflowsError::UnknownAutomation(id));
        }
        Ok(())
    }

    /// Every published version, newest first.
    pub async fn versions(&self, id: Uuid) -> Result<Vec<AutomationVersion>, WorkflowsError> {
        self.get(id).await?;
        self.engine.automations.versions(None, id).await
    }

    /// One page of the automation's enrollments, newest first, with the
    /// status of their runs.
    pub async fn enrollments(
        &self,
        id: Uuid,
        page: PageQuery,
    ) -> Result<Page<Enrollment>, WorkflowsError> {
        self.get(id).await?;
        let query = EnrollmentQuery {
            automation_id: Some(id),
            subject: None,
            limit: page.per_page,
            offset: page.offset(),
        };
        let total = self.engine.automations.count_enrollments(&query).await?;
        let records = self.engine.automations.enrollments(&query).await?;
        let data = self.with_status(records).await?;
        Ok(Page::new(data, page, total))
    }

    /// The enrollment `id`.
    pub async fn enrollment(&self, id: Uuid) -> Result<Enrollment, WorkflowsError> {
        let record = self
            .engine
            .automations
            .enrollment(id)
            .await?
            .ok_or(WorkflowsError::UnknownEnrollment(id))?;
        Ok(self.with_status(vec![record]).await?.remove(0))
    }

    /// Cancels an enrollment under way (its run); 409 when it finished.
    pub async fn cancel_enrollment(&self, id: Uuid) -> Result<Enrollment, WorkflowsError> {
        let enrollment = self.enrollment(id).await?;
        if !self
            .engine
            .store
            .cancel(None, enrollment.run_id, self.engine.now())
            .await?
        {
            return Err(WorkflowsError::conflict(
                codes::ENROLLMENT_FINISHED,
                format!(
                    "The enrollment has already finished ({}).",
                    enrollment.status
                ),
            ));
        }
        self.enrollment(id).await
    }

    /// Enrolls `event` in the automation's published version now, whatever
    /// its status, trigger settings and enrollment policy (a manual "run
    /// it for this record"); 409 when nothing was ever published.
    pub async fn enroll(&self, id: Uuid, event: Value) -> Result<Enrollment, WorkflowsError> {
        let automation = self.get(id).await?;
        let Some(version) = automation.version else {
            return Err(WorkflowsError::conflict(
                codes::AUTOMATION_NOT_PUBLISHED,
                "Publish the automation before enrolling.",
            ));
        };
        let published = self
            .engine
            .automations
            .version(id, version)
            .await?
            .ok_or_else(|| WorkflowsError::Other(format!("version {version} is missing")))?;
        let entry = self
            .engine
            .catalog
            .trigger(&published.definition.trigger.block_type)
            .ok_or_else(|| WorkflowsError::Other("the trigger is not registered".to_owned()))?;
        (entry.check_event)(&event).map_err(|error| {
            WorkflowsError::invalid(
                codes::INVALID_AUTOMATION_EVENT,
                format!("The event is not a `{}` event: {error}", entry.event),
            )
        })?;
        let subject = entry.subject_of(&event);
        let record =
            runner::start_enrollment(&self.engine, &automation, version, subject, event).await?;
        self.enrollment(record.id).await
    }

    /// Simulates an enrollment of `input.event` in the draft (or a
    /// published version): whether it would enroll and which nodes it would
    /// reach. Conditions are evaluated; nothing is stored and no action
    /// runs.
    pub async fn simulate(
        &self,
        id: Uuid,
        input: SimulateInput,
    ) -> Result<Simulation, WorkflowsError> {
        let automation = self.get(id).await?;
        let graph = match input.version {
            Some(version) => {
                self.engine
                    .automations
                    .version(id, version)
                    .await?
                    .ok_or_else(|| {
                        WorkflowsError::invalid(
                            codes::UNKNOWN_AUTOMATION_VERSION,
                            format!("The automation has no version {version}."),
                        )
                    })?
                    .definition
            }
            None => automation.definition.clone(),
        };
        runner::simulate(
            &self.engine,
            &automation,
            &graph,
            input.version,
            input.event,
            input.waits,
        )
        .await
    }

    /// `records` with the status of their runs.
    async fn with_status(
        &self,
        records: Vec<EnrollmentRecord>,
    ) -> Result<Vec<Enrollment>, WorkflowsError> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let query = RunQuery {
            ids: records.iter().map(|r| r.run_id).collect(),
            limit: records.len() as u64,
            ..RunQuery::default()
        };
        let runs: BTreeMap<Uuid, WorkflowRun> = self
            .engine
            .store
            .runs(&query)
            .await?
            .into_iter()
            .map(|run| (run.id, run))
            .collect();
        Ok(records
            .into_iter()
            .map(|record| {
                let run = runs.get(&record.run_id);
                Enrollment {
                    id: record.id,
                    automation_id: record.automation_id,
                    version: record.version,
                    workspace_id: record.workspace_id,
                    subject: record.subject,
                    run_id: record.run_id,
                    // A pruned run finished long ago.
                    status: run.map_or(RunStatus::Completed, |run| run.status),
                    error: run.and_then(|run| run.error.clone()),
                    created_at: record.created_at,
                    finished_at: run.and_then(|run| run.finished_at),
                }
            })
            .collect())
    }
}

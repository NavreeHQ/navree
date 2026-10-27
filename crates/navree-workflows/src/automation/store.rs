//! [`AutomationStore`]: where automations, their versions and enrollments
//! live, with the database and memory drivers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_db::{Db, Uow};
use navree_kernel::BoxFuture;
use sea_orm::{ConnectionTrait, QueryResult, Statement, Value as SqlValue};
use uuid::Uuid;

use super::graph::AutomationGraph;
use super::model::{
    Automation, AutomationQuery, AutomationStatus, AutomationVersion, EnrollmentQuery,
    EnrollmentRecord,
};
use crate::WorkflowsError;
use crate::migration::{AUTOMATIONS_TABLE, ENROLLMENTS_TABLE, VERSIONS_TABLE};
use crate::store::{StoreResult, from_millis, statement};

/// Where automations, their published versions and their enrollments are
/// stored. Two drivers ship: [`DatabaseAutomationStore`] (`automations`,
/// `automation_versions`, `automation_enrollments`) and
/// [`MemoryAutomationStore`]; both pass the same conformance suite. Writes
/// take `Option<&Uow>` like [`WorkflowStore`](crate::WorkflowStore)'s.
pub trait AutomationStore: Send + Sync + 'static {
    /// Stores a new automation.
    fn insert<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        automation: &'a Automation,
    ) -> BoxFuture<'a, StoreResult<()>>;

    /// Replaces automation `automation.id`; `false` when it is unknown.
    fn update<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        automation: &'a Automation,
    ) -> BoxFuture<'a, StoreResult<bool>>;

    /// Deletes an automation with its versions and enrollments; `false`
    /// when it is unknown.
    fn delete<'a>(&'a self, uow: Option<&'a Uow>, id: Uuid) -> BoxFuture<'a, StoreResult<bool>>;

    /// The automation `id`.
    fn get(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<Automation>>>;

    /// Automations passing `query`, newest first.
    fn list<'a>(&'a self, query: &'a AutomationQuery)
    -> BoxFuture<'a, StoreResult<Vec<Automation>>>;

    /// How many automations pass `query` (its limit and offset aside).
    fn count<'a>(&'a self, query: &'a AutomationQuery) -> BoxFuture<'a, StoreResult<u64>>;

    /// The active automations whose published trigger listens to `event`.
    fn active_for_event<'a>(&'a self, event: &'a str)
    -> BoxFuture<'a, StoreResult<Vec<Automation>>>;

    /// Stores a published version.
    fn insert_version<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        version: &'a AutomationVersion,
    ) -> BoxFuture<'a, StoreResult<()>>;

    /// Version `version` of automation `id`.
    fn version(&self, id: Uuid, version: u32)
    -> BoxFuture<'_, StoreResult<Option<AutomationVersion>>>;

    /// Every version of automation `id`, newest first.
    fn versions(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Vec<AutomationVersion>>>;

    /// Stores an enrollment.
    fn insert_enrollment<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        enrollment: &'a EnrollmentRecord,
    ) -> BoxFuture<'a, StoreResult<()>>;

    /// The enrollment `id`.
    fn enrollment(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<EnrollmentRecord>>>;

    /// Enrollments passing `query`, newest first.
    fn enrollments<'a>(
        &'a self,
        query: &'a EnrollmentQuery,
    ) -> BoxFuture<'a, StoreResult<Vec<EnrollmentRecord>>>;

    /// How many enrollments pass `query` (its limit and offset aside).
    fn count_enrollments<'a>(&'a self, query: &'a EnrollmentQuery)
    -> BoxFuture<'a, StoreResult<u64>>;
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MemoryState {
    automations: HashMap<Uuid, Automation>,
    versions: Vec<AutomationVersion>,
    enrollments: Vec<EnrollmentRecord>,
}

/// Automations in process memory (`workflows.store = "memory"`): lost on
/// restart. Writes given a unit of work apply once it commits.
#[derive(Clone, Default)]
pub struct MemoryAutomationStore {
    state: Arc<Mutex<MemoryState>>,
}

impl MemoryAutomationStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, MemoryState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self, uow: Option<&Uow>, apply: impl FnOnce(&mut MemoryState) + Send + 'static) {
        match uow {
            Some(uow) => {
                let store = self.clone();
                uow.after_commit(move || {
                    Box::pin(async move {
                        apply(&mut store.lock());
                    })
                });
            }
            None => apply(&mut self.lock()),
        }
    }
}

impl std::fmt::Debug for MemoryAutomationStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryAutomationStore")
            .field("automations", &self.lock().automations.len())
            .finish()
    }
}

fn page<T>(items: Vec<T>, offset: u64, limit: u64) -> Vec<T> {
    let offset = usize::try_from(offset).unwrap_or(usize::MAX);
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    items.into_iter().skip(offset).take(limit).collect()
}

fn ready<T: Send + 'static>(value: T) -> BoxFuture<'static, StoreResult<T>> {
    Box::pin(async move { Ok(value) })
}

impl AutomationStore for MemoryAutomationStore {
    fn insert<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        automation: &'a Automation,
    ) -> BoxFuture<'a, StoreResult<()>> {
        let automation = automation.clone();
        self.write(uow, move |state| {
            state.automations.insert(automation.id, automation);
        });
        ready(())
    }

    fn update<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        automation: &'a Automation,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        let known = self.lock().automations.contains_key(&automation.id);
        if known {
            let automation = automation.clone();
            self.write(uow, move |state| {
                if let Some(stored) = state.automations.get_mut(&automation.id) {
                    *stored = automation;
                }
            });
        }
        ready(known)
    }

    fn delete<'a>(&'a self, uow: Option<&'a Uow>, id: Uuid) -> BoxFuture<'a, StoreResult<bool>> {
        let known = self.lock().automations.contains_key(&id);
        if known {
            self.write(uow, move |state| {
                state.automations.remove(&id);
                state.versions.retain(|v| v.automation_id != id);
                state.enrollments.retain(|e| e.automation_id != id);
            });
        }
        ready(known)
    }

    fn get(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<Automation>>> {
        ready(self.lock().automations.get(&id).cloned())
    }

    fn list<'a>(
        &'a self,
        query: &'a AutomationQuery,
    ) -> BoxFuture<'a, StoreResult<Vec<Automation>>> {
        let mut automations: Vec<Automation> = self
            .lock()
            .automations
            .values()
            .filter(|a| query.matches(a))
            .cloned()
            .collect();
        automations.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        ready(page(automations, query.offset, query.effective_limit()))
    }

    fn count<'a>(&'a self, query: &'a AutomationQuery) -> BoxFuture<'a, StoreResult<u64>> {
        let count = self
            .lock()
            .automations
            .values()
            .filter(|a| query.matches(a))
            .count();
        ready(u64::try_from(count).unwrap_or(u64::MAX))
    }

    fn active_for_event<'a>(
        &'a self,
        event: &'a str,
    ) -> BoxFuture<'a, StoreResult<Vec<Automation>>> {
        let mut automations: Vec<Automation> = self
            .lock()
            .automations
            .values()
            .filter(|a| {
                a.status == AutomationStatus::Active && a.trigger_event.as_deref() == Some(event)
            })
            .cloned()
            .collect();
        automations.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        ready(automations)
    }

    fn insert_version<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        version: &'a AutomationVersion,
    ) -> BoxFuture<'a, StoreResult<()>> {
        let version = version.clone();
        self.write(uow, move |state| state.versions.push(version));
        ready(())
    }

    fn version(
        &self,
        id: Uuid,
        version: u32,
    ) -> BoxFuture<'_, StoreResult<Option<AutomationVersion>>> {
        ready(
            self.lock()
                .versions
                .iter()
                .find(|v| v.automation_id == id && v.version == version)
                .cloned(),
        )
    }

    fn versions(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Vec<AutomationVersion>>> {
        let mut versions: Vec<AutomationVersion> = self
            .lock()
            .versions
            .iter()
            .filter(|v| v.automation_id == id)
            .cloned()
            .collect();
        versions.sort_by(|a, b| b.version.cmp(&a.version));
        ready(versions)
    }

    fn insert_enrollment<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        enrollment: &'a EnrollmentRecord,
    ) -> BoxFuture<'a, StoreResult<()>> {
        let enrollment = enrollment.clone();
        self.write(uow, move |state| state.enrollments.push(enrollment));
        ready(())
    }

    fn enrollment(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<EnrollmentRecord>>> {
        ready(self.lock().enrollments.iter().find(|e| e.id == id).cloned())
    }

    fn enrollments<'a>(
        &'a self,
        query: &'a EnrollmentQuery,
    ) -> BoxFuture<'a, StoreResult<Vec<EnrollmentRecord>>> {
        let mut enrollments: Vec<EnrollmentRecord> = self
            .lock()
            .enrollments
            .iter()
            .filter(|e| query.matches(e))
            .cloned()
            .collect();
        enrollments.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        ready(page(enrollments, query.offset, query.effective_limit()))
    }

    fn count_enrollments<'a>(
        &'a self,
        query: &'a EnrollmentQuery,
    ) -> BoxFuture<'a, StoreResult<u64>> {
        let count = self
            .lock()
            .enrollments
            .iter()
            .filter(|e| query.matches(e))
            .count();
        ready(u64::try_from(count).unwrap_or(u64::MAX))
    }
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

const AUTOMATION_COLUMNS: &str = "id, workspace_id, name, description, status, definition, \
     enrollment, version, trigger_event, template, created_at, updated_at";

const VERSION_COLUMNS: &str = "automation_id, version, definition, published_by, created_at";

const ENROLLMENT_COLUMNS: &str =
    "id, automation_id, version, workspace_id, subject, run_id, created_at";

/// Automations in the app database (`workflows.store = "database"`, the
/// default): tables `automations`, `automation_versions` and
/// `automation_enrollments` from the `workflows` migration set.
#[derive(Clone, Debug)]
pub struct DatabaseAutomationStore {
    db: Db,
}

impl DatabaseAutomationStore {
    /// The store on `db`.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn stmt(&self, sql: &str, values: Vec<SqlValue>) -> Statement {
        statement(self.db.backend(), sql, values)
    }

    async fn execute_in(&self, uow: Option<&Uow>, stmt: Statement) -> StoreResult<u64> {
        let result = match uow {
            Some(uow) => {
                let tx = uow.tx().await?;
                tx.execute_raw(stmt).await?
            }
            None => self.db.conn().execute_raw(stmt).await?,
        };
        Ok(result.rows_affected())
    }

    async fn all(&self, sql: &str, values: Vec<SqlValue>) -> StoreResult<Vec<QueryResult>> {
        Ok(self.db.conn().query_all_raw(self.stmt(sql, values)).await?)
    }

    async fn count_of(&self, sql: &str, values: Vec<SqlValue>) -> StoreResult<u64> {
        let row = self.db.conn().query_one_raw(self.stmt(sql, values)).await?;
        let total: i64 = match row {
            Some(row) => row.try_get("", "total")?,
            None => 0,
        };
        Ok(u64::try_from(total).unwrap_or_default())
    }
}

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn uuid_text(id: Option<Uuid>) -> SqlValue {
    id.map(|id| id.to_string()).into()
}

fn parse_uuid(text: &str) -> StoreResult<Uuid> {
    Uuid::parse_str(text)
        .map_err(|_| WorkflowsError::Other(format!("stored id `{text}` is not a UUID")))
}

fn opt_uuid(row: &QueryResult, column: &str) -> StoreResult<Option<Uuid>> {
    row.try_get::<Option<String>>("", column)?
        .map(|text| parse_uuid(&text))
        .transpose()
}

fn graph(text: &str) -> StoreResult<AutomationGraph> {
    serde_json::from_str(text)
        .map_err(|error| WorkflowsError::payload("a stored automation definition cannot be read", error))
}

fn graph_text(graph: &AutomationGraph) -> StoreResult<String> {
    serde_json::to_string(graph)
        .map_err(|error| WorkflowsError::payload("an automation definition cannot be stored", error))
}

fn u32_column(row: &QueryResult, column: &str) -> StoreResult<u32> {
    let value: i64 = row.try_get("", column)?;
    Ok(u32::try_from(value).unwrap_or(u32::MAX))
}

fn automation_row(row: &QueryResult) -> StoreResult<Automation> {
    let status: String = row.try_get("", "status")?;
    let enrollment: String = row.try_get("", "enrollment")?;
    Ok(Automation {
        id: parse_uuid(&row.try_get::<String>("", "id")?)?,
        workspace_id: opt_uuid(row, "workspace_id")?,
        name: row.try_get("", "name")?,
        description: row.try_get("", "description")?,
        status: status.parse().map_err(WorkflowsError::Other)?,
        definition: graph(&row.try_get::<String>("", "definition")?)?,
        enrollment: enrollment.parse().map_err(WorkflowsError::Other)?,
        version: row
            .try_get::<Option<i64>>("", "version")?
            .map(|v| u32::try_from(v).unwrap_or(u32::MAX)),
        trigger_event: row.try_get("", "trigger_event")?,
        template: row.try_get("", "template")?,
        created_at: from_millis(row.try_get("", "created_at")?),
        updated_at: from_millis(row.try_get("", "updated_at")?),
    })
}

fn version_row(row: &QueryResult) -> StoreResult<AutomationVersion> {
    Ok(AutomationVersion {
        automation_id: parse_uuid(&row.try_get::<String>("", "automation_id")?)?,
        version: u32_column(row, "version")?,
        definition: graph(&row.try_get::<String>("", "definition")?)?,
        published_by: opt_uuid(row, "published_by")?,
        created_at: from_millis(row.try_get("", "created_at")?),
    })
}

fn enrollment_row(row: &QueryResult) -> StoreResult<EnrollmentRecord> {
    Ok(EnrollmentRecord {
        id: parse_uuid(&row.try_get::<String>("", "id")?)?,
        automation_id: parse_uuid(&row.try_get::<String>("", "automation_id")?)?,
        version: u32_column(row, "version")?,
        workspace_id: opt_uuid(row, "workspace_id")?,
        subject: row.try_get("", "subject")?,
        run_id: parse_uuid(&row.try_get::<String>("", "run_id")?)?,
        created_at: from_millis(row.try_get("", "created_at")?),
    })
}

fn automation_filter(query: &AutomationQuery) -> (String, Vec<SqlValue>) {
    let mut clauses = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();
    match query.scope {
        None => {}
        Some(None) => clauses.push("workspace_id IS NULL".to_owned()),
        Some(Some(workspace)) => {
            clauses.push("workspace_id = ?".to_owned());
            values.push(workspace.to_string().into());
        }
    }
    if let Some(status) = query.status {
        clauses.push("status = ?".to_owned());
        values.push(status.as_str().into());
    }
    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (clause, values)
}

fn enrollment_filter(query: &EnrollmentQuery) -> (String, Vec<SqlValue>) {
    let mut clauses = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();
    if let Some(id) = query.automation_id {
        clauses.push("automation_id = ?".to_owned());
        values.push(id.to_string().into());
    }
    if let Some(subject) = &query.subject {
        clauses.push("subject = ?".to_owned());
        values.push(subject.clone().into());
    }
    let clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (clause, values)
}

impl AutomationStore for DatabaseAutomationStore {
    fn insert<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        automation: &'a Automation,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let stmt = self.stmt(
                &format!(
                    "INSERT INTO {AUTOMATIONS_TABLE} ({AUTOMATION_COLUMNS}) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                ),
                vec![
                    automation.id.to_string().into(),
                    uuid_text(automation.workspace_id),
                    automation.name.clone().into(),
                    automation.description.clone().into(),
                    automation.status.as_str().into(),
                    graph_text(&automation.definition)?.into(),
                    automation.enrollment.as_str().into(),
                    automation.version.map(i64::from).into(),
                    automation.trigger_event.clone().into(),
                    automation.template.clone().into(),
                    automation.created_at.timestamp_millis().into(),
                    automation.updated_at.timestamp_millis().into(),
                ],
            );
            self.execute_in(uow, stmt).await?;
            Ok(())
        })
    }

    fn update<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        automation: &'a Automation,
    ) -> BoxFuture<'a, StoreResult<bool>> {
        Box::pin(async move {
            let stmt = self.stmt(
                &format!(
                    "UPDATE {AUTOMATIONS_TABLE} SET workspace_id = ?, name = ?, description = ?, \
                     status = ?, definition = ?, enrollment = ?, version = ?, \
                     trigger_event = ?, template = ?, updated_at = ? WHERE id = ?"
                ),
                vec![
                    uuid_text(automation.workspace_id),
                    automation.name.clone().into(),
                    automation.description.clone().into(),
                    automation.status.as_str().into(),
                    graph_text(&automation.definition)?.into(),
                    automation.enrollment.as_str().into(),
                    automation.version.map(i64::from).into(),
                    automation.trigger_event.clone().into(),
                    automation.template.clone().into(),
                    automation.updated_at.timestamp_millis().into(),
                    automation.id.to_string().into(),
                ],
            );
            Ok(self.execute_in(uow, stmt).await? == 1)
        })
    }

    fn delete<'a>(&'a self, uow: Option<&'a Uow>, id: Uuid) -> BoxFuture<'a, StoreResult<bool>> {
        Box::pin(async move {
            for table in [ENROLLMENTS_TABLE, VERSIONS_TABLE] {
                self.execute_in(
                    uow,
                    self.stmt(
                        &format!("DELETE FROM {table} WHERE automation_id = ?"),
                        vec![id.to_string().into()],
                    ),
                )
                .await?;
            }
            let deleted = self
                .execute_in(
                    uow,
                    self.stmt(
                        &format!("DELETE FROM {AUTOMATIONS_TABLE} WHERE id = ?"),
                        vec![id.to_string().into()],
                    ),
                )
                .await?;
            Ok(deleted == 1)
        })
    }

    fn get(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<Automation>>> {
        Box::pin(async move {
            let rows = self
                .all(
                    &format!("SELECT {AUTOMATION_COLUMNS} FROM {AUTOMATIONS_TABLE} WHERE id = ?"),
                    vec![id.to_string().into()],
                )
                .await?;
            rows.first().map(automation_row).transpose()
        })
    }

    fn list<'a>(
        &'a self,
        query: &'a AutomationQuery,
    ) -> BoxFuture<'a, StoreResult<Vec<Automation>>> {
        Box::pin(async move {
            let (clause, mut values) = automation_filter(query);
            values.push(to_i64(query.effective_limit()).into());
            values.push(to_i64(query.offset).into());
            let rows = self
                .all(
                    &format!(
                        "SELECT {AUTOMATION_COLUMNS} FROM {AUTOMATIONS_TABLE}{clause} \
                         ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
                    ),
                    values,
                )
                .await?;
            rows.iter().map(automation_row).collect()
        })
    }

    fn count<'a>(&'a self, query: &'a AutomationQuery) -> BoxFuture<'a, StoreResult<u64>> {
        Box::pin(async move {
            let (clause, values) = automation_filter(query);
            self.count_of(
                &format!("SELECT COUNT(*) AS total FROM {AUTOMATIONS_TABLE}{clause}"),
                values,
            )
            .await
        })
    }

    fn active_for_event<'a>(
        &'a self,
        event: &'a str,
    ) -> BoxFuture<'a, StoreResult<Vec<Automation>>> {
        Box::pin(async move {
            let rows = self
                .all(
                    &format!(
                        "SELECT {AUTOMATION_COLUMNS} FROM {AUTOMATIONS_TABLE} \
                         WHERE status = 'active' AND trigger_event = ? ORDER BY created_at, id"
                    ),
                    vec![event.into()],
                )
                .await?;
            rows.iter().map(automation_row).collect()
        })
    }

    fn insert_version<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        version: &'a AutomationVersion,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let stmt = self.stmt(
                &format!(
                    "INSERT INTO {VERSIONS_TABLE} ({VERSION_COLUMNS}) VALUES (?, ?, ?, ?, ?)"
                ),
                vec![
                    version.automation_id.to_string().into(),
                    i64::from(version.version).into(),
                    graph_text(&version.definition)?.into(),
                    uuid_text(version.published_by),
                    version.created_at.timestamp_millis().into(),
                ],
            );
            self.execute_in(uow, stmt).await?;
            Ok(())
        })
    }

    fn version(
        &self,
        id: Uuid,
        version: u32,
    ) -> BoxFuture<'_, StoreResult<Option<AutomationVersion>>> {
        Box::pin(async move {
            let rows = self
                .all(
                    &format!(
                        "SELECT {VERSION_COLUMNS} FROM {VERSIONS_TABLE} \
                         WHERE automation_id = ? AND version = ?"
                    ),
                    vec![id.to_string().into(), i64::from(version).into()],
                )
                .await?;
            rows.first().map(version_row).transpose()
        })
    }

    fn versions(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Vec<AutomationVersion>>> {
        Box::pin(async move {
            let rows = self
                .all(
                    &format!(
                        "SELECT {VERSION_COLUMNS} FROM {VERSIONS_TABLE} \
                         WHERE automation_id = ? ORDER BY version DESC"
                    ),
                    vec![id.to_string().into()],
                )
                .await?;
            rows.iter().map(version_row).collect()
        })
    }

    fn insert_enrollment<'a>(
        &'a self,
        uow: Option<&'a Uow>,
        enrollment: &'a EnrollmentRecord,
    ) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let stmt = self.stmt(
                &format!(
                    "INSERT INTO {ENROLLMENTS_TABLE} ({ENROLLMENT_COLUMNS}) \
                     VALUES (?, ?, ?, ?, ?, ?, ?)"
                ),
                vec![
                    enrollment.id.to_string().into(),
                    enrollment.automation_id.to_string().into(),
                    i64::from(enrollment.version).into(),
                    uuid_text(enrollment.workspace_id),
                    enrollment.subject.clone().into(),
                    enrollment.run_id.to_string().into(),
                    enrollment.created_at.timestamp_millis().into(),
                ],
            );
            self.execute_in(uow, stmt).await?;
            Ok(())
        })
    }

    fn enrollment(&self, id: Uuid) -> BoxFuture<'_, StoreResult<Option<EnrollmentRecord>>> {
        Box::pin(async move {
            let rows = self
                .all(
                    &format!("SELECT {ENROLLMENT_COLUMNS} FROM {ENROLLMENTS_TABLE} WHERE id = ?"),
                    vec![id.to_string().into()],
                )
                .await?;
            rows.first().map(enrollment_row).transpose()
        })
    }

    fn enrollments<'a>(
        &'a self,
        query: &'a EnrollmentQuery,
    ) -> BoxFuture<'a, StoreResult<Vec<EnrollmentRecord>>> {
        Box::pin(async move {
            let (clause, mut values) = enrollment_filter(query);
            values.push(to_i64(query.effective_limit()).into());
            values.push(to_i64(query.offset).into());
            let rows = self
                .all(
                    &format!(
                        "SELECT {ENROLLMENT_COLUMNS} FROM {ENROLLMENTS_TABLE}{clause} \
                         ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
                    ),
                    values,
                )
                .await?;
            rows.iter().map(enrollment_row).collect()
        })
    }

    fn count_enrollments<'a>(
        &'a self,
        query: &'a EnrollmentQuery,
    ) -> BoxFuture<'a, StoreResult<u64>> {
        Box::pin(async move {
            let (clause, values) = enrollment_filter(query);
            self.count_of(
                &format!("SELECT COUNT(*) AS total FROM {ENROLLMENTS_TABLE}{clause}"),
                values,
            )
            .await
        })
    }
}

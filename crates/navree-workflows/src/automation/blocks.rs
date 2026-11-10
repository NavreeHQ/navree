//! The blocks of an automation: [`Trigger`], [`Condition`] and [`Action`],
//! the [`AutomationCtx`] they run with, and the [`Catalog`] an editor builds
//! its palette from.

use std::any::{TypeId, type_name};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

use navree_events::Event;
use navree_kernel::{AppContext, BoxError, BoxFuture};
use navree_openapi::ToSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::matcher::lookup;
use crate::schema::json_schema;

/// What starts an automation: an event, and the settings a person picks in
/// the editor (the type itself is the configuration, so its JSON Schema
/// is the form). `#[trigger]` implements it from a function.
///
/// ```
/// use navree_events::Event;
/// use navree_workflows::Trigger;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Serialize, Deserialize)]
/// struct DealWon {
///     deal_id: u64,
///     owner_id: u64,
///     amount_cents: i64,
/// }
///
/// impl Event for DealWon {
///     const NAME: &'static str = "deal.won";
/// }
///
/// /// A deal was won.
/// #[derive(Serialize, Deserialize, navree_openapi::ToSchema)]
/// struct DealWonTrigger {
///     /// Only deals worth at least this much.
///     min_amount_cents: Option<i64>,
/// }
///
/// impl Trigger for DealWonTrigger {
///     const TYPE: &'static str = "deal.won";
///     const LABEL: &'static str = "Deal won";
///     const SUBJECT: Option<&'static str> = Some("owner_id");
///     type Event = DealWon;
///
///     fn matches(&self, event: &DealWon) -> bool {
///         self.min_amount_cents.is_none_or(|min| event.amount_cents >= min)
///     }
/// }
/// ```
pub trait Trigger: Serialize + DeserializeOwned + ToSchema + Send + Sync + 'static {
    /// The stable type name stored in definitions (`deal.won`).
    const TYPE: &'static str;
    /// The name people see in the editor (default: the type name).
    const LABEL: &'static str = "";
    /// A sentence for people (the doc comment, with `#[trigger]`).
    const DESCRIPTION: &'static str = "";
    /// The dotted path of the event field naming who or what is enrolled
    /// (`owner_id`): enrollment rules and waits for the same subject use
    /// it. `None`: every event is its own enrollment.
    const SUBJECT: Option<&'static str> = None;
    /// The dotted path of the event field holding the workspace id
    /// (default `workspace_id`): an automation of a workspace only enrolls
    /// that workspace's events. `None`: the events have no workspace.
    const WORKSPACE: Option<&'static str> = Some("workspace_id");

    /// The event that fires it.
    type Event: Event;

    /// Whether `event` fires an automation configured with `self`
    /// (default: every event).
    fn matches(&self, event: &Self::Event) -> bool {
        let _ = event;
        true
    }
}

/// A branch in an automation: decides between the `then` and `else` paths
/// from the enrollment's data. `#[condition]` implements it from a
/// function. It is evaluated once per enrollment (the answer is stored
/// with the run) and also by simulations, so keep it free of side effects.
pub trait Condition: Serialize + DeserializeOwned + ToSchema + Send + Sync + 'static {
    /// The stable type name stored in definitions (`deal.amount_at_least`).
    const TYPE: &'static str;
    /// The name people see in the editor (default: the type name).
    const LABEL: &'static str = "";
    /// A sentence for people (the doc comment, with `#[condition]`).
    const DESCRIPTION: &'static str = "";

    /// Whether the enrollment takes the `then` path.
    fn evaluate(&self, ctx: &AutomationCtx) -> impl Future<Output = Result<bool, BoxError>> + Send;
}

/// Something an automation does: send a message, create a record, call a
/// service. `#[action]` implements it from a function. It runs as a
/// durable step: once per enrollment, retried on errors, never during a
/// simulation (which shows [`preview`](Action::preview) instead).
pub trait Action: Serialize + DeserializeOwned + ToSchema + Send + Sync + 'static {
    /// The stable type name stored in definitions (`mail.send`).
    const TYPE: &'static str;
    /// The name people see in the editor (default: the type name).
    const LABEL: &'static str = "";
    /// A sentence for people (the doc comment, with `#[action]`).
    const DESCRIPTION: &'static str = "";

    /// What the action returns; stored with the run and readable by later
    /// nodes as `nodes.<id>`.
    type Output: Serialize + Send + 'static;

    /// Does it. Use [`AutomationCtx::idempotency_key`] with services that
    /// accept one: a step whose process dies runs again.
    fn run(self, ctx: AutomationCtx)
    -> impl Future<Output = Result<Self::Output, BoxError>> + Send;

    /// What a simulation shows instead of running it (default: the
    /// configuration).
    fn preview(&self, ctx: &AutomationCtx) -> Value {
        let _ = ctx;
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// What a condition or action gets: the enrollment's data (the trigger's
/// event, the subject, what earlier nodes returned) and the app.
#[derive(Clone)]
pub struct AutomationCtx {
    pub(crate) app: AppContext,
    pub(crate) automation_id: Uuid,
    pub(crate) version: u32,
    pub(crate) enrollment_id: Uuid,
    pub(crate) run_id: Uuid,
    pub(crate) node: String,
    pub(crate) workspace_id: Option<Uuid>,
    pub(crate) subject: Option<String>,
    pub(crate) data: Arc<Value>,
    pub(crate) simulation: bool,
}

impl AutomationCtx {
    /// The app, to resolve services.
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// The automation.
    pub fn automation_id(&self) -> Uuid {
        self.automation_id
    }

    /// The automation version the enrollment runs.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The enrollment (nil in a simulation).
    pub fn enrollment_id(&self) -> Uuid {
        self.enrollment_id
    }

    /// The workflow run behind the enrollment (nil in a simulation).
    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    /// The id of the node being run.
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The workspace the automation belongs to, if any.
    pub fn workspace_id(&self) -> Option<Uuid> {
        self.workspace_id
    }

    /// Who or what is enrolled (the trigger's `SUBJECT` field), if any.
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// Everything the enrollment knows, as JSON:
    /// `{"event": .., "subject": .., "workspace_id": .., "automation":
    /// {"id", "version"}, "nodes": {"<id>": <what it returned>}}`.
    pub fn data(&self) -> &Value {
        &self.data
    }

    /// The trigger's event as JSON.
    pub fn event(&self) -> &Value {
        self.data.get("event").unwrap_or(&Value::Null)
    }

    /// The trigger's event as `E`.
    pub fn event_as<E: DeserializeOwned>(&self) -> Result<E, BoxError> {
        Ok(serde_json::from_value(self.event().clone())?)
    }

    /// The value at a dotted path of [`data`](AutomationCtx::data)
    /// (`event.deal_id`, `nodes.welcome.message_id`).
    pub fn field(&self, path: &str) -> Option<&Value> {
        lookup(&self.data, path)
    }

    /// What node `id` returned, once it ran.
    pub fn output_of(&self, id: &str) -> Option<&Value> {
        self.data.get("nodes").and_then(|nodes| nodes.get(id))
    }

    /// Whether this is a simulation (`POST /api/automations/{id}/simulate`):
    /// conditions are evaluated, actions are not run.
    pub fn is_simulation(&self) -> bool {
        self.simulation
    }

    /// A key unique to this node of this enrollment
    /// (`<run id>:<node id>`), for services that deduplicate requests.
    pub fn idempotency_key(&self) -> String {
        format!("{}:{}", self.run_id, self.node)
    }
}

impl std::fmt::Debug for AutomationCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutomationCtx")
            .field("automation_id", &self.automation_id)
            .field("version", &self.version)
            .field("node", &self.node)
            .field("subject", &self.subject)
            .field("simulation", &self.simulation)
            .finish_non_exhaustive()
    }
}

/// What kind of block a catalog entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BlockKind {
    /// Starts automations ([`Trigger`]); also what `wait` nodes wait for.
    Trigger,
    /// Branches ([`Condition`]).
    Condition,
    /// Does something ([`Action`]).
    Action,
}

/// One block of the [`Catalog`], as an editor sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BlockInfo {
    /// Trigger, condition or action.
    pub kind: BlockKind,
    /// The type name used in definitions.
    #[serde(rename = "type")]
    pub block_type: String,
    /// The name people see.
    pub label: String,
    /// A sentence for people.
    pub description: String,
    /// For a trigger, the event that fires it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// For a trigger, the event field naming the subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// The JSON Schema of the block's configuration (from its `ToSchema`).
    #[schema(value_type = Object)]
    pub schema: Value,
}

/// Checks a configuration: `Err` holds what is wrong.
type CheckFn = fn(&Value) -> Result<(), String>;

pub(crate) struct TriggerEntry {
    pub(crate) info: BlockInfo,
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) event: &'static str,
    pub(crate) subject: Option<&'static str>,
    pub(crate) workspace: Option<&'static str>,
    pub(crate) check: CheckFn,
    /// Checks that a JSON value is the trigger's event.
    pub(crate) check_event: CheckFn,
    /// Whether an event (JSON) fires a configuration (JSON).
    pub(crate) matches: fn(&Value, &Value) -> Result<bool, String>,
}

pub(crate) struct ConditionEntry {
    pub(crate) info: BlockInfo,
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) check: CheckFn,
    pub(crate) evaluate: fn(Value, AutomationCtx) -> BoxFuture<'static, Result<bool, BoxError>>,
}

pub(crate) struct ActionEntry {
    pub(crate) info: BlockInfo,
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) check: CheckFn,
    pub(crate) run: fn(Value, AutomationCtx) -> BoxFuture<'static, Result<Value, BoxError>>,
    pub(crate) preview: fn(&Value, &AutomationCtx) -> Result<Value, String>,
}

/// One registered block, contributed by `m.trigger::<T>()`,
/// `m.condition::<C>()` or `m.action::<A>()`.
pub(crate) enum BlockRegistration {
    Trigger(TriggerEntry),
    Condition(ConditionEntry),
    Action(ActionEntry),
}

fn label(label: &'static str, kind: &'static str) -> String {
    if label.is_empty() {
        kind.to_owned()
    } else {
        label.to_owned()
    }
}

fn check<T: DeserializeOwned>(config: &Value) -> Result<(), String> {
    decode::<T>(config).map(|_| ())
}

/// Reads `value` as `T`; a missing configuration (`null`) that `T` does not
/// accept is read as `{}`, so blocks whose settings are all optional need
/// no `config`.
fn decode<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    match T::deserialize(value) {
        Ok(decoded) => Ok(decoded),
        Err(error) if value.is_null() => {
            T::deserialize(&Value::Object(serde_json::Map::new())).map_err(|_| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

impl BlockRegistration {
    pub(crate) fn trigger<T: Trigger>() -> Self {
        BlockRegistration::Trigger(TriggerEntry {
            info: BlockInfo {
                kind: BlockKind::Trigger,
                block_type: T::TYPE.to_owned(),
                label: label(T::LABEL, T::TYPE),
                description: T::DESCRIPTION.to_owned(),
                event: Some(T::Event::NAME.to_owned()),
                subject: T::SUBJECT.map(str::to_owned),
                schema: json_schema::<T>(),
            },
            type_id: TypeId::of::<T>(),
            type_name: type_name::<T>(),
            event: T::Event::NAME,
            subject: T::SUBJECT,
            workspace: T::WORKSPACE,
            check: check::<T>,
            check_event: check::<T::Event>,
            matches: |config, event| {
                let config: T = decode(config)?;
                let event: T::Event = decode(event)?;
                Ok(config.matches(&event))
            },
        })
    }

    pub(crate) fn condition<C: Condition>() -> Self {
        BlockRegistration::Condition(ConditionEntry {
            info: BlockInfo {
                kind: BlockKind::Condition,
                block_type: C::TYPE.to_owned(),
                label: label(C::LABEL, C::TYPE),
                description: C::DESCRIPTION.to_owned(),
                event: None,
                subject: None,
                schema: json_schema::<C>(),
            },
            type_id: TypeId::of::<C>(),
            type_name: type_name::<C>(),
            check: check::<C>,
            evaluate: |config, ctx| {
                Box::pin(async move {
                    let condition: C = decode(&config)?;
                    condition.evaluate(&ctx).await
                })
            },
        })
    }

    pub(crate) fn action<A: Action>() -> Self {
        BlockRegistration::Action(ActionEntry {
            info: BlockInfo {
                kind: BlockKind::Action,
                block_type: A::TYPE.to_owned(),
                label: label(A::LABEL, A::TYPE),
                description: A::DESCRIPTION.to_owned(),
                event: None,
                subject: None,
                schema: json_schema::<A>(),
            },
            type_id: TypeId::of::<A>(),
            type_name: type_name::<A>(),
            check: check::<A>,
            run: |config, ctx| {
                Box::pin(async move {
                    let action: A = decode(&config)?;
                    let output = action.run(ctx).await?;
                    Ok(serde_json::to_value(output)?)
                })
            },
            preview: |config, ctx| {
                let action: A = decode(config)?;
                Ok(action.preview(ctx))
            },
        })
    }
}

/// Every registered trigger, condition and action, by type name: what
/// definitions are validated against and what
/// `GET /api/automations/catalog` lists.
#[derive(Default)]
pub struct Catalog {
    pub(crate) triggers: BTreeMap<String, TriggerEntry>,
    pub(crate) conditions: BTreeMap<String, ConditionEntry>,
    pub(crate) actions: BTreeMap<String, ActionEntry>,
}

impl Catalog {
    /// The catalog of `registrations`: fails when two Rust types claim one
    /// type name (the same type registered twice is one block).
    pub(crate) fn new(
        registrations: impl IntoIterator<Item = Arc<BlockRegistration>>,
    ) -> Result<Self, String> {
        fn clash(kind: &str, name: &str, a: &str, b: &str) -> String {
            format!("two automation {kind}s are registered as `{name}`: {a} and {b}")
        }
        let mut catalog = Catalog::default();
        for registration in registrations {
            match &*registration {
                BlockRegistration::Trigger(entry) => {
                    if let Some(other) = catalog.triggers.get(&entry.info.block_type) {
                        if other.type_id != entry.type_id {
                            return Err(clash(
                                "trigger",
                                &entry.info.block_type,
                                other.type_name,
                                entry.type_name,
                            ));
                        }
                        continue;
                    }
                    catalog
                        .triggers
                        .insert(entry.info.block_type.clone(), clone_trigger(entry));
                }
                BlockRegistration::Condition(entry) => {
                    if let Some(other) = catalog.conditions.get(&entry.info.block_type) {
                        if other.type_id != entry.type_id {
                            return Err(clash(
                                "condition",
                                &entry.info.block_type,
                                other.type_name,
                                entry.type_name,
                            ));
                        }
                        continue;
                    }
                    catalog.conditions.insert(
                        entry.info.block_type.clone(),
                        ConditionEntry {
                            info: entry.info.clone(),
                            type_id: entry.type_id,
                            type_name: entry.type_name,
                            check: entry.check,
                            evaluate: entry.evaluate,
                        },
                    );
                }
                BlockRegistration::Action(entry) => {
                    if let Some(other) = catalog.actions.get(&entry.info.block_type) {
                        if other.type_id != entry.type_id {
                            return Err(clash(
                                "action",
                                &entry.info.block_type,
                                other.type_name,
                                entry.type_name,
                            ));
                        }
                        continue;
                    }
                    catalog.actions.insert(
                        entry.info.block_type.clone(),
                        ActionEntry {
                            info: entry.info.clone(),
                            type_id: entry.type_id,
                            type_name: entry.type_name,
                            check: entry.check,
                            run: entry.run,
                            preview: entry.preview,
                        },
                    );
                }
            }
        }
        Ok(catalog)
    }

    /// Every trigger, by type name.
    pub fn triggers(&self) -> Vec<BlockInfo> {
        self.triggers.values().map(|e| e.info.clone()).collect()
    }

    /// Every condition, by type name.
    pub fn conditions(&self) -> Vec<BlockInfo> {
        self.conditions.values().map(|e| e.info.clone()).collect()
    }

    /// Every action, by type name.
    pub fn actions(&self) -> Vec<BlockInfo> {
        self.actions.values().map(|e| e.info.clone()).collect()
    }

    /// The block of `kind` named `block_type`.
    pub fn block(&self, kind: BlockKind, block_type: &str) -> Option<BlockInfo> {
        match kind {
            BlockKind::Trigger => self.triggers.get(block_type).map(|e| e.info.clone()),
            BlockKind::Condition => self.conditions.get(block_type).map(|e| e.info.clone()),
            BlockKind::Action => self.actions.get(block_type).map(|e| e.info.clone()),
        }
    }

    /// The names of the events triggers listen to.
    pub(crate) fn trigger_events(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.triggers.values().map(|e| e.event)
    }

    pub(crate) fn trigger(&self, block_type: &str) -> Option<&TriggerEntry> {
        self.triggers.get(block_type)
    }

    pub(crate) fn condition(&self, block_type: &str) -> Option<&ConditionEntry> {
        self.conditions.get(block_type)
    }

    pub(crate) fn action(&self, block_type: &str) -> Option<&ActionEntry> {
        self.actions.get(block_type)
    }
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("triggers", &self.triggers.keys().collect::<Vec<_>>())
            .field("conditions", &self.conditions.keys().collect::<Vec<_>>())
            .field("actions", &self.actions.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn clone_trigger(entry: &TriggerEntry) -> TriggerEntry {
    TriggerEntry {
        info: entry.info.clone(),
        type_id: entry.type_id,
        type_name: entry.type_name,
        event: entry.event,
        subject: entry.subject,
        workspace: entry.workspace,
        check: entry.check,
        check_event: entry.check_event,
        matches: entry.matches,
    }
}

/// A subject value as text: strings as they are, anything else as JSON
/// (`7`, `true`).
pub(crate) fn subject_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

impl TriggerEntry {
    /// The subject of `event`, by the trigger's `SUBJECT` path.
    pub(crate) fn subject_of(&self, event: &Value) -> Option<String> {
        self.subject
            .and_then(|path| lookup(event, path))
            .and_then(subject_text)
    }

    /// The workspace of `event`, by the trigger's `WORKSPACE` path.
    pub(crate) fn workspace_of(&self, event: &Value) -> Option<Uuid> {
        self.workspace
            .and_then(|path| lookup(event, path))
            .and_then(Value::as_str)
            .and_then(|text| Uuid::parse_str(text).ok())
    }
}

/// The built-in condition `field`: compares a value of the enrollment's
/// data with a constant.
///
/// ```json
/// {"kind": "condition", "type": "field",
///  "config": {"path": "event.amount_cents", "op": "gte", "value": 100000}}
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FieldCondition {
    /// The dotted path into the enrollment's data (`event.amount_cents`,
    /// `nodes.check-score.score`, `subject`).
    pub path: String,
    /// How to compare.
    pub op: CompareOp,
    /// What to compare with (not needed by `exists` and `missing`).
    #[serde(default)]
    #[schema(value_type = Object)]
    pub value: Value,
}

/// How [`FieldCondition`] compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    /// Equal (as JSON).
    Eq,
    /// Not equal.
    Ne,
    /// Greater than (numbers, or strings in byte order).
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// The text contains the value, or the list holds it.
    Contains,
    /// The value (a list) holds the field.
    In,
    /// The field is present and not null.
    Exists,
    /// The field is absent or null.
    Missing,
}

impl FieldCondition {
    /// Whether `data` passes.
    pub fn test(&self, data: &Value) -> bool {
        let field = lookup(data, &self.path).filter(|v| !v.is_null());
        match self.op {
            CompareOp::Exists => field.is_some(),
            CompareOp::Missing => field.is_none(),
            CompareOp::Eq => field == Some(&self.value),
            CompareOp::Ne => field != Some(&self.value),
            CompareOp::Gt | CompareOp::Gte | CompareOp::Lt | CompareOp::Lte => {
                let Some(ordering) = field.and_then(|field| compare(field, &self.value)) else {
                    return false;
                };
                match self.op {
                    CompareOp::Gt => ordering.is_gt(),
                    CompareOp::Gte => ordering.is_ge(),
                    CompareOp::Lt => ordering.is_lt(),
                    _ => ordering.is_le(),
                }
            }
            CompareOp::Contains => match field {
                Some(Value::String(text)) => self.value.as_str().is_some_and(|v| text.contains(v)),
                Some(Value::Array(items)) => items.contains(&self.value),
                _ => false,
            },
            CompareOp::In => match (&self.value, field) {
                (Value::Array(items), Some(field)) => items.contains(field),
                _ => false,
            },
        }
    }
}

fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64()?.partial_cmp(&b.as_f64()?),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

impl Condition for FieldCondition {
    const TYPE: &'static str = "field";
    const LABEL: &'static str = "Compare a field";
    const DESCRIPTION: &'static str =
        "Takes the `then` path when a value of the enrollment's data compares as configured.";

    async fn evaluate(&self, ctx: &AutomationCtx) -> Result<bool, BoxError> {
        Ok(self.test(ctx.data()))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn field(path: &str, op: CompareOp, value: Value) -> FieldCondition {
        FieldCondition {
            path: path.to_owned(),
            op,
            value,
        }
    }

    #[test]
    fn field_conditions_compare_json() {
        let data = json!({"event": {"amount": 1500, "tags": ["vip"], "name": "Agra Inc"}});
        assert!(field("event.amount", CompareOp::Gte, json!(1000)).test(&data));
        assert!(!field("event.amount", CompareOp::Lt, json!(1000)).test(&data));
        assert!(field("event.amount", CompareOp::Eq, json!(1500)).test(&data));
        assert!(field("event.amount", CompareOp::Ne, json!(1)).test(&data));
        assert!(field("event.tags", CompareOp::Contains, json!("vip")).test(&data));
        assert!(field("event.name", CompareOp::Contains, json!("Agra")).test(&data));
        assert!(field("event.name", CompareOp::In, json!(["Agra Inc", "Other"])).test(&data));
        assert!(field("event.name", CompareOp::Exists, Value::Null).test(&data));
        assert!(field("event.missing", CompareOp::Missing, Value::Null).test(&data));
        // A missing field never compares.
        assert!(!field("event.missing", CompareOp::Gt, json!(0)).test(&data));
        assert!(!field("event.name", CompareOp::Gt, json!(0)).test(&data));
    }

    #[test]
    fn subjects_are_text() {
        assert_eq!(subject_text(&json!("u-1")).as_deref(), Some("u-1"));
        assert_eq!(subject_text(&json!(7)).as_deref(), Some("7"));
        assert_eq!(subject_text(&Value::Null), None);
    }
}

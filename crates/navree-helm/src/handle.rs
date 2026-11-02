//! [`Handle`]: a [`Resource`] behind the object-safe [`ErasedResource`],
//! which the routes, the presenter and the queued actions use.

use std::any::TypeId;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use chrono::{Datelike, Duration, NaiveDate};
use navree_access::{CREATE, DELETE, READ, Subject, UPDATE};
use navree_contracts::Dispatcher;
use navree_db::{PageMeta, PageQuery};
use navree_http::AppError;
use navree_kernel::{BoxFuture, ModuleId};
use navree_orm::access::record_json;
use navree_orm::ir::AggregateFunction;
use navree_orm::{Builder, Expr, FieldError, Model, Row, Selectable, Value, col};
use navree_validation::Messages;
use regex::Regex;
use serde_json::{Map, Value as Json, json};

use crate::action::Action;
use crate::cx::{Cx, action_entry};
use crate::field::{Computed, Field, FieldKind, Related};
use crate::filter::{Filter, FilterKind};
use crate::job::HelmActionJob;
use crate::metric::{
    Aggregate, Metric, MetricKind, MetricRange, MetricResult, Point, RangeOption, buckets, midnight,
};
use crate::query::ListQuery;
use crate::values::{self, Invalid, Shape, is_blank, key_from_str, key_text};
use crate::workspace::{self, NamedBy, Refused};
use crate::{
    ActionContext, ActionResult, FormData, HelmOptions, RecordOf, Resource, SaveContext, Workspaces,
};
use crate::{money, save};

/// What the panel knows about a resource, without its types.
pub(crate) struct ResourceInfo {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) module: ModuleId,
    pub(crate) slug: String,
    pub(crate) label: &'static str,
    pub(crate) singular: &'static str,
    pub(crate) group: String,
    pub(crate) icon: &'static str,
    pub(crate) order: i32,
    pub(crate) subject: &'static str,
    /// The permission prefix of the model (`deals` for `deals.update`).
    pub(crate) permission_prefix: &'static str,
    pub(crate) read_only: bool,
    pub(crate) count_cap: u64,
    /// The column that holds the workspace of a record, when the model
    /// belongs to workspaces.
    pub(crate) workspace_column: Option<&'static str>,
    pub(crate) export: bool,
    pub(crate) sidebar_count: bool,
    pub(crate) pk: &'static str,
    pub(crate) title: &'static str,
    pub(crate) search: Vec<&'static str>,
    pub(crate) per_page: u64,
    pub(crate) default_sort: String,
    pub(crate) fields: Vec<Field>,
    pub(crate) filters: Vec<FilterInfo>,
    pub(crate) actions: Vec<ActionInfo>,
    pub(crate) metrics: Vec<MetricInfo>,
    pub(crate) patterns: HashMap<String, Regex>,
    /// Columns that may hold `NULL`.
    pub(crate) nullable: HashSet<&'static str>,
    /// Where the resource works, when its model belongs to workspaces.
    pub(crate) workspaces: Option<Workspaces>,
}

impl ResourceInfo {
    /// The field named `name`.
    pub(crate) fn field(&self, name: &str) -> Option<&Field> {
        self.fields
            .iter()
            .find(|f| f.name == name && !matches!(f.kind, FieldKind::HasMany(_)))
    }

    /// The fields whose columns can be sorted by, with the default sort's.
    pub(crate) fn sortable(&self) -> Vec<&str> {
        let mut columns: Vec<&str> = self
            .fields
            .iter()
            .filter(|f| f.sortable && f.is_column())
            .map(|f| f.name.as_str())
            .collect();
        let default = self.default_sort.trim_start_matches('-');
        if !columns.contains(&default) {
            columns.push(default);
        }
        columns
    }
}

/// A filter, without its model type.
#[derive(Clone, Debug)]
pub(crate) struct FilterInfo {
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) kind: &'static str,
    pub(crate) options: Vec<(String, String)>,
    pub(crate) related: Option<Related>,
}

/// An action, without its model type.
#[derive(Clone, Debug)]
pub(crate) struct ActionInfo {
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) ability: &'static str,
    pub(crate) question: Option<String>,
    pub(crate) destructive: bool,
    pub(crate) fields: Vec<Field>,
    pub(crate) on_index: bool,
    pub(crate) on_detail: bool,
    pub(crate) queued: bool,
}

/// A metric, without its model type.
#[derive(Clone, Debug)]
pub(crate) struct MetricInfo {
    pub(crate) slug: String,
    pub(crate) label: String,
    pub(crate) kind: &'static str,
    pub(crate) on_dashboard: bool,
    pub(crate) ranged: bool,
}

/// A record read for the panel: its key, title, JSON form (column →
/// value, as access rules see it) and the values of computed and view
/// fields.
#[derive(Clone, Debug)]
pub(crate) struct RawRecord {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) json: Json,
    pub(crate) computed: HashMap<String, String>,
    pub(crate) html: HashMap<String, String>,
}

/// One page of a list.
#[derive(Clone, Debug)]
pub(crate) struct RawPage {
    pub(crate) records: Vec<RawRecord>,
    pub(crate) meta: PageMeta,
    /// The rows were counted up to the resource's `COUNT_CAP` and there are
    /// more: `meta.total` is that cap, and `meta.last_page` the page after this
    /// one if there is one.
    pub(crate) capped: bool,
}

/// A chunk of an export: its rows, and where the next chunk starts, if
/// there is one.
#[derive(Clone, Debug)]
pub(crate) struct ExportChunk {
    pub(crate) records: Vec<RawRecord>,
    pub(crate) next: Option<String>,
}

/// What an action run did.
#[derive(Clone, Debug)]
pub(crate) struct ActionOutcome {
    pub(crate) result: ActionResult,
    pub(crate) ran: usize,
    pub(crate) skipped: usize,
    pub(crate) queued: bool,
}

/// A resource, as the panel uses it.
pub(crate) trait ErasedResource: Send + Sync + 'static {
    fn info(&self) -> &ResourceInfo;

    /// A page of the list: the rows the user may read.
    fn list<'a>(&'a self, cx: &'a Cx, q: &'a ListQuery)
    -> BoxFuture<'a, Result<RawPage, AppError>>;

    /// A chunk of the list's rows for an export: see [`Handle::export_page`].
    fn export<'a>(
        &'a self,
        cx: &'a Cx,
        q: &'a ListQuery,
        cursor: Option<String>,
        limit: u64,
    ) -> BoxFuture<'a, Result<ExportChunk, AppError>>;

    /// How many rows the user may read, up to `cap` + 1.
    fn count<'a>(&'a self, cx: &'a Cx, cap: u64) -> BoxFuture<'a, Result<u64, AppError>>;

    /// The record `id`, if the user may read it.
    fn find<'a>(
        &'a self,
        cx: &'a Cx,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<RawRecord>, AppError>>;

    fn create<'a>(
        &'a self,
        cx: &'a Cx,
        input: Map<String, Json>,
    ) -> BoxFuture<'a, Result<RawRecord, AppError>>;

    fn update<'a>(
        &'a self,
        cx: &'a Cx,
        id: &'a str,
        input: Map<String, Json>,
    ) -> BoxFuture<'a, Result<RawRecord, AppError>>;

    fn delete<'a>(&'a self, cx: &'a Cx, id: &'a str) -> BoxFuture<'a, Result<(), AppError>>;

    /// Runs action `name` on the records `ids` the user may act on
    /// (`from_job`: inside the queued `helm.action` job).
    fn run_action<'a>(
        &'a self,
        cx: &'a Cx,
        name: &'a str,
        ids: Vec<String>,
        input: Map<String, Json>,
        from_job: bool,
    ) -> BoxFuture<'a, Result<ActionOutcome, AppError>>;

    fn metric<'a>(
        &'a self,
        cx: &'a Cx,
        slug: &'a str,
        range: MetricRange,
    ) -> BoxFuture<'a, Result<MetricResult, AppError>>;

    /// `(id, title)` of readable records: matching `search`, or with keys
    /// `ids`, by title, at most `limit`.
    fn lookup<'a>(
        &'a self,
        cx: &'a Cx,
        search: Option<&'a str>,
        ids: Option<&'a [String]>,
        limit: u64,
    ) -> BoxFuture<'a, Result<Vec<(String, String)>, AppError>>;
}

/// A [`Resource`] with everything checked and resolved at boot.
pub(crate) struct Handle<R: Resource> {
    resource: Arc<R>,
    info: ResourceInfo,
    filters: Vec<Filter<R::Model>>,
    actions: Vec<Action<R::Model>>,
    metrics: Vec<Metric<R::Model>>,
    shapes: HashMap<&'static str, Shape>,
    computed: Vec<(String, Computed)>,
    views: Vec<(String, String)>,
    /// The app's validation messages with each field named by its label.
    labelled: OnceLock<Messages>,
}

impl<R: Resource> Handle<R> {
    /// Checks `resource`'s definition against its model; the error names
    /// the resource and what is wrong.
    pub(crate) fn build(
        resource: Arc<R>,
        module: ModuleId,
        options: &HelmOptions,
    ) -> Result<Self, String> {
        let name = std::any::type_name::<R>();
        let meta = R::Model::meta();
        let table = meta.table();
        let fail = |what: String| format!("Helm resource `{name}`: {what}");
        let pk = meta.key_column();
        let shapes: HashMap<&'static str, Shape> = meta
            .all_columns()
            .iter()
            .map(|column| (column.name(), Shape::of(column)))
            .collect();
        let column = |column: &str| meta.column(column).map(|c| c.name());
        let shape = |column: &str| shapes.get(column).copied().unwrap_or(Shape::Other);
        let nullable: HashSet<&'static str> = meta
            .all_columns()
            .iter()
            .filter(|c| c.is_nullable())
            .map(|c| c.name())
            .collect();

        let mut fields = resource.fields();
        let mut seen = HashSet::new();
        let mut patterns = HashMap::new();
        let mut computed = Vec::new();
        let mut views = Vec::new();
        for field in &mut fields {
            if matches!(field.kind, FieldKind::Id) {
                field.name = pk.to_owned();
            }
            // A `has_many` is named by the foreign key of the records it lists, so
            // two may share one (and one may share a column's name, as a tree's
            // children and parent do): they differ by the resource they list.
            let identity = match &field.kind {
                FieldKind::HasMany(related) => format!("{}\u{0}{}", field.name, related.type_name),
                _ => field.name.clone(),
            };
            if !seen.insert(identity) {
                return Err(fail(format!("two fields are named `{}`", field.name)));
            }
            if field.is_column() && column(&field.name).is_none() {
                return Err(fail(format!(
                    "field `{}` is not a column of `{table}`",
                    field.name
                )));
            }
            match &field.kind {
                FieldKind::Money { currency_column } if column(currency_column).is_none() => {
                    return Err(fail(format!(
                        "the currency column `{currency_column}` of field `{}` is not a column of `{table}`",
                        field.name
                    )));
                }
                FieldKind::Computed(c) => {
                    if c.model != TypeId::of::<R::Model>() {
                        return Err(fail(format!(
                            "computed field `{}` takes `{}`, not the resource's model `{}`",
                            field.name,
                            c.model_name,
                            std::any::type_name::<R::Model>()
                        )));
                    }
                    computed.push((field.name.clone(), c.clone()));
                }
                FieldKind::View { template } => views.push((field.name.clone(), template.clone())),
                FieldKind::Date | FieldKind::DateTime
                    if !shape(&field.name).is_temporal() && shape(&field.name) != Shape::Text =>
                {
                    return Err(fail(format!(
                        "field `{}` is a date field but its column is not a date",
                        field.name
                    )));
                }
                _ => {}
            }
            if let Some(source) = &field.pattern {
                let regex = Regex::new(source).map_err(|error| {
                    fail(format!(
                        "the pattern of field `{}` is invalid: {error}",
                        field.name
                    ))
                })?;
                patterns.insert(field.name.clone(), regex);
            }
        }

        let title = column(R::TITLE)
            .ok_or_else(|| fail(format!("TITLE `{}` is not a column of `{table}`", R::TITLE)))?;
        let search = R::SEARCH
            .iter()
            .map(|name| {
                column(name).ok_or_else(|| {
                    fail(format!(
                        "SEARCH column `{name}` is not a column of `{table}`"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let filters = resource.filters();
        let mut filter_infos = Vec::new();
        let mut filter_names = HashSet::new();
        for filter in &filters {
            if !filter_names.insert(filter.name.clone()) {
                return Err(fail(format!("two filters are named `{}`", filter.name)));
            }
            let needs_column = matches!(
                filter.kind,
                FilterKind::Select | FilterKind::BelongsTo(_) | FilterKind::DateRange
            );
            if needs_column && column(&filter.name).is_none() {
                return Err(fail(format!(
                    "filter `{}` is not a column of `{table}`",
                    filter.name
                )));
            }
            if matches!(filter.kind, FilterKind::DateRange) && !shape(&filter.name).is_temporal() {
                return Err(fail(format!(
                    "date_range filter `{}` needs a date or date-time column",
                    filter.name
                )));
            }
            filter_infos.push(FilterInfo {
                name: filter.name.clone(),
                label: filter.label_text(),
                kind: filter.kind.name(),
                options: filter.options.clone(),
                related: match filter.kind {
                    FilterKind::BelongsTo(related) => Some(related),
                    _ => None,
                },
            });
        }

        let actions = resource.actions();
        let mut action_infos = Vec::new();
        let mut action_names = HashSet::new();
        for action in &actions {
            if action.name.is_empty() || !action_names.insert(action.name.clone()) {
                return Err(fail(format!(
                    "two actions are named `{}` (or one has no name)",
                    action.name
                )));
            }
            if action.handler.is_none() {
                return Err(fail(format!(
                    "action `{}` has no handler: add `.handle(|ctx, records| async move {{ .. }})`",
                    action.name
                )));
            }
            for field in &action.fields {
                if let Some(source) = &field.pattern {
                    Regex::new(source).map_err(|error| {
                        fail(format!(
                            "the pattern of field `{}` of action `{}` is invalid: {error}",
                            field.name, action.name
                        ))
                    })?;
                }
            }
            action_infos.push(ActionInfo {
                name: action.name.clone(),
                label: action.label.clone(),
                ability: action.ability,
                question: action.question(),
                destructive: action.destructive,
                fields: action.fields.clone(),
                on_index: action.on_index(),
                on_detail: action.on_detail(),
                queued: action.queued,
            });
        }

        let metrics = resource.metrics();
        let mut metric_infos = Vec::new();
        let mut metric_slugs = HashSet::new();
        for metric in &metrics {
            if !metric_slugs.insert(metric.slug.clone()) {
                return Err(fail(format!("two metrics are named `{}`", metric.label)));
            }
            let named = [
                metric.aggregate.column(),
                metric.over.as_ref(),
                metric.trend.as_ref().map(|t| &t.0),
                metric.group_by.as_ref(),
            ];
            if let Some(unknown) = named.into_iter().flatten().find(|c| column(c).is_none()) {
                return Err(fail(format!(
                    "metric `{}`: `{unknown}` is not a column of `{table}`",
                    metric.label
                )));
            }
            let temporal = |c: &str| shape(c).is_temporal();
            match metric.kind {
                MetricKind::Trend => match &metric.trend {
                    None => {
                        return Err(fail(format!(
                            "trend metric `{}` needs `.by_day(..)`, `.by_week(..)` or `.by_month(..)`",
                            metric.label
                        )));
                    }
                    Some((c, _, _)) if !temporal(c) => {
                        return Err(fail(format!(
                            "trend metric `{}` needs a date or date-time column",
                            metric.label
                        )));
                    }
                    _ => {}
                },
                MetricKind::Partition if metric.group_by.is_none() => {
                    return Err(fail(format!(
                        "partition metric `{}` needs `.group_by(..)`",
                        metric.label
                    )));
                }
                MetricKind::Value => {
                    if let Some(c) = &metric.over
                        && !temporal(c)
                    {
                        return Err(fail(format!(
                            "value metric `{}`: `.over(..)` needs a date or date-time column",
                            metric.label
                        )));
                    }
                }
                _ => {}
            }
            metric_infos.push(MetricInfo {
                slug: metric.slug.clone(),
                label: metric.label.clone(),
                kind: metric.kind.name(),
                on_dashboard: metric.on_dashboard,
                ranged: metric.over.is_some(),
            });
        }

        let default_sort = if R::DEFAULT_SORT.is_empty() {
            if column("created_at").is_some() {
                "-created_at".to_owned()
            } else {
                format!("-{pk}")
            }
        } else {
            R::DEFAULT_SORT.to_owned()
        };
        if column(default_sort.trim_start_matches('-')).is_none() {
            return Err(fail(format!(
                "DEFAULT_SORT `{default_sort}` is not a column of `{table}`"
            )));
        }
        let slug = if R::SLUG.is_empty() {
            table.replace('_', "-")
        } else {
            R::SLUG.to_owned()
        };
        if slug.is_empty()
            || !slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
        {
            return Err(fail(format!(
                "SLUG `{slug}` must use lower-case letters, digits, - and _"
            )));
        }
        let per_page = if R::PER_PAGE == 0 {
            options.per_page
        } else {
            R::PER_PAGE.min(navree_db::MAX_PER_PAGE)
        };

        let info = ResourceInfo {
            type_id: TypeId::of::<R>(),
            type_name: name,
            module,
            slug,
            label: R::LABEL,
            singular: R::SINGULAR,
            group: if R::GROUP.is_empty() {
                "Resources".to_owned()
            } else {
                R::GROUP.to_owned()
            },
            icon: R::ICON,
            order: R::ORDER,
            subject: <R::Model as Subject>::SUBJECT,
            permission_prefix: <R::Model as Subject>::PERMISSION_PREFIX,
            read_only: R::READ_ONLY,
            count_cap: R::COUNT_CAP,
            workspace_column: meta.workspace_column(),
            export: R::EXPORT,
            sidebar_count: R::SIDEBAR_COUNT,
            pk,
            title,
            search,
            per_page,
            default_sort,
            fields,
            filters: filter_infos,
            actions: action_infos,
            metrics: metric_infos,
            patterns,
            nullable,
            workspaces: meta.workspace_column().map(|_| R::WORKSPACES),
        };
        Ok(Self {
            resource,
            info,
            filters,
            actions,
            metrics,
            shapes,
            computed,
            views,
            labelled: OnceLock::new(),
        })
    }

    /// The app's validation messages with every field of the resource named by
    /// its label.
    fn messages<'a>(&'a self, cx: &'a Cx) -> &'a Messages {
        self.labelled
            .get_or_init(|| labelled(&cx.core.messages, &self.info.fields))
    }

    /// The rows the user may do `action` on, starting from the resource's
    /// query: the model's query (its global scopes apply when it runs),
    /// the resource's narrowing, then the user's access filter.
    fn base(&self, cx: &Cx, action: &str) -> Builder<R::Model> {
        let query = R::Model::query();
        // A credential limited to a workspace is scoped to it across workspaces too.
        let query = if self.info.workspaces == Some(Workspaces::Every) && cx.pin.is_none() {
            query.without_workspace_scope()
        } else {
            query
        };
        self.resource
            .query(query)
            .accessible_for(cx.ability(), action)
    }

    /// A 400 that asks for a workspace when the resource's model belongs to
    /// workspaces, the resource works in one, and the request has none.
    /// Nothing falls back to the records of every workspace.
    fn in_a_workspace(&self, cx: &Cx) -> Result<(), AppError> {
        if self.info.workspaces == Some(Workspaces::Current) && cx.uow.workspace().is_none() {
            if let Some(refused) = cx.refused_write {
                return Err(refused.error());
            }
            return Err(AppError::bad_request(format!(
                "Choose a workspace first: {} belong to workspaces.",
                self.info.label
            )));
        }
        Ok(())
    }

    /// A 403 for a write to a resource that is read-only.
    fn writable(&self) -> Result<(), AppError> {
        if self.info.read_only {
            return Err(AppError::forbidden()
                .with_code("read_only_resource")
                .with_detail(format!("{} are read-only.", self.info.label)));
        }
        Ok(())
    }

    fn shape(&self, column: &str) -> Shape {
        self.shapes.get(column).copied().unwrap_or(Shape::Other)
    }

    /// The workspace of `record`, when it belongs to one.
    fn owner_of(&self, record: &RecordOf<R>) -> Result<Option<uuid::Uuid>, AppError> {
        Ok(save::owner(record.model())?.flatten())
    }

    /// An error if the workspace of a record the user may otherwise write takes
    /// no such write from them.
    async fn record_writes(
        &self,
        cx: &Cx,
        record: &RecordOf<R>,
        action: &str,
    ) -> Result<(), AppError> {
        if self.info.workspaces.is_none() {
            return Ok(());
        }
        let prefix = <R::Model as Subject>::PERMISSION_PREFIX;
        cx.record_writes(self.owner_of(record)?, prefix, action)
            .await
    }

    /// The workspace a form across workspaces names.
    fn named_by_form(&self, input: &Map<String, Json>) -> Option<uuid::Uuid> {
        if self.info.workspaces != Some(Workspaces::Every) {
            return None;
        }
        let column = R::Model::meta().workspace_column()?;
        uuid::Uuid::parse_str(input.get(column)?.as_str()?.trim()).ok()
    }

    /// Column `name` of the model, qualified with its table.
    fn col(&self, name: &str) -> navree_orm::Column<navree_orm::Untyped, Value> {
        col(values::qualified::<R::Model>(name))
    }

    fn raw(&self, cx: &Cx, record: &RecordOf<R>) -> RawRecord {
        let json = record_json(record);
        let id = json
            .get(self.info.pk)
            .and_then(key_text)
            .unwrap_or_default();
        let title = title_of(&self.info, &json, &id);
        let computed = self
            .computed
            .iter()
            .filter_map(|(name, c)| {
                c.call::<R::Model>(record.model())
                    .map(|v| (name.clone(), v))
            })
            .collect();
        let mut html = HashMap::new();
        if let Some(views) = &cx.core.app_views {
            for (name, template) in &self.views {
                match views.render(template, json!({ "record": json, "field": name })) {
                    Ok(rendered) => {
                        html.insert(name.clone(), rendered);
                    }
                    Err(error) => {
                        tracing::warn!(target: "navree::helm", %error, field = %name, "a view field failed to render");
                    }
                }
            }
        }
        RawRecord {
            id,
            title,
            json,
            computed,
            html,
        }
    }

    /// The readable record `id` (locked for update when `lock` and the
    /// backend supports it).
    async fn load(&self, cx: &Cx, id: &str, lock: bool) -> Result<Option<RecordOf<R>>, AppError> {
        let Some(key) = key_from_str(self.shape(self.info.pk), id) else {
            return Ok(None);
        };
        let mut query = self.base(cx, READ).where_(self.col(self.info.pk).eq(key));
        if lock && cx.uow.lock_for_update() {
            query = query.lock_for_update();
        }
        Ok(query.first(&cx.uow).await?)
    }

    /// Converts form input to its JSON form for `field` (numbers from text,
    /// booleans from `on`, date-times to RFC 3339 in UTC, blanks to `null`
    /// for nullable columns).
    fn normalize(&self, field: &Field, value: Json, tz_offset: Option<i32>) -> Json {
        let value = match (&field.kind, value) {
            (FieldKind::Number | FieldKind::Money { .. }, Json::String(text))
                if !text.trim().is_empty() =>
            {
                let text = text.trim();
                if let Ok(n) = text.parse::<i64>() {
                    Json::from(n)
                } else if let Some(n) = text
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                {
                    Json::Number(n)
                } else {
                    Json::String(text.to_owned())
                }
            }
            (FieldKind::Boolean, value) => values::boolean(&value).map(Json::Bool).unwrap_or(value),
            (FieldKind::DateTime, Json::String(text)) if !text.trim().is_empty() => {
                match values::datetime(&Json::String(text.clone()), tz_offset) {
                    Some(at) => Json::String(at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                    None => Json::String(text),
                }
            }
            (FieldKind::Email | FieldKind::Url, Json::String(text)) => {
                Json::String(text.trim().to_owned())
            }
            (_, value) => value,
        };
        if is_blank(&value) && self.info.nullable.contains(field.name.as_str()) {
            Json::Null
        } else {
            value
        }
    }

    /// Whether `value` is the path of a file Helm stored for this resource
    /// (`{slug}/{40 letters and digits}.{extension}`): a file field never
    /// points at another file of its disk.
    fn stored_path(&self, value: &Json) -> bool {
        let Some(name) = value
            .as_str()
            .and_then(|path| path.strip_prefix(self.info.slug.as_str()))
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            return false;
        };
        let Some((token, extension)) = name.split_once('.') else {
            return false;
        };
        token.len() == 40
            && token.bytes().all(|b| b.is_ascii_alphanumeric())
            && !extension.is_empty()
            && extension.bytes().all(|b| b.is_ascii_alphanumeric())
    }

    /// Checks `input` against `fields` (the editable fields of a form, or
    /// an action's fields): unknown keys, rules, column types and related
    /// records. On create, adds defaults and reports missing required
    /// fields.
    async fn checked(
        &self,
        cx: &Cx,
        fields: &[Field],
        input: Map<String, Json>,
        create: bool,
        columns: bool,
    ) -> Result<Map<String, Json>, AppError> {
        // The fields checked name themselves: an action's field is "Reason" even
        // where the record's column is "Lost reason".
        let labelled = labelled(&cx.core.messages, fields);
        let messages = &labelled;
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut checked = Map::new();
        for (key, value) in input {
            let Some(field) = fields.iter().find(|f| f.name == key) else {
                // A property of the request, not a field the user sees.
                errors
                    .entry(key.clone())
                    .or_default()
                    .push(cx.core.messages.render(&key, "prohibited", &[]));
                continue;
            };
            let value = self.normalize(field, value, cx.tz_offset);
            let mut problems = Vec::new();
            let pattern = match &field.pattern {
                Some(_) if columns => self.info.patterns.get(&key).cloned(),
                Some(source) => Regex::new(source).ok(),
                None => None,
            };
            field.validate(&value, messages, pattern.as_ref(), &mut problems);
            if matches!(field.kind, FieldKind::File { .. })
                && !is_blank(&value)
                && !self.stored_path(&value)
            {
                problems.push(messages.render(&key, "invalid", &[]));
            }
            if problems.is_empty() && columns && field.is_column() {
                match values::to_input(self.shape(&key), &value, None) {
                    Err(Invalid(rule)) => problems.push(messages.render(&key, rule, &[])),
                    Ok(Json::Null) if !self.info.nullable.contains(key.as_str()) => {
                        problems.push(messages.render(&key, "required", &[]));
                    }
                    Ok(_) => {}
                }
            }
            if problems.is_empty() {
                checked.insert(key, value);
            } else {
                errors.insert(key, problems);
            }
        }
        if create {
            for field in fields {
                if checked.contains_key(&field.name) || errors.contains_key(&field.name) {
                    continue;
                }
                if let Some(default) = &field.default {
                    checked.insert(field.name.clone(), default.clone());
                } else if field.required {
                    errors.insert(
                        field.name.clone(),
                        vec![messages.render(&field.name, "required", &[])],
                    );
                }
            }
        }
        for field in fields {
            let FieldKind::BelongsTo(related) = field.kind else {
                continue;
            };
            let Some(id) = checked
                .get(&field.name)
                .filter(|v| !is_blank(v))
                .and_then(key_text)
            else {
                continue;
            };
            let found = match cx.core.registry.by_type(related.type_id) {
                Some(handle) => !handle
                    .lookup(cx, None, Some(std::slice::from_ref(&id)), 1)
                    .await?
                    .is_empty(),
                None => false,
            };
            if !found {
                errors
                    .entry(field.name.clone())
                    .or_default()
                    .push(messages.render(&field.name, "exists", &[]));
            }
        }
        if errors.is_empty() {
            Ok(checked)
        } else {
            Err(AppError::validation(errors))
        }
    }

    /// [`checked`](Self::checked) for a create: a form across workspaces refers to
    /// records of the workspace it names, as the create will find them. Someone
    /// who may not work in that one makes the record in no workspace, and checks it
    /// there.
    async fn checked_for_form(
        &self,
        cx: &Cx,
        fields: &[Field],
        input: Map<String, Json>,
    ) -> Result<Map<String, Json>, AppError> {
        let named = self
            .named_by_form(&input)
            .filter(|named| cx.uow.workspace() != Some(*named))
            .filter(|_| self.form_may_name_workspace(cx) || cx.uow.workspace().is_none());
        let Some(named) = named else {
            return self.checked(cx, fields, input, true, true).await;
        };
        let admitted =
            workspace::admission(&cx.app, &cx.core, cx.user_id, &cx.access, named, false).await?;
        let check = self.checked(cx, fields, input, true, true);
        match admitted {
            Ok(_) => workspace::within(&cx.app, &cx.uow, named, check).await,
            Err(_) => workspace::outside(&cx.app, &cx.uow, check).await,
        }
    }

    /// Whether the sidebar's workspace gives way to the one a form names: a header
    /// is the client's own word for the request.
    fn form_may_name_workspace(&self, cx: &Cx) -> bool {
        self.info.workspaces == Some(Workspaces::Every) && cx.named_by == Some(NamedBy::Cookie)
    }

    fn save_context(&self, cx: &Cx) -> SaveContext {
        let mut inner = cx.inner(self.info.module, Map::new());
        inner.form_may_name_workspace = self.form_may_name_workspace(cx);
        SaveContext::new(inner)
    }

    fn form(&self, cx: &Cx, values: Map<String, Json>) -> FormData {
        FormData::new(values)
            .with_app(cx.app.clone(), cx.tz_offset)
            .with_messages(self.messages(cx).clone())
    }

    /// The condition matching any of `columns` containing `term`, ignoring
    /// letter case.
    fn search_filter(&self, columns: &[&'static str], term: &str) -> navree_orm::Filter {
        navree_orm::Filter::any(
            columns
                .iter()
                .map(|column| self.col(column).contains_ignore_case(term)),
        )
    }

    /// The rows the list shows for `q`, before they are ordered: the user's
    /// rows narrowed by the relationship, search and filters, and the column the
    /// list sorts by (a leading `descending`). A 400 names what is wrong in `q`.
    /// The list and its export both start here.
    async fn filtered(
        &self,
        cx: &Cx,
        q: &ListQuery,
    ) -> Result<(Builder<R::Model>, String, bool), AppError> {
        let subject = <R::Model as Subject>::SUBJECT;
        if !cx.can(READ, subject, None) {
            return Err(AppError::forbidden());
        }
        self.in_a_workspace(cx)?;
        let mut query = self.base(cx, READ);
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Some((column, id)) = &q.via {
            let field = self
                .info
                .field(column)
                .filter(|f| matches!(f.kind, FieldKind::BelongsTo(_)));
            match (field, values::column::<R::Model>(column)) {
                (Some(_), Some(_)) => match key_from_str(self.shape(column), id) {
                    Some(key) => query = query.where_(self.col(column).eq(key)),
                    None => query = query.where_(nothing()),
                },
                _ => {
                    errors.insert(
                        format!("via.{column}"),
                        vec![format!(
                            "via.{column} is not a relationship of this resource"
                        )],
                    );
                }
            }
        }
        if let Some(term) = &q.search
            && !self.info.search.is_empty()
        {
            query = query.where_(self.search_filter(&self.info.search, term));
        }
        for (name, values) in &q.filters {
            match self.filters.iter().find(|f| f.name == *name) {
                Some(filter) => match self.apply_filter(cx, query.clone(), filter, values).await {
                    Ok(filtered) => query = filtered,
                    Err(message) => {
                        errors.insert(format!("filters.{name}"), vec![message]);
                    }
                },
                None => {
                    errors.insert(
                        format!("filters.{name}"),
                        vec![format!("filters.{name} is not a filter of this resource")],
                    );
                }
            }
        }
        let sort = q
            .sort
            .clone()
            .unwrap_or_else(|| self.info.default_sort.clone());
        let (descending, column_name) = match sort.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, sort.as_str()),
        };
        let sortable = self.info.sortable();
        if !sortable.contains(&column_name) {
            let allowed: Vec<String> = sortable
                .iter()
                .flat_map(|c| [c.to_string(), format!("-{c}")])
                .collect();
            errors.insert(
                "sort".into(),
                vec![
                    cx.core
                        .messages
                        .render("sort", "in", &[("values", allowed.join(", "))]),
                ],
            );
        }
        if !errors.is_empty() {
            return Err(AppError::validation(errors));
        }
        Ok((query, column_name.to_owned(), descending))
    }

    async fn list_page(&self, cx: &Cx, q: &ListQuery) -> Result<RawPage, AppError> {
        let (mut query, column_name, descending) = self.filtered(cx, q).await?;
        if values::column::<R::Model>(&column_name).is_some() {
            let direction = if descending {
                navree_orm::ir::Direction::Desc
            } else {
                navree_orm::ir::Direction::Asc
            };
            query = query.order_by_dir(values::qualified::<R::Model>(&column_name), direction);
            if column_name != self.info.pk {
                query = query.order_by_dir(values::qualified::<R::Model>(self.info.pk), direction);
            }
        }
        let per_page = q.per_page.unwrap_or(self.info.per_page);
        if q.capped {
            return self.capped_page(cx, query, q.page, per_page).await;
        }
        let page = query
            .paginate(&cx.uow, PageQuery::new(q.page, per_page))
            .await?;
        Ok(RawPage {
            records: page.data.iter().map(|r| self.raw(cx, r)).collect(),
            meta: page.meta,
            capped: false,
        })
    }

    /// A page of `query` (already ordered) for the list, with its rows counted
    /// only up to the resource's `COUNT_CAP`: the keys of at most that many rows
    /// and one more, and the page with one row more than it shows to know whether
    /// another follows. What the database does for a page and its count stops
    /// growing with the table.
    async fn capped_page(
        &self,
        cx: &Cx,
        query: Builder<R::Model>,
        page: u64,
        per_page: u64,
    ) -> Result<RawPage, AppError> {
        let cap = self.info.count_cap;
        let per_page = per_page.max(1);
        let page = page.max(1);
        let counted = query
            .clone()
            .reorder()
            .limit(cap.saturating_add(1))
            .pluck(&cx.uow, self.col(self.info.pk))
            .await?
            .len() as u64;
        let capped = counted > cap;
        let total = counted.min(cap);
        let offset = (page - 1).saturating_mul(per_page);
        let mut records = if !capped && offset >= total {
            Vec::new()
        } else {
            query
                .offset(offset)
                .limit(per_page.saturating_add(1))
                .get(&cx.uow)
                .await?
                .into_vec()
        };
        let more = records.len() as u64 > per_page;
        records.truncate(usize::try_from(per_page).unwrap_or(usize::MAX));
        let last_page = if capped {
            page + u64::from(more)
        } else {
            total.div_ceil(per_page).max(1)
        };
        Ok(RawPage {
            records: records.iter().map(|r| self.raw(cx, r)).collect(),
            meta: PageMeta {
                current_page: page,
                per_page,
                total,
                last_page,
            },
            capped,
        })
    }

    /// A chunk of the rows of the list for `q` after `cursor`, for an export:
    /// in the list's order when it is by a column that always has a value,
    /// else by key. No count and no offset, so a chunk costs the same however
    /// far into the table it is.
    async fn export_page(
        &self,
        cx: &Cx,
        q: &ListQuery,
        cursor: Option<String>,
        limit: u64,
    ) -> Result<ExportChunk, AppError> {
        if !self.info.export {
            return Err(AppError::forbidden()
                .with_code("export_off")
                .with_detail(format!("{} are not exported.", self.info.label)));
        }
        let (query, column_name, descending) = self.filtered(cx, q).await?;
        let by_column = values::column::<R::Model>(&column_name).is_some()
            && !self.info.nullable.contains(column_name.as_str());
        let query = if by_column {
            let direction = if descending {
                navree_orm::ir::Direction::Desc
            } else {
                navree_orm::ir::Direction::Asc
            };
            query.order_by_dir(values::qualified::<R::Model>(&column_name), direction)
        } else {
            query.order_by(values::qualified::<R::Model>(self.info.pk))
        };
        let request = navree_orm::pagination::CursorRequest {
            cursor,
            limit: Some(limit),
        };
        let page = query.cursor_paginate(&cx.uow, request).await?;
        Ok(ExportChunk {
            records: page.data.iter().map(|r| self.raw(cx, r)).collect(),
            next: page.next_cursor,
        })
    }

    /// How many rows the user may read, counted up to `cap` + 1: the keys of
    /// at most that many, so a table of millions costs what one of a thousand
    /// does.
    async fn count_up_to(&self, cx: &Cx, cap: u64) -> Result<u64, AppError> {
        if !cx.can(READ, <R::Model as Subject>::SUBJECT, None) {
            return Err(AppError::forbidden());
        }
        self.in_a_workspace(cx)?;
        let keys = self
            .base(cx, READ)
            .reorder()
            .limit(cap.saturating_add(1))
            .pluck(&cx.uow, self.col(self.info.pk))
            .await?;
        Ok(keys.len() as u64)
    }

    /// Applies `filter` with the submitted `values`; the message names what
    /// is wrong.
    async fn apply_filter(
        &self,
        cx: &Cx,
        query: Builder<R::Model>,
        filter: &Filter<R::Model>,
        values: &[String],
    ) -> Result<Builder<R::Model>, String> {
        let name = &filter.name;
        let shape = self.shape(name);
        match &filter.kind {
            FilterKind::Select => {
                if !filter.options.is_empty() {
                    let allowed: Vec<&str> =
                        filter.options.iter().map(|(v, _)| v.as_str()).collect();
                    if values.iter().any(|v| !allowed.contains(&v.as_str())) {
                        return Err(cx.core.messages.render(
                            &format!("filters.{name}"),
                            "in",
                            &[("values", allowed.join(", "))],
                        ));
                    }
                }
                let keys: Vec<Value> = values
                    .iter()
                    .map(|v| values::to_value(shape, &Json::String(v.clone())))
                    .collect::<Result<_, _>>()
                    .map_err(|Invalid(rule)| {
                        cx.core
                            .messages
                            .render(&format!("filters.{name}"), rule, &[])
                    })?;
                Ok(query.where_(self.col(name).is_in(keys)))
            }
            FilterKind::BelongsTo(_) => {
                let keys: Vec<Value> = values
                    .iter()
                    .filter_map(|v| key_from_str(shape, v))
                    .collect();
                if keys.is_empty() {
                    return Ok(query.where_(nothing()));
                }
                Ok(query.where_(self.col(name).is_in(keys)))
            }
            FilterKind::Boolean(f) => {
                let value = values
                    .last()
                    .map(|v| Json::String(v.clone()))
                    .unwrap_or(Json::Null);
                let on = values::boolean(&value).ok_or_else(|| {
                    cx.core
                        .messages
                        .render(&format!("filters.{name}"), "boolean", &[])
                })?;
                Ok(f(query, on))
            }
            FilterKind::DateRange => {
                let today = save::now(&cx.app).await.date_naive();
                let text = values.last().cloned().unwrap_or_default();
                let (from, to) = date_range(&text, today).ok_or_else(|| {
                    format!("filters.{name} must be a range of dates (YYYY-MM-DD..YYYY-MM-DD) or today, week, month or quarter")
                })?;
                let mut query = query;
                if let Some(from) = from {
                    query = query.where_(
                        self.col(name)
                            .gte(values::date_bound(shape, midnight(from))),
                    );
                }
                if let Some(to) = to {
                    let end = to + Duration::days(1);
                    query =
                        query.where_(self.col(name).lt(values::date_bound(shape, midnight(end))));
                }
                Ok(query)
            }
            FilterKind::Custom(f) => Ok(f(query, values.join(","))),
        }
    }

    /// The SQL aggregate of a metric.
    fn aggregate_expr(&self, aggregate: &Aggregate<String>) -> Expr {
        let column = |c: &str| Expr::col(values::qualified::<R::Model>(c));
        match aggregate {
            Aggregate::Count => Expr::count(column(self.info.pk)),
            Aggregate::Sum(c) => Expr::sum(column(c)),
            Aggregate::Average(c) => Expr::avg(column(c)),
            Aggregate::Min(c) => Expr::min(column(c)),
            Aggregate::Max(c) => Expr::max(column(c)),
        }
    }

    /// The aggregate over the rows of `query`, as a number (`None` when the
    /// database answers NULL).
    async fn aggregate(
        &self,
        cx: &Cx,
        query: Builder<R::Model>,
        aggregate: &Aggregate<String>,
    ) -> Result<Option<f64>, AppError> {
        let (function, column) = match aggregate {
            Aggregate::Count => return Ok(Some(query.count(&cx.uow).await? as f64)),
            Aggregate::Sum(c) => (AggregateFunction::Sum, c),
            Aggregate::Average(c) => (AggregateFunction::Avg, c),
            Aggregate::Min(c) => (AggregateFunction::Min, c),
            Aggregate::Max(c) => (AggregateFunction::Max, c),
        };
        let value = query
            .aggregate::<Value>(
                &cx.uow,
                function,
                Some(Expr::col(values::qualified::<R::Model>(column))),
                false,
            )
            .await?;
        Ok(value.as_ref().and_then(values::value_number))
    }

    fn display(&self, metric: &Metric<R::Model>, value: f64) -> String {
        match &metric.money {
            Some(currency) => money::format(value.round() as i64, currency),
            None => money::format_number(value),
        }
    }

    async fn compute(
        &self,
        cx: &Cx,
        metric: &Metric<R::Model>,
        range: MetricRange,
    ) -> Result<MetricResult, AppError> {
        let mut query = self.base(cx, READ).reorder();
        for condition in &metric.conditions {
            query = query.where_(condition.clone());
        }
        let now = save::now(&cx.app).await;
        let mut result = MetricResult {
            label: metric.label.clone(),
            slug: metric.slug.clone(),
            kind: metric.kind.name(),
            range: None,
            ranges: Vec::new(),
            value: None,
            previous: None,
            change: None,
            display: String::new(),
            currency: metric.money.clone(),
            points: Vec::new(),
            groups: Vec::new(),
        };
        match metric.kind {
            MetricKind::Value => {
                if let Some(over) = &metric.over {
                    let shape = self.shape(over);
                    let ((start, end), (previous_start, previous_end)) = range.periods(now);
                    let between = |from, to| {
                        query
                            .clone()
                            .where_(self.col(over).gte(values::date_bound(shape, from)))
                            .where_(self.col(over).lt(values::date_bound(shape, to)))
                    };
                    result.value = self
                        .aggregate(cx, between(start, end), &metric.aggregate)
                        .await?;
                    result.previous = self
                        .aggregate(cx, between(previous_start, previous_end), &metric.aggregate)
                        .await?;
                    result.range = Some(range.as_str());
                    result.ranges = MetricRange::ALL
                        .iter()
                        .map(|r| RangeOption {
                            value: r.as_str(),
                            label: r.label(),
                        })
                        .collect();
                } else {
                    result.value = self.aggregate(cx, query, &metric.aggregate).await?;
                }
                if matches!(metric.aggregate, Aggregate::Count | Aggregate::Sum(_)) {
                    result.value = Some(result.value.unwrap_or(0.0));
                    if metric.over.is_some() {
                        result.previous = Some(result.previous.unwrap_or(0.0));
                    }
                }
                result.change = match (result.value, result.previous) {
                    (Some(value), Some(previous)) if previous != 0.0 => {
                        Some((value - previous) / previous.abs())
                    }
                    _ => None,
                };
                result.display = result
                    .value
                    .map(|v| self.display(metric, v))
                    .unwrap_or_else(|| "—".to_owned());
            }
            MetricKind::Trend => {
                let Some((column, interval, count)) = &metric.trend else {
                    return Err(AppError::internal("a trend metric without a period"));
                };
                let shape = self.shape(column);
                let today = now.date_naive();
                let buckets = buckets(*interval, *count, today);
                let start = buckets.first().map(|b| b.1).unwrap_or(today);
                let in_window = query.where_(
                    self.col(column)
                        .gte(values::date_bound(shape, midnight(start))),
                );
                let mut grouped: Vec<Vec<f64>> = vec![Vec::new(); buckets.len()];
                match metric.aggregate.column() {
                    // A count reads the one column it buckets by, not the records
                    // (a table of a few hundred thousand rows, with their JSON
                    // columns, took seconds to load for a line of thirty points).
                    None => {
                        let days = in_window.pluck(&cx.uow, self.col(column)).await?;
                        for day in days.iter().filter_map(values::value_date) {
                            if let Some(index) =
                                buckets.iter().position(|b| b.1 <= day && day < b.2)
                            {
                                grouped[index].push(1.0);
                            }
                        }
                    }
                    // Another aggregate is asked of the database a bucket at a time.
                    Some(_) => {
                        for (index, bucket) in buckets.iter().enumerate() {
                            let within = in_window
                                .clone()
                                .where_(
                                    self.col(column)
                                        .gte(values::date_bound(shape, midnight(bucket.1))),
                                )
                                .where_(
                                    self.col(column)
                                        .lt(values::date_bound(shape, midnight(bucket.2))),
                                );
                            if let Some(value) =
                                self.aggregate(cx, within, &metric.aggregate).await?
                            {
                                grouped[index].push(value);
                            }
                        }
                    }
                }
                result.points = buckets
                    .iter()
                    .zip(grouped)
                    .map(|((label, _, _), values)| {
                        let value = metric.aggregate.fold(&values).unwrap_or(0.0);
                        Point {
                            key: label.clone(),
                            label: label.clone(),
                            value,
                            display: self.display(metric, value),
                        }
                    })
                    .collect();
                let total: f64 = result.points.iter().map(|p| p.value).sum();
                result.value = Some(total);
                result.display = self.display(metric, total);
            }
            MetricKind::Partition => {
                let Some(column) = &metric.group_by else {
                    return Err(AppError::internal("a partition metric without a column"));
                };
                let key = GroupKey {
                    expr: Expr::col(values::qualified::<R::Model>(column)),
                    shape: self.shape(column),
                };
                let rows = query
                    .group_by([values::qualified::<R::Model>(column)])
                    .pluck_keyed(&cx.uow, self.aggregate_expr(&metric.aggregate), key)
                    .await?;
                let mut groups: Vec<(String, f64)> = rows
                    .into_iter()
                    .map(|(key, value)| (key, values::value_number(&value).unwrap_or(0.0)))
                    .collect();
                groups.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                let labels = self.group_labels(cx, column, &groups).await?;
                result.groups = groups
                    .into_iter()
                    .map(|(key, value)| Point {
                        label: labels.get(&key).cloned().unwrap_or_else(|| {
                            if key.is_empty() {
                                "None".to_owned()
                            } else {
                                key.clone()
                            }
                        }),
                        key,
                        value,
                        display: self.display(metric, value),
                    })
                    .collect();
                let total: f64 = result.groups.iter().map(|g| g.value).sum();
                result.value = Some(total);
                result.display = self.display(metric, total);
            }
        }
        Ok(result)
    }

    /// Labels for partition groups: a select field's option labels, or the
    /// titles of related records.
    async fn group_labels(
        &self,
        cx: &Cx,
        column: &str,
        groups: &[(String, f64)],
    ) -> Result<HashMap<String, String>, AppError> {
        let Some(field) = self.info.field(column) else {
            return Ok(HashMap::new());
        };
        match &field.kind {
            FieldKind::Select => Ok(field.options.iter().cloned().collect()),
            FieldKind::BelongsTo(related) => {
                let ids: Vec<String> = groups
                    .iter()
                    .map(|g| g.0.clone())
                    .filter(|k| !k.is_empty())
                    .collect();
                match cx.core.registry.by_type(related.type_id) {
                    Some(handle) if !ids.is_empty() => Ok(handle
                        .lookup(cx, None, Some(&ids), ids.len() as u64)
                        .await?
                        .into_iter()
                        .collect()),
                    _ => Ok(HashMap::new()),
                }
            }
            _ => Ok(HashMap::new()),
        }
    }

    fn raw_id(&self, record: &RecordOf<R>) -> String {
        record_json(record)
            .get(self.info.pk)
            .and_then(key_text)
            .unwrap_or_default()
    }
}

/// `base` with each of `fields` named by its label (`Project should not be
/// empty`, not `project_id should not be empty`), unless the app named it
/// itself.
fn labelled(base: &Messages, fields: &[Field]) -> Messages {
    let mut messages = base.clone();
    for field in fields {
        if messages.attribute(&field.name) == field.name {
            messages = messages.with_attribute(field.name.clone(), field.label_text());
        }
    }
    messages
}

/// What names a record on screen: its `TITLE` column, or, where there is
/// none (the key is the default title, or the column is empty), the
/// resource's singular name and the end of its key (`Report #3f9a1c2e`),
/// never a whole UUID.
pub(crate) fn title_of(info: &ResourceInfo, json: &Json, id: &str) -> String {
    let named = match json.get(info.title) {
        None | Some(Json::Null) => None,
        Some(value) => Some(key_text(value).unwrap_or_else(|| value.to_string())),
    }
    .filter(|text| !text.trim().is_empty());
    match named {
        Some(text) if info.title != info.pk => text,
        _ => format!("{} #{}", info.singular, short_key(id)),
    }
}

/// The last eight characters of a long key (the random end of a UUID v7,
/// where the start is a timestamp), a short one whole.
pub(crate) fn short_key(id: &str) -> &str {
    if id.chars().count() > 12 {
        let start = id.char_indices().rev().nth(7).map_or(0, |(at, _)| at);
        &id[start..]
    } else {
        id
    }
}

/// A condition no row matches.
fn nothing() -> navree_orm::Filter {
    navree_orm::Filter::raw("1 = 0", Vec::<Value>::new())
}

/// The grouped column of a partition, read with the Rust type of the
/// column and spelled as record JSON spells it (`""` for NULL), so the keys
/// match related records' ids.
struct GroupKey {
    expr: Expr,
    shape: Shape,
}

impl Selectable for GroupKey {
    type Output = String;

    fn selected(&self) -> Expr {
        self.expr.clone()
    }

    fn read(&self, row: &Row, column: &str) -> Result<String, FieldError> {
        Ok(group_key(row, column, self.shape))
    }
}

/// The `column` of a partition row as text (see [`GroupKey`]).
fn group_key(row: &Row, column: &str, shape: Shape) -> String {
    let text = match shape {
        Shape::Uuid => row
            .get::<Option<uuid::Uuid>>(column)
            .ok()
            .flatten()
            .map(|u| u.hyphenated().to_string()),
        Shape::Bool => row
            .get::<Option<bool>>(column)
            .ok()
            .flatten()
            .map(|b| b.to_string()),
        Shape::Int { .. } => row
            .get::<Option<i64>>(column)
            .ok()
            .flatten()
            .map(|n| n.to_string()),
        Shape::Float => row
            .get::<Option<f64>>(column)
            .ok()
            .flatten()
            .map(|n| n.to_string()),
        Shape::Date => row
            .get::<Option<NaiveDate>>(column)
            .ok()
            .flatten()
            .map(|d| d.format("%Y-%m-%d").to_string()),
        _ => row
            .get::<Option<String>>(column)
            .ok()
            .flatten()
            .or_else(|| {
                row.value(column)
                    .ok()
                    .filter(|v| !matches!(v, Value::Null(_)))
                    .and_then(|v| key_text(&navree_orm::to_json(&v)))
            }),
    };
    text.unwrap_or_default()
}

/// `from..to` (either end optional) or a preset, relative to `today`.
pub(crate) fn date_range(
    text: &str,
    today: NaiveDate,
) -> Option<(Option<NaiveDate>, Option<NaiveDate>)> {
    let text = text.trim();
    let parse = |s: &str| -> Option<Option<NaiveDate>> {
        let s = s.trim();
        if s.is_empty() {
            Some(None)
        } else {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().map(Some)
        }
    };
    let first_of = |year: i32, month: u32| NaiveDate::from_ymd_opt(year, month, 1);
    match text {
        "today" => Some((Some(today), Some(today))),
        "week" => {
            let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
            Some((Some(monday), Some(monday + Duration::days(6))))
        }
        "month" => {
            let start = first_of(today.year(), today.month())?;
            let next = if today.month() == 12 {
                first_of(today.year() + 1, 1)?
            } else {
                first_of(today.year(), today.month() + 1)?
            };
            Some((Some(start), Some(next - Duration::days(1))))
        }
        "quarter" => {
            let first_month = (today.month() - 1) / 3 * 3 + 1;
            let start = first_of(today.year(), first_month)?;
            let next = if first_month == 10 {
                first_of(today.year() + 1, 1)?
            } else {
                first_of(today.year(), first_month + 3)?
            };
            Some((Some(start), Some(next - Duration::days(1))))
        }
        _ => {
            let (from, to) = text.split_once("..")?;
            let range = (parse(from)?, parse(to)?);
            (range.0.is_some() || range.1.is_some()).then_some(range)
        }
    }
}

impl<R: Resource> ErasedResource for Handle<R> {
    fn info(&self) -> &ResourceInfo {
        &self.info
    }

    fn list<'a>(
        &'a self,
        cx: &'a Cx,
        q: &'a ListQuery,
    ) -> BoxFuture<'a, Result<RawPage, AppError>> {
        Box::pin(self.list_page(cx, q))
    }

    fn export<'a>(
        &'a self,
        cx: &'a Cx,
        q: &'a ListQuery,
        cursor: Option<String>,
        limit: u64,
    ) -> BoxFuture<'a, Result<ExportChunk, AppError>> {
        Box::pin(self.export_page(cx, q, cursor, limit))
    }

    fn count<'a>(&'a self, cx: &'a Cx, cap: u64) -> BoxFuture<'a, Result<u64, AppError>> {
        Box::pin(self.count_up_to(cx, cap))
    }

    fn find<'a>(
        &'a self,
        cx: &'a Cx,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<RawRecord>, AppError>> {
        Box::pin(async move {
            if !cx.can(READ, <R::Model as Subject>::SUBJECT, None) {
                return Err(AppError::forbidden());
            }
            self.in_a_workspace(cx)?;
            Ok(self.load(cx, id, false).await?.map(|r| self.raw(cx, &r)))
        })
    }

    fn create<'a>(
        &'a self,
        cx: &'a Cx,
        input: Map<String, Json>,
    ) -> BoxFuture<'a, Result<RawRecord, AppError>> {
        Box::pin(async move {
            self.writable()?;
            let subject = <R::Model as Subject>::SUBJECT;
            if !cx.can(CREATE, subject, None) {
                return Err(AppError::forbidden());
            }
            self.in_a_workspace(cx)?;
            if self.info.workspaces.is_some() {
                let prefix = <R::Model as Subject>::PERMISSION_PREFIX;
                cx.role_writes(self.named_by_form(&input), prefix, CREATE)
                    .await?;
            }
            let editable: Vec<Field> = self
                .info
                .fields
                .iter()
                .filter(|f| f.editable())
                .cloned()
                .collect();
            let values = self.checked_for_form(cx, &editable, input).await?;
            let submitted: Vec<String> = values.keys().cloned().collect();
            let record = self
                .resource
                .create(&self.save_context(cx), self.form(cx, values))
                .await?;
            let json = record_json(&record);
            if !cx.can(CREATE, subject, Some(&json)) {
                return Err(AppError::forbidden().with_detail(format!(
                    "You may not create this {}.",
                    self.info.singular.to_lowercase()
                )));
            }
            let denied: Vec<&str> = submitted
                .iter()
                .map(String::as_str)
                .filter(|f| !cx.ability().can_field(CREATE, subject, Some(&json), f))
                .collect();
            if !denied.is_empty() {
                return Err(AppError::forbidden().with_detail(format!(
                    "You may not set these fields: {}.",
                    denied.join(", ")
                )));
            }
            Ok(self.raw(cx, &record))
        })
    }

    fn update<'a>(
        &'a self,
        cx: &'a Cx,
        id: &'a str,
        input: Map<String, Json>,
    ) -> BoxFuture<'a, Result<RawRecord, AppError>> {
        Box::pin(async move {
            self.writable()?;
            let subject = <R::Model as Subject>::SUBJECT;
            if !cx.can(READ, subject, None) {
                return Err(AppError::forbidden());
            }
            self.in_a_workspace(cx)?;
            let Some(record) = self.load(cx, id, true).await? else {
                return Err(AppError::not_found());
            };
            let json = record_json(&record);
            if !cx.can(UPDATE, subject, Some(&json)) {
                return Err(AppError::forbidden());
            }
            self.record_writes(cx, &record, UPDATE).await?;
            let denied: Vec<&str> = input
                .keys()
                .map(String::as_str)
                .filter(|key| self.info.field(key).is_some_and(Field::editable))
                .filter(|key| !cx.ability().can_field(UPDATE, subject, Some(&json), key))
                .collect();
            if !denied.is_empty() {
                return Err(AppError::forbidden().with_detail(format!(
                    "You may not change these fields: {}.",
                    denied.join(", ")
                )));
            }
            let editable: Vec<Field> = self
                .info
                .fields
                .iter()
                .filter(|f| f.editable())
                .cloned()
                .collect();
            // A record across workspaces refers to records of its own.
            let owner = self
                .owner_of(&record)?
                .filter(|_| self.info.workspaces == Some(Workspaces::Every));
            let values = match owner {
                Some(owner) if cx.uow.workspace() != Some(owner) => {
                    workspace::within(
                        &cx.app,
                        &cx.uow,
                        owner,
                        self.checked(cx, &editable, input, false, true),
                    )
                    .await?
                }
                _ => self.checked(cx, &editable, input, false, true).await?,
            };
            let record = self
                .resource
                .update(&self.save_context(cx), record, self.form(cx, values))
                .await?;
            Ok(self.raw(cx, &record))
        })
    }

    fn delete<'a>(&'a self, cx: &'a Cx, id: &'a str) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            self.writable()?;
            let subject = <R::Model as Subject>::SUBJECT;
            if !cx.can(READ, subject, None) {
                return Err(AppError::forbidden());
            }
            self.in_a_workspace(cx)?;
            let Some(record) = self.load(cx, id, true).await? else {
                return Err(AppError::not_found());
            };
            if !cx.can(DELETE, subject, Some(&record_json(&record))) {
                return Err(AppError::forbidden());
            }
            self.record_writes(cx, &record, DELETE).await?;
            self.resource.delete(&self.save_context(cx), record).await
        })
    }

    fn run_action<'a>(
        &'a self,
        cx: &'a Cx,
        name: &'a str,
        ids: Vec<String>,
        input: Map<String, Json>,
        from_job: bool,
    ) -> BoxFuture<'a, Result<ActionOutcome, AppError>> {
        Box::pin(async move {
            let subject = <R::Model as Subject>::SUBJECT;
            let Some(action) = self.actions.iter().find(|a| a.name == name) else {
                return Err(AppError::not_found().with_detail(format!("No action `{name}`.")));
            };
            self.in_a_workspace(cx)?;
            let mut unique = Vec::new();
            for id in ids {
                if !unique.contains(&id) {
                    unique.push(id);
                }
            }
            if unique.is_empty() {
                let mut errors = BTreeMap::new();
                errors.insert(
                    "ids".to_owned(),
                    vec![cx.core.messages.render("ids", "required", &[])],
                );
                return Err(AppError::validation(errors));
            }
            let keys: Vec<Value> = unique
                .iter()
                .filter_map(|id| key_from_str(self.shape(self.info.pk), id))
                .collect();
            let records: Vec<RecordOf<R>> = if keys.is_empty() {
                Vec::new()
            } else {
                let mut query = self
                    .base(cx, READ)
                    .where_(self.col(self.info.pk).is_in(keys));
                if cx.uow.lock_for_update() {
                    query = query.lock_for_update();
                }
                query.get(&cx.uow).await?.into_vec()
            };
            let permitted: Vec<RecordOf<R>> = records
                .into_iter()
                .filter(|r| cx.can(action.ability, subject, Some(&record_json(r))))
                .collect();
            let skipped = unique.len().saturating_sub(permitted.len());
            if permitted.is_empty() {
                return Err(AppError::forbidden()
                    .with_detail("You may not run this action on the selected records."));
            }
            for record in &permitted {
                self.record_writes(cx, record, action.ability).await?;
            }
            let input = self.checked(cx, &action.fields, input, true, false).await?;
            let permitted_ids: Vec<String> = permitted.iter().map(|r| self.raw_id(r)).collect();
            let dispatcher = if action.queued && !from_job {
                cx.app.try_get::<dyn Dispatcher>().await
            } else {
                None
            };
            let (result, queued) = match dispatcher {
                Some(dispatcher) => {
                    // The job asks again who may work in the workspace, and can't
                    // ask the app that entered it: what it would find is said now.
                    if let (None, Some(entered)) = (cx.named_by, cx.uow.workspace()) {
                        workspace::admission(
                            &cx.app, &cx.core, cx.user_id, &cx.access, entered, true,
                        )
                        .await?
                        .map_err(Refused::error)?;
                    }
                    let job = HelmActionJob {
                        resource: self.info.slug.clone(),
                        action: action.name.clone(),
                        ids: permitted_ids.clone(),
                        input,
                        user_id: cx.user_id,
                        workspace: cx.uow.workspace(),
                    };
                    dispatcher.dispatch(job).await.map_err(AppError::internal)?;
                    let message = format!(
                        "{} is running in the background on {} {}.",
                        action.label,
                        permitted_ids.len(),
                        if permitted_ids.len() == 1 {
                            "record"
                        } else {
                            "records"
                        }
                    );
                    (ActionResult::message(message), true)
                }
                None => {
                    let handler = action
                        .handler
                        .clone()
                        .ok_or_else(|| AppError::internal("an action without a handler"))?;
                    let ctx = ActionContext::new(cx.inner(self.info.module, input));
                    let result = handler(ctx, permitted).await?;
                    cx.audit(action_entry(
                        &self.info.slug,
                        subject,
                        &action.name,
                        &permitted_ids,
                        from_job,
                    ))
                    .await;
                    (result, false)
                }
            };
            Ok(ActionOutcome {
                result,
                ran: permitted_ids.len(),
                skipped,
                queued,
            })
        })
    }

    fn metric<'a>(
        &'a self,
        cx: &'a Cx,
        slug: &'a str,
        range: MetricRange,
    ) -> BoxFuture<'a, Result<MetricResult, AppError>> {
        Box::pin(async move {
            let Some(metric) = self.metrics.iter().find(|m| m.slug == slug) else {
                return Err(AppError::not_found().with_detail(format!("No metric `{slug}`.")));
            };
            if !cx.can(READ, <R::Model as Subject>::SUBJECT, None) {
                return Err(AppError::forbidden());
            }
            self.in_a_workspace(cx)?;
            let key = format!(
                "{}|{}|{}|{}|{}|{}",
                cx.user_id,
                cx.access.version,
                cx.uow.workspace().unwrap_or_default(),
                self.info.slug,
                metric.slug,
                if metric.over.is_some() {
                    range.as_str()
                } else {
                    "-"
                }
            );
            if let Some(cache) = &cx.core.metrics
                && let Some(hit) = cache.get(&key)
            {
                return Ok((*hit).clone());
            }
            let result = self.compute(cx, metric, range).await?;
            if let Some(cache) = &cx.core.metrics {
                cache.insert(key, Arc::new(result.clone()));
            }
            Ok(result)
        })
    }

    fn lookup<'a>(
        &'a self,
        cx: &'a Cx,
        search: Option<&'a str>,
        ids: Option<&'a [String]>,
        limit: u64,
    ) -> BoxFuture<'a, Result<Vec<(String, String)>, AppError>> {
        Box::pin(async move {
            if !cx.can(READ, <R::Model as Subject>::SUBJECT, None) {
                return Ok(Vec::new());
            }
            // A picker of records of a workspace has none to offer without one.
            if self.in_a_workspace(cx).is_err() {
                return Ok(Vec::new());
            }
            let mut query = self.base(cx, READ).reorder();
            if let Some(ids) = ids {
                let keys: Vec<Value> = ids
                    .iter()
                    .filter_map(|id| key_from_str(self.shape(self.info.pk), id))
                    .collect();
                if keys.is_empty() {
                    return Ok(Vec::new());
                }
                query = query.where_(self.col(self.info.pk).is_in(keys));
            }
            if let Some(term) = search.map(str::trim).filter(|t| !t.is_empty()) {
                let columns: Vec<&'static str> = if self.info.search.is_empty() {
                    vec![self.info.title]
                } else {
                    self.info.search.clone()
                };
                query = query.where_(self.search_filter(&columns, term));
            }
            let records = query
                .order_by(values::qualified::<R::Model>(self.info.title))
                .order_by(values::qualified::<R::Model>(self.info.pk))
                .limit(limit)
                .get(&cx.uow)
                .await?
                .into_vec();
            Ok(records
                .iter()
                .map(|r| {
                    let raw = self.raw(cx, r);
                    (raw.id, raw.title)
                })
                .collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_ranges_and_presets() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap();
        let d = |y, m, d| NaiveDate::from_ymd_opt(y, m, d);
        assert_eq!(date_range("today", today), Some((Some(today), Some(today))));
        assert_eq!(
            date_range("week", today),
            Some((d(2026, 10, 26), d(2026, 11, 1)))
        );
        assert_eq!(
            date_range("month", today),
            Some((d(2026, 10, 1), d(2026, 10, 31)))
        );
        assert_eq!(
            date_range("quarter", today),
            Some((d(2026, 10, 1), d(2026, 12, 31)))
        );
        assert_eq!(
            date_range("2026-01-01..2026-01-31", today),
            Some((d(2026, 1, 1), d(2026, 1, 31)))
        );
        assert_eq!(
            date_range("2026-01-01..", today),
            Some((d(2026, 1, 1), None))
        );
        assert_eq!(date_range("..", today), None);
        assert_eq!(date_range("soon", today), None);
    }
}

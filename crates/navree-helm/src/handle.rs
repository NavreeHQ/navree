//! [`Handle`]: a [`Resource`] behind the object-safe [`ErasedResource`],
//! which the routes, the presenter and the queued actions use.

use std::any::TypeId;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::{Datelike, Duration, NaiveDate};
use navree_access::{CREATE, DELETE, EntityColumns, READ, Subject, UPDATE, record_json};
use navree_contracts::Dispatcher;
use navree_db::{PageMeta, PageQuery, contains_ignore_case, paginate};
use navree_http::AppError;
use navree_kernel::{BoxFuture, ModuleId};
use regex::Regex;
use sea_orm::sea_query::{Alias, Expr, Func, SimpleExpr, Value as SqlValue};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbErr, EntityName, EntityTrait, IdenStatic,
    ModelTrait, Order, QueryFilter, QueryOrder, QueryResult, QuerySelect, QueryTrait, Select,
};
use serde_json::{Map, Value, json};

use crate::action::Action;
use crate::cx::{Cx, action_entry};
use crate::field::{Computed, Field, FieldKind, Related};
use crate::filter::{Filter, FilterKind};
use crate::job::HelmActionJob;
use crate::metric::{
    Aggregate, Metric, MetricKind, MetricRange, MetricResult, Point, RangeOption, buckets, midnight,
};
use crate::query::ListQuery;
use crate::values::{self, Invalid, is_blank, is_sql_null, key_from_str, key_text};
use crate::{ActionContext, ActionResult, FormData, HelmOptions, ModelOf, Resource, SaveContext};
use crate::{money, save};

type Col<R> = <<R as Resource>::Entity as EntityTrait>::Column;

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
}

impl ResourceInfo {
    /// The field named `name`.
    pub(crate) fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
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

/// A filter, without its entity type.
#[derive(Clone, Debug)]
pub(crate) struct FilterInfo {
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) kind: &'static str,
    pub(crate) options: Vec<(String, String)>,
    pub(crate) related: Option<Related>,
}

/// An action, without its entity type.
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

/// A metric, without its entity type.
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
    pub(crate) json: Value,
    pub(crate) computed: HashMap<String, String>,
    pub(crate) html: HashMap<String, String>,
}

/// One page of a list.
#[derive(Clone, Debug)]
pub(crate) struct RawPage {
    pub(crate) records: Vec<RawRecord>,
    pub(crate) meta: PageMeta,
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

    /// The record `id`, if the user may read it.
    fn find<'a>(
        &'a self,
        cx: &'a Cx,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<RawRecord>, AppError>>;

    fn create<'a>(
        &'a self,
        cx: &'a Cx,
        input: Map<String, Value>,
    ) -> BoxFuture<'a, Result<RawRecord, AppError>>;

    fn update<'a>(
        &'a self,
        cx: &'a Cx,
        id: &'a str,
        input: Map<String, Value>,
    ) -> BoxFuture<'a, Result<RawRecord, AppError>>;

    fn delete<'a>(&'a self, cx: &'a Cx, id: &'a str) -> BoxFuture<'a, Result<(), AppError>>;

    /// Runs action `name` on the records `ids` the user may act on
    /// (`from_job`: inside the queued `helm.action` job).
    fn run_action<'a>(
        &'a self,
        cx: &'a Cx,
        name: &'a str,
        ids: Vec<String>,
        input: Map<String, Value>,
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
    filters: Vec<Filter<R::Entity>>,
    actions: Vec<Action<R::Entity>>,
    metrics: Vec<Metric<R::Entity>>,
    templates: HashMap<&'static str, SqlValue>,
    pk: Col<R>,
    title: Col<R>,
    search: Vec<Col<R>>,
    computed: Vec<(String, Computed)>,
    views: Vec<(String, String)>,
}

impl<R: Resource> Handle<R> {
    /// Checks `resource`'s definition against its entity; the error names
    /// the resource and what is wrong.
    pub(crate) fn build(
        resource: Arc<R>,
        module: ModuleId,
        options: &HelmOptions,
    ) -> Result<Self, String> {
        let name = std::any::type_name::<R>();
        let table = <R::Entity as Default>::default().table_name();
        let fail = |what: String| format!("Helm resource `{name}`: {what}");
        let pk = values::primary_key::<R::Entity>().map_err(fail)?;
        let templates = values::templates::<R::Entity>();
        let column = |column: &str| values::column::<R::Entity>(column);
        let mut nullable = HashSet::new();
        for c in values::column_names::<R::Entity>() {
            if column(c).is_some_and(|col| col.def().is_null()) {
                nullable.insert(c);
            }
        }

        let mut fields = resource.fields();
        let mut seen = HashSet::new();
        let mut patterns = HashMap::new();
        let mut computed = Vec::new();
        let mut views = Vec::new();
        for field in &mut fields {
            if matches!(field.kind, FieldKind::Id) {
                field.name = pk.as_str().to_owned();
            }
            if !seen.insert(field.name.clone()) {
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
                    if c.model != TypeId::of::<ModelOf<R>>() {
                        return Err(fail(format!(
                            "computed field `{}` takes `{}`, not the resource's model `{}`",
                            field.name,
                            c.model_name,
                            std::any::type_name::<ModelOf<R>>()
                        )));
                    }
                    computed.push((field.name.clone(), c.clone()));
                }
                FieldKind::View { template } => views.push((field.name.clone(), template.clone())),
                FieldKind::Date | FieldKind::DateTime
                    if !values::is_temporal(&templates[field.name.as_str()])
                        && !matches!(templates[field.name.as_str()], SqlValue::String(_)) =>
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
            if matches!(filter.kind, FilterKind::DateRange)
                && !values::is_temporal(&templates[filter.name.as_str()])
            {
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
            let temporal = |c: &Col<R>| values::is_temporal(&templates[c.as_str()]);
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
                format!("-{}", pk.as_str())
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
            subject: <R::Entity as Subject>::SUBJECT,
            pk: pk.as_str(),
            title: title.as_str(),
            search: search.iter().map(|c| c.as_str()).collect(),
            per_page,
            default_sort,
            fields,
            filters: filter_infos,
            actions: action_infos,
            metrics: metric_infos,
            patterns,
            nullable,
        };
        Ok(Self {
            resource,
            info,
            filters,
            actions,
            metrics,
            templates,
            pk,
            title,
            search,
            computed,
            views,
        })
    }

    /// The rows the user may do `action` on, starting from the resource's
    /// query.
    fn base(&self, cx: &Cx, action: &str) -> Select<R::Entity> {
        // SeaORM shim of navree-access (until navree-helm is moved).
        let condition = cx.ability().sql_filter_with(
            action,
            <R::Entity as Subject>::SUBJECT,
            &EntityColumns::<R::Entity>::new(),
        );
        self.resource
            .query(<R::Entity as EntityTrait>::find())
            .filter(condition.to_sea_query())
    }

    fn template(&self, column: &str) -> &SqlValue {
        self.templates
            .get(column)
            .unwrap_or(&SqlValue::String(None))
    }

    fn raw(&self, cx: &Cx, model: &ModelOf<R>) -> RawRecord {
        let json = record_json(model);
        let id = json
            .get(self.info.pk)
            .and_then(key_text)
            .unwrap_or_default();
        let title = match json.get(self.title.as_str()) {
            None | Some(Value::Null) => format!("#{id}"),
            Some(value) => key_text(value).unwrap_or_else(|| value.to_string()),
        };
        let computed = self
            .computed
            .iter()
            .filter_map(|(name, c)| c.call::<ModelOf<R>>(model).map(|v| (name.clone(), v)))
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
    async fn load(&self, cx: &Cx, id: &str, lock: bool) -> Result<Option<ModelOf<R>>, AppError> {
        let Some(key) = key_from_str(self.template(self.info.pk), id) else {
            return Ok(None);
        };
        let mut select = self.base(cx, READ).filter(self.pk.eq(key));
        if lock && cx.uow.lock_for_update() {
            select = select.lock_exclusive();
        }
        let tx = cx.uow.tx().await?;
        Ok(select.one(&*tx).await?)
    }

    /// Converts form input to its JSON form for `field` (numbers from text,
    /// booleans from `on`, date-times to RFC 3339 in UTC, blanks to `null`
    /// for nullable columns).
    fn normalize(&self, field: &Field, value: Value, tz_offset: Option<i32>) -> Value {
        let value = match (&field.kind, value) {
            (FieldKind::Number | FieldKind::Money { .. }, Value::String(text))
                if !text.trim().is_empty() =>
            {
                let text = text.trim();
                if let Ok(n) = text.parse::<i64>() {
                    Value::from(n)
                } else if let Some(n) = text
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                {
                    Value::Number(n)
                } else {
                    Value::String(text.to_owned())
                }
            }
            (FieldKind::Boolean, value) => {
                values::boolean(&value).map(Value::Bool).unwrap_or(value)
            }
            (FieldKind::DateTime, Value::String(text)) if !text.trim().is_empty() => {
                match values::datetime(&Value::String(text.clone()), tz_offset) {
                    Some(at) => {
                        Value::String(at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
                    }
                    None => Value::String(text),
                }
            }
            (FieldKind::Email | FieldKind::Url, Value::String(text)) => {
                Value::String(text.trim().to_owned())
            }
            (_, value) => value,
        };
        if is_blank(&value) && self.info.nullable.contains(field.name.as_str()) {
            Value::Null
        } else {
            value
        }
    }

    /// Whether `value` is the path of a file Helm stored for this resource
    /// (`{slug}/{40 letters and digits}.{extension}`): a file field never
    /// points at another file of its disk.
    fn stored_path(&self, value: &Value) -> bool {
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
        input: Map<String, Value>,
        create: bool,
        columns: bool,
    ) -> Result<Map<String, Value>, AppError> {
        let messages = &cx.core.messages;
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut checked = Map::new();
        for (key, value) in input {
            let Some(field) = fields.iter().find(|f| f.name == key) else {
                errors.entry(key.clone()).or_default().push(messages.render(
                    &key,
                    "prohibited",
                    &[],
                ));
                continue;
            };
            let value = self.normalize(field, value, cx.tz_offset);
            let mut problems = Vec::new();
            let pattern = match &field.pattern {
                Some(source) if columns => self.info.patterns.get(&key).cloned(),
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
                match values::to_sql(self.template(&key), &value, None) {
                    Err(Invalid(rule)) => problems.push(messages.render(&key, rule, &[])),
                    Ok(sql) if is_sql_null(&sql) && !self.info.nullable.contains(key.as_str()) => {
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

    fn save_context(&self, cx: &Cx) -> SaveContext {
        SaveContext::new(cx.inner(self.info.module, Map::new()))
    }

    fn form(&self, cx: &Cx, values: Map<String, Value>) -> FormData {
        FormData::new(values).with_app(cx.app.clone(), cx.tz_offset)
    }

    async fn list_page(&self, cx: &Cx, q: &ListQuery) -> Result<RawPage, AppError> {
        let subject = <R::Entity as Subject>::SUBJECT;
        if !cx.can(READ, subject, None) {
            return Err(AppError::forbidden());
        }
        let mut select = self.base(cx, READ);
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Some((column, id)) = &q.via {
            let field = self
                .info
                .field(column)
                .filter(|f| matches!(f.kind, FieldKind::BelongsTo(_)));
            match (field, values::column::<R::Entity>(column)) {
                (Some(_), Some(col)) => match key_from_str(self.template(column), id) {
                    Some(key) => select = select.filter(col.eq(key)),
                    None => select = select.filter(Expr::cust("1 = 0")),
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
            && !self.search.is_empty()
        {
            let backend = cx.uow.db().backend();
            let mut any = Condition::any();
            for column in &self.search {
                any = any.add(contains_ignore_case(
                    backend,
                    Expr::col((<R::Entity as Default>::default(), *column)),
                    term,
                ));
            }
            select = select.filter(any);
        }
        for (name, values) in &q.filters {
            match self.filters.iter().find(|f| f.name == *name) {
                Some(filter) => match self.apply_filter(cx, select.clone(), filter, values).await {
                    Ok(filtered) => select = filtered,
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
        if let Some(column) = values::column::<R::Entity>(column_name) {
            let order = if descending { Order::Desc } else { Order::Asc };
            select = select.order_by(column, order.clone());
            if column_name != self.info.pk {
                select = select.order_by(self.pk, order);
            }
        }
        let per_page = q.per_page.unwrap_or(self.info.per_page);
        let page = {
            let tx = cx.uow.tx().await?;
            paginate(select, &*tx, PageQuery::new(q.page, per_page)).await?
        };
        Ok(RawPage {
            records: page.data.iter().map(|m| self.raw(cx, m)).collect(),
            meta: page.meta,
        })
    }

    /// Applies `filter` with the submitted `values`; the message names what
    /// is wrong.
    async fn apply_filter(
        &self,
        cx: &Cx,
        select: Select<R::Entity>,
        filter: &Filter<R::Entity>,
        values: &[String],
    ) -> Result<Select<R::Entity>, String> {
        let name = &filter.name;
        let column = || values::column::<R::Entity>(name);
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
                let col = column().ok_or("not a column")?;
                let keys: Vec<SqlValue> = values
                    .iter()
                    .map(|v| values::to_sql(self.template(name), &Value::String(v.clone()), None))
                    .collect::<Result<_, _>>()
                    .map_err(|Invalid(rule)| {
                        cx.core
                            .messages
                            .render(&format!("filters.{name}"), rule, &[])
                    })?;
                Ok(select.filter(col.is_in(keys)))
            }
            FilterKind::BelongsTo(_) => {
                let col = column().ok_or("not a column")?;
                let keys: Vec<SqlValue> = values
                    .iter()
                    .filter_map(|v| key_from_str(self.template(name), v))
                    .collect();
                if keys.is_empty() {
                    return Ok(select.filter(Expr::cust("1 = 0")));
                }
                Ok(select.filter(col.is_in(keys)))
            }
            FilterKind::Boolean(f) => {
                let value = values
                    .last()
                    .map(|v| Value::String(v.clone()))
                    .unwrap_or(Value::Null);
                let on = values::boolean(&value).ok_or_else(|| {
                    cx.core
                        .messages
                        .render(&format!("filters.{name}"), "boolean", &[])
                })?;
                Ok(f(select, on))
            }
            FilterKind::DateRange => {
                let col = column().ok_or("not a column")?;
                let today = save::now(&cx.app).await.date_naive();
                let text = values.last().cloned().unwrap_or_default();
                let (from, to) = date_range(&text, today).ok_or_else(|| {
                    format!("filters.{name} must be a range of dates (YYYY-MM-DD..YYYY-MM-DD) or today, week, month or quarter")
                })?;
                let template = self.template(name);
                let mut select = select;
                if let Some(from) = from {
                    select = select.filter(col.gte(values::date_bound(template, midnight(from))));
                }
                if let Some(to) = to {
                    let end = to + Duration::days(1);
                    select = select.filter(col.lt(values::date_bound(template, midnight(end))));
                }
                Ok(select)
            }
            FilterKind::Custom(f) => Ok(f(select, values.join(","))),
        }
    }

    async fn aggregate<C: ConnectionTrait>(
        &self,
        select: Select<R::Entity>,
        aggregate: &Aggregate<Col<R>>,
        conn: &C,
    ) -> Result<Option<f64>, DbErr> {
        let row = select
            .select_only()
            .column_as(self.aggregate_expr(aggregate), "value")
            .into_tuple::<Option<f64>>()
            .one(conn)
            .await?;
        Ok(row.flatten())
    }

    fn aggregate_expr(&self, aggregate: &Aggregate<Col<R>>) -> SimpleExpr {
        let col = |c: Col<R>| Expr::col((<R::Entity as Default>::default(), c));
        let call = match aggregate {
            Aggregate::Count => Func::count(col(self.pk)),
            Aggregate::Sum(c) => Func::sum(col(*c)),
            Aggregate::Average(c) => Func::avg(col(*c)),
            Aggregate::Min(c) => Func::min(col(*c)),
            Aggregate::Max(c) => Func::max(col(*c)),
        };
        Func::cast_as(call, Alias::new("DOUBLE PRECISION")).into()
    }

    fn display(&self, metric: &Metric<R::Entity>, value: f64) -> String {
        match &metric.money {
            Some(currency) => money::format(value.round() as i64, currency),
            None => money::format_number(value),
        }
    }

    async fn compute(
        &self,
        cx: &Cx,
        metric: &Metric<R::Entity>,
        range: MetricRange,
    ) -> Result<MetricResult, AppError> {
        let mut select = self.base(cx, READ);
        QueryTrait::query(&mut select).clear_order_by();
        for condition in &metric.conditions {
            select = select.filter(condition.clone());
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
        let tx = cx.uow.tx().await?;
        match metric.kind {
            MetricKind::Value => {
                if let Some(over) = metric.over {
                    let template = self.template(over.as_str()).clone();
                    let ((start, end), (previous_start, previous_end)) = range.periods(now);
                    let between = |from, to| {
                        select
                            .clone()
                            .filter(over.gte(values::date_bound(&template, from)))
                            .filter(over.lt(values::date_bound(&template, to)))
                    };
                    result.value = self
                        .aggregate(between(start, end), &metric.aggregate, &*tx)
                        .await?;
                    result.previous = self
                        .aggregate(
                            between(previous_start, previous_end),
                            &metric.aggregate,
                            &*tx,
                        )
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
                    result.value = self.aggregate(select, &metric.aggregate, &*tx).await?;
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
                let Some((column, interval, count)) = metric.trend else {
                    return Err(AppError::internal("a trend metric without a period"));
                };
                let template = self.template(column.as_str()).clone();
                let today = now.date_naive();
                let buckets = buckets(interval, count, today);
                let start = buckets.first().map(|b| b.1).unwrap_or(today);
                let models = select
                    .filter(column.gte(values::date_bound(&template, midnight(start))))
                    .all(&*tx)
                    .await?;
                let mut grouped: Vec<Vec<f64>> = vec![Vec::new(); buckets.len()];
                for model in &models {
                    let Some(day) = values::sql_date(&model.get(column)) else {
                        continue;
                    };
                    let Some(index) = buckets.iter().position(|b| b.1 <= day && day < b.2) else {
                        continue;
                    };
                    let value = match metric.aggregate.column() {
                        None => 1.0,
                        Some(c) => match values::sql_number(&model.get(*c)) {
                            Some(n) => n,
                            None => continue,
                        },
                    };
                    grouped[index].push(value);
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
                let Some(column) = metric.group_by else {
                    return Err(AppError::internal("a partition metric without a column"));
                };
                let statement = select
                    .select_only()
                    .column_as(
                        Expr::col((<R::Entity as Default>::default(), column)),
                        "key",
                    )
                    .column_as(self.aggregate_expr(&metric.aggregate), "value")
                    .group_by(column)
                    .build(tx.get_database_backend());
                let rows = tx.query_all_raw(statement).await?;
                drop(tx);
                let template = self.template(column.as_str()).clone();
                let mut groups: Vec<(String, f64)> = rows
                    .iter()
                    .map(|row| {
                        let value = row
                            .try_get::<Option<f64>>("", "value")
                            .ok()
                            .flatten()
                            .unwrap_or(0.0);
                        (group_key(row, &template), value)
                    })
                    .collect();
                groups.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                let labels = self.group_labels(cx, column.as_str(), &groups).await?;
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
                return Ok(result);
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
}

/// The `key` column of a partition row, read with the Rust type of the
/// grouped column and spelled as record JSON spells it (`""` for NULL).
fn group_key(row: &QueryResult, template: &SqlValue) -> String {
    fn get<T: sea_orm::TryGetable>(row: &QueryResult) -> Option<T> {
        row.try_get::<Option<T>>("", "key").ok().flatten()
    }
    let text = match template {
        SqlValue::Uuid(_) => get::<uuid::Uuid>(row).map(|u| u.hyphenated().to_string()),
        SqlValue::Bool(_) => get::<bool>(row).map(|b| b.to_string()),
        SqlValue::TinyInt(_) | SqlValue::SmallInt(_) | SqlValue::Int(_) => {
            get::<i32>(row).map(|n| n.to_string())
        }
        SqlValue::BigInt(_)
        | SqlValue::TinyUnsigned(_)
        | SqlValue::SmallUnsigned(_)
        | SqlValue::Unsigned(_) => get::<i64>(row).map(|n| n.to_string()),
        SqlValue::Float(_) | SqlValue::Double(_) => get::<f64>(row).map(|n| n.to_string()),
        SqlValue::ChronoDate(_) => get::<NaiveDate>(row).map(|d| d.format("%Y-%m-%d").to_string()),
        _ => get::<String>(row),
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

    fn find<'a>(
        &'a self,
        cx: &'a Cx,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<RawRecord>, AppError>> {
        Box::pin(async move {
            if !cx.can(READ, <R::Entity as Subject>::SUBJECT, None) {
                return Err(AppError::forbidden());
            }
            Ok(self.load(cx, id, false).await?.map(|m| self.raw(cx, &m)))
        })
    }

    fn create<'a>(
        &'a self,
        cx: &'a Cx,
        input: Map<String, Value>,
    ) -> BoxFuture<'a, Result<RawRecord, AppError>> {
        Box::pin(async move {
            let subject = <R::Entity as Subject>::SUBJECT;
            if !cx.can(CREATE, subject, None) {
                return Err(AppError::forbidden());
            }
            let editable: Vec<Field> = self
                .info
                .fields
                .iter()
                .filter(|f| f.editable())
                .cloned()
                .collect();
            let values = self.checked(cx, &editable, input, true, true).await?;
            let submitted: Vec<String> = values.keys().cloned().collect();
            let model = self
                .resource
                .create(&self.save_context(cx), self.form(cx, values))
                .await?;
            let json = record_json(&model);
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
            Ok(self.raw(cx, &model))
        })
    }

    fn update<'a>(
        &'a self,
        cx: &'a Cx,
        id: &'a str,
        input: Map<String, Value>,
    ) -> BoxFuture<'a, Result<RawRecord, AppError>> {
        Box::pin(async move {
            let subject = <R::Entity as Subject>::SUBJECT;
            if !cx.can(READ, subject, None) {
                return Err(AppError::forbidden());
            }
            let Some(model) = self.load(cx, id, true).await? else {
                return Err(AppError::not_found());
            };
            let json = record_json(&model);
            if !cx.can(UPDATE, subject, Some(&json)) {
                return Err(AppError::forbidden());
            }
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
            let values = self.checked(cx, &editable, input, false, true).await?;
            let model = self
                .resource
                .update(&self.save_context(cx), model, self.form(cx, values))
                .await?;
            Ok(self.raw(cx, &model))
        })
    }

    fn delete<'a>(&'a self, cx: &'a Cx, id: &'a str) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let subject = <R::Entity as Subject>::SUBJECT;
            if !cx.can(READ, subject, None) {
                return Err(AppError::forbidden());
            }
            let Some(model) = self.load(cx, id, true).await? else {
                return Err(AppError::not_found());
            };
            if !cx.can(DELETE, subject, Some(&record_json(&model))) {
                return Err(AppError::forbidden());
            }
            self.resource.delete(&self.save_context(cx), model).await
        })
    }

    fn run_action<'a>(
        &'a self,
        cx: &'a Cx,
        name: &'a str,
        ids: Vec<String>,
        input: Map<String, Value>,
        from_job: bool,
    ) -> BoxFuture<'a, Result<ActionOutcome, AppError>> {
        Box::pin(async move {
            let subject = <R::Entity as Subject>::SUBJECT;
            let Some(action) = self.actions.iter().find(|a| a.name == name) else {
                return Err(AppError::not_found().with_detail(format!("No action `{name}`.")));
            };
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
            let keys: Vec<SqlValue> = unique
                .iter()
                .filter_map(|id| key_from_str(self.template(self.info.pk), id))
                .collect();
            let models = if keys.is_empty() {
                Vec::new()
            } else {
                let mut select = self.base(cx, READ).filter(self.pk.is_in(keys));
                if cx.uow.lock_for_update() {
                    select = select.lock_exclusive();
                }
                let tx = cx.uow.tx().await?;
                select.all(&*tx).await?
            };
            let permitted: Vec<ModelOf<R>> = models
                .into_iter()
                .filter(|m| cx.can(action.ability, subject, Some(&record_json(m))))
                .collect();
            let skipped = unique.len().saturating_sub(permitted.len());
            if permitted.is_empty() {
                return Err(AppError::forbidden()
                    .with_detail("You may not run this action on the selected records."));
            }
            let input = self.checked(cx, &action.fields, input, true, false).await?;
            let permitted_ids: Vec<String> = permitted.iter().map(|m| self.raw_id(m)).collect();
            let dispatcher = if action.queued && !from_job {
                cx.app.try_get::<dyn Dispatcher>().await
            } else {
                None
            };
            let (result, queued) = match dispatcher {
                Some(dispatcher) => {
                    let job = HelmActionJob {
                        resource: self.info.slug.clone(),
                        action: action.name.clone(),
                        ids: permitted_ids.clone(),
                        input,
                        user_id: cx.user_id,
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
            if !cx.can(READ, <R::Entity as Subject>::SUBJECT, None) {
                return Err(AppError::forbidden());
            }
            let key = format!(
                "{}|{}|{}|{}|{}",
                cx.user_id,
                cx.access.version,
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
            if !cx.can(READ, <R::Entity as Subject>::SUBJECT, None) {
                return Ok(Vec::new());
            }
            let mut select = self.base(cx, READ);
            QueryTrait::query(&mut select).clear_order_by();
            if let Some(ids) = ids {
                let keys: Vec<SqlValue> = ids
                    .iter()
                    .filter_map(|id| key_from_str(self.template(self.info.pk), id))
                    .collect();
                if keys.is_empty() {
                    return Ok(Vec::new());
                }
                select = select.filter(self.pk.is_in(keys));
            }
            if let Some(term) = search.map(str::trim).filter(|t| !t.is_empty()) {
                let backend = cx.uow.db().backend();
                let columns = if self.search.is_empty() {
                    vec![self.title]
                } else {
                    self.search.clone()
                };
                let mut any = Condition::any();
                for column in columns {
                    any = any.add(contains_ignore_case(
                        backend,
                        Expr::col((<R::Entity as Default>::default(), column)),
                        term,
                    ));
                }
                select = select.filter(any);
            }
            let models = {
                let tx = cx.uow.tx().await?;
                select
                    .order_by(self.title, Order::Asc)
                    .order_by(self.pk, Order::Asc)
                    .limit(limit)
                    .all(&*tx)
                    .await?
            };
            Ok(models
                .iter()
                .map(|m| {
                    let raw = self.raw(cx, m);
                    (raw.id, raw.title)
                })
                .collect())
        })
    }
}

impl<R: Resource> Handle<R> {
    fn raw_id(&self, model: &ModelOf<R>) -> String {
        record_json(model)
            .get(self.info.pk)
            .and_then(key_text)
            .unwrap_or_default()
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

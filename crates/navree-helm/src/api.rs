//! The panel's JSON API under `{helm.path}/api`.

use std::sync::Arc;

use http::StatusCode;
use navree_access::{CREATE, READ};
use navree_http::{AppError, Json, NoContent, Path, RawQuery};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::cx::Cx;
use crate::field::{Field, FieldKind};
use crate::handle::ErasedResource;
use crate::markdown;
use crate::metric::MetricRange;
use crate::panel::Panel;
use crate::present::{Screen, present};
use crate::query::ListQuery;

/// The resource with `slug`, or a 404.
pub(crate) fn resource<'a>(
    cx: &'a Cx,
    slug: &str,
) -> Result<&'a Arc<dyn ErasedResource>, AppError> {
    cx.core
        .registry
        .by_slug(slug)
        .ok_or_else(|| AppError::not_found().with_detail(format!("No resource `{slug}`.")))
}

/// A JSON object body, or a 400.
fn object(body: Value) -> Result<Map<String, Value>, AppError> {
    match body {
        Value::Object(map) => Ok(map),
        _ => Err(AppError::bad_request("The body must be a JSON object.")),
    }
}

/// `GET /api/resources`: the resources the user may read, with their
/// fields, filters, actions and metrics, and the pages they may open.
pub(crate) async fn index(Panel { cx, .. }: Panel) -> Result<Json<Value>, AppError> {
    let base = cx.core.options.base();
    let data: Vec<Value> = cx
        .core
        .registry
        .resources()
        .iter()
        .filter(|r| cx.can(READ, r.info().subject, None))
        .map(|r| describe(&cx, r.as_ref()))
        .collect();
    let pages: Vec<Value> = cx
        .core
        .registry
        .pages()
        .iter()
        .filter(|p| p.page.can.as_ref().is_none_or(|(a, s)| cx.can(a, s, None)))
        .map(|p| {
            json!({
                "slug": p.page.slug,
                "title": p.page.title,
                "group": p.page.group.clone().unwrap_or_else(|| "Pages".to_owned()),
                "icon": p.page.icon,
                "url": format!("{base}/pages/{}", p.page.slug),
            })
        })
        .collect();
    Ok(Json(json!({ "data": data, "pages": pages })))
}

/// The metadata of one resource.
pub(crate) fn describe(cx: &Cx, resource: &dyn ErasedResource) -> Value {
    let info = resource.info();
    let readable = info.readable(cx.ability());
    let related_slug = |type_id| {
        cx.core
            .registry
            .by_type(type_id)
            .map(|r| r.info().slug.clone())
    };
    let field = |f: &Field| {
        let related = match &f.kind {
            FieldKind::BelongsTo(r) | FieldKind::HasMany(r) => related_slug(r.type_id),
            _ => None,
        };
        json!({
            "name": f.name,
            "label": f.label_text(),
            "kind": f.kind.name(),
            "sortable": f.sortable && readable.queryable(&f.name),
            "readonly": !f.editable(),
            "on_index": f.on_index,
            "on_detail": f.on_detail,
            "on_forms": f.in_forms(),
            "required": f.required,
            "help": f.help,
            "options": f.options.iter().map(|(v, l)| json!({ "value": v, "label": l })).collect::<Vec<_>>(),
            "related": related,
        })
    };
    json!({
        "slug": info.slug,
        "label": info.label,
        "singular": info.singular,
        "group": info.group,
        "icon": info.icon,
        "order": info.order,
        "subject": info.subject,
        "title": info.title,
        "search": !readable.search().is_empty(),
        "per_page": info.per_page,
        "default_sort": readable.default_sort(),
        "fields": info.fields.iter().map(field).collect::<Vec<_>>(),
        "filters": info.filters.iter().filter(|f| readable.filter(f)).map(|f| json!({
            "name": f.name,
            "label": f.label,
            "kind": f.kind,
            "options": f.options.iter().map(|(v, l)| json!({ "value": v, "label": l })).collect::<Vec<_>>(),
            "related": f.related.and_then(|r| related_slug(r.type_id)),
        })).collect::<Vec<_>>(),
        "actions": info.actions.iter().map(|a| json!({
            "name": a.name,
            "label": a.label,
            "ability": a.ability,
            "confirm": a.question,
            "destructive": a.destructive,
            "on_index": a.on_index,
            "on_detail": a.on_detail,
            "queued": a.queued,
            "fields": a.fields.iter().map(field).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "metrics": info.metrics.iter().filter(|m| readable.metric(m)).map(|m| json!({
            "slug": m.slug,
            "label": m.label,
            "kind": m.kind,
            "on_dashboard": m.on_dashboard,
            "ranged": m.ranged,
        })).collect::<Vec<_>>(),
        "can": { "create": cx.can(CREATE, info.subject, None) && cx.offers(info, CREATE) },
    })
}

/// `GET /api/resources/{resource}`: a page of the list.
pub(crate) async fn list(
    Panel { cx, .. }: Panel,
    Path(slug): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Json<Value>, AppError> {
    let resource = resource(&cx, &slug)?;
    let q = ListQuery::parse_api(query.as_deref().unwrap_or(""))?;
    let page = resource.list(&cx, &q).await?;
    let items = present(&cx, resource.as_ref(), &page.records, Screen::Index).await?;
    Ok(Json(json!({
        "data": items.iter().map(|i| i.api(false)).collect::<Vec<_>>(),
        "meta": page.meta,
    })))
}

/// `GET /api/resources/{resource}/{id}`: one record.
pub(crate) async fn show(
    Panel { cx, .. }: Panel,
    Path((slug, id)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let resource = resource(&cx, &slug)?;
    let record = resource
        .find(&cx, &id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let items = present(
        &cx,
        resource.as_ref(),
        std::slice::from_ref(&record),
        Screen::Detail,
    )
    .await?;
    Ok(Json(json!({ "data": items[0].api(true) })))
}

/// `POST /api/resources/{resource}`: creates a record.
pub(crate) async fn create(
    Panel { cx, .. }: Panel,
    Path(slug): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let resource = resource(&cx, &slug)?;
    let record = resource.create(&cx, object(body)?).await?;
    let items = present(
        &cx,
        resource.as_ref(),
        std::slice::from_ref(&record),
        Screen::Detail,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "data": items[0].api(true) })),
    ))
}

/// `PATCH /api/resources/{resource}/{id}`: updates a record.
pub(crate) async fn update(
    Panel { cx, .. }: Panel,
    Path((slug, id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let resource = resource(&cx, &slug)?;
    let record = resource.update(&cx, &id, object(body)?).await?;
    let items = present(
        &cx,
        resource.as_ref(),
        std::slice::from_ref(&record),
        Screen::Detail,
    )
    .await?;
    Ok(Json(json!({ "data": items[0].api(true) })))
}

/// `DELETE /api/resources/{resource}/{id}`: deletes a record.
pub(crate) async fn delete(
    Panel { cx, .. }: Panel,
    Path((slug, id)): Path<(String, String)>,
) -> Result<NoContent, AppError> {
    let resource = resource(&cx, &slug)?;
    resource.delete(&cx, &id).await?;
    Ok(NoContent)
}

/// The body of an action run.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ActionBody {
    ids: Vec<Value>,
    input: Map<String, Value>,
}

/// `POST /api/resources/{resource}/actions/{action}`: runs an action on
/// `ids` with `input`.
pub(crate) async fn action(
    Panel { cx, .. }: Panel,
    Path((slug, name)): Path<(String, String)>,
    Json(body): Json<ActionBody>,
) -> Result<Json<Value>, AppError> {
    let resource = resource(&cx, &slug)?;
    let ids = body
        .ids
        .iter()
        .filter_map(crate::values::key_text)
        .collect();
    let outcome = resource
        .run_action(&cx, &name, ids, body.input, false)
        .await?;
    let mut out = serde_json::to_value(&outcome.result).map_err(AppError::internal)?;
    out["ran"] = json!(outcome.ran);
    out["skipped"] = json!(outcome.skipped);
    out["queued"] = json!(outcome.queued);
    Ok(Json(out))
}

/// `GET /api/resources/{resource}/metrics/{metric}?range=30`: one metric.
pub(crate) async fn metric(
    Panel { cx, .. }: Panel,
    Path((slug, metric)): Path<(String, String)>,
    RawQuery(query): RawQuery,
) -> Result<Json<Value>, AppError> {
    let resource = resource(&cx, &slug)?;
    let range = parse_range(query.as_deref())?;
    let result = resource.metric(&cx, &metric, range).await?;
    Ok(Json(
        serde_json::to_value(result).map_err(AppError::internal)?,
    ))
}

/// The `range` parameter (default 30 days); a 400 for an unknown one.
pub(crate) fn parse_range(query: Option<&str>) -> Result<MetricRange, AppError> {
    let raw = form_urlencoded::parse(query.unwrap_or("").as_bytes())
        .find(|(k, _)| k == "range")
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty());
    match raw {
        None => Ok(MetricRange::default()),
        Some(text) => MetricRange::parse(&text).ok_or_else(|| {
            let mut errors = std::collections::BTreeMap::new();
            errors.insert(
                "range".to_owned(),
                vec![
                    "range must be one of the following values: 7, 30, 90, month, year".to_owned(),
                ],
            );
            AppError::validation(errors)
        }),
    }
}

/// `GET /api/resources/{resource}/options?search=..`: records for a
/// relationship picker (`[{id, title}]`, at most 20), only those the user
/// may read.
pub(crate) async fn options(
    Panel { cx, .. }: Panel,
    Path(slug): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Json<Value>, AppError> {
    let resource = resource(&cx, &slug)?;
    if !cx.can(READ, resource.info().subject, None) {
        return Err(AppError::forbidden());
    }
    let search = form_urlencoded::parse(query.as_deref().unwrap_or("").as_bytes())
        .find(|(k, _)| k == "search")
        .map(|(_, v)| v.into_owned());
    let found = resource.lookup(&cx, search.as_deref(), None, 20).await?;
    Ok(Json(json!({
        "data": found.into_iter().map(|(id, title)| json!({ "id": id, "title": title })).collect::<Vec<_>>(),
    })))
}

/// The body of a Markdown preview.
#[derive(Debug, Deserialize)]
pub(crate) struct MarkdownBody {
    text: String,
}

/// `POST /api/markdown`: the HTML of a Markdown field's preview.
pub(crate) async fn markdown_preview(
    _panel: Panel,
    Json(body): Json<MarkdownBody>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({ "html": markdown::render(&body.text) })))
}

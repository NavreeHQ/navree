//! A record's page with its related rows, its create and edit forms, and the pages
//! of the app's own.

use std::collections::{BTreeMap, HashMap};

use http::{HeaderMap, StatusCode};
use navree_access::{CREATE, READ};
use navree_http::extract::Request;
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Path};
use navree_ui::described::name;
use navree_ui::form::csrf_field;
use navree_ui::table::Total;
use navree_ui::{
    Alert, AlertTone, Button, ButtonVariant, Card, DataTable, FactCard, FactGrid, Node, Row,
    SafeHtml, Size, Stack, TablePage, Text, ToastTone, el, icons,
};
use serde_json::{Map, Value, json};

use super::forms::{self, FormSpec};
use super::frame::Frame;
use super::list::{bulk_actions, row_menu, rows_of, table_of};
use super::{
    Submission, cells, discard_uploads, form_input, page_error, redirect_with, submission,
};
use crate::api::resource;
use crate::cx::Cx;
use crate::field::{Field, FieldKind};
use crate::handle::{ErasedResource, RawRecord, ResourceInfo};
use crate::panel::Panel;
use crate::present::{Display, Item, Screen, encode, present, screen_fields};
use crate::query::ListQuery;

/// The key under which a form's error that no field can show is kept: the form draws
/// it over the fields.
const FORM_ERROR: &str = "_form";

/// A hidden field.
fn hidden(name: &'static str, value: impl Into<String>) -> Node {
    el("input")
        .attr("type", "hidden")
        .attr("name", name)
        .attr("value", value.into())
        .into()
}

/// What a record's page shows of it: the short values side by side, the long ones
/// each in a card of its own.
fn details(item: &Item) -> Node {
    let mut short = Vec::new();
    let mut long: Vec<Node> = Vec::new();
    for cell in &item.cells {
        match &cell.display {
            Display::Multiline { .. } | Display::Markdown { .. } | Display::Html { .. } => {
                long.push(
                    Card::titled(cell.label.clone())
                        .heading(2)
                        .body(cells::node(&cell.display))
                        .into(),
                );
            }
            display => short.push(FactCard::new(cell.label.clone(), cells::node(display))),
        }
    }
    Stack::gap(4)
        .attr("aria-label", "Details")
        .children((
            (!short.is_empty()).then(|| FactGrid::new().cards(short)),
            long,
        ))
        .into()
}

/// The buttons of a record's page: edit it, run an action on it, delete it.
fn record_actions(frame: &Frame<'_>, info: &ResourceInfo, item: &Item) -> Vec<Node> {
    let base = frame.base();
    let actions_url = format!("{base}/resources/{}/actions", info.slug);
    let csrf = frame.csrf();
    let mut out: Vec<Node> = Vec::new();
    if item.can.update {
        out.push(
            Button::primary("Edit")
                .icon(icons::PENCIL)
                .href(format!("{}/edit", item.url))
                .into(),
        );
    }
    for action in
        info.actions.iter().filter(|a| a.on_detail).filter(|a| {
            frame.cx.can(a.ability, info.subject, None) && frame.cx.offers(info, a.ability)
        })
    {
        let button = Button::new(action.label.clone())
            .variant(if action.destructive {
                ButtonVariant::Destructive
            } else {
                ButtonVariant::Outline
            })
            .named("action", action.name.clone())
            .submit();
        out.push(
            el("form")
                .attr("method", "post")
                .attr("action", actions_url.clone())
                .flag("data-ui-form", true)
                .children((
                    csrf.map(|token| csrf_field(token.to_owned())),
                    hidden("_return", item.url.clone()),
                    hidden("ids", item.id.clone()),
                    button,
                ))
                .into(),
        );
    }
    if item.can.delete {
        out.push(
            el("form")
                .attr("method", "post")
                .attr("action", format!("{}/delete", item.url))
                .flag("data-ui-form", true)
                .children((
                    csrf.map(|token| csrf_field(token.to_owned())),
                    Button::destructive("Delete").icon(icons::TRASH).submit(),
                ))
                .into(),
        );
    }
    out
}

/// The `has_many` panels of a record's page: the first rows of each related
/// resource the user may read, with the actions on them and a way to all of them.
async fn relation_panels(
    frame: &Frame<'_>,
    info: &ResourceInfo,
    record: &RawRecord,
) -> Result<Vec<Node>, AppError> {
    let cx = frame.cx;
    let base = cx.core.options.base();
    let mut panels: Vec<Node> = Vec::new();
    for (n, field) in info.fields.iter().filter(|f| f.on_detail).enumerate() {
        let FieldKind::HasMany(related) = &field.kind else {
            continue;
        };
        let Some(target) = cx.core.registry.by_type(related.type_id) else {
            continue;
        };
        let target_info = target.info();
        // Which records hang on this one is what the key says.
        if !cx.can(READ, target_info.subject, None)
            || !target_info.readable(cx.ability()).queryable(&field.name)
        {
            continue;
        }
        let q = ListQuery {
            page: 1,
            per_page: Some(10),
            via: Some((field.name.clone(), record.id.clone())),
            ..ListQuery::default()
        };
        let page = target.list(cx, &q).await?;
        let items = cells::in_zone(
            present(cx, target.as_ref(), &page.records, Screen::Index).await?,
            cx.zone,
        );
        let list_url = format!("{base}/resources/{}", target_info.slug);
        // The link to the whole list keeps the relationship and the default page size.
        let all = ListQuery {
            per_page: None,
            ..q.clone()
        }
        .to_query(None, None);
        let all_url = format!("{list_url}?{all}");
        let fields: Vec<&Field> = screen_fields(target_info, Screen::Index)
            .into_iter()
            .filter(|f| f.name != field.name)
            .collect();
        let shape = table_of(cx, target_info, &fields, true, None);
        let table_query = shape.query(list_url.clone(), &all);
        let rows = TablePage {
            rows: rows_of(&items, Some(&field.name)),
            total: Total::Exact(items.len() as u64),
            page: 1,
            has_more: false,
            next: None,
            prev: None,
            version: None,
        };
        let label = field
            .label
            .clone()
            .unwrap_or_else(|| target_info.label.to_owned());
        let actions = bulk_actions(cx, target_info, false);
        let by_key = |row: &navree_ui::described::DescribedRow| {
            items.iter().find(|item| Some(item.id.as_str()) == row.id())
        };
        let mut table = DataTable::new(name(&format!("helm-related-{n}")), &table_query, &rows)
            .caption(label.clone())
            .fixed_order()
            .fixed_widths()
            .without_select_all_matching()
            .empty(navree_ui::Empty::new("None yet."))
            .row_href(|row: &navree_ui::described::DescribedRow| {
                by_key(row).map(|item| item.url.clone()).unwrap_or_default()
            })
            .row_actions(|row: &navree_ui::described::DescribedRow| {
                by_key(row).map(row_menu).unwrap_or_default()
            });
        if actions.is_empty() {
            table = table.can_select(false);
        } else {
            table = table.bulk(format!("{list_url}/actions"), actions);
        }
        if let Some(token) = frame.csrf() {
            table = table.csrf(token.to_owned());
        }
        let table: Node = table.into();
        panels.push(
            Card::titled(Node::list((
                label,
                " ",
                Text::muted(page.meta.total.to_string()).inline(),
            )))
            .action(Button::ghost("View all").size(Size::Sm).href(all_url))
            .body(table)
            .into(),
        );
    }
    Ok(panels)
}

/// `GET {path}/resources/{resource}/{id}`: a record's page.
pub(crate) async fn show(
    panel: Panel,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        let info = resource.info();
        let record = resource
            .find(cx, &id)
            .await?
            .ok_or_else(AppError::not_found)?;
        let item = cells::in_zone(
            present(
                cx,
                resource.as_ref(),
                std::slice::from_ref(&record),
                Screen::Detail,
            )
            .await?,
            cx.zone,
        )
        .remove(0);
        let frame = Frame::new(&panel, &headers, item.url.clone());
        let base = frame.base();
        let list_url = format!("{base}/resources/{}", info.slug);
        let panels = relation_panels(&frame, info, &record).await?;
        let page = frame
            .page(item.title.clone())
            .await
            .crumb(cx.core.options.title.clone(), base.to_owned())
            .crumb(info.label, list_url)
            .actions(
                Row::gap(2)
                    .wrap()
                    .children(record_actions(&frame, info, &item)),
            )
            .body((details(&item), panels));
        Ok::<_, AppError>(page.into_response())
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// The fields of a form: editable fields; on edit also read-only ones (shown
/// beside the form).
fn form_fields_of(info: &ResourceInfo, create: bool) -> Vec<&Field> {
    info.fields
        .iter()
        .filter(|f| f.in_forms() && (!create || f.editable()))
        .collect()
}

/// The errors of a refused form, by field; an error no field can show goes over
/// the fields.
fn form_errors(error: Option<&AppError>) -> BTreeMap<String, Vec<String>> {
    let mut errors: BTreeMap<String, Vec<String>> = error
        .and_then(|e| e.errors.as_deref().cloned())
        .unwrap_or_default();
    if let Some(error) = error.filter(|e| e.errors.is_none()) {
        errors.insert(
            FORM_ERROR.to_owned(),
            vec![error.detail.clone().unwrap_or_else(|| error.title.clone())],
        );
    }
    errors
}

/// Renders the create or edit form.
async fn form_page(
    frame: &Frame<'_>,
    resource: &dyn ErasedResource,
    status: StatusCode,
    record: Option<(&RawRecord, &Item)>,
    submitted: Option<&HashMap<String, String>>,
    error: Option<&AppError>,
) -> Result<Response, AppError> {
    let cx = frame.cx;
    let info = resource.info();
    let base = cx.core.options.base();
    let list_url = format!("{base}/resources/{}", info.slug);
    let create = record.is_none();
    let fields = form_fields_of(info, create);
    let errors = form_errors(error);
    let locked = record
        .map(|(_, item)| item.readonly.clone())
        .unwrap_or_default();
    let (view, fixed) = forms::view(
        cx,
        FormSpec {
            id: if create { "helm-create" } else { "helm-edit" },
            fields: &fields,
            record: record.map(|(r, _)| &r.readable),
            shown: record.map(|(_, item)| item.cells.as_slice()),
            submitted,
            errors: &errors,
            locked: &locked,
        },
    )
    .await?;
    let singular = info.singular.to_lowercase();
    let (title, action, submit, cancel) = match record {
        None => (
            format!("New {singular}"),
            list_url.clone(),
            format!("Create {singular}"),
            list_url.clone(),
        ),
        Some((_, item)) => (
            format!("Edit {}", item.title),
            item.url.clone(),
            "Save changes".to_owned(),
            item.url.clone(),
        ),
    };
    let mut view = view.post(action).submit(submit).cancel("Cancel", cancel);
    if let Some(token) = frame.csrf() {
        view = view.csrf(token.to_owned());
    }
    let mut page = frame
        .page(title)
        .await
        .crumb(cx.core.options.title.clone(), base.to_owned())
        .crumb(info.label, list_url);
    if let Some((_, item)) = record {
        page = page.crumb(item.title.clone(), item.url.clone());
    }
    let summary = (!errors.is_empty())
        .then(|| Alert::new(AlertTone::Danger).title("Please fix the highlighted fields."));
    Ok(page
        .status(status)
        .body(Card::new().body((summary, fixed, view)))
        .into_response())
}

/// `GET {path}/resources/{resource}/new`: the create form.
pub(crate) async fn new_form(
    panel: Panel,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        if !cx.can(CREATE, resource.info().subject, None) || resource.info().read_only {
            return Err(AppError::forbidden());
        }
        let path = format!(
            "{}/resources/{}/new",
            cx.core.options.base(),
            resource.info().slug
        );
        let frame = Frame::new(&panel, &headers, path);
        form_page(&frame, resource.as_ref(), StatusCode::OK, None, None, None).await
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// `POST {path}/resources/{resource}`: creates a record from the form.
pub(crate) async fn create(
    panel: Panel,
    Path(slug): Path<String>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let core = panel.cx.core.clone();
    create_record(panel, slug, headers, request)
        .await
        .unwrap_or_else(|error| page_error(&core, error))
}

async fn create_record(
    mut panel: Panel,
    slug: String,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let resource = resource(&panel.cx, &slug)?.clone();
    let info = resource.info();
    // Checked before reading the form, so no upload is stored for a user
    // who may not create here (the record's own rules are checked on save).
    resource.may_create(&panel.cx).await?;
    let submission = submission(&panel.cx, info, request, true).await?;
    panel.cx.tz_offset = submission.tz_offset();
    let path = format!(
        "{}/resources/{}/new",
        panel.cx.core.options.base(),
        info.slug
    );
    let frame = Frame::new(&panel, &headers, path);
    if !submission.upload_errors.is_empty() {
        discard_uploads(frame.cx, info, &submission).await;
        let error = AppError::validation(submission.upload_errors.clone());
        return form_page(
            &frame,
            resource.as_ref(),
            StatusCode::BAD_REQUEST,
            None,
            Some(&submission.values),
            Some(&error),
        )
        .await;
    }
    let fields = form_fields_of(info, true);
    let input = form_input(&fields, &submission, None, frame.cx.zone);
    match resource.create(frame.cx, input).await {
        Ok(record) => {
            let url = format!(
                "{}/resources/{}/{}",
                frame.base(),
                info.slug,
                encode(&record.id)
            );
            Ok(redirect_with(
                &url,
                &format!("{} created.", info.singular),
                ToastTone::Success,
            ))
        }
        Err(error) if error.status == StatusCode::BAD_REQUEST => {
            discard_uploads(frame.cx, info, &submission).await;
            form_page(
                &frame,
                resource.as_ref(),
                StatusCode::BAD_REQUEST,
                None,
                Some(&submission.values),
                Some(&error),
            )
            .await
        }
        Err(error) => {
            discard_uploads(frame.cx, info, &submission).await;
            Err(error)
        }
    }
}

/// `GET {path}/resources/{resource}/{id}/edit`: the edit form.
pub(crate) async fn edit_form(
    panel: Panel,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        let record = resource
            .find(cx, &id)
            .await?
            .ok_or_else(AppError::not_found)?;
        let item = cells::in_zone(
            present(
                cx,
                resource.as_ref(),
                std::slice::from_ref(&record),
                Screen::Detail,
            )
            .await?,
            cx.zone,
        )
        .remove(0);
        if !item.can.update {
            return Err(AppError::forbidden());
        }
        let frame = Frame::new(&panel, &headers, format!("{}/edit", item.url));
        form_page(
            &frame,
            resource.as_ref(),
            StatusCode::OK,
            Some((&record, &item)),
            None,
            None,
        )
        .await
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// `POST {path}/resources/{resource}/{id}`: saves the edit form.
pub(crate) async fn update(
    panel: Panel,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let core = panel.cx.core.clone();
    update_record(panel, slug, id, headers, request)
        .await
        .unwrap_or_else(|error| page_error(&core, error))
}

async fn update_record(
    mut panel: Panel,
    slug: String,
    id: String,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let resource = resource(&panel.cx, &slug)?.clone();
    let info = resource.info();
    let record = resource
        .find(&panel.cx, &id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let item = cells::in_zone(
        present(
            &panel.cx,
            resource.as_ref(),
            std::slice::from_ref(&record),
            Screen::Detail,
        )
        .await?,
        panel.cx.zone,
    )
    .remove(0);
    // Checked before reading the form, so no upload is stored for a user
    // who may not change the record.
    if !item.can.update {
        return Err(AppError::forbidden());
    }
    let submission: Submission = submission(&panel.cx, info, request, true).await?;
    panel.cx.tz_offset = submission.tz_offset();
    let frame = Frame::new(&panel, &headers, format!("{}/edit", item.url));
    if !submission.upload_errors.is_empty() {
        discard_uploads(frame.cx, info, &submission).await;
        let error = AppError::validation(submission.upload_errors.clone());
        return form_page(
            &frame,
            resource.as_ref(),
            StatusCode::BAD_REQUEST,
            Some((&record, &item)),
            Some(&submission.values),
            Some(&error),
        )
        .await;
    }
    let fields: Vec<&Field> = form_fields_of(info, false)
        .into_iter()
        .filter(|f| f.editable() && !item.readonly.contains(&f.name))
        .collect();
    let mut input = form_input(&fields, &submission, Some(&record.readable), frame.cx.zone);
    // The form showed what the user may not read as blank: blank isn't a new value.
    input.retain(|name, value| {
        record.readable.get(name).is_some() || !crate::values::is_blank(value)
    });
    match resource.update(frame.cx, &id, input).await {
        Ok(_) => Ok(redirect_with(
            &item.url,
            &format!("{} updated.", info.singular),
            ToastTone::Success,
        )),
        Err(error) if error.status == StatusCode::BAD_REQUEST => {
            discard_uploads(frame.cx, info, &submission).await;
            form_page(
                &frame,
                resource.as_ref(),
                StatusCode::BAD_REQUEST,
                Some((&record, &item)),
                Some(&submission.values),
                Some(&error),
            )
            .await
        }
        Err(error) => {
            discard_uploads(frame.cx, info, &submission).await;
            Err(error)
        }
    }
}

/// `{id, name, email}` of the signed-in user, for a page's template.
async fn user_json(cx: &Cx) -> Value {
    if let Some(auth) = cx.app.try_get::<navree_auth::AuthService>().await
        && let Ok(Some(user)) = auth.find_user(cx.user_id).await
    {
        return json!({ "id": user.id, "name": user.name, "email": user.email });
    }
    json!({ "id": cx.user_id, "name": "Signed in", "email": "" })
}

/// `GET {path}/pages/{page}`: a page of the app's, its template drawn in the panel's
/// shell.
pub(crate) async fn custom_page(
    panel: Panel,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let cx = &panel.cx;
    let result = async {
        let entry = cx
            .core
            .registry
            .page(&slug)
            .ok_or_else(AppError::not_found)?;
        let page = &entry.page;
        if let Some((action, subject)) = &page.can
            && !cx.can(action, subject, None)
        {
            return Err(AppError::forbidden());
        }
        let data = match &page.data {
            Some(f) => f(crate::PageContext::new(cx.inner(entry.module, Map::new()))).await?,
            None => Value::Null,
        };
        let views = cx
            .core
            .app_views
            .as_ref()
            .ok_or_else(|| AppError::internal("custom pages need ViewModule"))?;
        let base = cx.core.options.base();
        let frame = Frame::new(&panel, &headers, format!("{base}/pages/{}", page.slug));
        let template = page.template.as_deref().unwrap_or_default();
        // The session's token, masked afresh for this page.
        let csrf_token = frame
            .csrf()
            .map(navree_http::csrf::page_token)
            .unwrap_or_default();
        let content = views
            .render(
                template,
                json!({
                    "title": page.title,
                    "user": user_json(cx).await,
                    "csrf_token": csrf_token,
                    "path": base,
                    "data": data,
                }),
            )
            .map_err(AppError::internal)?;
        // The app's own template, rendered by navree-view with its escaping.
        let content = Node::Raw(SafeHtml::trusted(content));
        Ok::<_, AppError>(
            frame
                .page(page.title.clone())
                .await
                .crumb(cx.core.options.title.clone(), base.to_owned())
                .body(
                    el("div")
                        .attr("data-helm-page", page.slug.clone())
                        .child(content),
                )
                .into_response(),
        )
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

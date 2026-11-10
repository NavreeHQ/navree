//! The Settings screens (feature `settings`): every settings type the app
//! registered with navree-settings, its values at one level with the source
//! of each, edited in place.
//!
//! - `GET {path}/settings`: the types the user may manage.
//! - `GET {path}/settings/{key}?level=&target=`: the values at a level
//!   (`global`, or an organization, workspace or user named by `target`),
//!   with where each comes from.
//! - `POST {path}/settings/{key}?level=&target=`: saves the fields whose
//!   value changed, so the others keep inheriting.
//! - `POST {path}/settings/{key}/inherit?level=&target=` (`_inherit` =
//!   the field): the field follows the level above again.
//!
//! A user manages a type with its permission (`#[settings(permission =
//! "..")]`), else `settings.manage`, held app-wide; the Settings menu entry
//! shows when the app has `SettingsModule` and the user manages at least one
//! type. Writes go through `Settings` in the request's unit of work, so they
//! are validated, audited and announced like any other.
//!
//! The permission says which types, and the level says whose: the settings of a
//! workspace or an organization are for its members, as in the rest of the panel
//! (a role that can't write only reads them), of a user for that user, and
//! anyone else gets them only through `helm_staff_workspaces` or
//! `helm_staff_organizations`, which is audited, reads included. A credential
//! limited to a workspace reaches that workspace's settings and its user's.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use http::{HeaderMap, StatusCode};
use navree_access::{AbilityFactory, PinnedWorkspace, WorkspaceWrites};
use navree_http::extract::Request;
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Path, RawQuery};
use navree_settings::{
    RegisteredSettings, ResolvedSettings, SETTINGS_MANAGE_PERMISSION, Scope, ScopeKind, Settings,
    Source,
};
use navree_ui::described::{DescribedForm, FieldSpec};
use navree_ui::form::{Widget, csrf_field};
use navree_ui::{
    Alert, AlertTone, Badge, Button, Card, Empty, Field, Input, InputKind, NativeSelect, Node, Row,
    Size, Stack, Table, TableCell, TableHead, TableRowEl, Text, ToastTone, el, icons,
};
use navree_workspace::{
    OrgRole, Organization, Organizations, WorkspaceConfig, WorkspaceError, Workspaces,
};
use serde_json::{Map, Number, Value};
use uuid::Uuid;

use super::frame::Frame;
use super::{page_error, redirect_with};
use crate::cx::Cx;
use crate::panel::Panel;
use crate::workspace::{self, Admitted, Refused, StaffOrganizationEntry};

/// The app's settings service, when it has `SettingsModule`.
async fn service(cx: &Cx) -> Option<Arc<Settings>> {
    cx.app.try_get::<Settings>().await
}

/// The permission that lets a user manage `entry` in the panel.
fn manage_permission(entry: &RegisteredSettings) -> &'static str {
    entry
        .definition
        .permission
        .unwrap_or(SETTINGS_MANAGE_PERMISSION)
}

/// Whether the user holds `entry`'s permission app-wide.
async fn may_manage(cx: &Cx, entry: &RegisteredSettings) -> bool {
    let Some(factory) = cx.app.try_get::<AbilityFactory>().await else {
        return false;
    };
    let (action, subject) = factory.permission_target(manage_permission(entry));
    cx.ability().has_unconditional(action, subject)
}

/// The types the user may manage, by key.
async fn manageable(cx: &Cx, settings: &Settings) -> Vec<Arc<RegisteredSettings>> {
    let mut out = Vec::new();
    for entry in settings.registry().all() {
        if may_manage(cx, &entry).await {
            out.push(entry);
        }
    }
    out
}

/// Whether the menu shows the Settings entry.
pub(crate) async fn visible(cx: &Cx) -> bool {
    match service(cx).await {
        Some(settings) => !manageable(cx, &settings).await.is_empty(),
        None => false,
    }
}

/// `portal` as `Portal`, `view-preferences` as `View preferences`.
fn humanize(name: &str) -> String {
    let words = name.replace(['-', '_', '.'], " ");
    let mut chars = words.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// How a field is edited, from its default (else its current value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Boolean,
    Integer,
    Number,
    Text,
    /// Anything else: JSON in a text area.
    Json,
}

fn kind_of(default: Option<&Value>, current: Option<&Value>) -> Kind {
    let value = match default {
        Some(Value::Null) | None => current,
        other => other,
    };
    match value {
        Some(Value::Bool(_)) => Kind::Boolean,
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => Kind::Integer,
        Some(Value::Number(_)) => Kind::Number,
        Some(Value::String(_)) => Kind::Text,
        _ => Kind::Json,
    }
}

/// A submitted value read as `kind`.
fn parse(kind: Kind, raw: &str) -> Result<Value, String> {
    let raw = raw.replace("\r\n", "\n");
    match kind {
        Kind::Boolean => Ok(Value::Bool(matches!(raw.as_str(), "true" | "on" | "1"))),
        Kind::Integer => {
            let trimmed = raw.trim();
            trimmed
                .parse::<i64>()
                .map(Value::from)
                .or_else(|_| trimmed.parse::<u64>().map(Value::from))
                .map_err(|_| "must be a whole number".to_owned())
        }
        Kind::Number => raw
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| "must be a number".to_owned()),
        Kind::Text => Ok(Value::String(raw)),
        Kind::Json => serde_json::from_str(&raw).map_err(|_| "must be valid JSON".to_owned()),
    }
}

/// The level and target a screen is about, from its query.
#[derive(Clone, Debug, Default)]
struct Selection {
    level: Option<String>,
    target: Option<String>,
}

impl Selection {
    fn read(query: Option<&str>) -> Selection {
        let mut selection = Selection::default();
        for (name, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
            let value = value.trim().to_owned();
            match name.as_ref() {
                "level" if !value.is_empty() => selection.level = Some(value),
                "target" if !value.is_empty() => selection.target = Some(value),
                _ => {}
            }
        }
        selection
    }

    /// `?level=..&target=..` (empty for the global level).
    fn query(level: ScopeKind, target: Option<&str>) -> String {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("level", level.as_str());
        if let Some(target) = target {
            query.append_pair("target", target);
        }
        format!("?{}", query.finish())
    }
}

/// The level a selection names for `entry`: the one asked for, else the
/// most general one it allows.
fn level_of(entry: &RegisteredSettings, selection: &Selection) -> Result<ScopeKind, AppError> {
    let level = match &selection.level {
        Some(raw) => raw.parse::<ScopeKind>().map_err(AppError::bad_request)?,
        None => entry
            .definition
            .levels()
            .first()
            .copied()
            .unwrap_or(ScopeKind::Global),
    };
    if !entry.definition.allows(level) {
        return Err(AppError::bad_request(format!(
            "These settings cannot be stored at the {level} level."
        )));
    }
    Ok(level)
}

/// The answer to a credential limited to a workspace, at a level beyond it.
fn beyond_the_pin() -> AppError {
    AppError::forbidden()
        .with_code("token_limited_to_workspace")
        .with_detail("This token is limited to a workspace.")
}

/// An id for a target that names nothing: a slug, or a word, that nobody may
/// learn more of than of a workspace they aren't in.
fn id_of_nothing(target: &str) -> Uuid {
    Uuid::parse_str(target).unwrap_or(Uuid::nil())
}

/// The scope of the workspace `target` names, with its name, if the user may
/// work with its settings (see [`workspace::admission_to_what_it_owns`]).
async fn workspace_scope(cx: &Cx, target: &str, writes: bool) -> Result<(Scope, String), AppError> {
    let (id, workspace) = match cx.app.try_get::<Workspaces>().await {
        Some(workspaces) => match workspaces.resolve(target).await {
            Ok(workspace) => (workspace.id, Some(workspace)),
            Err(WorkspaceError::WorkspaceNotFound(_)) => (id_of_nothing(target), None),
            Err(error) => return Err(error.to_app_error()),
        },
        None => (
            Uuid::parse_str(target)
                .map_err(|_| AppError::bad_request(format!("`{target}` is not an id.")))?,
            None,
        ),
    };
    if cx.pin.is_some_and(|pin| pin != id) {
        return Err(PinnedWorkspace::mismatch());
    }
    let admitted =
        workspace::admission_to_what_it_owns(&cx.app, &cx.core, cx.user_id, &cx.access, id, writes)
            .await?;
    match admitted {
        Ok(Admitted::Member) => {}
        Ok(Admitted::Staff) => {
            cx.audit_outside(workspace::staff_entry(id, "settings"))
                .await;
        }
        Err(refused) => return Err(refused.error()),
    }
    Ok(match workspace {
        Some(workspace) => (Scope::from(&workspace), workspace.name),
        None => (Scope::workspace(id), target.to_owned()),
    })
}

/// Whether an organization role writes nothing: `viewer`, or a role of the app's
/// own, unless the permissions the app set for it say otherwise.
async fn organization_role_is_read_only(cx: &Cx, role: &OrgRole) -> bool {
    let role_permissions = match cx.app.try_get::<WorkspaceConfig>().await {
        Some(config) => config
            .organizations
            .roles
            .get(role.as_str())
            .map(|options| options.permissions.clone())
            .unwrap_or_default(),
        None => Vec::new(),
    };
    WorkspaceWrites {
        read_only_demo: false,
        read_only_role: matches!(role, OrgRole::Viewer | OrgRole::Custom(_)),
        role_permissions,
    }
    .role_is_read_only()
}

/// Whether `helm_staff_organizations` lets the user work with the settings of
/// `organization`, and the audit entry if it does.
async fn staff_of_organization(cx: &Cx, organization: Uuid, checked: bool) -> Result<(), AppError> {
    let Some(may_enter) = &cx.core.staff_organizations else {
        return Err(if checked {
            Refused::NotAllowed.error()
        } else {
            Refused::Unchecked.error()
        });
    };
    let entry = StaffOrganizationEntry {
        app: cx.app.clone(),
        user_id: cx.user_id,
        organization,
        access: cx.access.clone(),
    };
    if !may_enter(entry).await? {
        return Err(Refused::NotAllowed.error());
    }
    Ok(())
}

/// The scope of the organization `target` names, with its name, if the user may
/// work with its settings.
async fn organization_scope(
    cx: &Cx,
    target: &str,
    writes: bool,
) -> Result<(Scope, String), AppError> {
    let staff_note = |id| cx.audit_outside(workspace::staff_organization_entry(id));
    let Some(organizations) = cx.app.try_get::<Organizations>().await else {
        let id = Uuid::parse_str(target)
            .map_err(|_| AppError::bad_request(format!("`{target}` is not an id.")))?;
        staff_of_organization(cx, id, false).await?;
        staff_note(id).await;
        return Ok((Scope::organization(id), target.to_owned()));
    };
    let organization: Option<Organization> = match organizations.resolve(target).await {
        Ok(organization) => Some(organization),
        Err(WorkspaceError::OrganizationNotFound(_)) => None,
        Err(error) => return Err(error.to_app_error()),
    };
    let Some(organization) = organization else {
        staff_of_organization(cx, id_of_nothing(target), true).await?;
        return Err(Refused::Missing.error());
    };
    let member = organizations
        .membership(organization.id, cx.user_id)
        .await
        .map_err(|error| error.to_app_error())?;
    match member {
        Some(member) => {
            if writes && organization_role_is_read_only(cx, &member.role()).await {
                return Err(WorkspaceWrites::read_only_role_error());
            }
        }
        None => {
            staff_of_organization(cx, organization.id, true).await?;
            staff_note(organization.id).await;
        }
    }
    Ok((Scope::from(&organization), organization.name))
}

/// The scope a level and target name, with a label for the target, if the user
/// may work with what it holds: with `writes`, change it.
async fn scope_of(
    cx: &Cx,
    level: ScopeKind,
    target: &str,
    writes: bool,
) -> Result<(Scope, String), AppError> {
    match level {
        ScopeKind::Global if cx.pin.is_some() => Err(beyond_the_pin()),
        ScopeKind::Global => Ok((Scope::global(), "Everyone".to_owned())),
        ScopeKind::Organization if cx.pin.is_some() => Err(beyond_the_pin()),
        ScopeKind::Organization => organization_scope(cx, target, writes).await,
        ScopeKind::Workspace => workspace_scope(cx, target, writes).await,
        ScopeKind::User => {
            let id = Uuid::parse_str(target)
                .map_err(|_| AppError::bad_request(format!("`{target}` is not an id.")))?;
            // As the settings API has it: users work with their own.
            if id != cx.user_id {
                return Err(AppError::forbidden());
            }
            Ok((Scope::user(id), target.to_owned()))
        }
    }
}

/// `entry` and the service, checked for the user: 404 without
/// `SettingsModule` or for an unknown key, 403 without the permission.
async fn entry_for(
    cx: &Cx,
    key: &str,
) -> Result<(Arc<Settings>, Arc<RegisteredSettings>), AppError> {
    let settings = service(cx).await.ok_or_else(AppError::not_found)?;
    let entry = settings
        .registry()
        .get(key)
        .ok_or_else(|| AppError::not_found().with_detail(format!("No settings `{key}`.")))?;
    if !may_manage(cx, &entry).await {
        return Err(AppError::forbidden());
    }
    Ok((settings, entry))
}

/// `GET {path}/settings`: the types the user may manage.
pub(crate) async fn index(panel: Panel, headers: HeaderMap) -> Response {
    let cx = &panel.cx;
    let base = cx.core.options.base();
    let frame = Frame::new(&panel, &headers, format!("{base}/settings"));
    let result = async {
        let settings = service(cx).await.ok_or_else(AppError::not_found)?;
        let entries = manageable(cx, &settings).await;
        if entries.is_empty() {
            return Err(AppError::forbidden());
        }
        let rows: Vec<Node> = entries
            .iter()
            .map(|entry| {
                let definition = &entry.definition;
                let url = format!("{base}/settings/{}", definition.key);
                let levels: Vec<Node> = definition
                    .levels()
                    .iter()
                    .map(|level| Badge::new(level.as_str()).into())
                    .collect();
                TableRowEl::new((
                    TableCell::new((
                        el("a").href(url).child(humanize(definition.key)),
                        " ",
                        Text::code(definition.key).inline(),
                    )),
                    TableCell::new(if definition.description.is_empty() {
                        Text::muted("—").inline().into()
                    } else {
                        Node::from(definition.description)
                    }),
                    TableCell::new(Row::gap(1).wrap().children(levels)),
                    TableCell::new(entry.fields.len().to_string()).numeric(),
                ))
                .key(definition.key)
                .into()
            })
            .collect();
        let table = Table::new()
            .label("Settings")
            .head((
                TableHead::new("Settings"),
                TableHead::new("Description"),
                TableHead::new("Levels"),
                TableHead::new("Fields").numeric(),
            ))
            .rows(rows);
        Ok::<_, AppError>(
            frame
                .page("Settings")
                .await
                .crumb(cx.core.options.title.clone(), base.to_owned())
                .body(Card::new().body(table))
                .into_response(),
        )
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// What the form page shows besides the layout.
struct FormState<'a> {
    entry: &'a RegisteredSettings,
    level: ScopeKind,
    target: Option<&'a str>,
    /// The scope and its label, once the target is known.
    scope: Option<(Scope, String)>,
    resolved: Option<&'a ResolvedSettings>,
    /// The submitted values to show again (after an error).
    submitted: Option<&'a HashMap<String, String>>,
    errors: BTreeMap<String, Vec<String>>,
    general_error: Option<String>,
}

fn source_text(source: Source, level: ScopeKind) -> String {
    match source {
        Source::Default => "Default value".to_owned(),
        source if source == level.source() => "Set at this level".to_owned(),
        source => format!("Inherited from the {source} level"),
    }
}

/// How a field is drawn, from its kind and what it holds.
fn widget_of(kind: Kind, value: &str) -> Widget {
    match kind {
        Kind::Boolean => Widget::Switch,
        Kind::Integer | Kind::Number => Widget::Input(InputKind::Number),
        Kind::Text if value.contains('\n') || value.chars().count() > 120 => {
            Widget::Textarea { rows: 5 }
        }
        Kind::Text => Widget::Input(InputKind::Text),
        Kind::Json => Widget::Textarea { rows: 8 },
    }
}

/// The form page.
async fn form_page(frame: &Frame<'_>, status: StatusCode, state: FormState<'_>) -> Response {
    let base = frame.base();
    let definition = &state.entry.definition;
    let key = definition.key;
    let label = humanize(key);
    let query = Selection::query(state.level, state.target);
    let url = format!("{base}/settings/{key}");
    let index_url = format!("{base}/settings");
    let csrf = frame.csrf();
    // Which level and whose: a form that asks again.
    let mut levels = NativeSelect::new("level").id("helm-level");
    for level in definition.levels() {
        levels = levels.option(level.as_str(), humanize(level.as_str()));
    }
    levels = levels.selected(state.level.as_str());
    let mut target = Input::text("target")
        .id("helm-target")
        .placeholder("Id or slug (not for the global level)");
    if let Some(value) = state.target {
        target = target.value(value);
    }
    let chooser = el("form")
        .attr("method", "get")
        .attr("action", url.clone())
        .attr("aria-label", "Choose the level")
        .child(Row::gap(3).wrap().children((
            Field::new("helm-level", "Level").control(levels),
            Field::new("helm-target", "Organization, workspace or user").control(target),
            Button::outline("Open").submit(),
        )));
    let body: Node = match state.resolved {
        Some(resolved) if state.level == ScopeKind::Global || state.scope.is_some() => {
            let mut form = DescribedForm::new("helm-settings");
            let mut values: Vec<(String, String)> = Vec::new();
            let mut overridden: Vec<&String> = Vec::new();
            for name in &state.entry.fields {
                let current = resolved.data.get(name);
                let kind = kind_of(state.entry.defaults.get(name), current);
                let shown = state.submitted.and_then(|values| values.get(name));
                let source = resolved.source(name).unwrap_or(Source::Default);
                let text = match (shown, kind) {
                    (Some(raw), _) => raw.clone(),
                    (None, Kind::Boolean) => {
                        if current == Some(&Value::Bool(true)) {
                            "true".to_owned()
                        } else {
                            String::new()
                        }
                    }
                    (None, Kind::Json) => current
                        .and_then(|value| serde_json::to_string_pretty(value).ok())
                        .unwrap_or_default(),
                    (None, _) => match current {
                        Some(Value::String(text)) => text.clone(),
                        Some(Value::Null) | None => String::new(),
                        Some(other) => other.to_string(),
                    },
                };
                form = form.field(
                    FieldSpec::new(name, &humanize(name), widget_of(kind, &text))
                        .optional()
                        .help(&source_text(source, state.level)),
                );
                if !text.is_empty() {
                    values.push((name.clone(), text));
                }
                if resolved.is_overridden(name) {
                    overridden.push(name);
                }
            }
            let mut view = form
                .view()
                .hidden("_form", "helm-settings")
                .post(format!("{url}{query}"))
                .with_values(values)
                .with_errors(state.errors.clone())
                .submit("Save")
                .cancel("Back", index_url.clone());
            if let Some(token) = csrf {
                view = view.csrf(token.to_owned());
            }
            // A value set here can follow the level above again.
            let inherit: Vec<Node> = overridden
                .into_iter()
                .map(|name| {
                    el("form")
                        .attr("method", "post")
                        .attr("action", format!("{url}/inherit{query}"))
                        .flag("data-ui-form", true)
                        .children((
                            csrf.map(|token| csrf_field(token.to_owned())),
                            Button::ghost(format!("Inherit {}", humanize(name).to_lowercase()))
                                .size(Size::Sm)
                                .named("_inherit", name.clone())
                                .submit(),
                        ))
                        .into()
                })
                .collect();
            let heading = match &state.scope {
                Some((_, target)) => format!("{}: {target}", humanize(state.level.as_str())),
                None => humanize(state.level.as_str()),
            };
            let summary = (!state.errors.is_empty())
                .then(|| Alert::new(AlertTone::Danger).title("Please fix the highlighted fields."));
            let general = state
                .general_error
                .clone()
                .map(|text| Alert::new(AlertTone::Danger).title(text));
            Card::titled(heading)
                .attr("aria-label", format!("{} values", humanize(state.level.as_str())))
                .body(Stack::gap(4).children((
                    summary,
                    general,
                    view,
                    Text::muted(
                        "Changing a value sets it at this level; the others keep following the level above.",
                    ),
                    (!inherit.is_empty()).then(|| Row::gap(2).wrap().children(inherit)),
                )))
                .into()
        }
        _ => Empty::new(format!(
            "Name the {} to edit its settings: its id{}.",
            state.level.as_str(),
            if state.level == ScopeKind::User {
                ""
            } else {
                " or slug"
            }
        ))
        .icon(icons::SLIDERS_HORIZONTAL)
        .into(),
    };
    frame
        .page(label.clone())
        .await
        .crumb(frame.core().options.title.clone(), base.to_owned())
        .crumb("Settings", index_url.clone())
        .status(status)
        .body((
            (!definition.description.is_empty()).then(|| Text::muted(definition.description)),
            Card::new().body(chooser),
            body,
        ))
        .into_response()
}

/// `GET {path}/settings/{key}`: the values at a level.
pub(crate) async fn show(
    panel: Panel,
    Path(key): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let frame = Frame::new(
        &panel,
        &headers,
        settings_path(&panel, &key, query.as_deref()),
    );
    let cx = &panel.cx;
    let result = async {
        let (settings, entry) = entry_for(cx, &key).await?;
        let selection = Selection::read(query.as_deref());
        let level = level_of(&entry, &selection)?;
        let target = selection.target.as_deref();
        let mut state = FormState {
            entry: &entry,
            level,
            target,
            scope: None,
            resolved: None,
            submitted: None,
            errors: BTreeMap::new(),
            general_error: None,
        };
        if level != ScopeKind::Global && target.is_none() {
            return Ok(form_page(&frame, StatusCode::OK, state).await);
        }
        let (scope, name) = scope_of(cx, level, target.unwrap_or_default(), false).await?;
        let resolved = settings
            .resolve_key(&key, scope)
            .await
            .map_err(settings_error)?;
        state.scope = Some((scope, name));
        state.resolved = Some(&resolved);
        Ok::<_, AppError>(form_page(&frame, StatusCode::OK, state).await)
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// The page of the settings `key`, with the level and target `query` names.
fn settings_path(panel: &Panel, key: &str, query: Option<&str>) -> String {
    let base = panel.cx.core.options.base();
    match query.filter(|query| !query.is_empty()) {
        Some(query) => format!("{base}/settings/{key}?{query}"),
        None => format!("{base}/settings/{key}"),
    }
}

/// A settings error as a response error.
fn settings_error(error: navree_settings::SettingsError) -> AppError {
    error
        .to_app_error()
        .unwrap_or_else(|| AppError::internal(error))
}

/// The urlencoded fields of a form, the last value of each name.
async fn form(request: Request) -> Result<HashMap<String, String>, AppError> {
    let bytes = navree_http::body::to_bytes(request.into_body(), 1024 * 1024)
        .await
        .map_err(|_| AppError::bad_request("The form could not be read."))?;
    Ok(form_urlencoded::parse(&bytes).into_owned().collect())
}

/// `POST {path}/settings/{key}`: saves the fields whose value changed.
pub(crate) async fn save(
    panel: Panel,
    Path(key): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let frame = Frame::new(
        &panel,
        &headers,
        settings_path(&panel, &key, query.as_deref()),
    );
    let cx = &panel.cx;
    let result = async {
        let (settings, entry) = entry_for(cx, &key).await?;
        let selection = Selection::read(query.as_deref());
        let level = level_of(&entry, &selection)?;
        let target = selection.target.as_deref();
        if level != ScopeKind::Global && target.is_none() {
            return Err(AppError::bad_request("Name the target of the level."));
        }
        let (scope, name) = scope_of(cx, level, target.unwrap_or_default(), true).await?;
        let submitted = form(request).await?;
        let current = settings
            .resolve_key(&key, scope)
            .await
            .map_err(settings_error)?;
        let mut patch = Map::new();
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for field in &entry.fields {
            let kind = kind_of(entry.defaults.get(field), current.data.get(field));
            // A switch that is off posts nothing: in a post of the form, a switch
            // left out is off.
            let off = String::from("false");
            let raw = match submitted.get(field) {
                Some(raw) => raw,
                None if kind == Kind::Boolean && submitted.contains_key("_form") => &off,
                None => continue,
            };
            match parse(kind, raw) {
                Ok(value) if current.data.get(field) != Some(&value) => {
                    patch.insert(field.clone(), value);
                }
                Ok(_) => {}
                Err(message) => errors
                    .entry(field.clone())
                    .or_default()
                    .push(format!("{} {message}", humanize(field))),
            }
        }
        let again = |errors, general_error, resolved| FormState {
            entry: &entry,
            level,
            target,
            scope: Some((scope, name.clone())),
            resolved,
            submitted: Some(&submitted),
            errors,
            general_error,
        };
        if !errors.is_empty() {
            return Ok(form_page(
                &frame,
                StatusCode::BAD_REQUEST,
                again(errors, None, Some(&current)),
            )
            .await);
        }
        let label = humanize(&key);
        let back = format!(
            "{}/settings/{key}{}",
            frame.base(),
            Selection::query(level, target)
        );
        if patch.is_empty() {
            return Ok(redirect_with(&back, "Nothing changed.", ToastTone::Success));
        }
        match settings.update_key(&cx.uow, &key, scope, patch).await {
            Ok(_) => Ok(redirect_with(
                &back,
                &format!("{label} settings saved."),
                ToastTone::Success,
            )),
            Err(error) => {
                let error = settings_error(error);
                match error.errors {
                    Some(errors) if error.status == StatusCode::BAD_REQUEST => Ok(form_page(
                        &frame,
                        StatusCode::BAD_REQUEST,
                        again(*errors, None, Some(&current)),
                    )
                    .await),
                    _ => Err(error),
                }
            }
        }
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// `POST {path}/settings/{key}/inherit`: the field `_inherit` follows the
/// level above again.
pub(crate) async fn inherit(
    panel: Panel,
    Path(key): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let frame = Frame::new(
        &panel,
        &headers,
        settings_path(&panel, &key, query.as_deref()),
    );
    let cx = &panel.cx;
    let result = async {
        let (settings, entry) = entry_for(cx, &key).await?;
        let selection = Selection::read(query.as_deref());
        let level = level_of(&entry, &selection)?;
        let target = selection.target.as_deref();
        if level != ScopeKind::Global && target.is_none() {
            return Err(AppError::bad_request("Name the target of the level."));
        }
        let (scope, _) = scope_of(cx, level, target.unwrap_or_default(), true).await?;
        let submitted = form(request).await?;
        let field = submitted
            .get("_inherit")
            .filter(|field| entry.has_field(field))
            .ok_or_else(|| AppError::bad_request("Name a field of these settings."))?
            .clone();
        settings
            .inherit_key(&cx.uow, &key, scope, std::slice::from_ref(&field))
            .await
            .map_err(settings_error)?;
        let back = format!(
            "{}/settings/{key}{}",
            frame.base(),
            Selection::query(level, target)
        );
        Ok::<_, AppError>(redirect_with(
            &back,
            &format!("{} inherits again.", humanize(&field)),
            ToastTone::Success,
        ))
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn names_read_as_labels() {
        assert_eq!(humanize("portal"), "Portal");
        assert_eq!(humanize("view-preferences"), "View preferences");
        assert_eq!(humanize("welcome_text"), "Welcome text");
        assert_eq!(humanize("mail.delivery"), "Mail delivery");
        assert_eq!(humanize(""), "");
    }

    #[test]
    fn fields_are_edited_by_the_type_of_their_default() {
        assert_eq!(kind_of(Some(&json!(true)), None), Kind::Boolean);
        assert_eq!(kind_of(Some(&json!(25)), None), Kind::Integer);
        assert_eq!(kind_of(Some(&json!(0.5)), None), Kind::Number);
        assert_eq!(kind_of(Some(&json!("")), None), Kind::Text);
        assert_eq!(kind_of(Some(&json!(["a"])), None), Kind::Json);
        // An optional field reads its current value.
        assert_eq!(kind_of(Some(&Value::Null), Some(&json!("x"))), Kind::Text);
        assert_eq!(kind_of(None, None), Kind::Json);
    }

    #[test]
    fn submitted_values_are_parsed_by_kind() {
        assert_eq!(parse(Kind::Boolean, "true"), Ok(json!(true)));
        assert_eq!(parse(Kind::Boolean, "false"), Ok(json!(false)));
        assert_eq!(parse(Kind::Integer, " 42 "), Ok(json!(42)));
        assert_eq!(parse(Kind::Integer, "-3"), Ok(json!(-3)));
        assert!(parse(Kind::Integer, "4.5").is_err());
        assert!(parse(Kind::Integer, "").is_err());
        assert_eq!(parse(Kind::Number, "0.25"), Ok(json!(0.25)));
        assert!(parse(Kind::Number, "NaN").is_err());
        assert_eq!(parse(Kind::Text, "Hi\r\nthere"), Ok(json!("Hi\nthere")));
        assert_eq!(parse(Kind::Json, "[1, 2]"), Ok(json!([1, 2])));
        assert!(parse(Kind::Json, "[1,").is_err());
    }

    #[test]
    fn selections_read_and_write_the_query() {
        let selection = Selection::read(Some("level=workspace&target=agra&other=x"));
        assert_eq!(selection.level.as_deref(), Some("workspace"));
        assert_eq!(selection.target.as_deref(), Some("agra"));
        let empty = Selection::read(Some("level=&target=%20"));
        assert!(empty.level.is_none() && empty.target.is_none());
        assert_eq!(
            Selection::query(ScopeKind::Workspace, Some("a b")),
            "?level=workspace&target=a+b"
        );
        assert_eq!(Selection::query(ScopeKind::Global, None), "?level=global");
    }

    #[test]
    fn sources_read_as_sentences() {
        assert_eq!(
            source_text(Source::Default, ScopeKind::Workspace),
            "Default value"
        );
        assert_eq!(
            source_text(Source::Workspace, ScopeKind::Workspace),
            "Set at this level"
        );
        assert_eq!(
            source_text(Source::Organization, ScopeKind::Workspace),
            "Inherited from the organization level"
        );
    }
}

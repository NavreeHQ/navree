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

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use axum::extract::{Path, RawQuery, Request};
use axum::response::Response;
use http::{HeaderMap, StatusCode};
use navree_access::AbilityFactory;
use navree_http::AppError;
use navree_settings::{
    RegisteredSettings, ResolvedSettings, SETTINGS_MANAGE_PERMISSION, Scope, ScopeKind, Settings,
    Source,
};
use navree_workspace::{Organizations, Workspaces};
use serde_json::{Map, Number, Value, json};
use uuid::Uuid;

use super::{Frame, Input, crumb, page_error, redirect_with};
use crate::cx::Cx;
use crate::panel::Panel;

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
        Some(raw) => raw
            .parse::<ScopeKind>()
            .map_err(|message| AppError::bad_request(message))?,
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

/// The scope a level and target name, with a label for the target.
async fn scope_of(
    cx: &Cx,
    level: ScopeKind,
    target: &str,
) -> Result<(Scope, String), AppError> {
    let as_id = || {
        Uuid::parse_str(target).map_err(|_| {
            AppError::bad_request(format!("`{target}` is not an id."))
        })
    };
    match level {
        ScopeKind::Global => Ok((Scope::global(), "Everyone".to_owned())),
        ScopeKind::Organization => match cx.app.try_get::<Organizations>().await {
            Some(organizations) => {
                let organization = organizations
                    .resolve(target)
                    .await
                    .map_err(|error| error.to_app_error())?;
                Ok((Scope::from(&organization), organization.name))
            }
            None => Ok((Scope::organization(as_id()?), target.to_owned())),
        },
        ScopeKind::Workspace => match cx.app.try_get::<Workspaces>().await {
            Some(workspaces) => {
                let workspace = workspaces
                    .resolve(target)
                    .await
                    .map_err(|error| error.to_app_error())?;
                Ok((Scope::from(&workspace), workspace.name))
            }
            None => Ok((Scope::workspace(as_id()?), target.to_owned())),
        },
        ScopeKind::User => Ok((Scope::user(as_id()?), target.to_owned())),
    }
}

/// `entry` and the service, checked for the user: 404 without
/// `SettingsModule` or for an unknown key, 403 without the permission.
async fn entry_for(cx: &Cx, key: &str) -> Result<(Arc<Settings>, Arc<RegisteredSettings>), AppError> {
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
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let settings = service(cx).await.ok_or_else(AppError::not_found)?;
        let entries = manageable(cx, &settings).await;
        if entries.is_empty() {
            return Err(AppError::forbidden());
        }
        let base = frame.base();
        let types: Vec<Value> = entries
            .iter()
            .map(|entry| {
                let definition = &entry.definition;
                json!({
                    "key": definition.key,
                    "label": humanize(definition.key),
                    "description": definition.description,
                    "levels": definition.levels().iter().map(|l| l.as_str()).collect::<Vec<_>>(),
                    "fields": entry.fields.len(),
                    "url": format!("{base}/settings/{}", definition.key),
                })
            })
            .collect();
        Ok::<_, AppError>(
            frame
                .page(
                    StatusCode::OK,
                    "helm/settings.html",
                    "Settings",
                    "settings",
                    vec![crumb("Settings", None)],
                    json!({ "types": types }),
                )
                .await,
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

/// The form page.
async fn form_page(frame: &Frame<'_>, status: StatusCode, state: FormState<'_>) -> Response {
    let base = frame.base();
    let definition = &state.entry.definition;
    let key = definition.key;
    let label = humanize(key);
    let query = Selection::query(state.level, state.target);
    let mut fields = Vec::new();
    if let Some(resolved) = state.resolved {
        for name in &state.entry.fields {
            let current = resolved.data.get(name);
            let kind = kind_of(state.entry.defaults.get(name), current);
            let shown = state.submitted.and_then(|values| values.get(name));
            let source = resolved.source(name).unwrap_or(Source::Default);
            let mut input = Input {
                name: name.clone(),
                label: humanize(name),
                help: Some(source_text(source, state.level)),
                errors: state.errors.get(name).cloned().unwrap_or_default(),
                ..Input::default()
            };
            let text = |value: Option<&Value>| match value {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            };
            match kind {
                Kind::Boolean => {
                    input.kind = "boolean";
                    input.checked = match shown {
                        Some(raw) => raw == "true",
                        None => current == Some(&Value::Bool(true)),
                    };
                }
                Kind::Integer | Kind::Number => {
                    input.kind = "number";
                    input.input_type = "number";
                    input.step = Some(if kind == Kind::Integer { "1" } else { "any" }.to_owned());
                    input.value = shown.cloned().unwrap_or_else(|| text(current));
                }
                Kind::Text => {
                    let value = shown.cloned().unwrap_or_else(|| text(current));
                    let long = value.contains('\n') || value.chars().count() > 120;
                    input.kind = if long { "textarea" } else { "text" };
                    input.input_type = "text";
                    input.value = value;
                }
                Kind::Json => {
                    input.kind = "textarea";
                    input.value = shown.cloned().unwrap_or_else(|| {
                        current
                            .and_then(|value| serde_json::to_string_pretty(value).ok())
                            .unwrap_or_default()
                    });
                }
            }
            let mut input = serde_json::to_value(input).unwrap_or_default();
            input["overridden"] = json!(resolved.is_overridden(name));
            input["source"] = json!(source.as_str());
            fields.push(input);
        }
    }
    let levels: Vec<Value> = definition
        .levels()
        .into_iter()
        .map(|level| {
            json!({
                "value": level.as_str(),
                "label": humanize(level.as_str()),
                "selected": level == state.level,
            })
        })
        .collect();
    let url = format!("{base}/settings/{key}");
    let extra = json!({
        "settings": {
            "key": key,
            "label": label,
            "description": definition.description,
            "url": url,
            "save_url": format!("{url}{query}"),
            "inherit_url": format!("{url}/inherit{query}"),
            "index_url": format!("{base}/settings"),
        },
        "level": state.level.as_str(),
        "level_label": humanize(state.level.as_str()),
        "levels": levels,
        "needs_target": state.level != ScopeKind::Global,
        "target": state.target.unwrap_or_default(),
        "target_label": state.scope.as_ref().map(|(_, label)| label.clone()),
        "fields": fields,
        "errors_summary": !state.errors.is_empty(),
        "general_error": state.general_error,
    });
    frame
        .page(
            status,
            "helm/settings_form.html",
            &format!("{label} settings"),
            "settings",
            vec![
                crumb("Settings", Some(format!("{base}/settings"))),
                crumb(label, None),
            ],
            extra,
        )
        .await
}

/// `GET {path}/settings/{key}`: the values at a level.
pub(crate) async fn show(
    panel: Panel,
    Path(key): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let frame = Frame::new(&panel, &headers);
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
        let (scope, name) = scope_of(cx, level, target.unwrap_or_default()).await?;
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

/// A settings error as a response error.
fn settings_error(error: navree_settings::SettingsError) -> AppError {
    error
        .to_app_error()
        .unwrap_or_else(|| AppError::internal(error))
}

/// The urlencoded fields of a form, the last value of each name.
async fn form(request: Request) -> Result<HashMap<String, String>, AppError> {
    let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
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
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let (settings, entry) = entry_for(cx, &key).await?;
        let selection = Selection::read(query.as_deref());
        let level = level_of(&entry, &selection)?;
        let target = selection.target.as_deref();
        if level != ScopeKind::Global && target.is_none() {
            return Err(AppError::bad_request("Name the target of the level."));
        }
        let (scope, name) = scope_of(cx, level, target.unwrap_or_default()).await?;
        let submitted = form(request).await?;
        let current = settings
            .resolve_key(&key, scope)
            .await
            .map_err(settings_error)?;
        let mut patch = Map::new();
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for field in &entry.fields {
            let Some(raw) = submitted.get(field) else {
                continue;
            };
            let kind = kind_of(entry.defaults.get(field), current.data.get(field));
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
            return Ok(redirect_with(
                &back,
                "Nothing changed.",
                "success",
                frame.core(),
                &frame.host,
            ));
        }
        match settings.update_key(&cx.uow, &key, scope, patch).await {
            Ok(_) => Ok(redirect_with(
                &back,
                &format!("{label} settings saved."),
                "success",
                frame.core(),
                &frame.host,
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
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let (settings, entry) = entry_for(cx, &key).await?;
        let selection = Selection::read(query.as_deref());
        let level = level_of(&entry, &selection)?;
        let target = selection.target.as_deref();
        if level != ScopeKind::Global && target.is_none() {
            return Err(AppError::bad_request("Name the target of the level."));
        }
        let (scope, _) = scope_of(cx, level, target.unwrap_or_default()).await?;
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
            "success",
            frame.core(),
            &frame.host,
        ))
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

#[cfg(test)]
mod tests {
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
        let selection = Selection::read(Some("level=workspace&target=acme&other=x"));
        assert_eq!(selection.level.as_deref(), Some("workspace"));
        assert_eq!(selection.target.as_deref(), Some("acme"));
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
        assert_eq!(source_text(Source::Default, ScopeKind::Workspace), "Default value");
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

//! The SCIM operations on one connection's users and groups.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use navree_orm::{Model as _, Record};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::ScimError;
use super::filter::{ENTERPRISE_USER_SCHEMA, Filter};
use super::patch;
use super::resources::{
    Meta, Projection, group_from_resource, list_response, render, stored_resource,
    user_from_resource,
};
use crate::apply::Applier;
use crate::diff::{CurrentGroup, CurrentUser, GroupStep, UserStep};
use crate::error::DirectoryError;
use crate::models;
use crate::runtime::Runtime;
use crate::types::{ChangeAction, DirectoryConnection};
use crate::user::{DirectoryGroup, DirectoryUser};

/// A list request's paging, filter and projection.
pub(crate) struct ListQuery {
    pub(crate) filter: Option<Filter>,
    pub(crate) start_index: usize,
    pub(crate) count: usize,
    pub(crate) projection: Projection,
}

/// The SCIM server for one connection.
pub(crate) struct Server {
    rt: Arc<Runtime>,
    connection: DirectoryConnection,
}

/// A directory error as a SCIM error: internal errors are logged, and a
/// 500 without details is sent.
pub(crate) fn scim_error(error: DirectoryError) -> ScimError {
    match error {
        DirectoryError::ConnectionNotFound => ScimError::not_found("Resource not found"),
        DirectoryError::InvalidSettings(message) => ScimError::invalid_value(message),
        DirectoryError::Paused => ScimError::forbidden("The directory connection is paused"),
        other => {
            tracing::error!(target: "navree::directory", error = %other, "a SCIM request failed");
            ScimError::internal()
        }
    }
}

impl Server {
    pub(crate) fn new(rt: Arc<Runtime>, connection: DirectoryConnection) -> Self {
        Self { rt, connection }
    }

    fn location(&self, kind: &str, id: Uuid) -> String {
        format!("{}/{kind}/{id}", self.rt.scim_base)
    }

    // -- users ----------------------------------------------------------------

    async fn user_row(&self, id: &str) -> Result<models::User, ScimError> {
        let not_found = || ScimError::not_found(format!("User {id} not found"));
        let id: Uuid = id.parse().map_err(|_| not_found())?;
        models::User::query()
            .where_(models::User::ID.eq(id))
            .where_(models::User::CONNECTION_ID.eq(self.connection.id))
            .first(&*self.rt.db)
            .await
            .map_err(|e| scim_error(e.into()))?
            .map(Record::into_inner)
            .ok_or_else(not_found)
    }

    /// The resource a row's user is stored as (or, for rows that came from
    /// elsewhere, one made from the normalized user).
    fn user_resource(row: &models::User) -> Value {
        row.resource
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or_else(|| {
                let user: DirectoryUser = serde_json::from_str(&row.data).unwrap_or_default();
                resource_of(&user)
            })
    }

    async fn render_users(
        &self,
        rows: &[models::User],
        projection: &Projection,
    ) -> Result<Vec<Value>, ScimError> {
        let groups = if projection.excludes("groups") {
            HashMap::new()
        } else {
            self.groups_of_users(rows.iter().map(|row| row.id).collect())
                .await?
        };
        Ok(rows
            .iter()
            .map(|row| {
                let mut extra = Vec::new();
                if !projection.excludes("groups") {
                    let list: Vec<Value> = groups
                        .get(&row.id)
                        .into_iter()
                        .flatten()
                        .map(|(id, name)| {
                            json!({
                                "value": id.to_string(),
                                "display": name,
                                "$ref": self.location("Groups", *id),
                                "type": "direct"
                            })
                        })
                        .collect();
                    extra.push(("groups", Value::Array(list)));
                }
                let rendered = render(
                    &Self::user_resource(row),
                    &Meta {
                        id: &row.id.to_string(),
                        resource_type: "User",
                        created: row.created_at,
                        last_modified: row.updated_at,
                        location: self.location("Users", row.id),
                        version: &row.fingerprint[..16.min(row.fingerprint.len())],
                    },
                    extra,
                );
                projection.apply(rendered)
            })
            .collect())
    }

    /// Each user's groups: (group id, name).
    async fn groups_of_users(
        &self,
        users: Vec<Uuid>,
    ) -> Result<HashMap<Uuid, Vec<(Uuid, String)>>, ScimError> {
        let db = &*self.rt.db;
        let mut memberships = Vec::new();
        for chunk in users.chunks(500) {
            memberships.extend(
                models::Membership::query()
                    .where_(models::Membership::USER_ID.is_in(chunk.iter().copied()))
                    .get(db)
                    .await
                    .map_err(|e| scim_error(e.into()))?
                    .into_models(),
            );
        }
        let group_ids: BTreeSet<Uuid> = memberships.iter().map(|m| m.group_id).collect();
        let mut names = HashMap::new();
        let group_ids: Vec<Uuid> = group_ids.into_iter().collect();
        for chunk in group_ids.chunks(500) {
            for group in models::Group::query()
                .where_(models::Group::ID.is_in(chunk.iter().copied()))
                .get(db)
                .await
                .map_err(|e| scim_error(e.into()))?
                .into_models()
            {
                names.insert(group.id, group.display_name);
            }
        }
        let mut by_user: HashMap<Uuid, Vec<(Uuid, String)>> = HashMap::new();
        for membership in memberships {
            if let Some(name) = names.get(&membership.group_id) {
                by_user
                    .entry(membership.user_id)
                    .or_default()
                    .push((membership.group_id, name.clone()));
            }
        }
        for groups in by_user.values_mut() {
            groups.sort_by(|a, b| a.1.cmp(&b.1));
        }
        Ok(by_user)
    }

    pub(crate) async fn get_user(
        &self,
        id: &str,
        projection: &Projection,
    ) -> Result<Value, ScimError> {
        let row = self.user_row(id).await?;
        Ok(self.render_users(&[row], projection).await?.remove(0))
    }

    pub(crate) async fn list_users(&self, query: &ListQuery) -> Result<Value, ScimError> {
        let db = &*self.rt.db;
        let base =
            || models::User::query().where_(models::User::CONNECTION_ID.eq(self.connection.id));
        let indexed =
            query
                .filter
                .as_ref()
                .and_then(Filter::simple_eq)
                .and_then(|(attr, value)| {
                    let builder = match attr.to_ascii_lowercase().as_str() {
                        "username" => base().where_(models::User::USER_NAME.eq_ignore_case(value)),
                        "externalid" => base().where_(models::User::EXTERNAL_ID.eq(value)),
                        "id" => base().where_(
                            models::User::ID.eq(value.parse::<Uuid>().unwrap_or(Uuid::nil())),
                        ),
                        _ => return None,
                    };
                    Some(builder)
                });
        let matched: Vec<models::User> = match (indexed, &query.filter) {
            (Some(builder), _) => builder
                .order_by(models::User::CREATED_AT)
                .order_by(models::User::ID)
                .get(db)
                .await
                .map_err(|e| scim_error(e.into()))?
                .into_models(),
            (None, filter) => {
                let rows = base()
                    .order_by(models::User::CREATED_AT)
                    .order_by(models::User::ID)
                    .get(db)
                    .await
                    .map_err(|e| scim_error(e.into()))?
                    .into_models();
                match filter {
                    None => rows,
                    Some(filter) => {
                        // Filters are evaluated on the rendered resources
                        // (with their groups when the filter names them).
                        let everything = Projection::default();
                        let rendered = self.render_users(&rows, &everything).await?;
                        rows.into_iter()
                            .zip(rendered)
                            .filter(|(_, resource)| filter.matches(resource))
                            .map(|(row, _)| row)
                            .collect()
                    }
                }
            }
        };
        let matched = exact_external_id(matched, query.filter.as_ref(), |row| &row.external_id);
        let total = matched.len();
        let page: Vec<models::User> = matched
            .into_iter()
            .skip(query.start_index - 1)
            .take(query.count)
            .collect();
        let resources = self.render_users(&page, &query.projection).await?;
        Ok(list_response(total, query.start_index, resources))
    }

    pub(crate) async fn create_user(&self, body: &Value) -> Result<(Uuid, Value), ScimError> {
        let stored = stored_resource(body)?;
        let row = self.write_user(None, stored).await?;
        let id = row.id;
        Ok((
            id,
            self.render_users(&[row], &Projection::default())
                .await?
                .remove(0),
        ))
    }

    pub(crate) async fn replace_user(&self, id: &str, body: &Value) -> Result<Value, ScimError> {
        let existing = self.user_row(id).await?;
        let stored = stored_resource(body)?;
        let row = self.write_user(Some(existing), stored).await?;
        Ok(self
            .render_users(&[row], &Projection::default())
            .await?
            .remove(0))
    }

    pub(crate) async fn patch_user(&self, id: &str, body: &Value) -> Result<Value, ScimError> {
        let existing = self.user_row(id).await?;
        let ops = patch::parse(body)?;
        let mut resource = Self::user_resource(&existing);
        patch::apply(&mut resource, &ops)?;
        let stored = stored_resource(&resource)?;
        let row = self.write_user(Some(existing), stored).await?;
        Ok(self
            .render_users(&[row], &Projection::default())
            .await?
            .remove(0))
    }

    pub(crate) async fn delete_user(&self, id: &str) -> Result<(), ScimError> {
        let row = self.user_row(id).await?;
        let applier = Applier::new(self.rt.clone(), self.connection.clone(), None)
            .await
            .map_err(scim_error)?;
        self.rt
            .write(
                None,
                |uow| async move { applier.delete_user(&uow, row).await },
            )
            .await
            .map_err(scim_error)
    }

    /// Creates or changes a user from a stored resource: uniqueness, the
    /// step, then the applier in the request's unit of work.
    async fn write_user(
        &self,
        existing: Option<models::User>,
        stored: Value,
    ) -> Result<models::User, ScimError> {
        let (mut user, has_external_id) = user_from_resource(&stored)?;
        let id = existing.as_ref().map_or_else(Uuid::now_v7, |row| row.id);
        if !has_external_id {
            user.external_id = id.to_string();
        }
        let db = &*self.rt.db;
        let taken = models::User::query()
            .where_(models::User::CONNECTION_ID.eq(self.connection.id))
            .where_(models::User::ID.ne(id))
            .where_(models::User::USER_NAME.eq_ignore_case(user.user_name.clone()))
            .exists(db)
            .await
            .map_err(|e| scim_error(e.into()))?;
        if taken {
            return Err(ScimError::uniqueness(format!(
                "userName {} is already taken",
                user.user_name
            )));
        }
        let taken = models::User::query()
            .where_(models::User::CONNECTION_ID.eq(self.connection.id))
            .where_(models::User::ID.ne(id))
            .where_(models::User::EXTERNAL_ID.eq(user.external_id.clone()))
            .exists(db)
            .await
            .map_err(|e| scim_error(e.into()))?;
        if taken {
            return Err(ScimError::uniqueness(format!(
                "externalId {} is already taken",
                user.external_id
            )));
        }
        let step = match &existing {
            None => UserStep {
                action: ChangeAction::Create,
                user: user.clone(),
                previous: None,
                changed: Vec::new(),
            },
            Some(row) => {
                let previous: DirectoryUser = serde_json::from_str(&row.data).unwrap_or_default();
                if row.fingerprint == user.fingerprint() {
                    // The same user: keep the client's resource as written.
                    return self.keep_resource(row.clone(), &stored).await;
                }
                let action = match (previous.active, user.active) {
                    (true, false) => ChangeAction::Deactivate,
                    (false, true) => ChangeAction::Reactivate,
                    _ => ChangeAction::Update,
                };
                UserStep {
                    action,
                    changed: user.changed_fields(&previous),
                    user: user.clone(),
                    previous: Some(CurrentUser {
                        id: row.id,
                        user: previous,
                        fingerprint: row.fingerprint.clone(),
                        local_id: row.local_id.clone(),
                    }),
                }
            }
        };
        let applier = Applier::new(self.rt.clone(), self.connection.clone(), None)
            .await
            .map_err(scim_error)?;
        self.rt
            .write(None, |uow| async move {
                applier.user(&uow, &step, Some(id), Some(&stored)).await
            })
            .await
            .map_err(scim_error)
    }

    async fn keep_resource(
        &self,
        row: models::User,
        stored: &Value,
    ) -> Result<models::User, ScimError> {
        let text = stored.to_string();
        if row.resource.as_deref() == Some(text.as_str()) {
            return Ok(row);
        }
        let id = row.id;
        let now = self.rt.now();
        let saved = text.clone();
        self.rt
            .write(None, |uow| async move {
                models::User::query()
                    .where_(models::User::ID.eq(id))
                    .update(
                        &uow,
                        [
                            models::User::RESOURCE.set(Some(saved)),
                            models::User::UPDATED_AT.set(now),
                        ],
                    )
                    .await?;
                uow.mark_modified();
                Ok(())
            })
            .await
            .map_err(scim_error)?;
        Ok(models::User {
            resource: Some(text),
            updated_at: now,
            ..row
        })
    }

    // -- groups ---------------------------------------------------------------

    async fn group_row(&self, id: &str) -> Result<models::Group, ScimError> {
        let not_found = || ScimError::not_found(format!("Group {id} not found"));
        let id: Uuid = id.parse().map_err(|_| not_found())?;
        models::Group::query()
            .where_(models::Group::ID.eq(id))
            .where_(models::Group::CONNECTION_ID.eq(self.connection.id))
            .first(&*self.rt.db)
            .await
            .map_err(|e| scim_error(e.into()))?
            .map(Record::into_inner)
            .ok_or_else(not_found)
    }

    fn group_resource(row: &models::Group) -> Value {
        row.resource
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or_else(|| {
                let group: DirectoryGroup = serde_json::from_str(&row.data).unwrap_or_default();
                json!({ "displayName": group.display_name })
            })
    }

    /// A group's members: (user id, external id, name).
    async fn members(&self, group: Uuid) -> Result<Vec<models::User>, ScimError> {
        crate::apply::member_rows(&*self.rt.db, group)
            .await
            .map_err(scim_error)
    }

    async fn render_groups(
        &self,
        rows: &[models::Group],
        projection: &Projection,
    ) -> Result<Vec<Value>, ScimError> {
        let mut rendered = Vec::with_capacity(rows.len());
        for row in rows {
            let mut extra = Vec::new();
            if !projection.excludes("members") {
                let members: Vec<Value> = self
                    .members(row.id)
                    .await?
                    .into_iter()
                    .map(|user| {
                        json!({
                            "value": user.id.to_string(),
                            "display": user.display_name.unwrap_or(user.user_name),
                            "$ref": self.location("Users", user.id),
                            "type": "User"
                        })
                    })
                    .collect();
                extra.push(("members", Value::Array(members)));
            }
            let resource = render(
                &Self::group_resource(row),
                &Meta {
                    id: &row.id.to_string(),
                    resource_type: "Group",
                    created: row.created_at,
                    last_modified: row.updated_at,
                    location: self.location("Groups", row.id),
                    version: &row.fingerprint[..16.min(row.fingerprint.len())],
                },
                extra,
            );
            rendered.push(projection.apply(resource));
        }
        Ok(rendered)
    }

    pub(crate) async fn get_group(
        &self,
        id: &str,
        projection: &Projection,
    ) -> Result<Value, ScimError> {
        let row = self.group_row(id).await?;
        Ok(self.render_groups(&[row], projection).await?.remove(0))
    }

    pub(crate) async fn list_groups(&self, query: &ListQuery) -> Result<Value, ScimError> {
        let db = &*self.rt.db;
        let base =
            || models::Group::query().where_(models::Group::CONNECTION_ID.eq(self.connection.id));
        let indexed =
            query
                .filter
                .as_ref()
                .and_then(Filter::simple_eq)
                .and_then(|(attr, value)| {
                    let builder = match attr.to_ascii_lowercase().as_str() {
                        "displayname" => {
                            base().where_(models::Group::DISPLAY_NAME.eq_ignore_case(value))
                        }
                        "externalid" => base().where_(models::Group::EXTERNAL_ID.eq(value)),
                        "id" => base().where_(
                            models::Group::ID.eq(value.parse::<Uuid>().unwrap_or(Uuid::nil())),
                        ),
                        _ => return None,
                    };
                    Some(builder)
                });
        let matched: Vec<models::Group> = match (indexed, &query.filter) {
            (Some(builder), _) => builder
                .order_by(models::Group::CREATED_AT)
                .order_by(models::Group::ID)
                .get(db)
                .await
                .map_err(|e| scim_error(e.into()))?
                .into_models(),
            (None, filter) => {
                let rows = base()
                    .order_by(models::Group::CREATED_AT)
                    .order_by(models::Group::ID)
                    .get(db)
                    .await
                    .map_err(|e| scim_error(e.into()))?
                    .into_models();
                match filter {
                    None => rows,
                    Some(filter) => {
                        let everything = Projection::default();
                        let rendered = self.render_groups(&rows, &everything).await?;
                        rows.into_iter()
                            .zip(rendered)
                            .filter(|(_, resource)| filter.matches(resource))
                            .map(|(row, _)| row)
                            .collect()
                    }
                }
            }
        };
        let matched = exact_external_id(matched, query.filter.as_ref(), |row| &row.external_id);
        let total = matched.len();
        let page: Vec<models::Group> = matched
            .into_iter()
            .skip(query.start_index - 1)
            .take(query.count)
            .collect();
        let resources = self.render_groups(&page, &query.projection).await?;
        Ok(list_response(total, query.start_index, resources))
    }

    pub(crate) async fn create_group(&self, body: &Value) -> Result<(Uuid, Value), ScimError> {
        let row = self.write_group(None, body).await?;
        let id = row.id;
        Ok((
            id,
            self.render_groups(&[row], &Projection::default())
                .await?
                .remove(0),
        ))
    }

    pub(crate) async fn replace_group(&self, id: &str, body: &Value) -> Result<Value, ScimError> {
        let existing = self.group_row(id).await?;
        let row = self.write_group(Some(existing), body).await?;
        Ok(self
            .render_groups(&[row], &Projection::default())
            .await?
            .remove(0))
    }

    pub(crate) async fn patch_group(
        &self,
        id: &str,
        body: &Value,
        projection: &Projection,
    ) -> Result<Value, ScimError> {
        let existing = self.group_row(id).await?;
        let ops = patch::parse(body)?;
        let mut resource = Self::group_resource(&existing);
        let members: Vec<Value> = self
            .members(existing.id)
            .await?
            .into_iter()
            .map(|user| json!({ "value": user.id.to_string() }))
            .collect();
        resource["members"] = Value::Array(members);
        patch::apply(&mut resource, &ops)?;
        let row = self.write_group(Some(existing), &resource).await?;
        Ok(self.render_groups(&[row], projection).await?.remove(0))
    }

    pub(crate) async fn delete_group(&self, id: &str) -> Result<(), ScimError> {
        let row = self.group_row(id).await?;
        let previous = self.current_group(&row).await?;
        let step = GroupStep {
            action: ChangeAction::Delete,
            group: previous.group.clone(),
            removed: previous.group.members.clone(),
            previous: Some(previous),
            changed: Vec::new(),
            added: Vec::new(),
        };
        let applier = Applier::new(self.rt.clone(), self.connection.clone(), None)
            .await
            .map_err(scim_error)?;
        self.rt
            .write(None, |uow| async move {
                applier.group(&uow, &step, None, None).await.map(|_| ())
            })
            .await
            .map_err(scim_error)
    }

    async fn current_group(&self, row: &models::Group) -> Result<CurrentGroup, ScimError> {
        let mut group: DirectoryGroup = serde_json::from_str(&row.data).unwrap_or_default();
        group.members = self
            .members(row.id)
            .await?
            .into_iter()
            .map(|user| user.external_id)
            .collect();
        group.members.sort();
        Ok(CurrentGroup {
            id: row.id,
            group,
            fingerprint: row.fingerprint.clone(),
            local_id: row.local_id.clone(),
        })
    }

    /// Creates or changes a group from a client's resource (members by
    /// SCIM id; ids that are not users of this connection are left out).
    async fn write_group(
        &self,
        existing: Option<models::Group>,
        body: &Value,
    ) -> Result<models::Group, ScimError> {
        let mut stored = stored_resource(body)?;
        let (mut group, member_ids) = group_from_resource(&stored)?;
        if let Value::Object(object) = &mut stored {
            object.retain(|key, _| !key.eq_ignore_ascii_case("members"));
        }
        let id = existing.as_ref().map_or_else(Uuid::now_v7, |row| row.id);
        if group.external_id.is_empty() {
            group.external_id = id.to_string();
        }
        let db = &*self.rt.db;
        let taken = models::Group::query()
            .where_(models::Group::CONNECTION_ID.eq(self.connection.id))
            .where_(models::Group::ID.ne(id))
            .where_(models::Group::EXTERNAL_ID.eq(group.external_id.clone()))
            .exists(db)
            .await
            .map_err(|e| scim_error(e.into()))?;
        if taken {
            return Err(ScimError::uniqueness(format!(
                "externalId {} is already taken",
                group.external_id
            )));
        }
        // Members by SCIM id to members by external id.
        let ids: Vec<Uuid> = member_ids.iter().filter_map(|id| id.parse().ok()).collect();
        let mut members = BTreeSet::new();
        for chunk in ids.chunks(500) {
            for user in models::User::query()
                .where_(models::User::CONNECTION_ID.eq(self.connection.id))
                .where_(models::User::ID.is_in(chunk.iter().copied()))
                .get(db)
                .await
                .map_err(|e| scim_error(e.into()))?
                .into_models()
            {
                members.insert(user.external_id);
            }
        }
        if members.len() < member_ids.len() {
            tracing::debug!(target: "navree::directory", connection = %self.connection.id, "a SCIM group named members that are not users of the connection");
        }
        group.members = members.iter().cloned().collect();
        let step = match &existing {
            None => GroupStep {
                action: ChangeAction::Create,
                added: group.members.clone(),
                group,
                previous: None,
                changed: Vec::new(),
                removed: Vec::new(),
            },
            Some(row) => {
                let previous = self.current_group(row).await?;
                let before: BTreeSet<String> = previous.group.members.iter().cloned().collect();
                let added: Vec<String> = members.difference(&before).cloned().collect();
                let removed: Vec<String> = before.difference(&members).cloned().collect();
                let mut changed = group.changed_fields(&previous.group);
                if !added.is_empty() || !removed.is_empty() {
                    changed.push("members".to_owned());
                }
                GroupStep {
                    action: ChangeAction::Update,
                    group,
                    previous: Some(previous),
                    changed,
                    added,
                    removed,
                }
            }
        };
        let applier = Applier::new(self.rt.clone(), self.connection.clone(), None)
            .await
            .map_err(scim_error)?;
        self.rt
            .write(None, |uow| async move {
                applier.group(&uow, &step, Some(id), Some(&stored)).await
            })
            .await
            .map_err(scim_error)
    }
}

/// A SCIM resource made from a normalized user (for rows that came from
/// elsewhere than a SCIM client).
pub(crate) fn resource_of(user: &DirectoryUser) -> Value {
    let mut resource = Map::new();
    resource.insert("userName".into(), json!(user.user_name));
    resource.insert("externalId".into(), json!(user.external_id));
    resource.insert("active".into(), json!(user.active));
    let mut name = Map::new();
    if let Some(given) = &user.given_name {
        name.insert("givenName".into(), json!(given));
    }
    if let Some(family) = &user.family_name {
        name.insert("familyName".into(), json!(family));
    }
    if !name.is_empty() {
        resource.insert("name".into(), Value::Object(name));
    }
    if let Some(display) = &user.display_name {
        resource.insert("displayName".into(), json!(display));
    }
    if !user.emails.is_empty() {
        resource.insert("emails".into(), json!(user.emails));
    }
    if !user.phone_numbers.is_empty() {
        resource.insert("phoneNumbers".into(), json!(user.phone_numbers));
    }
    if let Some(title) = &user.title {
        resource.insert("title".into(), json!(title));
    }
    if let Some(locale) = &user.locale {
        resource.insert("locale".into(), json!(locale));
    }
    let mut enterprise = Map::new();
    if let Some(number) = &user.employee_number {
        enterprise.insert("employeeNumber".into(), json!(number));
    }
    if let Some(department) = &user.department {
        enterprise.insert("department".into(), json!(department));
    }
    if let Some(organization) = &user.organization {
        enterprise.insert("organization".into(), json!(organization));
    }
    if let Some(manager) = &user.manager_id {
        enterprise.insert("manager".into(), json!({ "value": manager }));
    }
    if !enterprise.is_empty() {
        resource.insert(ENTERPRISE_USER_SCHEMA.into(), Value::Object(enterprise));
    }
    Value::Object(resource)
}

/// `externalId` compares case-exact, but a database whose collation ignores
/// case (MySQL's default) answers an indexed `externalId eq` lookup with
/// every spelling of it: keep the rows that match exactly.
fn exact_external_id<T>(
    rows: Vec<T>,
    filter: Option<&Filter>,
    external_id: impl Fn(&T) -> &String,
) -> Vec<T> {
    match filter
        .and_then(Filter::simple_eq)
        .filter(|(attribute, _)| attribute.eq_ignore_ascii_case("externalId"))
    {
        Some((_, wanted)) => rows
            .into_iter()
            .filter(|row| external_id(row) == wanted)
            .collect(),
        None => rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resource_made_from_a_user_reads_back_as_the_same_user() {
        let user = DirectoryUser::new("u1", "anu@acme.test")
            .name("Anu", "Lakhotia")
            .display_name("Anu Lakhotia")
            .email("anu@acme.test")
            .phone("mobile", "+44 7700 900123")
            .title("Engineer")
            .department("Engineering")
            .employee_number("1815")
            .manager("u0");
        let (back, has_external_id) = user_from_resource(&resource_of(&user)).unwrap();
        assert!(has_external_id);
        assert_eq!(back, user);
    }
}

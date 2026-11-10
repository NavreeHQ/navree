//! Applying changes: the directory rows, your sinks and the events, in one
//! unit of work. Sync runs and the SCIM server both apply through here.

use std::collections::HashMap;
use std::sync::Arc;

use navree_db::Uow;
use navree_orm::{Model as _, Record};
use serde_json::Value;
use uuid::Uuid;

use crate::diff::{GroupStep, UserStep};
use crate::error::DirectoryError;
use crate::events::{DirectoryUserCreated, DirectoryUserDeactivated, DirectoryUserUpdated};
use crate::models;
use crate::runtime::Runtime;
use crate::sink::{DirectorySink, GroupChange, SinkContext, UserChange};
use crate::types::{ChangeAction, DirectoryConnection};
use crate::user::{DirectoryGroup, DirectoryUser};

/// Applies the changes of one connection.
pub(crate) struct Applier {
    rt: Arc<Runtime>,
    connection: DirectoryConnection,
    run_id: Option<Uuid>,
    sinks: Vec<Arc<dyn DirectorySink>>,
}

impl Applier {
    pub(crate) async fn new(
        rt: Arc<Runtime>,
        connection: DirectoryConnection,
        run_id: Option<Uuid>,
    ) -> Result<Applier, DirectoryError> {
        let sinks = rt.sinks().await?;
        Ok(Applier {
            rt,
            connection,
            run_id,
            sinks,
        })
    }

    fn context(&self, uow: &Uow) -> Result<SinkContext, DirectoryError> {
        Ok(SinkContext {
            connection: self.connection.clone(),
            uow: uow.clone(),
            app: self.rt.app()?,
            run_id: self.run_id,
        })
    }

    /// Applies a user step in `uow`: the row (created with `id` when
    /// given), the sinks, and the event after the commit. `resource` is
    /// the SCIM resource to keep.
    pub(crate) async fn user(
        &self,
        uow: &Uow,
        step: &UserStep,
        id: Option<Uuid>,
        resource: Option<&Value>,
    ) -> Result<models::User, DirectoryError> {
        let now = self.rt.now();
        let user = &step.user;
        let data = serde_json::to_string(user).unwrap_or_default();
        let resource = resource.map(|r| serde_json::to_string(r).unwrap_or_default());
        let mut row = {
            let mut record = match &step.previous {
                None => Record::new(models::User {
                    id: id.unwrap_or_else(Uuid::now_v7),
                    connection_id: self.connection.id,
                    created_at: now,
                    ..models::User::default()
                }),
                Some(previous) => models::User::query()
                    .where_(models::User::ID.eq(previous.id))
                    .first(uow)
                    .await?
                    .ok_or(DirectoryError::ConnectionNotFound)?,
            };
            record.external_id = user.external_id.clone();
            record.user_name = user.user_name.clone();
            record.email = user.primary_email().map(str::to_owned);
            record.display_name = Some(user.full_name());
            record.active = user.active;
            record.data = data;
            if resource.is_some() {
                record.resource = resource;
            }
            record.fingerprint = user.fingerprint();
            record.updated_at = now;
            record.deactivated_at = match (user.active, record.deactivated_at) {
                (true, _) => None,
                (false, Some(at)) => Some(at),
                (false, None) => Some(now),
            };
            record.save(uow).await?;
            uow.mark_modified();
            record.into_inner()
        };
        let change = UserChange {
            action: step.action,
            directory_user_id: row.id,
            user: user.clone(),
            previous: step.previous.as_ref().map(|p| p.user.clone()),
            changed: step.changed.clone(),
            local_id: row.local_id.clone(),
            deleted: false,
        };
        let ctx = self.context(uow)?;
        let mut local_id = row.local_id.clone();
        for sink in &self.sinks {
            if step.action == ChangeAction::Deactivate {
                sink.deactivate_user(&ctx, &change)
                    .await
                    .map_err(DirectoryError::Sink)?;
            } else if let Some(id) = sink
                .upsert_user(&ctx, &change)
                .await
                .map_err(DirectoryError::Sink)?
            {
                local_id = Some(id);
            }
        }
        if local_id != row.local_id {
            models::User::query()
                .where_(models::User::ID.eq(row.id))
                .update(uow, [models::User::LOCAL_ID.set(local_id.clone())])
                .await?;
            row.local_id = local_id;
        }
        self.user_event(uow, step, &row, user.clone(), false).await;
        Ok(row)
    }

    async fn user_event(
        &self,
        uow: &Uow,
        step: &UserStep,
        row: &models::User,
        user: DirectoryUser,
        deleted: bool,
    ) {
        let connection_id = self.connection.id;
        let owner = self.connection.owner();
        let run_id = self.run_id;
        match step.action {
            ChangeAction::Create => {
                self.rt
                    .emit_after_commit(
                        uow,
                        DirectoryUserCreated {
                            connection_id,
                            owner,
                            directory_user_id: row.id,
                            local_id: row.local_id.clone(),
                            user,
                            run_id,
                        },
                    )
                    .await;
            }
            ChangeAction::Deactivate => {
                self.rt
                    .emit_after_commit(
                        uow,
                        DirectoryUserDeactivated {
                            connection_id,
                            owner,
                            directory_user_id: row.id,
                            local_id: row.local_id.clone(),
                            user,
                            deleted,
                            run_id,
                        },
                    )
                    .await;
            }
            ChangeAction::Update | ChangeAction::Reactivate | ChangeAction::Delete => {
                self.rt
                    .emit_after_commit(
                        uow,
                        DirectoryUserUpdated {
                            connection_id,
                            owner,
                            directory_user_id: row.id,
                            local_id: row.local_id.clone(),
                            user,
                            changed: step.changed.clone(),
                            reactivated: step.action == ChangeAction::Reactivate,
                            run_id,
                        },
                    )
                    .await;
            }
        }
    }

    /// Deletes a user the SCIM client deleted: the sinks deactivate them
    /// (`deleted`), then the row and its memberships go.
    pub(crate) async fn delete_user(
        &self,
        uow: &Uow,
        row: models::User,
    ) -> Result<(), DirectoryError> {
        let mut user: DirectoryUser = serde_json::from_str(&row.data).unwrap_or_default();
        let previous = user.clone();
        user.active = false;
        let change = UserChange {
            action: ChangeAction::Deactivate,
            directory_user_id: row.id,
            user: user.clone(),
            previous: Some(previous),
            changed: vec!["active".to_owned()],
            local_id: row.local_id.clone(),
            deleted: true,
        };
        let ctx = self.context(uow)?;
        for sink in &self.sinks {
            sink.deactivate_user(&ctx, &change)
                .await
                .map_err(DirectoryError::Sink)?;
        }
        {
            models::Membership::query()
                .where_(models::Membership::USER_ID.eq(row.id))
                .delete(uow)
                .await?;
            models::User::query()
                .where_(models::User::ID.eq(row.id))
                .delete(uow)
                .await?;
            uow.mark_modified();
        }
        let step = UserStep {
            action: ChangeAction::Deactivate,
            user: user.clone(),
            previous: None,
            changed: change.changed.clone(),
        };
        self.user_event(uow, &step, &row, user, true).await;
        Ok(())
    }

    /// Applies a group step in `uow`: the row, its memberships and the
    /// sinks. `resource` is the SCIM resource to keep (without members).
    pub(crate) async fn group(
        &self,
        uow: &Uow,
        step: &GroupStep,
        id: Option<Uuid>,
        resource: Option<&Value>,
    ) -> Result<models::Group, DirectoryError> {
        let now = self.rt.now();
        let group = &step.group;
        if step.action == ChangeAction::Delete {
            let previous = step
                .previous
                .as_ref()
                .ok_or(DirectoryError::ConnectionNotFound)?;
            let row = {
                models::Group::query()
                    .where_(models::Group::ID.eq(previous.id))
                    .first(uow)
                    .await?
                    .map(Record::into_inner)
            };
            let change = self.group_change(step, previous.id, previous.local_id.clone());
            let ctx = self.context(uow)?;
            for sink in &self.sinks {
                sink.delete_group(&ctx, &change)
                    .await
                    .map_err(DirectoryError::Sink)?;
            }
            models::Membership::query()
                .where_(models::Membership::GROUP_ID.eq(previous.id))
                .delete(uow)
                .await?;
            models::Group::query()
                .where_(models::Group::ID.eq(previous.id))
                .delete(uow)
                .await?;
            uow.mark_modified();
            return Ok(row.unwrap_or_default());
        }
        let data = serde_json::to_string(&group.without_members()).unwrap_or_default();
        let resource = resource.map(|r| serde_json::to_string(r).unwrap_or_default());
        let mut row = {
            let mut record = match &step.previous {
                None => Record::new(models::Group {
                    id: id.unwrap_or_else(Uuid::now_v7),
                    connection_id: self.connection.id,
                    created_at: now,
                    ..models::Group::default()
                }),
                Some(previous) => models::Group::query()
                    .where_(models::Group::ID.eq(previous.id))
                    .first(uow)
                    .await?
                    .ok_or(DirectoryError::ConnectionNotFound)?,
            };
            record.external_id = group.external_id.clone();
            record.display_name = group.display_name.clone();
            record.data = data;
            if resource.is_some() {
                record.resource = resource;
            }
            record.fingerprint = group.fingerprint();
            record.updated_at = now;
            record.save(uow).await?;
            uow.mark_modified();
            record.into_inner()
        };
        self.write_members(uow, row.id, &step.added, &step.removed)
            .await?;
        let change = self.group_change(step, row.id, row.local_id.clone());
        let ctx = self.context(uow)?;
        let mut local_id = row.local_id.clone();
        for sink in &self.sinks {
            if let Some(id) = sink
                .upsert_group(&ctx, &change)
                .await
                .map_err(DirectoryError::Sink)?
            {
                local_id = Some(id);
            }
        }
        if local_id != row.local_id {
            models::Group::query()
                .where_(models::Group::ID.eq(row.id))
                .update(uow, [models::Group::LOCAL_ID.set(local_id.clone())])
                .await?;
            row.local_id = local_id;
        }
        Ok(row)
    }

    fn group_change(&self, step: &GroupStep, id: Uuid, local_id: Option<String>) -> GroupChange {
        GroupChange {
            action: step.action,
            directory_group_id: id,
            group: step.group.clone(),
            previous: step.previous.as_ref().map(|p| p.group.clone()),
            changed: step.changed.clone(),
            added: step.added.clone(),
            removed: step.removed.clone(),
            local_id,
        }
    }

    /// Adds and removes memberships of group `group_id`, by user external
    /// id.
    async fn write_members(
        &self,
        uow: &Uow,
        group_id: Uuid,
        added: &[String],
        removed: &[String],
    ) -> Result<(), DirectoryError> {
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }
        let ids = user_ids(uow, self.connection.id, added.iter().chain(removed)).await?;
        let rows: Vec<models::Membership> = added
            .iter()
            .filter_map(|external| ids.get(external))
            .map(|user_id| models::Membership {
                id: Uuid::now_v7(),
                connection_id: self.connection.id,
                group_id,
                user_id: *user_id,
            })
            .collect();
        if !rows.is_empty() {
            models::Membership::query()
                .insert_or_ignore(uow, rows)
                .await?;
        }
        let gone: Vec<Uuid> = removed
            .iter()
            .filter_map(|external| ids.get(external).copied())
            .collect();
        for chunk in gone.chunks(500) {
            models::Membership::query()
                .where_(models::Membership::GROUP_ID.eq(group_id))
                .where_(models::Membership::USER_ID.is_in(chunk.iter().copied()))
                .delete(uow)
                .await?;
        }
        uow.mark_modified();
        Ok(())
    }
}

/// The directory ids of users of `connection`, by external id.
async fn user_ids<'a, E: navree_orm::Executor + ?Sized>(
    exec: &E,
    connection: Uuid,
    external_ids: impl Iterator<Item = &'a String>,
) -> Result<HashMap<String, Uuid>, DirectoryError> {
    let external_ids: Vec<String> = external_ids.cloned().collect();
    let mut found = HashMap::new();
    for chunk in external_ids.chunks(500) {
        let rows = models::User::query()
            .where_(models::User::CONNECTION_ID.eq(connection))
            .where_(models::User::EXTERNAL_ID.is_in(chunk.iter().cloned()))
            .get(exec)
            .await?
            .into_models();
        for row in rows {
            found.insert(row.external_id, row.id);
        }
    }
    Ok(found)
}

/// The groups a user belongs to.
pub(crate) async fn groups_of<E: navree_orm::Executor + ?Sized>(
    exec: &E,
    user_id: Uuid,
) -> Result<Vec<models::Group>, DirectoryError> {
    let group_ids: Vec<Uuid> = models::Membership::query()
        .where_(models::Membership::USER_ID.eq(user_id))
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(|m| m.group_id)
        .collect();
    if group_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(models::Group::query()
        .where_(models::Group::ID.is_in(group_ids))
        .order_by(models::Group::DISPLAY_NAME)
        .get(exec)
        .await?
        .into_models())
}

/// The normalized group of a row, with its members' external ids.
pub(crate) async fn group_with_members<E: navree_orm::Executor + ?Sized>(
    exec: &E,
    row: &models::Group,
) -> Result<DirectoryGroup, DirectoryError> {
    let mut group: DirectoryGroup = serde_json::from_str(&row.data).unwrap_or_default();
    group.members = member_rows(exec, row.id)
        .await?
        .into_iter()
        .map(|user| user.external_id)
        .collect();
    Ok(group)
}

/// The users in group `group_id`, by sign-in name.
pub(crate) async fn member_rows<E: navree_orm::Executor + ?Sized>(
    exec: &E,
    group_id: Uuid,
) -> Result<Vec<models::User>, DirectoryError> {
    let user_ids: Vec<Uuid> = models::Membership::query()
        .where_(models::Membership::GROUP_ID.eq(group_id))
        .get(exec)
        .await?
        .into_models()
        .into_iter()
        .map(|m| m.user_id)
        .collect();
    let mut users = Vec::new();
    for chunk in user_ids.chunks(500) {
        users.extend(
            models::User::query()
                .where_(models::User::ID.is_in(chunk.iter().copied()))
                .get(exec)
                .await?
                .into_models(),
        );
    }
    users.sort_by(|a, b| a.user_name.cmp(&b.user_name));
    Ok(users)
}

//! Workspace settings and notification preferences.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use navree::db::Uow;
use navree::http::AppError;
use navree::injectable;
use navree::orm::{Model as _, OrmError, Record};
use navree::workspace::{Workspace, WorkspaceRepo, Workspaces};
use uuid::Uuid;

use super::dto::{NotificationSettings, UpdateNotifications, UpdateWorkspace};
use super::entity::NotificationSetting;

/// Changes a workspace's name and slug, and members' notification
/// preferences.
#[injectable]
pub struct SettingsService {
    workspaces: Arc<Workspaces>,
}

impl SettingsService {
    /// Renames `current` and changes its slug, for the fields present in
    /// `input`. A taken slug is a 400 on `slug`.
    pub async fn update_workspace(
        &self,
        uow: &Uow,
        current: Workspace,
        input: UpdateWorkspace,
    ) -> Result<Workspace, AppError> {
        let mut workspace = current;
        if let Some(name) = input.name {
            workspace = self.workspaces.rename(uow, workspace.id, &name).await?;
        }
        if let Some(slug) = input.slug
            && slug != workspace.slug
        {
            if self.workspaces.find_by_slug(&slug).await?.is_some() {
                return Err(AppError::validation(BTreeMap::from([(
                    "slug".to_owned(),
                    vec!["slug has already been taken".to_owned()],
                )])));
            }
            let Some(mut record) = Workspace::query()
                .where_key(workspace.id)
                .first(uow)
                .await?
            else {
                return Err(AppError::not_found());
            };
            record.slug = slug;
            record.updated_at = Utc::now();
            record.save(uow).await?;
            workspace = record.model().clone();
        }
        Ok(workspace)
    }

    /// The preferences of `user` in `workspace` (the defaults until they
    /// save some).
    pub async fn notifications(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
    ) -> Result<NotificationSettings, OrmError> {
        let saved = Self::saved(uow, workspace, user).await?;
        Ok(saved
            .as_ref()
            .map_or_else(NotificationSettings::default, preferences))
    }

    /// Saves the fields present in `input`.
    pub async fn update_notifications(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
        input: UpdateNotifications,
    ) -> Result<NotificationSettings, OrmError> {
        let saved = Self::saved(uow, workspace, user).await?;
        let current = saved
            .as_ref()
            .map_or_else(NotificationSettings::default, preferences);
        let wanted = NotificationSettings {
            project_activity: input.project_activity.unwrap_or(current.project_activity),
            weekly_summary: input.weekly_summary.unwrap_or(current.weekly_summary),
            product_updates: input.product_updates.unwrap_or(current.product_updates),
        };
        match saved {
            Some(mut row) => {
                row.project_activity = wanted.project_activity;
                row.weekly_summary = wanted.weekly_summary;
                row.product_updates = wanted.product_updates;
                WorkspaceRepo::<NotificationSetting>::update(uow, workspace, &mut row).await?;
            }
            None => {
                WorkspaceRepo::<NotificationSetting>::insert(
                    uow,
                    workspace,
                    NotificationSetting {
                        user_id: user,
                        project_activity: wanted.project_activity,
                        weekly_summary: wanted.weekly_summary,
                        product_updates: wanted.product_updates,
                        ..NotificationSetting::default()
                    },
                )
                .await?;
            }
        }
        Ok(wanted)
    }

    /// The row holding `user`'s preferences in `workspace`, if they saved any.
    async fn saved(
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
    ) -> Result<Option<Record<NotificationSetting>>, OrmError> {
        WorkspaceRepo::<NotificationSetting>::query(workspace)
            .where_(NotificationSetting::USER_ID.eq(user))
            .first(uow)
            .await
    }
}

/// The preferences a stored row holds.
fn preferences(row: &Record<NotificationSetting>) -> NotificationSettings {
    NotificationSettings {
        project_activity: row.project_activity,
        weekly_summary: row.weekly_summary,
        product_updates: row.product_updates,
    }
}

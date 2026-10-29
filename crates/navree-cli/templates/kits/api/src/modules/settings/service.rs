//! Workspace settings and notification preferences.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use navree::db::{Repo, Uow};
use navree::http::AppError;
use navree::injectable;
use navree::workspace::entity::workspace;
// COMPAT (data-layer move): the SeaORM form of `WorkspaceRepo`.
use navree::workspace::sea_compat::WorkspaceRepo;
use navree::workspace::{Workspace, Workspaces};
use sea_orm::{ActiveValue::Set, ColumnTrait, DbErr, IntoActiveModel, QueryFilter};
use uuid::Uuid;

use super::dto::{NotificationSettings, UpdateNotifications, UpdateWorkspace};
use super::entity::{ActiveModel, Column, Entity};

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
            let mut active: workspace::ActiveModel = workspace.into_active_model();
            active.slug = Set(slug);
            active.updated_at = Set(Utc::now());
            workspace = Repo::<workspace::Entity>::update(uow, active).await?;
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
    ) -> Result<NotificationSettings, DbErr> {
        let saved = WorkspaceRepo::<Entity>::select(workspace)
            .filter(Column::UserId.eq(user))
            .one(&*uow.tx().await?)
            .await?;
        Ok(
            saved.map_or_else(NotificationSettings::default, |row| NotificationSettings {
                project_activity: row.project_activity,
                weekly_summary: row.weekly_summary,
                product_updates: row.product_updates,
            }),
        )
    }

    /// Saves the fields present in `input`.
    pub async fn update_notifications(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
        input: UpdateNotifications,
    ) -> Result<NotificationSettings, DbErr> {
        let now = Utc::now();
        let saved = WorkspaceRepo::<Entity>::select(workspace)
            .filter(Column::UserId.eq(user))
            .one(&*uow.tx().await?)
            .await?;
        let current = self.notifications(uow, workspace, user).await?;
        let wanted = NotificationSettings {
            project_activity: input.project_activity.unwrap_or(current.project_activity),
            weekly_summary: input.weekly_summary.unwrap_or(current.weekly_summary),
            product_updates: input.product_updates.unwrap_or(current.product_updates),
        };
        match saved {
            Some(row) => {
                let mut active: ActiveModel = row.into();
                active.project_activity = Set(wanted.project_activity);
                active.weekly_summary = Set(wanted.weekly_summary);
                active.product_updates = Set(wanted.product_updates);
                active.updated_at = Set(now);
                WorkspaceRepo::<Entity>::update(uow, workspace, active).await?;
            }
            None => {
                WorkspaceRepo::<Entity>::insert(
                    uow,
                    workspace,
                    ActiveModel {
                        id: Set(Uuid::now_v7()),
                        user_id: Set(user),
                        project_activity: Set(wanted.project_activity),
                        weekly_summary: Set(wanted.weekly_summary),
                        product_updates: Set(wanted.product_updates),
                        created_at: Set(now),
                        updated_at: Set(now),
                        ..Default::default()
                    },
                )
                .await?;
            }
        }
        Ok(wanted)
    }
}

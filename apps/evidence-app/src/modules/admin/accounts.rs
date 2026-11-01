//! Accounts in the panel: users and workspaces.

use chrono::Utc;
use navree::BoxFuture;
use navree::auth::User;
use navree::helm::SaveContext;
use navree::helm::{Action, ActionResult, Field, Filter, FormData, Metric, RecordOf, Resource};
use navree::prelude::*;
use navree::workspace::Workspace;

use super::work::ProjectAdmin;

/// Users: every account, searchable by name and email.
#[injectable]
pub struct UserAdmin;

impl Resource for UserAdmin {
    type Model = User;

    const LABEL: &'static str = "Users";
    const SINGULAR: &'static str = "User";
    const GROUP: &'static str = "Accounts";
    const ICON: &'static str = "users";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "email"];
    const ORDER: i32 = 1;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().max_length(100).sortable(),
            Field::email("email").readonly().sortable(),
            Field::datetime("email_verified_at")
                .label("Verified")
                .readonly()
                .sortable(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn filters(&self) -> Vec<Filter<User>> {
        vec![
            Filter::boolean("verified", "Email verified", |query, on| {
                if on {
                    query.where_(User::EMAIL_VERIFIED_AT.is_not_null())
                } else {
                    query.where_(User::EMAIL_VERIFIED_AT.is_null())
                }
            }),
            Filter::date_range("created_at").label("Signed up"),
        ]
    }

    fn actions(&self) -> Vec<Action<User>> {
        vec![
            Action::<User>::new("verify-email", "Mark email as verified")
                .confirm("Mark the email address of the selected users as verified?")
                .handle(|ctx, users| async move {
                    let mut verified = 0;
                    for mut user in users {
                        if user.email_verified_at.is_some() {
                            continue;
                        }
                        user.update(ctx.uow(), |user| {
                            let now = Utc::now();
                            user.email_verified_at = Some(now);
                            user.updated_at = now;
                        })
                        .await?;
                        verified += 1;
                    }
                    Ok(ActionResult::message(format!(
                        "{verified} email addresses marked as verified."
                    )))
                }),
        ]
    }

    fn metrics(&self) -> Vec<Metric<User>> {
        vec![
            Metric::value("Users").count().on_dashboard(),
            Metric::value("New users").count().over(User::CREATED_AT),
            Metric::trend("Sign-ups")
                .count()
                .by_day(User::CREATED_AT, 30)
                .on_dashboard(),
        ]
    }

    /// Accounts sign up through the API (`POST /api/auth/register`).
    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>>> {
        Box::pin(async move { Err(AppError::bad_request("Accounts sign up in the app.")) })
    }
}

/// Workspaces.
#[injectable]
pub struct WorkspaceAdmin;

impl Resource for WorkspaceAdmin {
    type Model = Workspace;

    const LABEL: &'static str = "Workspaces";
    const SINGULAR: &'static str = "Workspace";
    const GROUP: &'static str = "Accounts";
    const ICON: &'static str = "building-2";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "slug"];
    const ORDER: i32 = 2;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().max_length(100).sortable(),
            Field::text("slug").readonly().sortable(),
            Field::select("kind")
                .options([("live", "Live"), ("demo", "Demo")])
                .badge()
                .readonly(),
            Field::belongs_to::<UserAdmin>("owner_id")
                .label("Owner")
                .readonly(),
            Field::text("timezone").readonly().hide_on_index(),
            Field::datetime("created_at").readonly().sortable(),
            Field::has_many::<ProjectAdmin>("workspace_id").label("Projects"),
        ]
    }

    fn filters(&self) -> Vec<Filter<Workspace>> {
        vec![
            Filter::select("kind").options([("live", "Live"), ("demo", "Demo")]),
            Filter::belongs_to::<UserAdmin>("owner_id").label("Owner"),
            Filter::date_range("created_at").label("Created"),
        ]
    }

    fn metrics(&self) -> Vec<Metric<Workspace>> {
        vec![
            Metric::value("Workspaces").count().on_dashboard(),
            Metric::partition("By kind")
                .count()
                .group_by(Workspace::KIND),
        ]
    }

    /// Workspaces are created by their owners (`POST /api/workspaces`).
    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>>> {
        Box::pin(async move {
            Err(AppError::bad_request(
                "Workspaces are created by their owners in the app.",
            ))
        })
    }
}

//! The admin panel's resources: what Helm shows and edits for each entity.

use garde::Validate;
use navree::BoxFuture;
use navree::auth::{AuthService, user};
use navree::billing::entity::subscription;
use navree::crypto::generate_key;
use navree::helm::{Field, Filter, FormData, Metric, Resource, SaveContext};
use navree::prelude::*;
use navree::workspace::Workspaces;
use navree::workspace::entity::workspace;
use sea_orm::{ColumnTrait, QueryFilter};
use serde::Deserialize;

/// The fields of a user created in the panel.
#[derive(Debug, Deserialize, Validate)]
struct NewUserForm {
    #[garde(length(min = 1, max = 100))]
    name: String,
    #[garde(email)]
    email: String,
}

/// Users: every account, with search by name and email.
#[injectable]
pub struct UserAdmin;

impl Resource for UserAdmin {
    type Entity = user::Entity;

    const LABEL: &'static str = "Users";
    const SINGULAR: &'static str = "User";
    const GROUP: &'static str = "Accounts";
    const ICON: &'static str = "users";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "email"];

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().max_length(100).sortable(),
            Field::email("email").required().sortable(),
            Field::datetime("email_verified_at")
                .label("Verified")
                .readonly(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn filters(&self) -> Vec<Filter<user::Entity>> {
        vec![Filter::boolean(
            "unverified",
            "Unverified only",
            |select, on| {
                if on {
                    select.filter(user::Column::EmailVerifiedAt.is_null())
                } else {
                    select
                }
            },
        )]
    }

    fn metrics(&self) -> Vec<Metric<user::Entity>> {
        vec![
            Metric::value("Users").count().on_dashboard(),
            Metric::trend("Sign-ups")
                .count()
                .by_day(user::Column::CreatedAt, 30),
        ]
    }

    /// Creates the account with a random password through `AuthService`:
    /// the person sets their own with "Forgot password".
    fn create<'a>(
        &'a self,
        ctx: &'a SaveContext,
        form: FormData,
    ) -> BoxFuture<'a, Result<user::Model>> {
        Box::pin(async move {
            let input: NewUserForm = form.parse().await?;
            let auth = ctx.get::<AuthService>().await?;
            Ok(auth
                .create_user(&input.name, &input.email, &generate_key())
                .await?)
        })
    }
}

/// Workspaces: name, slug and owner.
#[injectable]
pub struct WorkspaceAdmin;

impl Resource for WorkspaceAdmin {
    type Entity = workspace::Entity;

    const LABEL: &'static str = "Workspaces";
    const SINGULAR: &'static str = "Workspace";
    const GROUP: &'static str = "Accounts";
    const ICON: &'static str = "building-2";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "slug"];

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().max_length(100).sortable(),
            Field::text("slug").readonly().sortable(),
            Field::belongs_to::<UserAdmin>("owner_id")
                .label("Owner")
                .readonly(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn metrics(&self) -> Vec<Metric<workspace::Entity>> {
        vec![Metric::value("Workspaces").count().on_dashboard()]
    }

    /// Workspaces are created by their owners (`POST /api/workspaces`).
    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<workspace::Model>> {
        Box::pin(async move {
            Err(AppError::bad_request(
                "Workspaces are created by their owners in the app.",
            ))
        })
    }

    /// Deletes through `Workspaces`, which removes the members and
    /// invitations too.
    fn delete<'a>(
        &'a self,
        ctx: &'a SaveContext,
        record: workspace::Model,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let workspaces = ctx.get::<Workspaces>().await?;
            workspaces.delete(ctx.uow(), record.id).await?;
            Ok(())
        })
    }
}

const STATUSES: [(&str, &str); 6] = [
    ("trialing", "Trialing"),
    ("active", "Active"),
    ("past_due", "Past due"),
    ("paused", "Paused"),
    ("canceled", "Canceled"),
    ("incomplete", "Incomplete"),
];

/// Subscriptions: the local copy of the payment provider's, read-only (they
/// change through checkout, the customer portal and the provider's
/// webhooks).
#[injectable]
pub struct SubscriptionAdmin;

impl Resource for SubscriptionAdmin {
    type Entity = subscription::Entity;

    const LABEL: &'static str = "Subscriptions";
    const SINGULAR: &'static str = "Subscription";
    const GROUP: &'static str = "Billing";
    const ICON: &'static str = "credit-card";
    const TITLE: &'static str = "plan";
    const SEARCH: &'static [&'static str] = &["plan", "provider_id"];

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::belongs_to::<WorkspaceAdmin>("billable_id")
                .label("Workspace")
                .readonly(),
            Field::text("plan").readonly().sortable(),
            Field::text("interval").readonly(),
            Field::select("status")
                .options(STATUSES)
                .badge()
                .readonly()
                .sortable(),
            Field::number("quantity").label("Seats").readonly(),
            Field::datetime("trial_ends_at").readonly(),
            Field::datetime("current_period_end")
                .label("Renews")
                .readonly(),
            Field::datetime("ends_at").readonly(),
            Field::text("provider").readonly().hide_on_index(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn filters(&self) -> Vec<Filter<subscription::Entity>> {
        vec![Filter::select("status").options(STATUSES)]
    }

    fn metrics(&self) -> Vec<Metric<subscription::Entity>> {
        vec![
            Metric::value("Paying subscriptions")
                .count()
                .filter(subscription::Column::Status.eq("active"))
                .on_dashboard(),
            Metric::partition("Subscriptions by plan")
                .count()
                .group_by(subscription::Column::Plan),
        ]
    }

    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<subscription::Model>> {
        Box::pin(async move {
            Err(AppError::bad_request(
                "Subscriptions start at checkout in the app.",
            ))
        })
    }

    fn delete<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _record: subscription::Model,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            Err(AppError::bad_request(
                "Cancel subscriptions in the workspace's billing settings or the customer portal.",
            ))
        })
    }
}

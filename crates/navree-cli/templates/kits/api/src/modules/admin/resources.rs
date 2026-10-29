//! The admin panel's resources: what Helm shows and edits for each entity.
//! (A subscriptions resource returns once Helm takes navree-orm models.)

use garde::Validate;
use navree::BoxFuture;
use navree::auth::{AuthService, user};
use navree::crypto::generate_key;
use navree::helm::{Field, Filter, FormData, Metric, Resource, SaveContext};
use navree::prelude::*;
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

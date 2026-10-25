//! [`UserService`]: listing, creating and editing users with their roles.

use std::sync::Arc;

use keel::access::{AccessService, Accessible, Read};
use keel::auth::{AuthService, NewUser, user};
use keel::contracts::Clock;
use keel::db::paginate;
use keel::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Expr, ExprTrait, Func, LikeExpr};
use sea_orm::{Condition, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder};

use crate::dto::{CreateUser, UpdateUser, UserFilter};
use crate::resource::{UserView, roles_by_user};

/// Users and their CRM roles. Writes run in the caller's unit of work, so
/// the account, its role and their audit rows commit together.
#[injectable]
pub struct UserService {
    auth: Arc<AuthService>,
    access: Arc<AccessService>,
    clock: Arc<dyn Clock>,
}

impl UserService {
    /// One page of the users `scope` allows, by name, matching `filter`.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<user::Entity, Read>,
        filter: UserFilter,
        page: PageQuery,
    ) -> Result<Page<UserView>> {
        let mut select = scope.apply(user::Entity::find());
        if let Some(search) = filter.search.as_deref().map(str::trim)
            && !search.is_empty()
        {
            let pattern = || LikeExpr::new(contains(search)).escape('\\');
            select = select.filter(
                Condition::any()
                    .add(Expr::expr(Func::lower(Expr::col(user::Column::Name))).like(pattern()))
                    .add(Expr::expr(Func::lower(Expr::col(user::Column::Email))).like(pattern())),
            );
        }
        let select = select
            .order_by_asc(user::Column::Name)
            .order_by_asc(user::Column::Id);
        let tx = uow.tx().await?;
        let page = paginate(select, &*tx, page).await?;
        let ids: Vec<_> = page.data.iter().map(|user| user.id).collect();
        let mut roles = roles_by_user(&*tx, &ids).await?;
        Ok(page.map(|user| {
            let user_roles = roles.remove(&user.id).unwrap_or_default();
            UserView::new(&user, user_roles)
        }))
    }

    /// Creates an account with `input.role` (the password policy and a
    /// unique email are enforced).
    pub async fn create(&self, uow: &Uow, input: CreateUser) -> Result<UserView> {
        let user = self
            .auth
            .register(
                uow,
                NewUser {
                    name: input.name,
                    email: input.email,
                    password: input.password,
                },
            )
            .await
            .map_err(|error| error.into_app_error())?;
        self.access
            .assign_role(uow, user.id, input.role.as_str())
            .await?;
        Ok(UserView::new(&user, vec![input.role.as_str().to_owned()]))
    }

    /// Changes the fields present in `input`; a role replaces the user's
    /// roles.
    pub async fn update(
        &self,
        uow: &Uow,
        user: user::Model,
        input: UpdateUser,
    ) -> Result<UserView> {
        let user = match input.name {
            Some(name) => {
                let mut active = user.into_active_model();
                active.name = Set(name.trim().to_owned());
                active.updated_at = Set(self.clock.now());
                Repo::<user::Entity>::update(uow, active).await?
            }
            None => user,
        };
        if let Some(role) = input.role {
            self.access
                .sync_user_roles(uow, user.id, [role.as_str()])
                .await?;
        }
        let tx = uow.tx().await?;
        let roles = roles_by_user(&*tx, &[user.id])
            .await?
            .remove(&user.id)
            .unwrap_or_default();
        Ok(UserView::new(&user, roles))
    }
}

/// A `LIKE` pattern matching values that contain `search`, lower-cased,
/// with `%`, `_` and `\` taken literally (escape `\`).
fn contains(search: &str) -> String {
    let mut pattern = String::from("%");
    for ch in search.to_lowercase().chars() {
        if matches!(ch, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

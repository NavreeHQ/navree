//! [`UserService`]: listing, creating and editing users with their roles.

use std::sync::Arc;

use crm_core::models::User;
use navree::access::{AccessService, Accessible, Read};
use navree::auth::{AuthService, NewUser};
use navree::orm::Record;
use navree::orm::access::AccessibleQuery;
use navree::orm::list::{ListQuery, ListQueryExt, ListResource};
use navree::prelude::*;

use crate::dto::{CreateUser, UpdateUser, UserList};
use crate::resource::UserView;

/// Users and their CRM roles. Accounts are made by navree-auth (which
/// hashes the password and keeps emails unique) and roles assigned by
/// navree-access; the users and their roles are read through the models.
/// Writes run in the caller's unit of work, so the account, its role and
/// their audit rows commit together.
#[injectable]
pub struct UserService {
    auth: Arc<AuthService>,
    access: Arc<AccessService>,
}

impl UserService {
    /// One page of the users `scope` allows that match `list` (read against
    /// [`UserList`]), by name unless it sorts, with their roles.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<User, Read>,
        list: &ListQuery,
    ) -> Result<Page<UserView>> {
        let page = scope
            .apply(User::query())
            .list(list, &UserList::SPEC)?
            .with_all(UserView::EMBEDS)
            .paginate(uow, list.page_request(&UserList::SPEC))
            .await?;
        Ok(page.map(|user| user.to_resource::<UserView>()))
    }

    /// Creates an account with `input.role` (the password policy and a
    /// unique email are enforced).
    pub async fn create(&self, uow: &Uow, input: CreateUser) -> Result<UserView> {
        let account = self
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
            .assign_role(uow, account.id, input.role.as_str())
            .await?;
        let user = User::query()
            .with_all(UserView::EMBEDS)
            .find_or_fail(uow, account.id)
            .await?;
        Ok(user.to_resource())
    }

    /// Changes the fields present in `input`; a role replaces the user's
    /// roles.
    pub async fn update(
        &self,
        uow: &Uow,
        mut user: Record<User>,
        input: UpdateUser,
    ) -> Result<UserView> {
        if let Some(name) = input.name {
            user.update(uow, |user| user.name = name.trim().to_owned())
                .await?;
        }
        if let Some(role) = input.role {
            self.access
                .sync_user_roles(uow, user.id, [role.as_str()])
                .await?;
        }
        // Read in the request's transaction, which sees the new roles.
        user.load_all(uow, UserView::EMBEDS).await?;
        Ok(user.to_resource())
    }
}

//! `GET /api/v1/me` and `/api/v1/users`.

use crm_core::User;
use navree::access::CurrentAccess;
use navree::orm::list::List;
use navree::prelude::*;

use crate::dto::{CreateUser, UpdateUser, UserList};
use crate::resource::{Me, UserPage, UserView};
use crate::service::UserService;

/// `GET /api/v1/me`: the signed-in user with their roles.
#[injectable]
pub struct MeController;

#[controller("/me", version = "1")]
impl MeController {
    /// `{id, name, email, roles}`.
    #[get("/")]
    #[authenticated]
    #[api(summary = "The signed-in user and their roles", tag = "Users", response = Me, responses(401))]
    async fn show(
        &self,
        CurrentUser(user): CurrentUser,
        CurrentAccess(access): CurrentAccess,
    ) -> Json<Me> {
        Json(Me {
            id: user.id,
            name: user.name.clone(),
            email: user.email.clone(),
            roles: access.roles.clone(),
        })
    }
}

/// `/api/v1/users`: everyone signed in lists users (owner pickers); admins
/// create them and change names and roles.
#[injectable]
pub struct UsersController {
    users: Arc<UserService>,
}

#[controller("/users", version = "1")]
impl UsersController {
    /// A page of users by name; `search` matches name or email.
    #[get("/")]
    #[can(Read, User)]
    #[api(
        summary = "List users",
        description = "Query: `search` (part of the name or email, case-insensitive), `sort` (`name`, `email`, `created_at`; `-` in front for descending; default `name`), `page`, `per_page` (default 15, at most 100).",
        tag = "Users",
        response = UserPage,
        responses(400, 401, 403)
    )]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<User, Read>,
        list: List<UserList>,
    ) -> Result<Page<UserView>> {
        self.users.list(&uow, scope, &list).await
    }

    /// Creates an account with a role (admin).
    #[post("/")]
    #[can(Create, User)]
    #[api(
        summary = "Create a user",
        tag = "Users",
        request = CreateUser,
        response = UserView,
        status = 201,
        responses(400, 401, 403)
    )]
    async fn store(
        &self,
        uow: Uow,
        Valid(Json(input)): Valid<Json<CreateUser>>,
    ) -> Result<Created<UserView>> {
        let user = self.users.create(&uow, input).await?;
        Ok(Created(format!("/api/v1/users/{}", user.id), user))
    }

    /// Changes a user's name and/or role (admin).
    #[patch("/{id}")]
    #[can(Update, User, from = "id")]
    #[api(
        summary = "Update a user",
        tag = "Users",
        request = UpdateUser,
        response = UserView,
        responses(400, 401, 403, 404)
    )]
    async fn update(
        &self,
        uow: Uow,
        user: Authorized<User, Update>,
        Valid(Json(input)): Valid<Json<UpdateUser>>,
    ) -> Result<Json<UserView>> {
        Ok(Json(
            self.users.update(&uow, user.into_inner(), input).await?,
        ))
    }
}

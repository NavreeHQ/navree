//! `/api/v1/account`: sessions and sign-in options.

use navree::prelude::*;
use navree::social::Social;
use uuid::Uuid;

use super::dto::{AccountSessionList, ProviderList, ProviderResource};
use super::service::SessionService;

/// The account routes.
#[injectable]
pub struct AccountController {
    sessions: Arc<SessionService>,
    social: Arc<Social>,
}

#[controller("/account", version = "1")]
impl AccountController {
    /// `GET /api/v1/account/sessions`: where the user is signed in.
    #[get("/sessions")]
    #[authenticated]
    #[api(
        summary = "List my sessions",
        tag = "Account",
        response = AccountSessionList,
        responses(401)
    )]
    async fn sessions(
        &self,
        uow: Uow,
        CurrentUser(user): CurrentUser,
    ) -> Result<Json<AccountSessionList>> {
        let data = self.sessions.list(&uow, user.id).await?;
        Ok(Json(AccountSessionList { data }))
    }

    /// `DELETE /api/v1/account/sessions/{id}`: signs that session out.
    #[delete("/sessions/{id}")]
    #[authenticated]
    #[api(
        summary = "End a session",
        tag = "Account",
        status = 204,
        responses(401, 404)
    )]
    async fn revoke(
        &self,
        uow: Uow,
        CurrentUser(user): CurrentUser,
        Path(id): Path<Uuid>,
    ) -> Result<NoContent> {
        if self.sessions.revoke(&uow, user.id, id).await? {
            Ok(NoContent)
        } else {
            Err(AppError::not_found())
        }
    }

    /// `GET /api/v1/account/providers`: the social sign-in providers, one
    /// button each on the sign-in page.
    #[get("/providers")]
    #[public]
    #[api(
        summary = "List the social sign-in providers",
        tag = "Account",
        response = ProviderList
    )]
    async fn providers(&self) -> Result<Json<ProviderList>> {
        let data = self
            .social
            .providers()
            .map_err(AppError::internal)?
            .into_iter()
            .map(|key| {
                let name = self
                    .social
                    .provider_name(&key)
                    .unwrap_or_else(|_| key.clone());
                ProviderResource { key, name }
            })
            .collect();
        Ok(Json(ProviderList { data }))
    }
}

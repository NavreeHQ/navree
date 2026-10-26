//! `/api/deals`.

use navis::prelude::*;

use super::dto::{CreateDeal, DealsQuery};
use super::resource::DealResource;
use super::service::DealService;

/// `/api/deals` (no API version): any signed-in user. The JWT strategy
/// authenticates the bearer token in the pipeline; [`CurrentUser`] then
/// loads the user row (401 when it no longer exists).
#[injectable]
pub struct DealsController {
    deals: Arc<DealService>,
}

#[controller("/deals")]
impl DealsController {
    /// A page of deals, newest first.
    #[get("/")]
    #[authenticated]
    async fn index(
        &self,
        uow: Uow,
        CurrentUser(_user): CurrentUser,
        Valid(Query(query)): Valid<Query<DealsQuery>>,
    ) -> Result<Page<DealResource>> {
        self.deals.list(&uow, &query).await
    }

    /// Creates a deal owned by the signed-in user.
    #[post("/")]
    #[authenticated]
    async fn store(
        &self,
        uow: Uow,
        CurrentUser(user): CurrentUser,
        Valid(Json(input)): Valid<Json<CreateDeal>>,
    ) -> Result<Created<DealResource>> {
        let deal = self.deals.create(&uow, &user, input).await?;
        Ok(Created(format!("/api/deals/{}", deal.id), deal))
    }
}

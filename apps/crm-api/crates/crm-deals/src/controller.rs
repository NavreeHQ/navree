//! `/api/v1/deals`.

use crm_core::{Deal, Move};
use navree::orm::list::List;
use navree::prelude::*;

use crate::dto::{BoardFilter, CreateDeal, DealList, MoveDeal, UpdateDeal};
use crate::resource::{DealBoard, DealPage, DealResource};
use crate::service::DealService;

/// `/api/v1/deals`: everyone with a CRM role reads every deal and creates
/// their own; reps change, move and delete the deals they own (not won or
/// lost ones for delete), managers and admins any.
#[injectable]
pub struct DealsController {
    deals: Arc<DealService>,
}

#[controller("/deals", version = "1")]
impl DealsController {
    /// A page of deals, newest first unless `sort` says otherwise. Filters
    /// `filter[stage]`, `filter[owner_id]` and `filter[company_id]`; `search`
    /// (title); `sort`; `page` and `per_page` (see [`DealList`]).
    #[get("/")]
    #[can(Read, Deal)]
    #[api(
        summary = "List deals",
        tag = "Deals",
        response = DealPage,
        responses(400, 401, 403)
    )]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<Deal, Read>,
        list: List<DealList>,
    ) -> Result<Page<DealResource>> {
        self.deals.list(&uow, scope, &list).await
    }

    /// Every stage with its deals in board order, counts and totals;
    /// `owner_id` keeps one user's deals.
    #[get("/board")]
    #[can(Read, Deal)]
    #[api(
        summary = "The deal board",
        tag = "Deals",
        response = DealBoard,
        responses(400, 401, 403)
    )]
    async fn board(
        &self,
        uow: Uow,
        scope: Accessible<Deal, Read>,
        Valid(Query(filter)): Valid<Query<BoardFilter>>,
    ) -> Result<Json<DealBoard>> {
        Ok(Json(self.deals.board(&uow, scope, filter).await?))
    }

    /// Creates a deal at the end of its stage.
    #[post("/")]
    #[can(Create, Deal)]
    #[api(
        summary = "Create a deal",
        tag = "Deals",
        request = CreateDeal,
        response = DealResource,
        status = 201,
        responses(400, 401, 403)
    )]
    async fn store(
        &self,
        uow: Uow,
        CurrentUser(user): CurrentUser,
        Abilities(ability): Abilities,
        Valid(Json(input)): Valid<Json<CreateDeal>>,
    ) -> Result<Created<DealResource>> {
        let deal = self.deals.create(&uow, &ability, user.id, input).await?;
        Ok(Created(format!("/api/v1/deals/{}", deal.id), deal))
    }

    /// One deal.
    #[get("/{id}")]
    #[can(Read, Deal, from = "id")]
    #[api(
        summary = "Show a deal",
        tag = "Deals",
        response = DealResource,
        responses(401, 403, 404)
    )]
    async fn show(&self, uow: Uow, deal: Authorized<Deal, Read>) -> Result<Json<DealResource>> {
        Ok(Json(self.deals.show(&uow, deal.into_inner()).await?))
    }

    /// Changes the fields sent.
    #[patch("/{id}")]
    #[can(Update, Deal, from = "id")]
    #[api(
        summary = "Update a deal",
        tag = "Deals",
        request = UpdateDeal,
        response = DealResource,
        responses(400, 401, 403, 404)
    )]
    async fn update(
        &self,
        uow: Uow,
        CurrentUser(user): CurrentUser,
        deal: Authorized<Deal, Update>,
        Valid(Json(input)): Valid<Json<UpdateDeal>>,
    ) -> Result<Json<DealResource>> {
        Ok(Json(self.deals.update(&uow, user.id, deal, input).await?))
    }

    /// Moves a deal on the board: to `stage`, at the 0-based `position`
    /// within it (default: the end). `lost` needs `lost_reason`.
    #[post("/{id}/move")]
    #[can(Move, Deal, from = "id")]
    #[api(
        summary = "Move a deal to a stage and position",
        tag = "Deals",
        request = MoveDeal,
        response = DealResource,
        responses(400, 401, 403, 404)
    )]
    async fn move_deal(
        &self,
        uow: Uow,
        CurrentUser(user): CurrentUser,
        deal: Authorized<Deal, Move>,
        Valid(Json(input)): Valid<Json<MoveDeal>>,
    ) -> Result<Json<DealResource>> {
        let deal = self
            .deals
            .move_to(&uow, user.id, deal.into_inner(), input)
            .await?;
        Ok(Json(deal))
    }

    /// Deletes a deal (won and lost deals only by admins).
    #[delete("/{id}")]
    #[can(Delete, Deal, from = "id")]
    #[api(
        summary = "Delete a deal",
        tag = "Deals",
        status = 204,
        responses(401, 403, 404)
    )]
    async fn destroy(&self, uow: Uow, deal: Authorized<Deal, Delete>) -> Result<NoContent> {
        self.deals.delete(&uow, deal.into_inner()).await?;
        Ok(NoContent)
    }
}

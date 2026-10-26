use axum::http::StatusCode;
use loco_rs::{
    controller::extractor::validate::QueryValidateWithMessage,
    model::query::PaginationQuery,
    prelude::*,
};
use serde::Deserialize;

use crate::{
    extractors::auth::CurrentUser,
    models::deals::{self, CreateParams},
    views::deals::{DealPageResponse, DealResponse},
};

#[derive(Debug, Deserialize, Validate)]
pub struct ListParams {
    #[serde(default = "default_page")]
    #[validate(range(min = 1, message = "page must be at least 1"))]
    pub page: u64,
    #[serde(default = "default_per_page")]
    #[validate(range(min = 1, max = 100, message = "per_page must be between 1 and 100"))]
    pub per_page: u64,
}

const fn default_page() -> u64 {
    1
}

const fn default_per_page() -> u64 {
    20
}

/// Extractors run in argument order, so a bad token (401) wins over bad
/// input (400/422).
#[debug_handler]
async fn list(
    _auth: CurrentUser,
    State(ctx): State<AppContext>,
    QueryValidateWithMessage(params): QueryValidateWithMessage<ListParams>,
) -> Result<Response> {
    let pagination = PaginationQuery {
        page: params.page,
        page_size: params.per_page,
    };
    let page = deals::Model::page_with_owners(&ctx.db, &pagination).await?;
    format::json(DealPageResponse::try_from(page)?)
}

#[debug_handler]
async fn create(
    auth: CurrentUser,
    State(ctx): State<AppContext>,
    JsonValidateWithMessage(params): JsonValidateWithMessage<CreateParams>,
) -> Result<Response> {
    let deal = deals::Model::create(&ctx.db, auth.user.id, params).await?;
    format::render()
        .status(StatusCode::CREATED)
        .json(DealResponse::new(deal, auth.user))
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/deals")
        .add("/", get(list))
        .add("/", post(create))
}

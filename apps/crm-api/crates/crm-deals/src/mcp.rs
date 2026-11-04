//! [`DealsMcp`]: the deals part of the CRM's MCP server (`/mcp`, with
//! `McpModule` in the app): the `list_deals` and `move_deal` tools, the
//! `crm://deals/{id}` and `crm://pipeline` resources and the
//! `follow-up-email` prompt, each checked like the matching route.

use crm_core::{Deal, Move, Stage};
use navree::mcp::{McpContext, PromptMessage, mcp_server};
use navree::openapi::ToSchema;
use navree::orm::list::{Condition, ListQuery, Op};
use navree::prelude::*;
use navree::validation::Validate;
use serde::Deserialize;
use uuid::Uuid;

use crate::{DealBoard, DealResource, DealService, MoveDeal};

/// The condition `filter[field]=value` of a list query.
fn equals(field: &str, value: impl ToString) -> Condition {
    Condition {
        field: field.to_owned(),
        op: Op::Eq,
        values: vec![value.to_string()],
    }
}

/// The arguments of `list_deals`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct ListDeals {
    /// Words from the deal title.
    #[validate(length(max = 100))]
    pub query: Option<String>,
    /// Only deals in this stage.
    #[validate(skip)]
    pub stage: Option<Stage>,
    /// Only the user's own deals.
    #[validate(skip)]
    #[serde(default)]
    pub mine: bool,
}

/// The arguments of `move_deal`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct MoveDealArgs {
    /// The deal to move.
    #[validate(skip)]
    pub deal_id: Uuid,
    /// The stage to move it to.
    #[validate(skip)]
    pub stage: Stage,
    /// Why the deal was lost: required when `stage` is `lost`.
    #[validate(length(max = 1000))]
    pub lost_reason: Option<String>,
}

/// The arguments of the `follow-up-email` prompt.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct FollowUpArgs {
    /// The deal to follow up on.
    #[validate(skip)]
    pub deal_id: Uuid,
}

/// The deals part of the CRM's MCP server.
#[injectable]
pub struct DealsMcp {
    deals: Arc<DealService>,
}

#[mcp_server]
impl DealsMcp {
    /// Lists the deals the user can see, newest first, optionally only one stage
    /// or the user's own. Returns at most 25.
    #[mcp_tool(read_only)]
    #[can(Read, Deal)]
    async fn list_deals(&self, cx: &McpContext, args: ListDeals) -> Result<Vec<DealResource>> {
        let mut list = ListQuery::new();
        list.per_page = Some(25);
        list.search = args
            .query
            .map(|query| query.trim().to_owned())
            .filter(|query| !query.is_empty());
        if let Some(stage) = args.stage {
            list.filter.push(equals("stage", stage.as_str()));
        }
        if args.mine {
            list.filter.push(equals("owner_id", cx.user_id()));
        }
        Ok(self
            .deals
            .list(cx.uow(), cx.accessible::<Deal, Read>(), &list)
            .await?
            .data)
    }

    /// Moves a deal to another stage. Moving to `lost` needs a reason.
    #[mcp_tool(destructive)]
    #[can(Update, Deal)]
    async fn move_deal(&self, cx: &McpContext, args: MoveDealArgs) -> Result<DealResource> {
        let deal = cx.authorize::<Deal, Move>(args.deal_id).await?;
        self.deals
            .move_to(
                cx.uow(),
                cx.user_id(),
                deal.into_inner(),
                MoveDeal {
                    stage: Some(args.stage),
                    position: None,
                    lost_reason: args.lost_reason,
                },
            )
            .await
    }

    /// A deal with its company, contact, owner, stage and amount.
    #[mcp_resource("crm://deals/{id}", mime = "application/json")]
    #[can(Read, Deal, from = "id")]
    async fn deal(&self, cx: &McpContext, deal: Authorized<Deal, Read>) -> Result<DealResource> {
        self.deals.show(cx.uow(), deal.into_inner()).await
    }

    /// The pipeline: every stage with its deals, counts and totals.
    #[mcp_resource("crm://pipeline", mime = "application/json")]
    #[can(Read, Deal)]
    async fn pipeline(&self, cx: &McpContext) -> Result<DealBoard> {
        self.deals
            .board(cx.uow(), cx.accessible::<Deal, Read>(), Default::default())
            .await
    }

    /// Drafts a follow-up email to a deal's main contact from its recent activity.
    #[mcp_prompt(name = "follow-up-email", title = "Draft a follow-up email")]
    #[can(Read, Deal)]
    async fn follow_up(&self, cx: &McpContext, args: FollowUpArgs) -> Result<Vec<PromptMessage>> {
        let deal = cx.authorize::<Deal, Read>(args.deal_id).await?;
        let deal = self.deals.show(cx.uow(), deal.into_inner()).await?;
        Ok(vec![
            PromptMessage::resource(format!("crm://deals/{}", deal.id)),
            PromptMessage::user(format!(
                "Read the deal and its activity (use list_deals and the deal resource), then draft \
                 a short follow-up email from me to {}. No filler. End with one clear next step.",
                deal.contact
                    .as_ref()
                    .map_or("the main contact", |c| c.full_name.as_str()),
            )),
        ])
    }
}

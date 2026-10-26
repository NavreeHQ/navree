use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

use crate::{
    AppState,
    auth::AuthUser,
    error::AppError,
    extract::{ValidJson, ValidQuery},
};

#[derive(Serialize)]
pub struct Deal {
    id: Uuid,
    title: String,
    amount_cents: i64,
    stage: String,
    owner: Owner,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct Owner {
    id: Uuid,
    name: String,
}

/// A `deals` row joined with its owner's name.
#[derive(FromRow)]
struct DealRow {
    id: Uuid,
    title: String,
    amount_cents: i64,
    stage: String,
    created_at: DateTime<Utc>,
    owner_id: Uuid,
    owner_name: String,
}

impl From<DealRow> for Deal {
    fn from(row: DealRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            amount_cents: row.amount_cents,
            stage: row.stage,
            owner: Owner {
                id: row.owner_id,
                name: row.owner_name,
            },
            created_at: row.created_at,
        }
    }
}

// ------------------------------------------------------------------ list

#[derive(Deserialize, Validate)]
pub struct ListParams {
    #[serde(default = "default_page")]
    #[validate(range(min = 1))]
    page: i64,
    #[serde(default = "default_per_page")]
    #[validate(range(min = 1, max = 100))]
    per_page: i64,
}

fn default_page() -> i64 {
    1
}

fn default_per_page() -> i64 {
    20
}

#[derive(Serialize)]
pub struct DealPage {
    data: Vec<Deal>,
    meta: PageMeta,
}

#[derive(Serialize)]
struct PageMeta {
    current_page: i64,
    per_page: i64,
    total: i64,
    last_page: i64,
}

pub async fn list(
    State(state): State<AppState>,
    _user: AuthUser,
    ValidQuery(params): ValidQuery<ListParams>,
) -> Result<Json<DealPage>, AppError> {
    let ListParams { page, per_page } = params;

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM deals")
        .fetch_one(&state.db)
        .await?;
    let rows: Vec<DealRow> = sqlx::query_as(
        "SELECT d.id, d.title, d.amount_cents, d.stage, d.created_at, \
                u.id AS owner_id, u.name AS owner_name \
           FROM deals d \
           JOIN users u ON u.id = d.owner_id \
          ORDER BY d.created_at DESC, d.id DESC \
          LIMIT $1 OFFSET $2",
    )
    .bind(per_page)
    .bind((page - 1).saturating_mul(per_page))
    .fetch_all(&state.db)
    .await?;

    Ok(Json(DealPage {
        data: rows.into_iter().map(Deal::from).collect(),
        meta: PageMeta {
            current_page: page,
            per_page,
            total,
            last_page: ((total + per_page - 1) / per_page).max(1),
        },
    }))
}

// ------------------------------------------------------------------ create

#[derive(Deserialize, Validate)]
pub struct NewDeal {
    #[validate(length(min = 1, max = 200))]
    title: String,
    #[validate(range(min = 0, max = 1_000_000_000_000_i64))]
    amount_cents: i64,
    stage: Stage,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Stage {
    Lead,
    Qualified,
    Proposal,
    Negotiation,
    Won,
    Lost,
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Qualified => "qualified",
            Self::Proposal => "proposal",
            Self::Negotiation => "negotiation",
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ValidJson(input): ValidJson<NewDeal>,
) -> Result<(StatusCode, Json<Deal>), AppError> {
    let id = Uuid::new_v4();
    let created_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO deals (id, title, amount_cents, stage, owner_id, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, now(), now()) \
         RETURNING created_at",
    )
    .bind(id)
    .bind(&input.title)
    .bind(input.amount_cents)
    .bind(input.stage.as_str())
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

    let deal = Deal {
        id,
        title: input.title,
        amount_cents: input.amount_cents,
        stage: input.stage.as_str().to_owned(),
        owner: Owner {
            id: user.id,
            name: user.name,
        },
        created_at,
    };
    Ok((StatusCode::CREATED, Json(deal)))
}

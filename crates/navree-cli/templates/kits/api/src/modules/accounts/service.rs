//! Sessions, read from navree-auth's `refresh_tokens`: every sign-in starts
//! a token family, and the family's one unspent token keeps it alive.

use chrono::Utc;
use navree::auth::refresh_token::{Column, Entity as RefreshToken};
use navree::db::Uow;
use navree::injectable;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, DbErr, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use super::dto::SessionResource;

/// Lists and ends a user's sessions.
#[injectable]
pub struct SessionService;

impl SessionService {
    /// The active sessions of `user`, most recently active first.
    pub async fn list(&self, uow: &Uow, user: Uuid) -> Result<Vec<SessionResource>, DbErr> {
        let tokens = RefreshToken::find()
            .filter(Column::UserId.eq(user))
            .filter(Column::RevokedAt.is_null())
            .filter(Column::ExpiresAt.gt(Utc::now()))
            .order_by_desc(Column::CreatedAt)
            .all(&*uow.tx().await?)
            .await?;
        Ok(tokens
            .into_iter()
            .map(|token| SessionResource {
                id: token.family_id,
                ip: token.ip,
                user_agent: token.user_agent,
                last_active_at: token.created_at,
                expires_at: token.expires_at,
            })
            .collect())
    }

    /// Ends session `id` of `user`: its refresh token stops working (the
    /// access token it already holds lasts until it expires, 15 minutes by
    /// default). `false` when there is no such active session.
    pub async fn revoke(&self, uow: &Uow, user: Uuid, id: Uuid) -> Result<bool, DbErr> {
        let result = RefreshToken::update_many()
            .col_expr(Column::RevokedAt, Expr::value(Utc::now()))
            .filter(Column::UserId.eq(user))
            .filter(Column::FamilyId.eq(id))
            .filter(Column::RevokedAt.is_null())
            .exec(&*uow.tx().await?)
            .await?;
        Ok(result.rows_affected > 0)
    }
}

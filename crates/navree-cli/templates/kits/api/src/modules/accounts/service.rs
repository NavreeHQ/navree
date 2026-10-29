//! Sessions, read from navree-auth's `refresh_tokens`: every sign-in starts
//! a token family, and the family's one unspent token keeps it alive.

use chrono::Utc;
use navree::auth::refresh_token::Model as RefreshToken;
use navree::db::Uow;
use navree::injectable;
use navree::orm::{Model as _, OrmError};
use uuid::Uuid;

use super::dto::AccountSession;

/// Lists and ends a user's sessions.
#[injectable]
pub struct SessionService;

impl SessionService {
    /// The active sessions of `user`, most recently active first.
    pub async fn list(&self, uow: &Uow, user: Uuid) -> Result<Vec<AccountSession>, OrmError> {
        let tokens = RefreshToken::query()
            .where_(RefreshToken::USER_ID.eq(user))
            .where_null(RefreshToken::REVOKED_AT)
            .where_(RefreshToken::EXPIRES_AT.gt(Utc::now()))
            .order_by_desc(RefreshToken::CREATED_AT)
            .get(uow)
            .await?;
        Ok(tokens
            .into_models()
            .into_iter()
            .map(|token| AccountSession {
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
    pub async fn revoke(&self, uow: &Uow, user: Uuid, id: Uuid) -> Result<bool, OrmError> {
        let revoked = RefreshToken::query()
            .where_(RefreshToken::USER_ID.eq(user))
            .where_(RefreshToken::FAMILY_ID.eq(id))
            .where_null(RefreshToken::REVOKED_AT)
            .update(uow, [RefreshToken::REVOKED_AT.set(Utc::now())])
            .await?;
        Ok(revoked > 0)
    }
}

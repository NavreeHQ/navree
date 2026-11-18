//! Database model for the notice board (`notices_notices`).

/// The `notice` entity (`notices_notices` table, subject `notices.Notice`).
pub mod notice {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    /// One notice posted to the board.
    #[derive(Clone, Debug, Default, PartialEq, Eq, navree::orm::Model, Serialize, Deserialize)]
    #[model(
        table = "notices_notices",
        timestamps(created_at = "created_at", updated_at = false),
        subject = "notices.Notice",
        permission_prefix = "notices"
    )]
    pub struct Model {
        /// Primary key.
        pub id: i64,
        /// Short heading of the notice (1 to 120 characters).
        pub title: String,
        /// Body text of the notice (up to 4,000 characters).
        pub body: String,
        /// Identifier of the user who posted the notice.
        pub posted_by: Uuid,
        /// Instant when the notice expires.
        pub expires_at: DateTime<Utc>,
        /// Instant when the notice was created.
        pub created_at: DateTime<Utc>,
    }
}

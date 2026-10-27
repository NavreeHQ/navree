//! The auth module's migrations (`MigrationSet` `auth`): `users`,
//! `refresh_tokens`, `password_reset_tokens`, the session columns of
//! `refresh_tokens`, `login_tokens` and `passkeys`.

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the auth module's migration set; feature modules whose
/// tables reference `users` run `.after([AUTH_MIGRATIONS])`.
pub const AUTH_MIGRATIONS: &str = "auth";

/// The auth module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(AUTH_MIGRATIONS)
        .migration(CreateUsers)
        .migration(CreateRefreshTokens)
        .migration(CreatePasswordResetTokens)
        .migration(AddSessionColumns)
        .migration(CreateLoginTokens)
        .migration(CreatePasskeys)
}

/// Creates `users`.
struct CreateUsers;

impl MigrationName for CreateUsers {
    fn name(&self) -> &str {
        "m20261025_000001_create_users_table"
    }
}

#[async_trait]
impl MigrationTrait for CreateUsers {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("users")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("email").string().not_null().unique_key())
                    .col(ColumnDef::new("password_hash").string().not_null())
                    .col(
                        ColumnDef::new("email_verified_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("two_factor_secret").text().null())
                    .col(ColumnDef::new("two_factor_recovery_codes").text().null())
                    .col(
                        ColumnDef::new("two_factor_confirmed_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new("updated_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("users").to_owned())
            .await
    }
}

/// Creates `refresh_tokens` (rows go with their user).
struct CreateRefreshTokens;

impl MigrationName for CreateRefreshTokens {
    fn name(&self) -> &str {
        "m20261025_000002_create_refresh_tokens_table"
    }
}

#[async_trait]
impl MigrationTrait for CreateRefreshTokens {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("refresh_tokens")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("family_id").uuid().not_null())
                    .col(
                        ColumnDef::new("token_hash")
                            .string_len(64)
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new("expires_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new("revoked_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new("replaced_by").uuid().null())
                    .col(ColumnDef::new("ip").string_len(45).null())
                    .col(ColumnDef::new("user_agent").string_len(512).null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from("refresh_tokens", "user_id")
                            .to("users", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("refresh_tokens_user_id_index")
                    .table("refresh_tokens")
                    .col("user_id")
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("refresh_tokens_family_id_index")
                    .table("refresh_tokens")
                    .col("family_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("refresh_tokens").to_owned())
            .await
    }
}

/// Creates `password_reset_tokens`.
struct CreatePasswordResetTokens;

impl MigrationName for CreatePasswordResetTokens {
    fn name(&self) -> &str {
        "m20261025_000003_create_password_reset_tokens_table"
    }
}

#[async_trait]
impl MigrationTrait for CreatePasswordResetTokens {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("password_reset_tokens")
                    .col(ColumnDef::new("email").string().not_null().primary_key())
                    .col(ColumnDef::new("token_hash").string_len(64).not_null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("password_reset_tokens").to_owned())
            .await
    }
}

/// Adds the session columns to `refresh_tokens`: `authenticated_at`,
/// `impersonator_id` and `audience` (one `ALTER TABLE` each, for SQLite).
struct AddSessionColumns;

impl MigrationName for AddSessionColumns {
    fn name(&self) -> &str {
        "m20261027_000001_add_session_columns_to_refresh_tokens"
    }
}

#[async_trait]
impl MigrationTrait for AddSessionColumns {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in [
            ColumnDef::new("authenticated_at")
                .timestamp_with_time_zone()
                .null()
                .to_owned(),
            ColumnDef::new("impersonator_id").uuid().null().to_owned(),
            ColumnDef::new("audience").string_len(32).null().to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table("refresh_tokens")
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in ["audience", "impersonator_id", "authenticated_at"] {
            manager
                .alter_table(
                    Table::alter()
                        .table("refresh_tokens")
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

/// Creates `login_tokens` (rows go with their user).
struct CreateLoginTokens;

impl MigrationName for CreateLoginTokens {
    fn name(&self) -> &str {
        "m20261027_000002_create_login_tokens_table"
    }
}

#[async_trait]
impl MigrationTrait for CreateLoginTokens {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("login_tokens")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("kind").string_len(16).not_null())
                    .col(ColumnDef::new("token_hash").string_len(64).not_null())
                    .col(ColumnDef::new("attempts").integer().not_null().default(0))
                    .col(
                        ColumnDef::new("expires_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new("used_at").timestamp_with_time_zone().null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from("login_tokens", "user_id")
                            .to("users", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("login_tokens_user_id_kind_index")
                    .table("login_tokens")
                    .col("user_id")
                    .col("kind")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("login_tokens").to_owned())
            .await
    }
}

/// Creates `passkeys` (rows go with their user).
struct CreatePasskeys;

impl MigrationName for CreatePasskeys {
    fn name(&self) -> &str {
        "m20261027_000003_create_passkeys_table"
    }
}

#[async_trait]
impl MigrationTrait for CreatePasskeys {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("passkeys")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("user_id").uuid().not_null())
                    .col(ColumnDef::new("credential_id").text().not_null())
                    .col(
                        ColumnDef::new("credential_hash")
                            .string_len(64)
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new("public_key").text().not_null())
                    .col(ColumnDef::new("algorithm").integer().not_null())
                    .col(ColumnDef::new("sign_count").big_integer().not_null())
                    .col(ColumnDef::new("transports").string_len(255).null())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new("last_used_at")
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from("passkeys", "user_id")
                            .to("users", "id")
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("passkeys_user_id_index")
                    .table("passkeys")
                    .col("user_id")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("passkeys").to_owned())
            .await
    }
}

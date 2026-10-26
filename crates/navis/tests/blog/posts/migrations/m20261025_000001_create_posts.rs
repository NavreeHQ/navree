//! Creates the `posts` table.

use sea_orm_migration::prelude::*;

/// `m20261025_000001_create_posts`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Posts::Table)
                    .col(ColumnDef::new(Posts::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Posts::AuthorId).uuid().not_null())
                    .col(ColumnDef::new(Posts::Title).string().not_null())
                    .col(ColumnDef::new(Posts::Body).text().not_null())
                    .col(
                        ColumnDef::new(Posts::Published)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Posts::PublishedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Posts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Posts::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_posts_author_id")
                            .from(Posts::Table, Posts::AuthorId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Posts::Table).to_owned())
            .await
    }
}

/// The `posts` table and its columns.
#[derive(DeriveIden)]
enum Posts {
    Table,
    Id,
    AuthorId,
    Title,
    Body,
    Published,
    PublishedAt,
    CreatedAt,
    UpdatedAt,
}

/// navis-auth's `users` table.
#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

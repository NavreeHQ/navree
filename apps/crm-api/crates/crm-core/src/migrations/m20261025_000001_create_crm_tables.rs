//! Creates `companies`, `contacts`, `deals` and `activities` with their
//! indexes.

use sea_orm_migration::prelude::*;

/// `m20261025_000001_create_crm_tables`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(companies()).await?;
        manager.create_table(contacts()).await?;
        manager.create_table(deals()).await?;
        manager.create_table(activities()).await?;
        for index in indexes() {
            manager.create_index(index).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            Activities::Table.into_iden(),
            Deals::Table.into_iden(),
            Contacts::Table.into_iden(),
            Companies::Table.into_iden(),
        ] {
            manager
                .drop_table(Table::drop().table(table).to_owned())
                .await?;
        }
        Ok(())
    }
}

/// A `NOT NULL` UTC timestamp column.
fn timestamp(column: impl IntoIden) -> ColumnDef {
    ColumnDef::new(column)
        .timestamp_with_time_zone()
        .not_null()
        .to_owned()
}

/// A nullable UTC timestamp column.
fn nullable_timestamp(column: impl IntoIden) -> ColumnDef {
    ColumnDef::new(column)
        .timestamp_with_time_zone()
        .null()
        .to_owned()
}

/// A nullable string column.
fn text(column: impl IntoIden) -> ColumnDef {
    ColumnDef::new(column).string().null().to_owned()
}

/// `owner_id` → `users.id` (a user with records cannot be deleted).
fn owner_fk(
    table: impl IntoIden + Clone,
    name: &str,
    column: impl IntoIden,
) -> ForeignKeyCreateStatement {
    ForeignKey::create()
        .name(name)
        .from(table, column)
        .to(Users::Table, Users::Id)
        .on_delete(ForeignKeyAction::Restrict)
        .to_owned()
}

fn companies() -> TableCreateStatement {
    Table::create()
        .table(Companies::Table)
        .col(
            ColumnDef::new(Companies::Id)
                .uuid()
                .not_null()
                .primary_key(),
        )
        .col(ColumnDef::new(Companies::Name).string().not_null())
        .col(text(Companies::Domain))
        .col(text(Companies::Industry))
        .col(ColumnDef::new(Companies::Size).string_len(16).null())
        .col(text(Companies::Phone))
        .col(text(Companies::Website))
        .col(text(Companies::City))
        .col(text(Companies::Country))
        .col(ColumnDef::new(Companies::OwnerId).uuid().not_null())
        .col(timestamp(Companies::CreatedAt))
        .col(timestamp(Companies::UpdatedAt))
        .col(nullable_timestamp(Companies::DeletedAt))
        .foreign_key(&mut owner_fk(
            Companies::Table,
            "fk_companies_owner_id",
            Companies::OwnerId,
        ))
        .to_owned()
}

fn contacts() -> TableCreateStatement {
    Table::create()
        .table(Contacts::Table)
        .col(ColumnDef::new(Contacts::Id).uuid().not_null().primary_key())
        .col(ColumnDef::new(Contacts::FirstName).string().not_null())
        .col(ColumnDef::new(Contacts::LastName).string().not_null())
        .col(text(Contacts::Email))
        .col(text(Contacts::Phone))
        .col(text(Contacts::Title))
        .col(
            ColumnDef::new(Contacts::Status)
                .string_len(16)
                .not_null()
                .default("lead"),
        )
        .col(ColumnDef::new(Contacts::CompanyId).uuid().null())
        .col(ColumnDef::new(Contacts::OwnerId).uuid().not_null())
        .col(timestamp(Contacts::CreatedAt))
        .col(timestamp(Contacts::UpdatedAt))
        .col(nullable_timestamp(Contacts::DeletedAt))
        .foreign_key(
            ForeignKey::create()
                .name("fk_contacts_company_id")
                .from(Contacts::Table, Contacts::CompanyId)
                .to(Companies::Table, Companies::Id)
                .on_delete(ForeignKeyAction::SetNull),
        )
        .foreign_key(&mut owner_fk(
            Contacts::Table,
            "fk_contacts_owner_id",
            Contacts::OwnerId,
        ))
        .to_owned()
}

fn deals() -> TableCreateStatement {
    Table::create()
        .table(Deals::Table)
        .col(ColumnDef::new(Deals::Id).uuid().not_null().primary_key())
        .col(ColumnDef::new(Deals::Title).string().not_null())
        .col(ColumnDef::new(Deals::AmountCents).big_integer().not_null())
        .col(
            ColumnDef::new(Deals::Currency)
                .string_len(3)
                .not_null()
                .default("USD"),
        )
        .col(
            ColumnDef::new(Deals::Stage)
                .string_len(16)
                .not_null()
                .default("lead"),
        )
        .col(
            ColumnDef::new(Deals::Position)
                .integer()
                .not_null()
                .default(0),
        )
        .col(ColumnDef::new(Deals::ExpectedCloseDate).date().null())
        .col(nullable_timestamp(Deals::ClosedAt))
        .col(text(Deals::LostReason))
        .col(ColumnDef::new(Deals::CompanyId).uuid().not_null())
        .col(ColumnDef::new(Deals::ContactId).uuid().null())
        .col(ColumnDef::new(Deals::OwnerId).uuid().not_null())
        .col(timestamp(Deals::CreatedAt))
        .col(timestamp(Deals::UpdatedAt))
        .foreign_key(
            ForeignKey::create()
                .name("fk_deals_company_id")
                .from(Deals::Table, Deals::CompanyId)
                .to(Companies::Table, Companies::Id)
                .on_delete(ForeignKeyAction::Restrict),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_deals_contact_id")
                .from(Deals::Table, Deals::ContactId)
                .to(Contacts::Table, Contacts::Id)
                .on_delete(ForeignKeyAction::SetNull),
        )
        .foreign_key(&mut owner_fk(
            Deals::Table,
            "fk_deals_owner_id",
            Deals::OwnerId,
        ))
        .to_owned()
}

fn activities() -> TableCreateStatement {
    Table::create()
        .table(Activities::Table)
        .col(
            ColumnDef::new(Activities::Id)
                .uuid()
                .not_null()
                .primary_key(),
        )
        .col(ColumnDef::new(Activities::Kind).string_len(16).not_null())
        .col(ColumnDef::new(Activities::Subject).string().not_null())
        .col(ColumnDef::new(Activities::Body).text().null())
        .col(nullable_timestamp(Activities::DueAt))
        .col(nullable_timestamp(Activities::CompletedAt))
        .col(
            ColumnDef::new(Activities::SubjectType)
                .string_len(16)
                .not_null(),
        )
        .col(ColumnDef::new(Activities::SubjectId).uuid().not_null())
        .col(ColumnDef::new(Activities::OwnerId).uuid().not_null())
        .col(ColumnDef::new(Activities::AssigneeId).uuid().null())
        .col(timestamp(Activities::CreatedAt))
        .col(timestamp(Activities::UpdatedAt))
        .foreign_key(&mut owner_fk(
            Activities::Table,
            "fk_activities_owner_id",
            Activities::OwnerId,
        ))
        .foreign_key(
            ForeignKey::create()
                .name("fk_activities_assignee_id")
                .from(Activities::Table, Activities::AssigneeId)
                .to(Users::Table, Users::Id)
                .on_delete(ForeignKeyAction::SetNull),
        )
        .to_owned()
}

/// `(name, table, columns)` of every index.
fn indexes() -> Vec<IndexCreateStatement> {
    fn index<T: IntoIden + Copy + 'static>(
        name: &str,
        table: impl IntoIden,
        columns: &[T],
    ) -> IndexCreateStatement {
        let mut index = Index::create();
        index.name(name).table(table);
        for column in columns {
            index.col(*column);
        }
        index.to_owned()
    }
    vec![
        index(
            "idx_companies_owner_id",
            Companies::Table,
            &[Companies::OwnerId],
        ),
        index("idx_companies_name", Companies::Table, &[Companies::Name]),
        index(
            "idx_companies_deleted_at",
            Companies::Table,
            &[Companies::DeletedAt],
        ),
        index(
            "idx_contacts_company_id",
            Contacts::Table,
            &[Contacts::CompanyId],
        ),
        index(
            "idx_contacts_owner_id",
            Contacts::Table,
            &[Contacts::OwnerId],
        ),
        index("idx_contacts_status", Contacts::Table, &[Contacts::Status]),
        index("idx_contacts_email", Contacts::Table, &[Contacts::Email]),
        index(
            "idx_contacts_deleted_at",
            Contacts::Table,
            &[Contacts::DeletedAt],
        ),
        index(
            "idx_deals_stage_position",
            Deals::Table,
            &[Deals::Stage, Deals::Position],
        ),
        index("idx_deals_company_id", Deals::Table, &[Deals::CompanyId]),
        index("idx_deals_contact_id", Deals::Table, &[Deals::ContactId]),
        index("idx_deals_owner_id", Deals::Table, &[Deals::OwnerId]),
        index(
            "idx_activities_subject",
            Activities::Table,
            &[Activities::SubjectType, Activities::SubjectId],
        ),
        index(
            "idx_activities_owner_id",
            Activities::Table,
            &[Activities::OwnerId],
        ),
        index(
            "idx_activities_assignee_id",
            Activities::Table,
            &[Activities::AssigneeId],
        ),
        index(
            "idx_activities_due_at",
            Activities::Table,
            &[Activities::DueAt],
        ),
    ]
}

#[derive(DeriveIden, Clone, Copy)]
enum Companies {
    Table,
    Id,
    Name,
    Domain,
    Industry,
    Size,
    Phone,
    Website,
    City,
    Country,
    OwnerId,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden, Clone, Copy)]
enum Contacts {
    Table,
    Id,
    FirstName,
    LastName,
    Email,
    Phone,
    Title,
    Status,
    CompanyId,
    OwnerId,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden, Clone, Copy)]
enum Deals {
    Table,
    Id,
    Title,
    AmountCents,
    Currency,
    Stage,
    Position,
    ExpectedCloseDate,
    ClosedAt,
    LostReason,
    CompanyId,
    ContactId,
    OwnerId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden, Clone, Copy)]
enum Activities {
    Table,
    Id,
    Kind,
    Subject,
    Body,
    DueAt,
    CompletedAt,
    SubjectType,
    SubjectId,
    OwnerId,
    AssigneeId,
    CreatedAt,
    UpdatedAt,
}

/// navree-auth's `users` table.
#[derive(DeriveIden, Clone, Copy)]
enum Users {
    Table,
    Id,
}

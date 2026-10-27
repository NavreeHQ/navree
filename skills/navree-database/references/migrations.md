# Migrations in Navree

Each module owns its migrations as one `MigrationSet`. The app's migrator
runs every set: sets in dependency order (`.after([..])`, otherwise
alphabetical), migrations inside a set in name order (names start with a
timestamp). Applied migrations are recorded in `navree_migrations`
(`module`, `name`, `batch`, `applied_at`), not in SeaORM's
`seaql_migrations`.

## Generate

```sh
navree make:migration posts create_posts_table     # CREATE TABLE with id + timestamps
navree make:migration posts add_published_to_posts # empty up/down
```

The file is `src/modules/posts/migrations/m<YYYYMMDD>_<HHMMSS>_<name>.rs`;
the generator adds `mod ...;` and `.migration(...::Migration)` to the set in
`migrations/mod.rs`, creating that file (and the `m.migrations(..)` line in
the module's `wire`) when it is the module's first migration.

## Create a table

```rust
use sea_orm_migration::prelude::*;

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
                    .col(ColumnDef::new(Posts::OwnerId).uuid().null())
                    .col(ColumnDef::new(Posts::Name).string().not_null())
                    .col(ColumnDef::new(Posts::CreatedAt).timestamp_with_time_zone().not_null())
                    .col(ColumnDef::new(Posts::UpdatedAt).timestamp_with_time_zone().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_posts_owner_id")
                    .table(Posts::Table)
                    .col(Posts::OwnerId)
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

/// `Posts::Table` is `posts`, `Posts::OwnerId` is `owner_id`.
#[derive(DeriveIden)]
enum Posts {
    Table,
    Id,
    OwnerId,
    Name,
    CreatedAt,
    UpdatedAt,
}
```

`async_trait` comes from `sea_orm_migration::prelude`. Match the entity:
`uuid()` for `Uuid`, `timestamp_with_time_zone()` for `DateTimeUtc`,
`boolean()` for `bool`, `big_integer()` for `i64`, `string()`/`text()` for
`String`, `.null()` for `Option<T>`.

## Add a column

```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Posts::Table)
                    .add_column(ColumnDef::new(Posts::Published).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(Table::alter().table(Posts::Table).drop_column(Posts::Published).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Published,
}
```

## Foreign keys

```rust
.col(ColumnDef::new(Deals::CompanyId).uuid().not_null())
.foreign_key(
    ForeignKey::create()
        .name("fk_deals_company_id")
        .from(Deals::Table, Deals::CompanyId)
        .to(Companies::Table, Companies::Id)
        .on_delete(ForeignKeyAction::Restrict),
)
```

Declare the other table's iden locally (`enum Companies { Table, Id }`) and
order the sets: `MigrationSet::new("deals").after(["companies"])`. A table
referencing `users` runs after `"auth"`. Framework sets: `auth` (users,
refresh and reset tokens), `access` (after `auth`), `audit`, `queue`,
`notify`, `schedule`, `session`. An `after` entry naming a set that isn't in
the app is ignored.

## Transactions

On SQLite and PostgreSQL each migration runs in its own transaction with its
record row, so a failed migration leaves nothing behind. For statements that
can't run in a transaction (PostgreSQL `CREATE INDEX CONCURRENTLY`, SQLite
table rebuilds needing `PRAGMA foreign_keys`), override
`fn use_transaction(&self) -> Option<bool> { Some(false) }`.

MySQL and MariaDB commit DDL on their own, so there a migration runs outside
a transaction unless it returns `Some(true)` (do that for data migrations:
their rows then roll back with the record row). A MySQL migration that fails
part way keeps what ran before the failure and isn't recorded: one DDL
statement per migration, or an `up` that can run again (`if_not_exists()`).

## Commands

| Command | Does |
|---|---|
| `navree migrate` | runs pending migrations as one batch ("Nothing to migrate." when none) |
| `navree migrate:status` | every migration with its batch and status |
| `navree migrate:rollback` | reverts the last batch, newest first (repeat for more; no `--step`) |
| `navree migrate:fresh [--seed]` | drops every table, migrates (and seeds). Never in production |

Every command boots the app; with `migrate_on_boot = true` it first applies
pending migrations, so roll back with
`APP_DATABASE__MIGRATE_ON_BOOT=false navree migrate:rollback`. In production
turn `migrate_on_boot` off and run `./<app> migrate` as a deploy step.

In tests, `app.run_command("migrate:rollback", &[])` and
`navree::db::Migrator::status(app.ctx())` check the `down` methods.

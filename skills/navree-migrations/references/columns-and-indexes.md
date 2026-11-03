# Columns, indexes and foreign keys

`use navree::schema::prelude::*;` brings `Schema`, `SchemaResult`,
`Blueprint`, `raw`, `ReferentialAction` and the TimescaleDB types.

## Column types

| Method | PostgreSQL | MySQL | SQLite |
|---|---|---|---|
| `id()` | `BIGINT` identity primary key | `BIGINT UNSIGNED AUTO_INCREMENT` | `INTEGER PRIMARY KEY AUTOINCREMENT` |
| `uuid_primary()` | `UUID` primary key `id` | `CHAR(36)` (`UUID` on MariaDB 10.7+) | `VARCHAR` |
| `ulid_primary()` | `CHAR(26)` | `CHAR(26)` | `VARCHAR` |
| `string(name)` | `VARCHAR(255)`, `.length(n)` | `VARCHAR(255)` | `VARCHAR` |
| `char(name, n)`, `text`, `medium_text`, `long_text` | `CHAR(n)`, `TEXT` | `CHAR(n)`, `TEXT`, `MEDIUMTEXT`, `LONGTEXT` | `TEXT` |
| `small_integer`, `integer`, `big_integer`, `unsigned_big_integer` | `SMALLINT`, `INTEGER`, `BIGINT` | `SMALLINT`, `INT`, `BIGINT`, `UNSIGNED` | `INTEGER` |
| `decimal(name, p, s)` | `DECIMAL(p, s)` | `DECIMAL(p, s)` | `NUMERIC` |
| `float`, `double` | `FLOAT(53)`, `DOUBLE PRECISION` | `FLOAT(53)`, `DOUBLE` | `FLOAT`, `DOUBLE` |
| `boolean` | `BOOLEAN` | `TINYINT(1)` | `TINYINT(1)` |
| `date`, `time`, `time_tz` | `DATE`, `TIME ..` | `DATE`, `TIME(6)` | `DATE`, `TIME` |
| `date_time`, `date_time_tz` | `TIMESTAMP [WITH TIME ZONE]` | `DATETIME(6)` | `DATETIME` |
| `timestamp`, `timestamp_tz` | `TIMESTAMP [WITH TIME ZONE]` | `TIMESTAMP(6)` (ends 2038) | `DATETIME` |
| `json`, `jsonb` | `JSON`, `JSONB` | `JSON` | `TEXT` |
| `binary(name)` | `BYTEA` | `BLOB`; `VARBINARY(n)` with `.length(n)`, `BINARY(n)` with `.fixed()` | `BLOB` |
| `uuid`, `ulid` | `UUID`, `CHAR(26)` | `CHAR(36)`, `CHAR(26)` | `VARCHAR` |
| `enumeration(name, [..])` | `VARCHAR` + `CHECK` | `ENUM(..)` | `VARCHAR` + `CHECK` |
| `vector(name, n)` | pgvector `VECTOR(n)` | MySQL 9 / MariaDB 11.7 | not supported |
| `tsvector`, `set`, `geometry`, `geography`, `ip_address`, `mac_address`, `year`, `raw_column(name, "SQL")` | | | |

Time columns keep microseconds (`schema.default_time_precision = 6`).
A type a database lacks fails before any SQL runs, naming the alternative.

Helpers: `timestamps_tz()` (nullable `created_at`, `updated_at`),
`soft_deletes_tz()` (`deleted_at`), `morphs("subject")` (`subject_type`,
`subject_id`, indexed; `uuid_morphs`, `nullable_morphs`),
`remember_token()`, `foreign_id` / `foreign_uuid` / `foreign_ulid(name)`.

## For model fields

| Model field | Column |
|---|---|
| `Uuid` key, `Uuid` foreign key | `uuid_primary()`, `foreign_uuid(..)`; on MySQL `binary(..).length(16).fixed()` |
| `i64` key the database assigns | `id()` |
| `Ulid` key | `ulid_primary()` |
| `String` | `string`, `text`, `char` |
| `i32`, `i64`, `bool`, `Decimal` | `integer`, `big_integer`, `boolean`, `decimal(name, p, s)` |
| `DateTime<Utc>` | `timestamp_tz` (`timestamps_tz()`, `soft_deletes_tz()`) |
| `NaiveDate` | `date` |
| `cast = "json"`, `serde_json::Value` | `json` (`jsonb` on PostgreSQL) |
| `cast = "string_enum"` | `string(..).length(n)` or `enumeration` |
| `Option<T>` | the same, `.nullable()` |

## Modifiers

`.nullable()`, `.default(value)` (`raw("CURRENT_TIMESTAMP")` for SQL),
`.use_current()`, `.unsigned()`, `.comment("..")`, `.collation("C")`,
`.length(n)`, `.precision(p)`, `.after("title")` (MySQL),
`.stored_as("price * quantity")`, `.virtual_as(..)`, `.generated_as()`,
`.change()`, and the inline indexes `.index()`, `.unique()`,
`.primary()`, `.fulltext()`, `.spatial_index()`.

## Indexes

```rust,ignore
t.unique(["workspace_id", "email"]).name("members_email");
t.index(["stage", "created_at"]);
t.unique(["email"]).where_raw("deleted_at IS NULL");   // partial (PostgreSQL, SQLite)
t.raw_index("lower(email)", "users_email_lower_index");
t.fulltext(["title", "body"]).language("english");     // not on SQLite
```

Options: `.name(..)`, `.algorithm("gin")`, `.operator_class(..)`,
`.where_raw(..)`, `.nulls_not_distinct()` (PostgreSQL 15+), `.online()`
(`CONCURRENTLY`: needs `within_transaction` false), `.deferrable()`,
`.lock("none")`, `.inplace()` (MySQL). Conventional names:
`{table}_{columns}_{kind}` (`deals_stage_created_at_index`), shortened
with a hash past 63 bytes; drops by columns find them.

## Foreign keys

```rust,ignore
t.foreign_uuid("company_id").constrained().cascade_on_delete();    // companies.id
t.foreign_id("user_id").nullable().constrained().null_on_delete(); // users.id
t.foreign_uuid("owner_id").constrained_on("users", "id");
t.foreign("currency").references("code").on("currencies").restrict_on_delete();
```

`constrained()` takes the table from the column (`company_id` ->
`companies`). Actions: `cascade_on_delete`, `restrict_on_delete`,
`null_on_delete`, `no_action_on_delete`, the `_on_update` forms,
`on_delete(ReferentialAction::SetDefault)`; `.name(..)`, `.deferrable()`,
`.not_valid()` (PostgreSQL). A violation answers 409 over HTTP.

## Tables

`schema.create`, `table`, `rename(from, to)`, `drop`, `drop_if_exists`,
`drop_columns(table, [..])`, `create_hypertable(table, time_column, |t| ..)`,
`statement(sql)`. Inside `create`: `t.if_not_exists()`, `t.temporary()`,
`t.comment(..)`, `t.engine("InnoDB")`, `t.charset(..)`, `t.collation(..)`.
Outside migrations: `Schema::new(&db)` (also a `Pool`, `Transaction` or `Uow`: anything implementing `SchemaConnection`).

Docs: https://navree.com/docs/database/columns/,
https://navree.com/docs/database/indexes-and-foreign-keys/,
https://navree.com/docs/database/schema-builder/

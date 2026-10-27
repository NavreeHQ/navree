# The query builder

`Deal::query()` returns a `Builder<Deal>`; `Deal::where_(cond)` starts one
with a condition. Shaping methods take and return the builder; running
methods are `async` and take the executor first. Builders are values:
`clone()` one to run two variations. Values are always bound.

## Running a query

| Method | Returns |
|---|---|
| `get(exec)` | `Collection<M>` (derefs to a slice of `Record<M>`) |
| `first(exec)` / `first_or_fail(exec)` | `Option<Record<M>>` / the record or `ModelNotFound` (404) |
| `sole(exec)` | the only match; `ModelNotFound` or `MultipleRecordsFound` otherwise |
| `find(exec, key)`, `find_or_fail`, `find_many(exec, keys)` | by key, within the query's conditions |
| `value(exec, Deal::TITLE)`, `pluck(exec, Deal::TITLE)`, `pluck_keyed(exec, value, key)` | one value, a `Vec`, an `IndexMap` |
| `exists(exec)`, `doesnt_exist(exec)` | `bool` |
| `count`, `count_distinct(exec, col)`, `sum(exec, col)`, `avg`, `min`, `max` | `u64`, `sum::<T>`, `Option<T>` |
| `paginate(exec, page)`, `simple_paginate`, `cursor_paginate` | `Page<Record<M>>` (`{data, meta}`), pages without a total, cursor pages |
| `chunk(exec, size, f)`, `chunk_by_id`, `lazy(..)`, `lazy_by_id(..)`, `cursor(..)` | in batches, as a stream |
| `to_sql(Dialect::Postgres)` | the SQL and values, without running |

`where_key(key)`, `where_key_in(keys)` filter by the model's key.

## Conditions

Typed columns build conditions from values of the column's type:

| Column method | SQL |
|---|---|
| `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `op("<>", v)` | comparisons |
| `is_in([..])`, `not_in([..])`, `in_subquery(q)` | `IN` (an empty list matches nothing) |
| `is_null()`, `is_not_null()` | `IS NULL` |
| `between(a, b)`, `not_between(..)` | `BETWEEN` |
| `like(p)` / `not_like` | ignores case (`ILIKE` on PostgreSQL); `like_case_sensitive`, `ilike` |
| `regex`, `regex_case_insensitive`, `not_regex` | `~`, `REGEXP_LIKE`, `REGEXP` |
| `null_safe_eq(v)`, `is_distinct_from(v)` | `NULL`-safe equality |
| `eq_column(&other)`, `op_column(op, &other)` | two columns |
| `contains(v)`, `contained_by(v)`, `overlaps(vs)`, `has_key(k)` | JSON |

```rust
Deal::query()
    .where_(Deal::CLOSED_AT.is_null())
    .where_group(|q| {
        q.where_(Deal::STAGE.eq(Stage::Won))
            .or_where(Deal::AMOUNT_CENTS.gte(1_000_000))
    })
    .when_some(filter.search, |q, term| q.where_like(Deal::TITLE, format!("%{term}%")))
    .when(filter.mine, |q| q.where_(Deal::OWNER_ID.eq(me)))
```

- `or_where`, `where_not`, `or_where_not`; `where_group`, `or_where_group`,
  `where_not_group`; values combine with `a.and(b)`, `a.or(b)`,
  `Filter::all([..])`, `Filter::any([..])`.
- Shortcuts: `where_in`, `where_not_in`, `where_null`, `where_not_null`,
  `where_between`, `where_column(a, ">", b)`, `where_like`, each with an
  `or_` form.
- Dates: `where_date`, `where_time`, `where_day`, `where_month`,
  `where_year` (`(column, "=", value)`).
- JSON: `col("meta->source").eq("referral")`, `Deal::META.json("source")`,
  `where_json_contains`, `where_json_length`, `where_json_contains_key`.
- Full text: `where_full_text([Deal::TITLE], "renewal")`,
  `where_full_text_with(.., FullText::websearch())` (PostgreSQL, MySQL; not
  SQLite).
- Subqueries: `where_exists(query)`, `where_in_sub(col, query)`,
  `select_sub(query, "alias")`.
- Raw: `where_raw("amount_cents > ? * 100", [500])`, `raw(sql, values)` as a
  condition or expression. `?` placeholders on every database; never format
  input into SQL.
- Any column: `col("companies.name")`, `col_as::<i64>("total")`.

## Selecting, joining, grouping, ordering

```rust
Deal::query().select([Deal::ID.expr(), Deal::TITLE.expr()]);
Deal::query().select_expr(Expr::col("amount_cents").mul(2), "doubled");
Deal::query()
    .join("companies", Deal::COMPANY_ID, "=", Company::ID)
    .add_select([("companies.name", "company_name")]);
table("deals")
    .select(["stage"])
    .select_expr(Expr::count_all(), "deals")
    .group_by(["stage"])
    .having(Expr::count_all().gte(5));
```

- Columns that aren't model columns become **extras**:
  `record.extra::<i64>("company_count")?`. A partial `select` of model
  columns fails in strict mode unless the missing fields are
  `#[model(default)]`.
- `left_join`, `right_join`, `cross_join`, the `_on` forms with a closure,
  `join_sub`, `join_lateral`.
- `order_by(col)`, `order_by_desc(col)`, `latest()`, `oldest()` (need
  `timestamps`; else `latest_by(col)`), `in_random_order()`,
  `reorder()`, `limit(n)`, `offset(n)`, `group_limit(n, col)`.
- `union(q)`, `union_all(q)`.

## Locks

Inside a unit of work: `lock_for_update()` (`FOR UPDATE`), `shared_lock()`,
`skip_locked()`, `no_wait()`, `lock(LockMode::NoKeyUpdate)`. SQLite leaves
lock clauses out (one writer at a time). `record.refresh_for_update(&uow)`
rereads a record under a lock.

## Writing many rows (no model events, no audit changes)

```rust
let lost: u64 = Deal::query()
    .where_(Deal::STAGE.eq(Stage::Lead))
    .where_(Deal::UPDATED_AT.lt(cutoff))
    .update(&uow, [Deal::STAGE.set(Stage::Lost), Deal::CLOSED_AT.set(Utc::now())])
    .await?;
Activity::query().where_(Activity::COMPLETED_AT.lt(cutoff)).delete(&uow).await?;
Tag::upsert(&uow, tags, [Tag::NAME], [Tag::COLOR]).await?;
```

`increment(&uow, col, n)`, `decrement`, `touch`, `force_delete`,
`restore` work on queries too. Bulk inserts: `insert` (as is),
`fill_and_insert` (keys and timestamps filled), `insert_or_ignore`,
`upsert(exec, models, unique_by, update)`, `truncate`. A statement over the
bind limit is split and run in one transaction.

## Without a model, and raw SQL

```rust
use navree::orm::{Row, affecting_statement, raw, scalar, select};

let rows: Vec<Row> = table("deal_tag").where_(col("deal_id").eq(deal_id)).get(&db).await?;
let tag_id: i64 = rows[0].get("tag_id")?;
let open: Option<i64> = scalar(&db, raw("SELECT COUNT(*) FROM deals WHERE stage = ?", ["lead"])).await?;
let changed: u64 = affecting_statement(&uow, raw("UPDATE deals SET stage = ? WHERE id = ?", [..])).await?;
```

`select`, `select_one`, `scalar`, `statement`, `affecting_statement`,
`unprepared`, `cursor`: reads go where the executor reads, statements where
it writes.

## Watching queries

`navree::db::QueryLog::capture(future).await` returns `(output, queries)`
with each statement's SQL, values and duration. In tests:
`navree::orm::testing::{count_queries, expect_query_count}`.

Docs: https://navree.com/docs/orm/query-builder/,
https://navree.com/docs/orm/pagination-and-streaming/

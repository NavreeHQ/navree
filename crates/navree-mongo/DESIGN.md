# navree-mongo: design

MongoDB for Navree, on the official `mongodb` driver: the ORM's models on
collections, the connection as a module, units of work on sessions, and the
schema builder and migrations on collections, with time-series collections
as the time-series engine.

```rust
use navree::mongo::{MongoDb, ObjectId};
use navree::orm::Model;

#[derive(Model, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[model(collection = "deals", driver = "mongodb", timestamps, soft_deletes)]
pub struct Deal {
    pub id: ObjectId,
    pub company_id: Option<ObjectId>,
    pub title: String,
    pub amount: i64,
    #[model(cast = "json")]
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn biggest(mongo: &MongoDb) -> navree::orm::Result<Vec<String>> {
    Deal::query()
        .where_json_contains("tags", "vip")
        .order_by_desc(Deal::AMOUNT)
        .limit(10)
        .pluck(mongo, Deal::TITLE)
        .await
}
```

## 1. Place in the framework

A runtime crate at **layer 5**. It depends on navree-kernel, -contracts,
-config, -telemetry, -schema, -db and -orm (the HIERARCHY row in
`crates/navree/tests/dependency_graph.rs`), and on `mongodb` (rustls with
ring, DNS seed lists) and `bson`. The facade re-exports it as
`navree::mongo` behind the opt-in features `mongo` and `mongodb`; the
prelude then has `MongoModule`, `MongoDb` and `ModuleBuilderMongoExt`.

## 2. Modules

| Module | What it holds |
|---|---|
| `options` | `MongoOptions`: the `[database.mongodb]` section, checked at boot; named connections |
| `connection` | `MongoDb` (the connection, an executor), `MongoSession`, `MongoExecutor` |
| `driver` | `MongoDriver: DocumentDriver`: runs commands, reports telemetry, counts integer keys |
| `compile/` | the IR as commands: `filter` (conditions), `expr` (values, functions, time buckets, accumulators), `read` (plans: joins, projection, order, page, unions), `group`, `subquery` (`$lookup`s), `write`, `scope` (where a column is), `like` |
| `command` | `MongoCommand`: what runs, shown in the shell's syntax; `shape()` without values |
| `convert`, `extended`, `object_id` | `Value` and BSON both ways, serde helpers, `ObjectId` |
| `uow` | `MongoUow`, `MongoChangeObserver` |
| `native`, `bulk`, `stream` | `Filter::document`, `pipeline` stages after a query, `to_mongo()`, bulk writes, cursor streams |
| `schema/` | `MongoSchema`, `MongoBlueprint`, the blueprint compiler |
| `migrate/` | `MongoMigration`, `MongoMigrationSet`, `MongoMigrator`, the `mongo:migrate` commands |
| `module` | `MongoModule`, the `mongodb` readiness check, `ModuleBuilderMongoExt` |
| `error` | `MongoError`; how MongoDB's errors become `OrmError`s |

## 3. Queries

A query compiles to a **plan**: the collection and stages. A plan whose
stages are a `$match`, `$sort`, `$skip`, `$limit` and `$project` (each at
most once, in that order) runs as a `find`; anything else as an
aggregation pipeline. A plain `count(*)` is a `countDocuments`.

| Query | MongoDB |
|---|---|
| comparisons with a value | the query language (`{"amount": {"$gte": 10}}`), documents and arrays as `$eq` |
| comparisons between fields or computed values | `$expr` |
| `LIKE` | an escaped `$regex`: anchored unless the pattern starts or ends with `%`, `i` for case-insensitive, `s` so `.` crosses lines |
| `contains_ignore_case`, `eq_ignore_case` (`Where::IgnoreCase`) | the text with its wildcards escaped, as a `LIKE` (`%text%`, or the whole value) with `i`: MongoDB's regular expressions fold every letter with case forms and keep accents |
| date parts | `$dateToString`, `$dayOfMonth`, `$month`, `$year` in UTC |
| JSON paths, containment, length, keys | dot paths, `$all` and dotted equalities, `$size`, `$exists` |
| full text | `$text`, first in the pipeline, joined with AND at the top |
| joins | `$lookup` and `$unwind` (inner drops unmatched documents, left keeps them) |
| `EXISTS`, `IN (subquery)`, scalar subqueries | a `$lookup` per document, correlated through `let` variables |
| unions | `$unionWith`, then `$group` on `$$ROOT` for `UNION`, left to right |
| groups, `HAVING` | `$group` with the group keys under `_id`, `$match` after it |
| `DISTINCT`, `DISTINCT ON` | `$group` on the selected fields; `$first` of `$$ROOT` per key |
| `time_bucket`, `first`, `last` | TimescaleDB's alignment (origin Monday 2000-01-03) in milliseconds for fixed widths, `$dateTrunc` for months and years; `$top` and `$bottom` |
| orders | `$sort`, with `$set` first for computed keys, NULL placement and `$rand` |

Decisions:

- **NULL.** In SQL no comparison holds for NULL; in aggregation null sorts
  below every value, so `null < 5` holds. Every `$expr` comparison checks
  the side a null would satisfy (`{"$gt": [x, null]}`) unless it is a
  constant; equality between two nullable sides checks one of them;
  `<=>` keeps null equal to null. Negations (`<>`, `NOT IN`, `NOT LIKE`,
  `where_not`) keep MongoDB's rule, documented: they match documents
  whose field is null or missing.
- **Join keys.** An equality between a field of the looked-up collection
  and one of the enclosing document is taken out of the conditions and
  matched by `localField`/`foreignField`, which an index serves (an `$expr`
  on a variable reads the whole collection for every document). MongoDB
  matches a null local field with documents whose foreign field is null or
  missing; SQL matches nothing, so unless one side is `_id` a
  `{"foreign": {"$ne": null}}` stage comes first.
- **Names.** In a subquery or a join, an alias hides the table's name, as
  in SQL: in `companies AS twins`, `companies.country` is the enclosing
  document's. A union's columns take the first query's names, by
  position. A computed column needs an alias (documents have named
  fields).
- **Refused**, with `OrmError::Unsupported` naming what to do instead: raw
  SQL (conditions, expressions, orders), row locks (two transactions
  writing the same document conflict and one is retried), `RIGHT JOIN`,
  full text inside OR, NOT or a group.

## 4. Writes and keys

- Inserts are one `insertMany`. ObjectId, UUID and ULID keys are made
  before the insert; integer keys the database assigns (`KeyKind::
  Increments`) come from `navree_sequences`: `findOneAndUpdate` with
  `$inc` and `upsert` on `{_id: "<collection>.<field>"}`, in the session
  when there is one.
- Updates are one `updateMany`: `$set` and `$inc` for values and
  increments, an update pipeline when a value is computed from the
  document. Deletes are one `deleteMany`. A write with an uncorrelated
  `IN (subquery)` resolves the subquery first; a limited or ordered write,
  or one whose filter needs stages, finds the `_id`s first.
- Duplicate keys (codes 11000, 11001, 12582) are unique violations (409).
- Date-times are kept to the millisecond: `time_precision()` is 3, so the
  ORM's timestamps are truncated before they are saved.
- Inserts with `on_conflict` (the builders' `insert_or_ignore` and
  `upsert`, on tables and models; `bulk.rs`): skipping conflicts on any
  unique index is an unordered `insertMany` that skips duplicate-key
  errors (not inside a transaction, which MongoDB aborts on any write
  error); skipping conflicts on named fields, and updating them, is one
  unordered `update` command of `{q: unique fields, u: {$set: updated,
  $setOnInsert: the rest}, upsert: true}` statements, which works in a
  transaction. An upsert sets a field to the row's value
  (`Expr::Excluded`) or to a value; an update computed from the stored
  document, an insert that returns its rows while skipping conflicts, and
  `insert_using` (`INSERT … SELECT`) answer `Unsupported`.

### Streams

`MongoStream::stream(exec)` yields a query's records (or a table query's
rows) as the cursor delivers them, batch by batch. A `find` streams as the
equivalent pipeline. In a session the cursor's batches are fetched with the
session locked only for the fetch, so other commands run in the session
(and its transaction) between batches. Records run their `retrieved`
hooks as they arrive; the stream ends after its first error. The driver's
`DocumentDriver::stream` runs the same code (`MongoDriver::stream_documents`),
so `cursor` and the `lazy` family on the builders read MongoDB cursors too.

## 5. Sessions and units of work

- `MongoSession`: a session (reads see their writes); transactions by
  hand.
- `MongoUow`: a session with a transaction, the actor and request id, the
  change events of model writes, after-commit hooks. `commit` shows the
  events to the app's `MongoChangeObserver`s inside the transaction (an
  error rolls back), commits (once more on an unknown commit result), then
  runs the hooks. `scope` makes it the task's current unit of work, which a
  plain `MongoDb` of the same connection joins.
- `MongoDb::transaction(work)` runs `work` in a unit of work and runs it
  again (three attempts) on a transient transaction error (a write
  conflict).
- `MongoDb::begin_in(&Uow)` joins navree-db's unit of work: change events
  go to it (its observers, the audit trail), and the MongoDB transaction
  commits in an after-commit hook of it. The two commits are not atomic
  together; a MongoDB commit failing after the SQL one is logged.
- Transactions need a replica set or a sharded cluster; a standalone
  server's refusal is explained (`MongoError::Transactions`).

## 6. Schema and migrations

`MongoSchema` takes navree-schema's blueprints:

- `create`: `create` with a `$jsonSchema` validator (one property per
  column, typed, `null` allowed for nullable columns, `maxLength`, `enum`,
  `minimum: 0` for unsigned; required unless nullable), then
  `createIndexes`: unique and regular indexes, unique indexes over
  nullable fields partial on `$type` (NULLs do not collide), text and
  `2dsphere` indexes, TTL indexes (`MongoBlueprint::expire_after`), an
  index for each foreign key (MongoDB enforces no references; a notice
  says so). Generated columns and `tsvector` are refused.
- `table`: fields added (with a default, existing documents get it; a
  required field without one is refused when documents exist), changed,
  dropped (`$unset` in every document, and their indexes) and renamed
  (`$rename`, and their indexes rebuilt), indexes created, dropped and
  renamed (rebuilt), then the validator through `collMod`. Rewrites bypass
  the validator the documents are leaving; refused documents fail the
  change.
- Hypertables are time-series collections: `timeField`, `metaField` from
  the space partition, `granularity` from the chunk interval (seconds up
  to an hour, minutes up to a day, hours beyond), `expireAfterSeconds`
  from the retention. MongoDB takes no validator on them (a notice says
  so) and cannot convert a collection into one (the error gives the
  recipe). Described continuous aggregates are views.
- `pretending()` records the commands instead of running them;
  `statements()` and `notices()` report what ran and what MongoDB does
  differently.

Migrations have their own runner, so an app on MongoDB alone needs no SQL
database: `MongoMigration` (`up`, `down`, `within_transaction`), sets per
module ordered by `after`, records `{_id: "module/name", module, name,
batch, applied_at}` in `navree_migrations`. Schema changes run outside
transactions (MongoDB refuses most in one); a data migration opts in and
its writes commit with its record. Commands: `mongo:migrate` (`--pretend`,
`--step`), `:rollback` (`--step`, `--batch`), `:reset`, `:refresh`,
`:fresh`, `:status`; `migrate_on_boot` runs them while `MongoDb` is built.

## 7. Telemetry and errors

Every command is a `QueryExecuted` event with its shape (values replaced
by `?`), its duration, the rows returned or affected, and the connection
(the URL without its password, and the database). Commands slower than
`slow_query_ms` are logged as warnings; `log_queries` logs every shape.

`MongoError` covers what happens outside a model query (options, the
driver, schema, migrations, finished units of work, the app's own errors);
`From<MongoError> for OrmError` keeps duplicate keys as unique violations
and `Unsupported` as `Unsupported`.

## 8. Testing

`tests/mongo/` is one suite:

- `commands`: what queries compile to (insta snapshots, one stage per
  line), without a server; `module` boots apps with an unreachable server.
- Everything else runs against `NAVREE_TEST_MONGODB_URL` (a replica set:
  `scripts/dev-databases.ps1` starts MongoDB 8.0 as `rs0` on port 57017;
  CI runs `.github/workflows/mongodb.yml`), each test in a database of its
  own, dropped at the end. Without the variable those tests pass with a
  notice, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.

## 9. Hand-offs

- **orm-streams** (merged): `on_conflict` in `InsertIr` compiles to the
  bulk writes of section 4; `DocumentDriver::stream` lets `cursor` and
  `lazy` read MongoDB cursors through `stream_documents`.
- **orm-relations**: relations load through `IN` lists (`$in`);
  existence queries and relation aggregates compile to `$lookup`s already.
- **orm-factories**: factories create models through `Model::create`, which
  runs on `MongoDb` like on `Db`.

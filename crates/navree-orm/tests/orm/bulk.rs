//! Bulk writes on SQLite: multi-row inserts (split under the bind limit),
//! insert-or-ignore, keys read back, `INSERT … SELECT`, upserts,
//! update-or-insert, `UPDATE … FROM`, JSON path updates, table writes with
//! joins and limits, truncation, model inserts and `save_or_ignore`.

use std::sync::{Arc, Mutex};

use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
use navree_orm::hooks::{ModelEvent, ModelHook};
use navree_orm::{ConflictUpdate, ExecContext, Expr, Kind, Model, OrmError, Record, Result, Value, col, col_as, raw, table, unprepared};

use crate::models::{Company, Deal, DealMeta, Event, Setting, Stage, Tag};
use crate::support::{app, count_rows, sqlite};

async fn tag(db: &navree_db::Db, name: &str, color: &str) -> Record<Tag> {
    Tag::create(
        db,
        Tag {
            name: name.into(),
            color: color.into(),
            ..Tag::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn table_inserts_write_every_row_in_one_go() {
    let t = sqlite().await;
    let inserted = table("tags")
        .insert(
            &t.db,
            [
                vec![("name", Value::from("a")), ("color", Value::from("red"))],
                // Columns in another order line up by name.
                vec![("color", Value::from("blue")), ("name", Value::from("b"))],
            ],
        )
        .await
        .unwrap();
    assert_eq!(inserted, 2);
    let colors: Vec<String> = table("tags")
        .order_by("name")
        .pluck(&t.db, col_as::<String>("color"))
        .await
        .unwrap();
    assert_eq!(colors, ["red", "blue"]);

    // Nothing to insert: no statement.
    let none: Vec<Vec<(&str, Value)>> = Vec::new();
    assert_eq!(table("tags").insert(&t.db, none).await.unwrap(), 0);

    let error = table("tags")
        .insert(
            &t.db,
            [
                vec![("name", Value::from("c")), ("color", Value::from("x"))],
                vec![("name", Value::from("d"))],
            ],
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: row 2 of an insert into `tags` has 1 columns, the first has 2"
    );
}

/// 10,000 rows of 4 values pass SQLite's 32,766 bind values: the insert is
/// split in two statements that run in one transaction, so a failure in
/// the second part leaves nothing behind.
#[tokio::test]
async fn large_inserts_split_under_the_bind_limit_and_stay_whole() {
    let t = sqlite().await;
    unprepared(
        &t.db,
        "CREATE TABLE readings (id INTEGER PRIMARY KEY, sensor TEXT NOT NULL, celsius REAL NOT NULL, taken TEXT NOT NULL UNIQUE)",
    )
    .await
    .unwrap();
    let rows = |n: usize, poison: bool| {
        (0..n)
            .map(move |i| {
                // With `poison`, the last row repeats the first row's unique value.
                let taken = if poison && i == n - 1 { 0 } else { i };
                vec![
                    ("id", Value::from(i64::try_from(i).unwrap() + 1)),
                    ("sensor", Value::from("a")),
                    ("celsius", Value::from(20.5)),
                    ("taken", Value::from(format!("t{taken}"))),
                ]
            })
            .collect::<Vec<_>>()
    };
    let error = table("readings")
        .insert(&t.db, rows(10_000, true))
        .await
        .unwrap_err();
    assert!(error.is_unique_violation(), "{error}");
    assert_eq!(
        count_rows(&t.db, "readings").await,
        0,
        "the first part rolled back"
    );

    assert_eq!(
        table("readings")
            .insert(&t.db, rows(10_000, false))
            .await
            .unwrap(),
        10_000
    );
    assert_eq!(count_rows(&t.db, "readings").await, 10_000);
}

#[tokio::test]
async fn keys_come_back_from_single_inserts() {
    let t = sqlite().await;
    tag(&t.db, "first", "red").await;
    let id: i64 = table("tags")
        .insert_get_id(&t.db, [("name", "second"), ("color", "blue")])
        .await
        .unwrap();
    assert_eq!(id, 2);
    let key: i32 = table("user_accounts")
        .insert_get_key(&t.db, [("display_name", "Ada")], "account_id")
        .await
        .unwrap();
    assert_eq!(key, 1);
}

#[tokio::test]
async fn insert_or_ignore_skips_rows_that_conflict() {
    let t = sqlite().await;
    tag(&t.db, "vip", "gold").await;
    let inserted = table("tags")
        .insert_or_ignore(
            &t.db,
            [
                [("name", "vip"), ("color", "red")],
                [("name", "new"), ("color", "green")],
            ],
        )
        .await
        .unwrap();
    assert_eq!(inserted, 1);
    assert_eq!(
        table("tags")
            .where_(col("name").eq("vip"))
            .value(&t.db, col_as::<String>("color"))
            .await
            .unwrap()
            .as_deref(),
        Some("gold"),
        "the stored row is untouched"
    );

    // Only uniqueness conflicts are skipped: other errors still fail.
    let error = table("tags")
        .insert_or_ignore(
            &t.db,
            [[("name", Value::Null(Kind::String)), ("color", Value::from("x"))]],
        )
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Db(_)), "{error}");

    let created = table("tags")
        .insert_or_ignore_returning(
            &t.db,
            [
                [("name", "vip"), ("color", "pink")],
                [("name", "fresh"), ("color", "teal")],
            ],
            ["id", "name"],
            ["name"],
        )
        .await
        .unwrap();
    assert_eq!(created.len(), 1, "only the inserted row comes back");
    assert_eq!(created[0].get::<String>("name").unwrap(), "fresh");
    let stored: Option<i64> = table("tags")
        .where_(col("name").eq("fresh"))
        .value(&t.db, col_as::<i64>("id"))
        .await
        .unwrap();
    assert_eq!(Some(created[0].get::<i64>("id").unwrap()), stored);

    let error = table("tags")
        .insert_or_ignore_returning(
            &t.db,
            [[("name", "x"), ("color", "y")]],
            Vec::<&str>::new(),
            ["name"],
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: insert_or_ignore_returning into `tags` names no column to return"
    );
}

#[tokio::test]
async fn inserts_from_queries_copy_rows() {
    let t = sqlite().await;
    unprepared(
        &t.db,
        "CREATE TABLE palette (name TEXT PRIMARY KEY, color TEXT NOT NULL)",
    )
    .await
    .unwrap();
    tag(&t.db, "vip", "gold").await;
    tag(&t.db, "risk", "red").await;
    let copied = table("palette")
        .insert_using(
            &t.db,
            ["name", "color"],
            table("tags")
                .select(["name", "color"])
                .where_(col("color").ne("red")),
        )
        .await
        .unwrap();
    assert_eq!(copied, 1);
    let again = table("palette")
        .insert_or_ignore_using(
            &t.db,
            ["name", "color"],
            table("tags").select(["name", "color"]),
        )
        .await
        .unwrap();
    assert_eq!(again, 1, "vip exists, risk is new");
    assert_eq!(count_rows(&t.db, "palette").await, 2);
}

#[tokio::test]
async fn table_upserts_insert_or_update_by_unique_columns() {
    let t = sqlite().await;
    unprepared(
        &t.db,
        "CREATE TABLE page_views (path TEXT PRIMARY KEY, views INTEGER NOT NULL, title TEXT NOT NULL)",
    )
    .await
    .unwrap();
    let visit = |path: &'static str, title: &'static str| {
        vec![
            ("path", Value::from(path)),
            ("views", Value::from(1)),
            ("title", Value::from(title)),
        ]
    };
    let counter = || -> Vec<ConflictUpdate> {
        vec![
            col("views")
                .set_expr(Expr::col("page_views.views").add(Expr::excluded("views")))
                .into(),
            "title".into(),
        ]
    };
    table("page_views")
        .upsert(&t.db, [visit("/", "Home")], ["path"], counter())
        .await
        .unwrap();
    table("page_views")
        .upsert(
            &t.db,
            [visit("/", "Start"), visit("/pricing", "Pricing")],
            ["path"],
            counter(),
        )
        .await
        .unwrap();
    let rows = table("page_views")
        .order_by("path")
        .get(&t.db)
        .await
        .unwrap();
    let pages: Vec<(String, i64, String)> = rows
        .iter()
        .map(|row| {
            (
                row.get("path").unwrap(),
                row.get("views").unwrap(),
                row.get("title").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        pages,
        [
            ("/".to_owned(), 2, "Start".to_owned()),
            ("/pricing".to_owned(), 1, "Pricing".to_owned())
        ]
    );

    // Nothing to update: conflicting rows are skipped.
    let skipped = table("page_views")
        .upsert(
            &t.db,
            [visit("/", "Again")],
            ["path"],
            Vec::<ConflictUpdate>::new(),
        )
        .await
        .unwrap();
    assert_eq!(skipped, 0);
    let error = table("page_views")
        .upsert(&t.db, [visit("/", "Again")], Vec::<&str>::new(), ["title"])
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: an upsert into `page_views` needs the unique columns its rows conflict on"
    );
}

#[tokio::test]
async fn update_or_insert_writes_one_or_the_other() {
    let t = sqlite().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let values = |seen: Arc<Mutex<Vec<bool>>>| {
        move |exists: bool| {
            seen.lock().unwrap().push(exists);
            vec![col("color").set(if exists { "updated" } else { "created" })]
        }
    };
    assert!(
        table("tags")
            .update_or_insert(&t.db, [col("name").set("vip")], values(seen.clone()))
            .await
            .unwrap()
    );
    assert!(
        table("tags")
            .update_or_insert(&t.db, [col("name").set("vip")], values(seen.clone()))
            .await
            .unwrap()
    );
    assert_eq!(*seen.lock().unwrap(), [false, true]);
    assert_eq!(count_rows(&t.db, "tags").await, 1);
    assert_eq!(
        table("tags")
            .value(&t.db, col_as::<String>("color"))
            .await
            .unwrap()
            .as_deref(),
        Some("updated")
    );
}

#[tokio::test]
async fn updates_and_deletes_through_joins_and_limits() {
    let t = sqlite().await;
    let acme = Company::create(
        &t.db,
        Company {
            name: "Acme".into(),
            active: false,
            ..Company::default()
        },
    )
    .await
    .unwrap();
    for (title, company) in [("a", Some(acme.id)), ("b", None), ("c", Some(acme.id))] {
        Deal::create(
            &t.db,
            Deal {
                company_id: company,
                ..Deal::new(title, 10)
            },
        )
        .await
        .unwrap();
    }
    // Through a join: SQLite's `rowid IN (subquery)`.
    let lost = table("deals as d")
        .join("companies", "companies.id", "=", "d.company_id")
        .where_(col("companies.active").eq(false))
        .update(&t.db, [col("stage").set("closed_lost")])
        .await
        .unwrap();
    assert_eq!(lost, 2);
    // `UPDATE … FROM`.
    let renamed = table("deals")
        .join("companies", "companies.id", "=", "deals.company_id")
        .where_(col("companies.name").eq("Acme"))
        .update_from(
            &t.db,
            [col("title").set_expr(Expr::raw(
                "companies.name || ': ' || deals.title",
                Vec::<Value>::new(),
            ))],
        )
        .await
        .unwrap();
    assert_eq!(renamed, 2);
    let titles: Vec<String> = Deal::query()
        .order_by(Deal::TITLE)
        .pluck(&t.db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(titles, ["Acme: a", "Acme: c", "b"]);
    // An ordered, limited delete removes the rows it selects.
    let deleted = table("deals")
        .order_by_desc("title")
        .limit(1)
        .delete(&t.db)
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(count_rows(&t.db, "deals").await, 2);
    assert_eq!(
        Deal::query()
            .with_trashed()
            .where_(Deal::TITLE.eq("b"))
            .count(&t.db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn json_paths_update_in_place() {
    let t = sqlite().await;
    let deal = Deal::create(
        &t.db,
        Deal {
            meta: DealMeta {
                source: "web".into(),
                score: 3,
            },
            ..Deal::new("Renewal", 10)
        },
    )
    .await
    .unwrap();
    Deal::query()
        .where_key(deal.id)
        .update(
            &t.db,
            [
                col("meta->source").set("partner"),
                Deal::META.json("score").set(9),
            ],
        )
        .await
        .unwrap();
    let fresh = Deal::find_or_fail(&t.db, deal.id).await.unwrap();
    assert_eq!(
        fresh.meta,
        DealMeta {
            source: "partner".into(),
            score: 9
        }
    );
    // A path and its column cannot be set in one update.
    let error = Deal::query()
        .update(
            &t.db,
            [
                Deal::META.set(DealMeta::default()),
                col("meta->score").set(1),
            ],
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("sets `meta` twice"), "{error}");
}

#[tokio::test]
async fn truncation_empties_the_table_and_restarts_its_keys() {
    let t = sqlite().await;
    tag(&t.db, "a", "x").await;
    tag(&t.db, "b", "x").await;
    table("tags").truncate(&t.db).await.unwrap();
    assert_eq!(count_rows(&t.db, "tags").await, 0);
    assert_eq!(tag(&t.db, "c", "x").await.id, 1, "the sequence restarted");
    Tag::query().truncate(&t.db).await.unwrap();
    assert_eq!(count_rows(&t.db, "tags").await, 0);
    let error = table("tags").truncate_cascade(&t.db).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "TRUNCATE … CASCADE is not supported on SQLite"
    );
}

#[tokio::test]
async fn models_insert_as_they_are_or_filled_like_save() {
    let t = sqlite().await;
    let inserted = Company::insert(
        &t.db,
        [
            Company {
                name: "Acme".into(),
                ..Company::default()
            },
            Company {
                id: 10,
                name: "Globex".into(),
                ..Company::default()
            },
        ],
    )
    .await
    .unwrap();
    assert_eq!(inserted, 2, "an empty key is left to the database");
    let mut ids: Vec<i64> = Company::query().pluck(&t.db, Company::ID).await.unwrap();
    ids.sort_unstable();
    assert_eq!(ids, [1, 10]);

    // Filled like save: generated keys and timestamps.
    let deals = (0..3).map(|n| Deal::new(&format!("D{n}"), n));
    assert_eq!(Deal::fill_and_insert(&t.db, deals).await.unwrap(), 3);
    let stored = Deal::query().get(&t.db).await.unwrap();
    assert!(stored.iter().all(|deal| !deal.id.is_nil()));
    assert!(stored.iter().all(|deal| deal.created_at.timestamp() > 0));

    let filled = Event::query()
        .fill_for_insert(&t.db, [Event::default()])
        .await
        .unwrap();
    assert!(!filled[0].id.is_nil(), "a ULID was generated");
    assert_eq!(count_rows(&t.db, "events").await, 0, "nothing was written");

    let id = Company::query()
        .fill_and_insert_get_id(
            &t.db,
            Company {
                name: "Initech".into(),
                ..Company::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(id, 11);
    let key = Deal::query()
        .fill_and_insert_get_id(&t.db, Deal::new("Keyed", 1))
        .await
        .unwrap();
    assert_eq!(Deal::find_or_fail(&t.db, key).await.unwrap().title, "Keyed");

    // Manual keys must be set.
    let error = Setting::fill_and_insert(&t.db, [Setting::default()])
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::MissingKey { .. }), "{error}");

    tag(&t.db, "vip", "gold").await;
    let inserted = Tag::insert_or_ignore(
        &t.db,
        [
            Tag {
                name: "vip".into(),
                color: "red".into(),
                ..Tag::default()
            },
            Tag {
                name: "new".into(),
                color: "red".into(),
                ..Tag::default()
            },
        ],
    )
    .await
    .unwrap();
    assert_eq!(inserted, 1);
}

#[tokio::test]
async fn model_upserts_update_chosen_columns_and_the_update_timestamp() {
    let (_app, db, clock) = app(Vec::new()).await;
    Setting::upsert(
        &*db,
        [Setting {
            key: "theme".into(),
            value: "light".into(),
            ..Setting::default()
        }],
        [Setting::KEY],
        [Setting::VALUE],
    )
    .await
    .unwrap();
    clock.advance(std::time::Duration::from_secs(300));
    Setting::upsert(
        &*db,
        [
            Setting {
                key: "theme".into(),
                value: "dark".into(),
                ..Setting::default()
            },
            Setting {
                key: "lang".into(),
                value: "en".into(),
                ..Setting::default()
            },
        ],
        [Setting::KEY],
        [ConflictUpdate::All],
    )
    .await
    .unwrap();
    let theme = Setting::find_or_fail(&*db, "theme").await.unwrap();
    assert_eq!(theme.value, "dark");
    assert_eq!(
        theme.updated_at,
        crate::support::epoch() + chrono::Duration::minutes(5),
        "the update timestamp moved"
    );
    assert_eq!(Setting::query().count(&*db).await.unwrap(), 2);

    // Tags by name: only the color changes, keys stay.
    let first = tag(&db, "vip", "gold").await;
    Tag::upsert(
        &*db,
        [Tag {
            name: "vip".into(),
            color: "platinum".into(),
            ..Tag::default()
        }],
        [Tag::NAME],
        [Tag::COLOR],
    )
    .await
    .unwrap();
    let stored = Tag::find_or_fail(&*db, first.id).await.unwrap();
    assert_eq!(stored.color, "platinum");
}

/// Notes the tags it sees created.
struct Created(Arc<Mutex<Vec<String>>>);

impl ModelHook<Tag> for Created {
    fn after<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        record: &'a mut Record<Tag>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if event == ModelEvent::Created {
                self.0.lock().unwrap().push(record.name.clone());
            }
            Ok(())
        })
    }
}

struct CreatedHook(Arc<Mutex<Vec<String>>>);

impl Module for CreatedHook {
    fn register(&self, m: &mut ModuleBuilder) {
        let seen = self.0.clone();
        m.multi(Factory::<dyn ModelHook<Tag>>::new(move |_| {
            let seen = seen.clone();
            async move { Ok(Arc::new(Created(seen)) as Arc<dyn ModelHook<Tag>>) }
        }));
    }
}

#[tokio::test]
async fn save_or_ignore_inserts_unless_a_unique_index_conflicts() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (_app, db, _clock) = app(vec![Box::new(CreatedHook(seen.clone()))]).await;
    let mut first = Record::new(Tag {
        name: "vip".into(),
        color: "gold".into(),
        ..Tag::default()
    });
    assert!(first.save_or_ignore(&*db).await.unwrap());
    assert!(first.exists());
    assert_eq!(first.id, 1, "the key came back");

    let mut second = Record::new(Tag {
        name: "vip".into(),
        color: "red".into(),
        ..Tag::default()
    });
    assert!(!second.save_or_ignore_on(&*db, [Tag::NAME]).await.unwrap());
    assert!(!second.exists(), "a skipped record stays new");
    assert_eq!(*seen.lock().unwrap(), ["vip"], "created ran once");

    let error = first.save_or_ignore(&*db).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: save_or_ignore inserts new records; this Tag is stored already (use save)"
    );
}

#[tokio::test]
async fn model_update_from_sets_the_update_timestamp() {
    let t = sqlite().await;
    let acme = Company::create(
        &t.db,
        Company {
            name: "Acme".into(),
            country: Some("US".into()),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    Deal::create(
        &t.db,
        Deal {
            company_id: Some(acme.id),
            stage: Stage::Qualified,
            ..Deal::new("a", 1)
        },
    )
    .await
    .unwrap();
    let changed = Deal::query()
        .join("companies", Company::ID, "=", Deal::COMPANY_ID)
        .where_(Company::COUNTRY.eq("US"))
        .update_from(&t.db, [Deal::STAGE.set(Stage::ClosedWon)])
        .await
        .unwrap();
    assert_eq!(changed, 1);
    let stored = Deal::query().first(&t.db).await.unwrap().unwrap();
    assert_eq!(stored.stage, Stage::ClosedWon);
    assert!(stored.updated_at >= stored.created_at);
    // Raw statements see the same rows.
    let won: Option<i64> = navree_orm::scalar(
        &t.db,
        raw("SELECT COUNT(*) FROM deals WHERE stage = ?", ["closed_won"]),
    )
    .await
    .unwrap();
    assert_eq!(won, Some(1));
}

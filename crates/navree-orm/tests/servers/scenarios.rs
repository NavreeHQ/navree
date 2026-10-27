//! What runs on every real server.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use navree_db::{ChangeOp, Db};
use navree_orm::ir::FullText;
use navree_orm::{CursorRequest, Decimal, Expr, Model, Record, Value, col, col_as, table};
use uuid::Uuid;

use crate::models::{Company, Deal, DealMeta, Event, Note, Priority, Stage, Tag};
use crate::support::Server;

fn at(day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, day, hour, 30, 0).unwrap()
}

/// Two companies and four deals.
async fn seed(db: &Db) -> (Record<Company>, Record<Company>) {
    let acme = Company::create(
        db,
        Company {
            name: "Acme".into(),
            country: Some("US".into()),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    let globex = Company::create(
        db,
        Company {
            name: "Globex".into(),
            country: Some("DE".into()),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    for (title, company, cents, stage, tags, closed) in [
        (
            "Renewal for Acme",
            Some(acme.id),
            5_000,
            Stage::Lead,
            vec!["vip"],
            None,
        ),
        (
            "Pricing upgrade",
            Some(acme.id),
            25_000,
            Stage::Qualified,
            vec!["q1", "vip"],
            None,
        ),
        (
            "Platform deal",
            Some(globex.id),
            90_000,
            Stage::ClosedWon,
            vec![],
            Some(at(20, 14)),
        ),
        (
            "Small change",
            None,
            50,
            Stage::ClosedLost,
            vec!["q1"],
            Some(at(21, 9)),
        ),
    ] {
        Deal::create(
            db,
            Deal {
                company_id: company,
                stage,
                tags: tags.into_iter().map(Into::into).collect(),
                closed_at: closed,
                meta: DealMeta {
                    source: if cents > 20_000 {
                        "ads".into()
                    } else {
                        "web".into()
                    },
                    score: 7,
                },
                ..Deal::new(title, cents)
            },
        )
        .await
        .unwrap();
    }
    (acme, globex)
}

fn titles(deals: &navree_orm::Collection<Deal>) -> Vec<String> {
    let mut titles = deals.map(|deal| deal.title.clone());
    titles.sort();
    titles
}

/// Keys, types, casts, timestamps and dirty updates round-trip exactly.
pub async fn records_round_trip(db: Db) {
    let acme = Company::create(
        &db,
        Company {
            name: "Acme".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    let globex = Company::create(
        &db,
        Company {
            name: "Globex".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert!(acme.id > 0);
    assert_eq!(globex.id, acme.id + 1, "the database assigned the keys");

    let deal = Deal::create(
        &db,
        Deal {
            company_id: Some(acme.id),
            stage: Stage::ClosedWon,
            priority: Priority::High,
            meta: DealMeta {
                source: "partner".into(),
                score: 9,
            },
            tags: vec!["a".into(), "b".into()],
            probability: 0.75,
            value: Decimal::new(1_234_567, 2),
            closes_on: NaiveDate::from_ymd_opt(2026, 12, 31),
            closed_at: Some(
                Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 15).unwrap()
                    + Duration::microseconds(123_456),
            ),
            ..Deal::new("Exact", 1_000_000_000_000)
        },
    )
    .await
    .unwrap();
    let stored = Deal::find_or_fail(&db, deal.id).await.unwrap();
    assert_eq!(*stored, *deal, "every column reads back as written");
    assert_eq!(stored.value.to_string(), "12345.67");

    let mut stored = stored;
    stored.stage = Stage::Lead;
    stored.save(&db).await.unwrap();
    assert_eq!(
        stored
            .get_changes()
            .iter()
            .map(|(c, _)| *c)
            .collect::<Vec<_>>(),
        ["stage", "updated_at"]
    );
    assert_eq!(
        Deal::find_or_fail(&db, deal.id).await.unwrap().stage,
        Stage::Lead
    );

    let mut acme = acme;
    acme.increment(&db, Company::EMPLOYEES, 5).await.unwrap();
    assert_eq!(
        Company::find_or_fail(&db, acme.id).await.unwrap().employees,
        5
    );

    let event = Event::create(
        &db,
        Event {
            name: "signup".into(),
            occurred_at: at(1, 1),
            payload: Some(serde_json::json!({"plan": "pro"})),
            ..Event::default()
        },
    )
    .await
    .unwrap();
    let found = Event::find_or_fail(&db, event.id).await.unwrap();
    assert_eq!(*found, *event);

    let mut gone = Deal::find_or_fail(&db, deal.id).await.unwrap();
    assert!(gone.delete(&db).await.unwrap());
    assert!(gone.trashed());
    assert!(Deal::find(&db, deal.id).await.unwrap().is_none());
    assert!(
        Deal::query()
            .with_trashed()
            .find(&db, deal.id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(Company::destroy(&db, [globex.id]).await.unwrap(), 1);
}

/// The where family, selects, joins, groups, aggregates and plucks.
pub async fn queries(db: Db, server: Server) {
    let (acme, _globex) = seed(&db).await;
    let deals = |q: navree_orm::Builder<Deal>| {
        let db = db.clone();
        async move { titles(&q.get(&db).await.unwrap()) }
    };

    assert_eq!(
        deals(Deal::query().where_(Deal::AMOUNT_CENTS.gte(25_000))).await,
        ["Platform deal", "Pricing upgrade"]
    );
    assert_eq!(
        deals(Deal::query().where_in(Deal::STAGE, [Stage::Lead, Stage::ClosedLost])).await,
        ["Renewal for Acme", "Small change"]
    );
    assert_eq!(
        deals(Deal::query().where_null(Deal::COMPANY_ID)).await,
        ["Small change"]
    );
    assert_eq!(
        deals(Deal::query().where_between(Deal::AMOUNT_CENTS, 1_000, 30_000)).await,
        ["Pricing upgrade", "Renewal for Acme"]
    );
    assert_eq!(
        deals(Deal::query().where_like(Deal::TITLE, "%ACME%")).await,
        ["Renewal for Acme"]
    );
    assert!(
        deals(Deal::query().where_like_case_sensitive(Deal::TITLE, "%ACME%"))
            .await
            .is_empty()
    );
    assert_eq!(
        deals(Deal::query().where_json_contains(Deal::TAGS, "vip")).await,
        ["Pricing upgrade", "Renewal for Acme"]
    );
    assert_eq!(
        deals(Deal::query().where_json_length(Deal::TAGS, ">", 1)).await,
        ["Pricing upgrade"]
    );
    assert_eq!(
        deals(Deal::query().where_(col("meta->source").eq("ads"))).await,
        ["Platform deal", "Pricing upgrade"]
    );
    assert_eq!(
        deals(
            Deal::query()
                .where_(Deal::META.json("score").eq(7))
                .where_(Deal::AMOUNT_CENTS.lt(100))
        )
        .await,
        ["Small change"]
    );
    assert_eq!(
        deals(Deal::query().where_date(
            Deal::CLOSED_AT,
            "=",
            NaiveDate::from_ymd_opt(2026, 3, 20).unwrap()
        ))
        .await,
        ["Platform deal"]
    );
    assert_eq!(
        deals(Deal::query().where_day(Deal::CLOSED_AT, "=", 21)).await,
        ["Small change"]
    );
    assert_eq!(
        deals(
            Deal::query()
                .where_month(Deal::CLOSED_AT, "=", 3)
                .where_year(Deal::CLOSED_AT, "=", 2026)
        )
        .await
        .len(),
        2
    );
    assert_eq!(
        deals(Deal::query().where_time(
            Deal::CLOSED_AT,
            "<",
            NaiveTime::from_hms_opt(12, 0, 0).unwrap()
        ))
        .await,
        ["Small change"]
    );
    assert_eq!(
        deals(
            Deal::query()
                .where_(Deal::PRIORITY.eq(Priority::Low))
                .where_(Deal::STAGE.eq(Stage::Lead))
        )
        .await,
        ["Renewal for Acme"]
    );
    assert_eq!(
        deals(Deal::query().where_raw("amount_cents > ?", [80_000_i64])).await,
        ["Platform deal"]
    );
    assert_eq!(
        deals(
            Deal::query().where_exists(
                Company::query()
                    .where_column(Company::ID, "=", Deal::COMPANY_ID)
                    .where_(Company::COUNTRY.eq("DE"))
            )
        )
        .await,
        ["Platform deal"]
    );
    assert_eq!(
        deals(
            Deal::query().where_in_sub(
                Deal::COMPANY_ID,
                Company::query()
                    .select([Company::ID])
                    .where_(Company::NAME.eq("Acme"))
            )
        )
        .await,
        ["Pricing upgrade", "Renewal for Acme"]
    );
    let joined = Deal::query()
        .join("companies", Deal::COMPANY_ID, "=", Company::ID)
        .where_(Company::COUNTRY.eq("US"))
        .get(&db)
        .await
        .unwrap();
    assert_eq!(titles(&joined), ["Pricing upgrade", "Renewal for Acme"]);

    assert_eq!(Deal::query().count(&db).await.unwrap(), 4);
    assert_eq!(
        Deal::query()
            .sum::<i64>(&db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        120_050
    );
    assert_eq!(
        Deal::query()
            .avg::<Decimal>(&db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        Some(Decimal::new(300_125, 1))
    );
    assert_eq!(
        Deal::query()
            .max::<i64>(&db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        Some(90_000)
    );
    assert_eq!(
        Deal::query()
            .group_by([Deal::COMPANY_ID])
            .count(&db)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        Deal::query()
            .count_distinct(&db, Deal::COMPANY_ID)
            .await
            .unwrap(),
        2
    );
    let won = Deal::query()
        .where_(Deal::STAGE.eq(Stage::ClosedWon))
        .value(&db, Deal::STAGE)
        .await
        .unwrap();
    assert_eq!(won, Some(Stage::ClosedWon));
    let by_title = Deal::query()
        .order_by(Deal::AMOUNT_CENTS)
        .pluck_keyed(&db, Deal::AMOUNT_CENTS, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(by_title["Small change"], 50);
    let summary = table("deals")
        .select(["stage"])
        .select_expr(Expr::count_all(), "deals")
        .select_expr(Expr::sum(col("amount_cents")), "total")
        .group_by(["stage"])
        .having(Expr::count_all().gte(1))
        .order_by_desc("total")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(summary.len(), 4);
    assert_eq!(summary[0].get::<String>("stage").unwrap(), "closed_won");
    assert_eq!(summary[0].get::<i64>("total").unwrap(), 90_000);
    let extras = Deal::query()
        .select_raw("amount_cents * ? AS doubled", [2_i64])
        .where_(Deal::STAGE.eq(Stage::ClosedLost))
        .first(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(extras.extra::<i64>("doubled").unwrap(), Some(100));

    let names: Vec<String> = table("companies")
        .select(["name"])
        .union(
            table("deals")
                .select(["title"])
                .where_(col("amount_cents").lt(100)),
        )
        .pluck(&db, col_as::<String>("name"))
        .await
        .unwrap();
    assert_eq!(names.len(), 3);

    let locked = Deal::query()
        .where_(Deal::COMPANY_ID.eq(acme.id))
        .lock_for_update()
        .get(&db.uow(None, None).await)
        .await
        .unwrap();
    assert_eq!(locked.len(), 2);

    let full_text = Deal::query()
        .where_full_text([Deal::TITLE], "pricing")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(titles(&full_text), ["Pricing upgrade"]);
    // Search-engine syntax (`websearch_to_tsquery`, InnoDB's boolean mode):
    // `platform` wanted, `pricing` excluded.
    let websearch = Deal::query()
        .where_full_text_with([Deal::TITLE], "platform -pricing", FullText::websearch())
        .get(&db)
        .await
        .unwrap();
    assert_eq!(titles(&websearch), ["Platform deal"], "on {server:?}");
}

/// Mass writes and the find-or-create family.
pub async fn writes(db: Db) {
    seed(&db).await;
    let updated = Deal::query()
        .where_(Deal::STAGE.eq(Stage::Lead))
        .update(&db, [Deal::STAGE.set(Stage::Qualified)])
        .await
        .unwrap();
    assert_eq!(updated, 1);
    // A limited, ordered update runs through a key subquery.
    let bumped = Deal::query()
        .order_by_desc(Deal::AMOUNT_CENTS)
        .limit(2)
        .increment(&db, Deal::AMOUNT_CENTS, 1)
        .await
        .unwrap();
    assert_eq!(bumped, 2);
    assert_eq!(
        Deal::query()
            .max::<i64>(&db, Deal::AMOUNT_CENTS)
            .await
            .unwrap(),
        Some(90_001)
    );
    let joined_delete = Deal::query()
        .join("companies", Deal::COMPANY_ID, "=", Company::ID)
        .where_(Company::COUNTRY.eq("DE"))
        .delete(&db)
        .await
        .unwrap();
    assert_eq!(joined_delete, 1, "soft-deleted through the join");
    assert_eq!(Deal::query().count(&db).await.unwrap(), 3);

    let tag = Tag::first_or_create(&db, [Tag::NAME.set("vip")], |t| t.color = "gold".into())
        .await
        .unwrap();
    assert!(tag.was_recently_created());
    // Inside a transaction the losing insert runs in a savepoint: the
    // transaction (and PostgreSQL's in particular) goes on.
    let uow = db.uow(None, None).await;
    let same = Tag::create_or_first(&uow, [Tag::NAME.set("vip")], |t| t.color = "red".into())
        .await
        .unwrap();
    assert_eq!(same.id, tag.id);
    let other = Tag::update_or_create(&uow, [Tag::NAME.set("new")], |t| t.color = "green".into())
        .await
        .unwrap();
    assert!(other.was_recently_created());
    uow.commit().await.unwrap();
    assert_eq!(Tag::query().count(&db).await.unwrap(), 2);
    let renamed = Tag::update_or_create(&db, [Tag::NAME.set("vip")], |t| t.color = "blue".into())
        .await
        .unwrap();
    assert_eq!(renamed.color, "blue");
    assert_eq!(
        table("tags")
            .where_(col("name").eq("vip"))
            .value(&db, col_as::<String>("color"))
            .await
            .unwrap()
            .as_deref(),
        Some("blue")
    );
}

/// Pages and cursors.
pub async fn pagination(db: Db) {
    for n in 1..=7 {
        Company::create(
            &db,
            Company {
                name: format!("C{n:02}"),
                employees: n % 3,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    let page = Company::query()
        .order_by(Company::NAME)
        .paginate(&db, (2, 3))
        .await
        .unwrap();
    assert_eq!(page.meta.total, 7);
    assert_eq!(
        page.data
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["C04", "C05", "C06"]
    );
    let simple = Company::query()
        .order_by(Company::NAME)
        .simple_paginate(&db, (3, 3))
        .await
        .unwrap();
    assert!(!simple.meta.has_more);

    let query = || Company::query().order_by_desc(Company::EMPLOYEES);
    let first = query()
        .cursor_paginate(&db, CursorRequest::first().limit(3))
        .await
        .unwrap();
    let second = query()
        .cursor_paginate(
            &db,
            CursorRequest::after(first.next_cursor.clone().unwrap()).limit(3),
        )
        .await
        .unwrap();
    let back = query()
        .cursor_paginate(
            &db,
            CursorRequest::after(second.prev_cursor.clone().unwrap()).limit(3),
        )
        .await
        .unwrap();
    let names = |page: &navree_orm::CursorPage<Record<Company>>| {
        page.data.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(names(&first), ["C02", "C05", "C01"]);
    assert_eq!(names(&second), ["C04", "C07", "C03"]);
    assert_eq!(names(&back), names(&first));
}

/// The unit of work: change events, rollback, joining, settings.
pub async fn unit_of_work(db: Db, server: Server) {
    let actor = Uuid::now_v7();
    let uow = db.uow(Some(actor), Some("req-7".into())).await;
    let mut acme = Company::create(
        &uow,
        Company {
            name: "Acme".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    acme.name = "Acme Corp".into();
    acme.save(&uow).await.unwrap();
    let note = Note::create(
        &uow,
        Note {
            body: "called them".into(),
            ..Note::default()
        },
    )
    .await
    .unwrap();
    let ops: Vec<ChangeOp> = uow.changes().iter().map(|change| change.op).collect();
    assert_eq!(
        ops,
        [ChangeOp::Created, ChangeOp::Updated, ChangeOp::Created]
    );
    let fresh = note.fresh(&uow).await.unwrap().unwrap();
    if server == Server::Postgres {
        assert_eq!(
            fresh.written_by,
            actor.to_string(),
            "the statement ran in the unit of work's transaction"
        );
    }
    uow.commit().await.unwrap();

    let rolled = db.uow(None, None).await;
    Company::create(
        &rolled,
        Company {
            name: "Ghost".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(Company::query().count(&rolled).await.unwrap(), 2);
    rolled.rollback().await.unwrap();
    assert_eq!(Company::query().count(&db).await.unwrap(), 1);

    let scoped = db.uow(None, None).await;
    let pool = db.clone();
    scoped
        .scope(async {
            Company::create(
                &pool,
                Company {
                    name: "Joined".into(),
                    ..Company::default()
                },
            )
            .await
            .unwrap();
        })
        .await;
    assert_eq!(scoped.changes().len(), 1);
    scoped.rollback().await.unwrap();
    assert_eq!(Company::query().count(&db).await.unwrap(), 1);
}

/// JSON overlaps, keys and containment, regular expressions, null-safe
/// inequality, full text in raw syntax, hand-written operators and (with
/// pgvector installed) vector distances.
pub async fn json_patterns_and_vectors(db: Db, server: Server) {
    use navree_orm::{Filter, OrmError};

    let (acme, _globex) = seed(&db).await;
    let deals = |q: navree_orm::Builder<Deal>| {
        let db = db.clone();
        async move { titles(&q.get(&db).await.unwrap()) }
    };
    assert_eq!(
        deals(Deal::query().where_json_overlaps(Deal::TAGS, ["q1", "nope"])).await,
        ["Pricing upgrade", "Small change"]
    );
    assert_eq!(
        deals(Deal::query().where_(Deal::TAGS.contained_by(["vip"]))).await,
        ["Platform deal", "Renewal for Acme"],
        "an empty array fits in anything"
    );
    assert_eq!(
        deals(Deal::query().where_json_contains_key("tags[1]")).await,
        ["Pricing upgrade"]
    );
    assert_eq!(
        deals(Deal::query().where_json_contains_key("tags[-1]")).await,
        ["Pricing upgrade", "Renewal for Acme", "Small change"]
    );
    assert_eq!(deals(Deal::query().where_(Deal::META.has_key("source"))).await.len(), 4);
    assert!(deals(Deal::query().where_json_contains_key("meta->owner")).await.is_empty());
    assert_eq!(
        deals(Deal::query().where_(Deal::TITLE.regex("^P[a-z]+ (deal|upgrade)$"))).await,
        ["Platform deal", "Pricing upgrade"]
    );
    assert!(
        deals(Deal::query().where_(Deal::TITLE.regex("^pricing"))).await.is_empty(),
        "case matters"
    );
    assert_eq!(
        deals(Deal::query().where_(Deal::TITLE.regex_case_insensitive("^pricing"))).await,
        ["Pricing upgrade"]
    );
    assert_eq!(
        deals(Deal::query().where_(Deal::COMPANY_ID.is_distinct_from(acme.id))).await,
        ["Platform deal", "Small change"],
        "a NULL company id is distinct from Acme's"
    );
    let raw_text = match server {
        Server::Postgres => "pricing & upgrade",
        Server::MySql => "+pricing +upgrade",
    };
    assert_eq!(
        deals(Deal::query().where_full_text_with([Deal::TITLE], raw_text, FullText::raw())).await,
        ["Pricing upgrade"]
    );
    if server == Server::Postgres {
        assert_eq!(
            deals(Deal::query().where_(Filter::op_raw(
                Deal::TAGS,
                "?",
                Expr::val("q1")
            )))
            .await,
            ["Pricing upgrade", "Small change"]
        );
        let stripped: Vec<Value> = Deal::query()
            .where_(Deal::STAGE.eq(Stage::Lead))
            .pluck(&db, Expr::col("meta").binary("-", Expr::val("score")))
            .await
            .unwrap();
        assert_eq!(navree_orm::to_json(&stripped[0]), serde_json::json!({"source": "web"}));

        let has_vector: Option<i64> = navree_orm::scalar(
            &db,
            navree_orm::raw(
                "SELECT COUNT(*) FROM pg_available_extensions WHERE name = 'vector'",
                [] as [i64; 0],
            ),
        )
        .await
        .unwrap();
        if has_vector == Some(1) {
            navree_orm::unprepared(
                &db,
                "CREATE EXTENSION IF NOT EXISTS vector; CREATE TABLE documents (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL, embedding vector(3) NOT NULL)",
            )
            .await
            .unwrap();
            for (title, embedding) in [("north", "[0,1,0]"), ("north-east", "[0.7,0.7,0]"), ("south", "[0,-1,0]")] {
                navree_orm::statement(
                    &db,
                    navree_orm::raw(
                        "INSERT INTO documents (title, embedding) VALUES (?, CAST(? AS vector))",
                        [title, embedding],
                    ),
                )
                .await
                .unwrap();
            }
            let near: Vec<String> = table("documents")
                .where_vector_similar_to("embedding", &[0.0, 1.0, 0.0], 0.5)
                .pluck(&db, col_as::<String>("title"))
                .await
                .unwrap();
            assert_eq!(near, ["north", "north-east"], "most similar first");
        } else {
            eprintln!("pgvector is not installed: vector distances were not run");
        }
    } else {
        let error = Deal::query()
            .where_(Filter::op_raw(Deal::TAGS, "?", Expr::val("q1")))
            .get(&db)
            .await
            .unwrap_err();
        assert!(matches!(error, OrmError::InvalidQuery(_)), "{error}");
    }
}

/// Compound queries: lateral joins, limits per group, unions with orders
/// and limits, derived tables and (PostgreSQL) a table function.
pub async fn compound(db: Db, server: Server) {
    let (acme, globex) = seed(&db).await;
    // Each company with its biggest deal, through a lateral join.
    let biggest = Deal::query()
        .select([Deal::TITLE.expr()])
        .where_column(Deal::COMPANY_ID, "=", Company::ID)
        .order_by_desc(Deal::AMOUNT_CENTS)
        .limit(1);
    let rows = Company::query()
        .join_lateral(biggest, "biggest")
        .add_select(["biggest.title"])
        .order_by(Company::ID)
        .get(&db)
        .await
        .unwrap();
    let pairs: Vec<(i64, String)> = rows
        .iter()
        .map(|c| (c.id, c.extra::<String>("title").unwrap().unwrap()))
        .collect();
    assert_eq!(
        pairs,
        [
            (acme.id, "Pricing upgrade".to_owned()),
            (globex.id, "Platform deal".to_owned())
        ]
    );
    // The smallest deal of each company.
    let mut smallest: Vec<String> = Deal::query()
        .where_not_null(Deal::COMPANY_ID)
        .order_by(Deal::AMOUNT_CENTS)
        .group_limit(1, Deal::COMPANY_ID)
        .pluck(&db, Deal::TITLE)
        .await
        .unwrap();
    smallest.sort();
    assert_eq!(smallest, ["Platform deal", "Renewal for Acme"]);
    // A union sorted and cut as a whole, its second part with its own limit.
    let titles: Vec<String> = Deal::query()
        .where_(Deal::STAGE.eq(Stage::Lead))
        .union(
            Deal::query()
                .where_(Deal::AMOUNT_CENTS.gte(25_000))
                .order_by_desc(Deal::AMOUNT_CENTS)
                .limit(1),
        )
        .order_by(Deal::TITLE)
        .pluck(&db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(titles, ["Platform deal", "Renewal for Acme"]);
    let from_union: Vec<String> = Deal::query()
        .from_sub(
            Deal::query()
                .where_(Deal::STAGE.eq(Stage::Lead))
                .union_all(Deal::query().where_(Deal::STAGE.eq(Stage::ClosedLost))),
        )
        .order_by_desc(Deal::TITLE)
        .pluck(&db, Deal::TITLE)
        .await
        .unwrap();
    assert_eq!(from_union, ["Small change", "Renewal for Acme"]);
    if server == Server::Postgres {
        let days: Vec<i64> = table("ignored")
            .from_raw("generate_series(?::int, ?::int) AS day", [1_i64, 3])
            .pluck(&db, col_as::<i64>("day"))
            .await
            .unwrap();
        assert_eq!(days, [1, 2, 3]);
    }
}

/// Row locks between two units of work: `SKIP LOCKED` leaves the held row
/// out, `NOWAIT` fails at once, shared locks wait for none of the free
/// rows, and the holder's change commits.
pub async fn locks(db: Db) {
    use navree_orm::{LockMode, OrmError};

    let mut ids = Vec::new();
    for name in ["A", "B", "C"] {
        let company = Company::create(
            &db,
            Company {
                name: name.into(),
                ..Company::default()
            },
        )
        .await
        .unwrap();
        ids.push(company.id);
    }
    let holder = db.uow(None, None).await;
    assert!(holder.lock_for_update());
    let mut held = Company::find_or_fail(&holder, ids[0]).await.unwrap();
    held.refresh_for_update(&holder).await.unwrap();

    let other = db.uow(None, None).await;
    let free: Vec<i64> = Company::query()
        .order_by(Company::ID)
        .lock_for_update()
        .skip_locked()
        .pluck(&other, Company::ID)
        .await
        .unwrap();
    assert_eq!(free, ids[1..]);
    let busy = Company::query()
        .where_key(ids[0])
        .lock(LockMode::NoKeyUpdate)
        .no_wait()
        .get(&other)
        .await
        .unwrap_err();
    assert!(matches!(busy, OrmError::Db(_)), "{busy}");
    other.rollback().await.unwrap();

    let reader = db.uow(None, None).await;
    let shared: Vec<i64> = Company::query()
        .order_by(Company::ID)
        .shared_lock()
        .skip_locked()
        .pluck(&reader, Company::ID)
        .await
        .unwrap();
    assert_eq!(shared, ids[1..], "the row held for update is skipped");
    reader.rollback().await.unwrap();

    held.name = "Held".into();
    held.save(&holder).await.unwrap();
    holder.commit().await.unwrap();
    assert_eq!(
        Company::find_or_fail(&db, ids[0]).await.unwrap().name,
        "Held"
    );
}

/// Bulk writes: inserts split under the bind limit, insert-or-ignore,
/// upserts, keys read back, JSON path updates, `UPDATE … FROM`, joined
/// table writes, truncation and `save_or_ignore`.
pub async fn bulk_writes(db: Db, server: Server) {
    use navree_orm::{ConflictUpdate, OrmError};

    // 22,000 rows of 3 values pass the 65,535-value bind limit: two
    // statements, one transaction.
    let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let rows: Vec<Vec<(&str, Value)>> = (0..22_000)
        .map(|i| {
            vec![
                ("taken_at", Value::from(base + Duration::seconds(i))),
                ("sensor", Value::from("a")),
                ("celsius", Value::from(20.0)),
            ]
        })
        .collect();
    assert_eq!(table("readings").insert(&db, rows).await.unwrap(), 22_000);
    assert_eq!(table("readings").count(&db).await.unwrap(), 22_000);

    // Keys read back: RETURNING on PostgreSQL, LAST_INSERT_ID() on MySQL.
    let first: i64 = table("tags")
        .insert_get_id(&db, [("name", "first"), ("color", "red")])
        .await
        .unwrap();
    let second = Tag::query()
        .fill_and_insert_get_id(
            &db,
            Tag {
                name: "second".into(),
                color: "blue".into(),
                ..Tag::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(second, first + 1);

    let inserted = table("tags")
        .insert_or_ignore(
            &db,
            [
                [("name", "first"), ("color", "green")],
                [("name", "third"), ("color", "green")],
            ],
        )
        .await
        .unwrap();
    assert_eq!(inserted, 1);
    let returned = table("tags")
        .insert_or_ignore_returning(
            &db,
            [[("name", "third"), ("color", "x")], [("name", "fourth"), ("color", "x")]],
            ["name"],
            ["name"],
        )
        .await;
    match server {
        Server::Postgres => {
            let rows = returned.unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get::<String>("name").unwrap(), "fourth");
        }
        Server::MySql => assert!(matches!(returned, Err(OrmError::Unsupported { .. }))),
    }

    // Upserts: a counter adds, a model updates every other column.
    let ddl = match server {
        Server::Postgres => {
            "CREATE TABLE page_views (path TEXT PRIMARY KEY, views BIGINT NOT NULL, title TEXT NOT NULL)"
        }
        Server::MySql => {
            "CREATE TABLE page_views (path VARCHAR(191) PRIMARY KEY, views BIGINT NOT NULL, title VARCHAR(255) NOT NULL)"
        }
    };
    navree_orm::unprepared(&db, ddl).await.unwrap();
    for title in ["Home", "Start"] {
        table("page_views")
            .upsert(
                &db,
                [[
                    ("path", Value::from("/")),
                    ("views", Value::from(1_i64)),
                    ("title", Value::from(title)),
                ]],
                ["path"],
                [
                    ConflictUpdate::from(col("views").set_expr(
                        Expr::col("page_views.views").add(Expr::excluded("views")),
                    )),
                    "title".into(),
                ],
            )
            .await
            .unwrap();
    }
    let page = table("page_views").first(&db).await.unwrap().unwrap();
    assert_eq!(page.get::<i64>("views").unwrap(), 2);
    assert_eq!(page.get::<String>("title").unwrap(), "Start");
    Tag::upsert(
        &db,
        [Tag {
            name: "first".into(),
            color: "gold".into(),
            ..Tag::default()
        }],
        [Tag::NAME],
        [ConflictUpdate::All],
    )
    .await
    .unwrap();
    let gold = Tag::query()
        .where_(Tag::NAME.eq("first"))
        .first(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((gold.id, gold.color.as_str()), (first, "gold"));

    // save_or_ignore: one insert, then a skipped duplicate.
    let mut duplicate = Record::new(Tag {
        name: "first".into(),
        color: "pink".into(),
        ..Tag::default()
    });
    assert!(!duplicate.save_or_ignore(&db).await.unwrap());
    let mut fresh = Record::new(Tag {
        name: "fifth".into(),
        color: "pink".into(),
        ..Tag::default()
    });
    assert!(fresh.save_or_ignore(&db).await.unwrap());
    assert!(fresh.id > 0);

    // JSON paths updated in place.
    let (acme, _globex) = seed(&db).await;
    let deal = Deal::query()
        .where_(Deal::COMPANY_ID.eq(acme.id))
        .where_(Deal::STAGE.eq(Stage::Lead))
        .first(&db)
        .await
        .unwrap()
        .unwrap();
    Deal::query()
        .where_key(deal.id)
        .update(
            &db,
            [col("meta->source").set("partner"), Deal::META.json("score").set(9)],
        )
        .await
        .unwrap();
    let meta = Deal::find_or_fail(&db, deal.id).await.unwrap().meta.clone();
    assert_eq!(
        meta,
        DealMeta {
            source: "partner".into(),
            score: 9
        }
    );

    // UPDATE … FROM (PostgreSQL), UPDATE … JOIN (MySQL).
    let renamed = table("deals")
        .join("companies", "companies.id", "=", "deals.company_id")
        .where_(col("companies.country").eq("DE"))
        .update_from(&db, [col("stage").set("closed_lost")])
        .await
        .unwrap();
    assert_eq!(renamed, 1);
    // A joined table write: by ctid on PostgreSQL; MySQL has no row id.
    let joined = table("deals")
        .join("companies", "companies.id", "=", "deals.company_id")
        .where_(col("companies.name").eq("Acme"))
        .update(&db, [col("probability").set(0.5)])
        .await;
    match server {
        Server::Postgres => assert_eq!(joined.unwrap(), 2),
        Server::MySql => assert!(matches!(joined, Err(OrmError::Unsupported { .. }))),
    }

    // Truncation restarts the identity.
    table("page_views").truncate(&db).await.unwrap();
    assert_eq!(table("page_views").count(&db).await.unwrap(), 0);
    table("notes").insert(&db, [[("body", "a")]]).await.unwrap();
    table("notes").truncate(&db).await.unwrap();
    let id: i64 = table("notes")
        .insert_get_id(&db, [("body", "b")])
        .await
        .unwrap();
    assert_eq!(id, 1, "the identity restarted");
}

/// Chunks, lazy streams and cursors, on the pool and inside a unit of
/// work, and raw cursors.
pub async fn streams(db: Db) {
    use std::ops::ControlFlow;

    use futures::{StreamExt, TryStreamExt};

    for n in 1..=7 {
        Company::create(
            &db,
            Company {
                name: format!("C{n}"),
                employees: n,
                active: true,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    // Key pages stay correct while the callback moves rows out of the set.
    let pool = db.clone();
    let mut pages = Vec::new();
    Company::query()
        .where_(Company::ACTIVE.eq(true))
        .chunk_by_id(&db, 3, |companies| {
            pages.push(companies.map(|c| c.name.clone()));
            let pool = pool.clone();
            async move {
                Company::query()
                    .where_key_in(companies.model_keys())
                    .update(&pool, [Company::ACTIVE.set(false)])
                    .await?;
                Ok(ControlFlow::Continue(()))
            }
        })
        .await
        .unwrap();
    assert_eq!(
        pages,
        [vec!["C1", "C2", "C3"], vec!["C4", "C5", "C6"], vec!["C7"]]
    );
    let down: Vec<i32> = Company::query()
        .lazy_by_id_desc(&db, 2)
        .map_ok(|c| c.employees)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(down, [7, 6, 5, 4, 3, 2, 1]);
    let evens: Vec<String> = Company::query()
        .where_raw("employees % 2 = ?", [0])
        .order_by(Company::EMPLOYEES)
        .cursor(&db)
        .map_ok(|c| c.name.clone())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(evens, ["C2", "C4", "C6"]);

    // In a unit of work the cursor reads the uncommitted rows and holds the
    // transaction's connection: statements through it are refused until
    // the cursor is dropped.
    let uow = db.uow(None, None).await;
    Company::create(
        &uow,
        Company {
            name: "C8".into(),
            employees: 8,
            ..Company::default()
        },
    )
    .await
    .unwrap();
    {
        let mut cursor = Company::query().order_by(Company::EMPLOYEES).cursor(&uow);
        assert_eq!(cursor.try_next().await.unwrap().unwrap().name, "C1");
        let busy = Company::query().count(&uow).await.unwrap_err();
        assert!(busy.to_string().contains("busy streaming a cursor"), "{busy}");
        let rest: Vec<String> = cursor
            .map_ok(|c| c.name.clone())
            .try_collect()
            .await
            .unwrap();
        assert_eq!(rest.len(), 7);
        assert_eq!(rest.last().map(String::as_str), Some("C8"));
    }
    assert_eq!(Company::query().count(&uow).await.unwrap(), 8);
    uow.rollback().await.unwrap();

    let raw_rows: Vec<i64> = navree_orm::cursor(
        &db,
        navree_orm::raw(
            "SELECT employees FROM companies WHERE employees > ? ORDER BY employees",
            [4_i64],
        ),
    )
    .map(|row| row.unwrap().get::<i64>("employees").unwrap())
    .collect()
    .await;
    assert_eq!(raw_rows, [5, 6, 7]);
}

/// Time buckets: TimescaleDB's on PostgreSQL, the emulation on MySQL give
/// the same buckets.
pub async fn time_buckets(db: Db, server: Server) {
    let base = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
    for (minutes, celsius) in [
        (0, 10.0),
        (10, 12.0),
        (20, 11.0),
        (70, 15.0),
        (75, 13.0),
        (130, 9.0),
    ] {
        table("readings")
            .insert(
                &db,
                [vec![
                    ("taken_at", Value::from(base + Duration::minutes(minutes))),
                    ("sensor", Value::from("a")),
                    ("celsius", Value::from(celsius)),
                ]],
            )
            .await
            .unwrap();
    }
    let rows = table("readings")
        .select_time_bucket("1 hour", "taken_at", "bucket")
        .select_expr(Expr::count_all(), "readings")
        .select_expr(Expr::max(col("celsius")), "peak")
        .group_by(["bucket"])
        .order_by("bucket")
        .get(&db)
        .await
        .unwrap();
    let buckets: Vec<(DateTime<Utc>, i64, f64)> = rows
        .iter()
        .map(|row| {
            (
                row.get::<DateTime<Utc>>("bucket").unwrap(),
                row.get::<i64>("readings").unwrap(),
                row.get::<f64>("peak").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        buckets,
        [
            (base, 3, 12.0),
            (base + Duration::hours(1), 2, 15.0),
            (base + Duration::hours(2), 1, 9.0)
        ]
    );
    if server == Server::Postgres {
        let rows = table("readings")
            .select_time_bucket("1 hour", "taken_at", "bucket")
            .select_first("celsius", "taken_at", "opening")
            .select_last("celsius", "taken_at", "closing")
            .group_by(["bucket"])
            .order_by("bucket")
            .get(&db)
            .await
            .unwrap();
        let first_last: Vec<(f64, f64)> = rows
            .iter()
            .map(|row| {
                (
                    row.get::<f64>("opening").unwrap(),
                    row.get::<f64>("closing").unwrap(),
                )
            })
            .collect();
        assert_eq!(first_last, [(10.0, 11.0), (15.0, 13.0), (9.0, 9.0)]);
    }
}

/// Statement timeouts: a slow read is stopped with `OrmError::Timeout`,
/// quick ones run, and a unit of work survives a stopped read (PostgreSQL
/// runs it in a savepoint and restores the transaction's own timeout).
pub async fn timeouts(db: Db, server: Server) {
    use std::time::{Duration as StdDuration, Instant};

    Company::create(
        &db,
        Company {
            name: "Acme".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    let slow_select = match server {
        Server::Postgres => "pg_sleep(3) AS slept",
        // MySQL stops a statement under MAX_EXECUTION_TIME between rows;
        // this one reads billions of them.
        Server::MySql => {
            "(SELECT COUNT(*) FROM information_schema.columns a CROSS JOIN information_schema.columns b CROSS JOIN information_schema.columns c) AS rows_read"
        }
    };
    let slow = || {
        table("companies")
            .select_raw(slow_select, Vec::<Value>::new())
            .timeout(StdDuration::from_millis(200))
    };
    let started = Instant::now();
    let error = slow().get(&db).await.unwrap_err();
    assert!(error.is_timeout(), "{error:?}");
    assert!(
        started.elapsed() < StdDuration::from_secs(3),
        "stopped early: {:?}",
        started.elapsed()
    );
    assert!(
        error.to_string().contains("ran longer than its timeout"),
        "{error}"
    );
    assert_eq!(
        table("companies")
            .timeout(StdDuration::from_secs(10))
            .count(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Company::query()
            .timeout(StdDuration::from_secs(10))
            .pluck(&db, Company::NAME)
            .await
            .unwrap(),
        ["Acme"]
    );

    let uow = db.uow(None, None).await;
    Company::create(
        &uow,
        Company {
            name: "Globex".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    let error = slow().get(&uow).await.unwrap_err();
    assert!(error.is_timeout(), "{error:?}");
    assert_eq!(
        Company::query()
            .timeout(StdDuration::from_secs(10))
            .count(&uow)
            .await
            .unwrap(),
        2,
        "the unit of work goes on"
    );
    if server == Server::Postgres {
        let setting: Option<String> = navree_orm::scalar(
            &uow,
            navree_orm::raw(
                "SELECT current_setting('statement_timeout')",
                Vec::<Value>::new(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            setting.as_deref(),
            Some("0"),
            "the transaction's own timeout is back"
        );
    }
    uow.commit().await.unwrap();
    assert_eq!(Company::query().count(&db).await.unwrap(), 2);
}

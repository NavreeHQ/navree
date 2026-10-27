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
    let acme = Company::create(db, Company { name: "Acme".into(), country: Some("US".into()), ..Company::default() })
        .await
        .unwrap();
    let globex = Company::create(db, Company { name: "Globex".into(), country: Some("DE".into()), ..Company::default() })
        .await
        .unwrap();
    for (title, company, cents, stage, tags, closed) in [
        ("Renewal for Acme", Some(acme.id), 5_000, Stage::Lead, vec!["vip"], None),
        ("Pricing upgrade", Some(acme.id), 25_000, Stage::Qualified, vec!["q1", "vip"], None),
        ("Platform deal", Some(globex.id), 90_000, Stage::ClosedWon, vec![], Some(at(20, 14))),
        ("Small change", None, 50, Stage::ClosedLost, vec!["q1"], Some(at(21, 9))),
    ] {
        Deal::create(
            db,
            Deal {
                company_id: company,
                stage,
                tags: tags.into_iter().map(Into::into).collect(),
                closed_at: closed,
                meta: DealMeta { source: if cents > 20_000 { "ads".into() } else { "web".into() }, score: 7 },
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
    let acme = Company::create(&db, Company { name: "Acme".into(), ..Company::default() }).await.unwrap();
    let globex = Company::create(&db, Company { name: "Globex".into(), ..Company::default() }).await.unwrap();
    assert!(acme.id > 0);
    assert_eq!(globex.id, acme.id + 1, "the database assigned the keys");

    let deal = Deal::create(
        &db,
        Deal {
            company_id: Some(acme.id),
            stage: Stage::ClosedWon,
            priority: Priority::High,
            meta: DealMeta { source: "partner".into(), score: 9 },
            tags: vec!["a".into(), "b".into()],
            probability: 0.75,
            value: Decimal::new(1_234_567, 2),
            closes_on: NaiveDate::from_ymd_opt(2026, 12, 31),
            closed_at: Some(Utc.with_ymd_and_hms(2026, 10, 27, 8, 30, 15).unwrap() + Duration::microseconds(123_456)),
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
    assert_eq!(stored.get_changes().iter().map(|(c, _)| *c).collect::<Vec<_>>(), ["stage", "updated_at"]);
    assert_eq!(Deal::find_or_fail(&db, deal.id).await.unwrap().stage, Stage::Lead);

    let mut acme = acme;
    acme.increment(&db, Company::EMPLOYEES, 5).await.unwrap();
    assert_eq!(Company::find_or_fail(&db, acme.id).await.unwrap().employees, 5);

    let event = Event::create(&db, Event { name: "signup".into(), occurred_at: at(1, 1), payload: Some(serde_json::json!({"plan": "pro"})), ..Event::default() })
        .await
        .unwrap();
    let found = Event::find_or_fail(&db, event.id).await.unwrap();
    assert_eq!(*found, *event);

    let mut gone = Deal::find_or_fail(&db, deal.id).await.unwrap();
    assert!(gone.delete(&db).await.unwrap());
    assert!(gone.trashed());
    assert!(Deal::find(&db, deal.id).await.unwrap().is_none());
    assert!(Deal::query().with_trashed().find(&db, deal.id).await.unwrap().is_some());
    assert_eq!(Company::destroy(&db, [globex.id]).await.unwrap(), 1);
}

/// The where family, selects, joins, groups, aggregates and plucks.
pub async fn queries(db: Db, server: Server) {
    let (acme, _globex) = seed(&db).await;
    let deals = |q: navree_orm::Builder<Deal>| {
        let db = db.clone();
        async move { titles(&q.get(&db).await.unwrap()) }
    };

    assert_eq!(deals(Deal::query().where_(Deal::AMOUNT_CENTS.gte(25_000))).await, ["Platform deal", "Pricing upgrade"]);
    assert_eq!(deals(Deal::query().where_in(Deal::STAGE, [Stage::Lead, Stage::ClosedLost])).await, ["Renewal for Acme", "Small change"]);
    assert_eq!(deals(Deal::query().where_null(Deal::COMPANY_ID)).await, ["Small change"]);
    assert_eq!(deals(Deal::query().where_between(Deal::AMOUNT_CENTS, 1_000, 30_000)).await, ["Pricing upgrade", "Renewal for Acme"]);
    assert_eq!(deals(Deal::query().where_like(Deal::TITLE, "%ACME%")).await, ["Renewal for Acme"]);
    assert!(deals(Deal::query().where_like_case_sensitive(Deal::TITLE, "%ACME%")).await.is_empty());
    assert_eq!(deals(Deal::query().where_json_contains(Deal::TAGS, "vip")).await, ["Pricing upgrade", "Renewal for Acme"]);
    assert_eq!(deals(Deal::query().where_json_length(Deal::TAGS, ">", 1)).await, ["Pricing upgrade"]);
    assert_eq!(deals(Deal::query().where_(col("meta->source").eq("ads"))).await, ["Platform deal", "Pricing upgrade"]);
    assert_eq!(deals(Deal::query().where_(Deal::META.json("score").eq(7)).where_(Deal::AMOUNT_CENTS.lt(100))).await, ["Small change"]);
    assert_eq!(deals(Deal::query().where_date(Deal::CLOSED_AT, "=", NaiveDate::from_ymd_opt(2026, 3, 20).unwrap())).await, ["Platform deal"]);
    assert_eq!(deals(Deal::query().where_day(Deal::CLOSED_AT, "=", 21)).await, ["Small change"]);
    assert_eq!(deals(Deal::query().where_month(Deal::CLOSED_AT, "=", 3).where_year(Deal::CLOSED_AT, "=", 2026)).await.len(), 2);
    assert_eq!(deals(Deal::query().where_time(Deal::CLOSED_AT, "<", NaiveTime::from_hms_opt(12, 0, 0).unwrap())).await, ["Small change"]);
    assert_eq!(deals(Deal::query().where_(Deal::PRIORITY.eq(Priority::Low)).where_(Deal::STAGE.eq(Stage::Lead))).await, ["Renewal for Acme"]);
    assert_eq!(deals(Deal::query().where_raw("amount_cents > ?", [80_000_i64])).await, ["Platform deal"]);
    assert_eq!(
        deals(Deal::query().where_exists(Company::query().where_column(Company::ID, "=", Deal::COMPANY_ID).where_(Company::COUNTRY.eq("DE")))).await,
        ["Platform deal"]
    );
    assert_eq!(
        deals(Deal::query().where_in_sub(Deal::COMPANY_ID, Company::query().select([Company::ID]).where_(Company::NAME.eq("Acme")))).await,
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
    assert_eq!(Deal::query().sum::<i64>(&db, Deal::AMOUNT_CENTS).await.unwrap(), 120_050);
    assert_eq!(Deal::query().avg::<Decimal>(&db, Deal::AMOUNT_CENTS).await.unwrap(), Some(Decimal::new(300_125, 1)));
    assert_eq!(Deal::query().max::<i64>(&db, Deal::AMOUNT_CENTS).await.unwrap(), Some(90_000));
    assert_eq!(Deal::query().group_by([Deal::COMPANY_ID]).count(&db).await.unwrap(), 3);
    assert_eq!(Deal::query().count_distinct(&db, Deal::COMPANY_ID).await.unwrap(), 2);
    let won = Deal::query().where_(Deal::STAGE.eq(Stage::ClosedWon)).value(&db, Deal::STAGE).await.unwrap();
    assert_eq!(won, Some(Stage::ClosedWon));
    let by_title = Deal::query().order_by(Deal::AMOUNT_CENTS).pluck_keyed(&db, Deal::AMOUNT_CENTS, Deal::TITLE).await.unwrap();
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
        .union(table("deals").select(["title"]).where_(col("amount_cents").lt(100)))
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

    let full_text = Deal::query().where_full_text([Deal::TITLE], "pricing").get(&db).await.unwrap();
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
    assert_eq!(Deal::query().max::<i64>(&db, Deal::AMOUNT_CENTS).await.unwrap(), Some(90_001));
    let joined_delete = Deal::query()
        .join("companies", Deal::COMPANY_ID, "=", Company::ID)
        .where_(Company::COUNTRY.eq("DE"))
        .delete(&db)
        .await
        .unwrap();
    assert_eq!(joined_delete, 1, "soft-deleted through the join");
    assert_eq!(Deal::query().count(&db).await.unwrap(), 3);

    let tag = Tag::first_or_create(&db, [Tag::NAME.set("vip")], |t| t.color = "gold".into()).await.unwrap();
    assert!(tag.was_recently_created());
    // Inside a transaction the losing insert runs in a savepoint: the
    // transaction (and PostgreSQL's in particular) goes on.
    let uow = db.uow(None, None).await;
    let same = Tag::create_or_first(&uow, [Tag::NAME.set("vip")], |t| t.color = "red".into()).await.unwrap();
    assert_eq!(same.id, tag.id);
    let other = Tag::update_or_create(&uow, [Tag::NAME.set("new")], |t| t.color = "green".into()).await.unwrap();
    assert!(other.was_recently_created());
    uow.commit().await.unwrap();
    assert_eq!(Tag::query().count(&db).await.unwrap(), 2);
    let renamed = Tag::update_or_create(&db, [Tag::NAME.set("vip")], |t| t.color = "blue".into()).await.unwrap();
    assert_eq!(renamed.color, "blue");
    assert_eq!(table("tags").where_(col("name").eq("vip")).value(&db, col_as::<String>("color")).await.unwrap().as_deref(), Some("blue"));
}

/// Pages and cursors.
pub async fn pagination(db: Db) {
    for n in 1..=7 {
        Company::create(&db, Company { name: format!("C{n:02}"), employees: n % 3, ..Company::default() })
            .await
            .unwrap();
    }
    let page = Company::query().order_by(Company::NAME).paginate(&db, (2, 3)).await.unwrap();
    assert_eq!(page.meta.total, 7);
    assert_eq!(page.data.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["C04", "C05", "C06"]);
    let simple = Company::query().order_by(Company::NAME).simple_paginate(&db, (3, 3)).await.unwrap();
    assert!(!simple.meta.has_more);

    let query = || Company::query().order_by_desc(Company::EMPLOYEES);
    let first = query().cursor_paginate(&db, CursorRequest::first().limit(3)).await.unwrap();
    let second = query().cursor_paginate(&db, CursorRequest::after(first.next_cursor.clone().unwrap()).limit(3)).await.unwrap();
    let back = query().cursor_paginate(&db, CursorRequest::after(second.prev_cursor.clone().unwrap()).limit(3)).await.unwrap();
    let names = |page: &navree_orm::CursorPage<Record<Company>>| page.data.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&first), ["C02", "C05", "C01"]);
    assert_eq!(names(&second), ["C04", "C07", "C03"]);
    assert_eq!(names(&back), names(&first));
}

/// The unit of work: change events, rollback, joining, settings.
pub async fn unit_of_work(db: Db, server: Server) {
    let actor = Uuid::now_v7();
    let uow = db.uow(Some(actor), Some("req-7".into())).await;
    let mut acme = Company::create(&uow, Company { name: "Acme".into(), ..Company::default() }).await.unwrap();
    acme.name = "Acme Corp".into();
    acme.save(&uow).await.unwrap();
    let note = Note::create(&uow, Note { body: "called them".into(), ..Note::default() }).await.unwrap();
    let ops: Vec<ChangeOp> = uow.changes().iter().map(|change| change.op).collect();
    assert_eq!(ops, [ChangeOp::Created, ChangeOp::Updated, ChangeOp::Created]);
    let fresh = note.fresh(&uow).await.unwrap().unwrap();
    if server == Server::Postgres {
        assert_eq!(fresh.written_by, actor.to_string(), "the statement ran in the unit of work's transaction");
    }
    uow.commit().await.unwrap();

    let rolled = db.uow(None, None).await;
    Company::create(&rolled, Company { name: "Ghost".into(), ..Company::default() }).await.unwrap();
    assert_eq!(Company::query().count(&rolled).await.unwrap(), 2);
    rolled.rollback().await.unwrap();
    assert_eq!(Company::query().count(&db).await.unwrap(), 1);

    let scoped = db.uow(None, None).await;
    let pool = db.clone();
    scoped
        .scope(async {
            Company::create(&pool, Company { name: "Joined".into(), ..Company::default() }).await.unwrap();
        })
        .await;
    assert_eq!(scoped.changes().len(), 1);
    scoped.rollback().await.unwrap();
    assert_eq!(Company::query().count(&db).await.unwrap(), 1);
}

/// Time buckets: TimescaleDB's on PostgreSQL, the emulation on MySQL give
/// the same buckets.
pub async fn time_buckets(db: Db, server: Server) {
    let base = Utc.with_ymd_and_hms(2026, 10, 27, 8, 0, 0).unwrap();
    for (minutes, celsius) in [(0, 10.0), (10, 12.0), (20, 11.0), (70, 15.0), (75, 13.0), (130, 9.0)] {
        table("readings")
            .insert(&db, [vec![
                ("taken_at", Value::from(base + Duration::minutes(minutes))),
                ("sensor", Value::from("a")),
                ("celsius", Value::from(celsius)),
            ]])
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
            .map(|row| (row.get::<f64>("opening").unwrap(), row.get::<f64>("closing").unwrap()))
            .collect();
        assert_eq!(first_last, [(10.0, 11.0), (15.0, 13.0), (9.0, 9.0)]);
    }
}

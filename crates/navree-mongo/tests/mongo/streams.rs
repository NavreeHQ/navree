//! Results streamed from MongoDB's cursors, batch by batch, on the
//! connection and in a unit of work's session.

use futures_util::TryStreamExt;
use navree_mongo::MongoStream;
use navree_orm::{Expr, Model, OrmError, Row, Value, table};

use crate::models::Ticket;
use crate::support::TestMongo;

/// More tickets than a cursor's first batch (101 documents).
const TICKETS: i64 = 250;

#[tokio::test]
async fn cursors_stream_records_batch_by_batch() {
    let Some(t) = TestMongo::connect("cursors_stream_records_batch_by_batch").await else {
        return;
    };
    let mongo = &t.db;
    table("tickets")
        .insert(
            mongo,
            (1..=TICKETS).map(|n| {
                [
                    ("_id", Value::from(n)),
                    ("subject", Value::from(format!("#{n}"))),
                ]
            }),
        )
        .await
        .unwrap();
    let subjects: Vec<String> = Ticket::query()
        .order_by(Ticket::ID)
        .stream(mongo)
        .map_ok(|ticket| ticket.subject.clone())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(subjects.len(), 250);
    assert_eq!(
        (subjects[0].as_str(), subjects[249].as_str()),
        ("#1", "#250")
    );

    // Filters, orders and limits stream; so do pipelines.
    let last: Vec<i64> = Ticket::query()
        .where_(Ticket::ID.gt(240))
        .order_by_desc(Ticket::ID)
        .limit(3)
        .stream(mongo)
        .map_ok(|ticket| ticket.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(last, [250, 249, 248]);
    let counted: Vec<Row> = table("tickets")
        .select_expr(Expr::count_all(), "n")
        .stream(mongo)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(counted[0].get::<i64>("n").unwrap(), TICKETS);

    // What the query cannot be fails as the first item.
    let error = Ticket::query()
        .where_raw("1 = 1", Vec::<Value>::new())
        .stream(mongo)
        .try_collect::<Vec<_>>()
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Unsupported { .. }), "{error:?}");
    t.cleanup().await;
}

#[tokio::test]
async fn a_unit_of_work_streams_in_its_session() {
    let Some(t) = TestMongo::connect("a_unit_of_work_streams_in_its_session").await else {
        return;
    };
    let mongo = &t.db;
    table("tickets")
        .insert(
            mongo,
            (1..=TICKETS).map(|n| {
                [
                    ("_id", Value::from(n)),
                    ("subject", Value::from(format!("#{n}"))),
                ]
            }),
        )
        .await
        .unwrap();
    let uow = mongo.begin().await.unwrap();
    Ticket::query()
        .where_(Ticket::ID.gt(200))
        .update(&uow, [Ticket::SUBJECT.set("closed".to_owned())])
        .await
        .unwrap();
    let mut tickets = Ticket::query().order_by(Ticket::ID).stream(&uow);
    let mut seen = 0;
    let mut closed = 0;
    while let Some(ticket) = tickets.try_next().await.unwrap() {
        seen += 1;
        if ticket.subject == "closed" {
            closed += 1;
        }
        if seen == 150 {
            // Past the first batch: other commands run in the session
            // between the cursor's batches.
            assert_eq!(
                Ticket::query()
                    .where_(Ticket::SUBJECT.eq("closed"))
                    .count(&uow)
                    .await
                    .unwrap(),
                50
            );
        }
    }
    drop(tickets);
    assert_eq!(
        (seen, closed),
        (250, 50),
        "the cursor sees the unit of work's writes"
    );
    uow.rollback().await.unwrap();
    assert_eq!(
        Ticket::query()
            .where_(Ticket::SUBJECT.eq("closed"))
            .count(mongo)
            .await
            .unwrap(),
        0
    );
    t.cleanup().await;
}

/// The builders' `cursor` and `lazy_by_id` read through the driver's
/// cursors too, on the connection and in a unit of work's session.
#[tokio::test]
async fn builder_cursors_read_from_mongodb_cursors() {
    let Some(t) = TestMongo::connect("builder_cursors_read_from_mongodb_cursors").await else {
        return;
    };
    let mongo = &t.db;
    table("tickets")
        .insert(
            mongo,
            (1..=TICKETS).map(|n| {
                [
                    ("_id", Value::from(n)),
                    ("subject", Value::from(format!("#{n}"))),
                ]
            }),
        )
        .await
        .unwrap();
    let ids: Vec<i64> = Ticket::query()
        .where_(Ticket::ID.gt(100))
        .order_by(Ticket::ID)
        .cursor(mongo)
        .map_ok(|ticket| ticket.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(ids.len(), 150);
    assert_eq!((ids[0], ids[149]), (101, 250));
    let paged: Vec<i64> = Ticket::query()
        .lazy_by_id(mongo, 40)
        .map_ok(|ticket| ticket.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(paged, (1..=TICKETS).collect::<Vec<_>>());

    let uow = mongo.begin().await.unwrap();
    Ticket::query()
        .where_(Ticket::ID.lte(10))
        .update(&uow, [Ticket::SUBJECT.set("closed".to_owned())])
        .await
        .unwrap();
    let closed: Vec<i64> = Ticket::query()
        .where_(Ticket::SUBJECT.eq("closed"))
        .order_by(Ticket::ID)
        .cursor(&uow)
        .map_ok(|ticket| ticket.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(
        closed,
        (1..=10).collect::<Vec<_>>(),
        "the unit of work's writes"
    );
    uow.rollback().await.unwrap();
    t.cleanup().await;
}

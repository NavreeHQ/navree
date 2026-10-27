//! Chunks and streams: `chunk`, `chunk_map`, `each`, `chunk_by_id` and
//! friends, `lazy`, `lazy_by_id`, `cursor`, on models, tables and raw SQL.

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
use navree_orm::hooks::{ModelEvent, ModelHook};
use navree_orm::{Collection, ExecContext, Model, Record, Result, Value, col, raw, table};

use crate::models::{Company, Deal, Stage, Tag};
use crate::support::{app, sqlite};

/// Companies `C1` .. `Cn` (ids 1 to n), in creation order.
async fn companies(db: &navree_db::Db, n: usize) {
    for i in 1..=n {
        Company::create(
            db,
            Company {
                name: format!("C{i}"),
                employees: i32::try_from(i).unwrap(),
                active: true,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
}

fn names(records: &Collection<Company>) -> Vec<String> {
    records.map(|company| company.name.clone())
}

#[tokio::test]
async fn chunks_visit_every_record_in_key_order() {
    let t = sqlite().await;
    companies(&t.db, 7).await;
    let pages = Arc::new(Mutex::new(Vec::new()));
    let seen = pages.clone();
    let completed = Company::query()
        .chunk(&t.db, 3, move |records| {
            seen.lock().unwrap().push(names(&records));
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert!(completed);
    assert_eq!(
        *pages.lock().unwrap(),
        [vec!["C1", "C2", "C3"], vec!["C4", "C5", "C6"], vec!["C7"]]
    );
}

#[tokio::test]
async fn a_chunk_callback_stops_the_walk() {
    let t = sqlite().await;
    companies(&t.db, 7).await;
    let mut calls = 0;
    let completed = Company::query()
        .chunk(&t.db, 2, |_records| {
            calls += 1;
            let stop = calls == 2;
            async move {
                Ok(if stop {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                })
            }
        })
        .await
        .unwrap();
    assert!(!completed, "stopped early");
    assert_eq!(calls, 2);
}

#[tokio::test]
async fn chunks_follow_the_query_order_offset_and_limit() {
    let t = sqlite().await;
    companies(&t.db, 9).await;
    let mut pages = Vec::new();
    Company::query()
        .where_(Company::EMPLOYEES.lt(9))
        .order_by_desc(Company::EMPLOYEES)
        .skip(1)
        .take(5)
        .chunk(&t.db, 2, |records| {
            pages.push(names(&records));
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(
        pages,
        [vec!["C7", "C6"], vec!["C5", "C4"], vec!["C3"]],
        "skips one, stops after five"
    );
}

#[tokio::test]
async fn an_error_in_a_chunk_callback_is_returned() {
    let t = sqlite().await;
    companies(&t.db, 3).await;
    let error = Company::query()
        .chunk(&t.db, 2, |_| async {
            Err::<ControlFlow<()>, _>(navree_orm::OrmError::InvalidQuery("stop here".into()))
        })
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "invalid query: stop here");
}

#[tokio::test]
async fn chunk_sizes_must_be_positive() {
    let t = sqlite().await;
    let error = Company::query()
        .chunk(&t.db, 0, |_| async { Ok(ControlFlow::Continue(())) })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: the chunk size must be at least 1"
    );
    let error = Company::query()
        .lazy(&t.db, 0)
        .try_collect::<Vec<_>>()
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: the chunk size must be at least 1"
    );
}

#[tokio::test]
async fn chunk_map_collects_one_value_per_record() {
    let t = sqlite().await;
    companies(&t.db, 5).await;
    let doubled: Vec<i32> = Company::query()
        .chunk_map(&t.db, 2, |company| company.employees * 2)
        .await
        .unwrap();
    assert_eq!(doubled, [2, 4, 6, 8, 10]);
}

#[tokio::test]
async fn each_visits_records_one_by_one_and_can_stop() {
    let t = sqlite().await;
    companies(&t.db, 5).await;
    let mut visited = Vec::new();
    let completed = Company::query()
        .each(&t.db, 2, |company| {
            let stop = company.name == "C3";
            visited.push(company.name.clone());
            async move {
                Ok(if stop {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                })
            }
        })
        .await
        .unwrap();
    assert!(!completed);
    assert_eq!(visited, ["C1", "C2", "C3"]);
}

#[tokio::test]
async fn chunk_by_id_stays_correct_while_the_callback_changes_rows() {
    let t = sqlite().await;
    companies(&t.db, 7).await;
    // Every visited company leaves the matching set; offset pages would
    // skip half of them, key pages do not.
    let db = t.db.clone();
    let mut pages = Vec::new();
    Company::query()
        .where_(Company::ACTIVE.eq(true))
        .chunk_by_id(&t.db, 2, |records| {
            pages.push(names(&records));
            let db = db.clone();
            async move {
                Company::query()
                    .where_key_in(records.model_keys())
                    .update(&db, [Company::ACTIVE.set(false)])
                    .await?;
                Ok(ControlFlow::Continue(()))
            }
        })
        .await
        .unwrap();
    assert_eq!(
        pages,
        [
            vec!["C1", "C2"],
            vec!["C3", "C4"],
            vec!["C5", "C6"],
            vec!["C7"]
        ]
    );
    assert_eq!(
        Company::query()
            .where_(Company::ACTIVE.eq(true))
            .count(&t.db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn chunk_by_id_desc_walks_newest_first_and_replaces_other_orders() {
    let t = sqlite().await;
    companies(&t.db, 5).await;
    let mut pages = Vec::new();
    Company::query()
        .order_by(Company::NAME)
        .chunk_by_id_desc(&t.db, 2, |records| {
            pages.push(names(&records));
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(pages, [vec!["C5", "C4"], vec!["C3", "C2"], vec!["C1"]]);
}

#[tokio::test]
async fn chunk_by_id_keeps_or_conditions_together() {
    let t = sqlite().await;
    companies(&t.db, 6).await;
    let mut seen = Vec::new();
    Company::query()
        .where_(Company::NAME.eq("C1"))
        .or_where(Company::NAME.eq("C5"))
        .or_where(Company::NAME.eq("C6"))
        .chunk_by_id(&t.db, 1, |records| {
            seen.extend(names(&records));
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(seen, ["C1", "C5", "C6"], "each page stays inside the ORs");
}

#[tokio::test]
async fn chunk_by_id_honours_offset_on_the_first_page_and_the_limit() {
    let t = sqlite().await;
    companies(&t.db, 9).await;
    let mut pages = Vec::new();
    Company::query()
        .skip(2)
        .take(5)
        .chunk_by_id(&t.db, 2, |records| {
            pages.push(names(&records));
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(pages, [vec!["C3", "C4"], vec!["C5", "C6"], vec!["C7"]]);
}

#[tokio::test]
async fn chunk_by_column_pages_by_another_unique_column() {
    let t = sqlite().await;
    for (name, color) in [
        ("delta", "d"),
        ("alpha", "a"),
        ("charlie", "c"),
        ("bravo", "b"),
    ] {
        Tag::create(
            &t.db,
            Tag {
                name: name.into(),
                color: color.into(),
                ..Tag::default()
            },
        )
        .await
        .unwrap();
    }
    let mut pages = Vec::new();
    Tag::query()
        .chunk_by_column(&t.db, 3, Tag::NAME, "name", |tags| {
            pages.push(tags.map(|tag| tag.name.clone()));
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(
        pages,
        [vec!["alpha", "bravo", "charlie"], vec!["delta"]],
        "ordered and paged by name"
    );

    // The alias names a selected column the records carry as an extra.
    let mut colors = Vec::new();
    Tag::query()
        .select_expr(Tag::NAME.expr(), "label")
        .chunk_by_column(&t.db, 2, Tag::NAME, "label", |tags| {
            colors.extend(tags.map(|tag| tag.color.clone()));
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(colors, ["a", "b", "c", "d"]);

    let error = Tag::query()
        .chunk_by_column(&t.db, 2, Tag::NAME, "nickname", |_| async {
            Ok(ControlFlow::Continue(()))
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: chunk_by_id was stopped: the result has no `nickname` column to continue from (select it, or name the column it is selected as)"
    );
}

#[tokio::test]
async fn each_by_id_visits_records_by_key() {
    let t = sqlite().await;
    companies(&t.db, 4).await;
    let mut visited = Vec::new();
    let completed = Company::query()
        .each_by_id(&t.db, 3, |company| {
            visited.push(company.id);
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert!(completed);
    assert_eq!(visited, [1, 2, 3, 4]);
}

#[tokio::test]
async fn uuid_keys_page_in_creation_order() {
    let t = sqlite().await;
    let mut created = Vec::new();
    for i in 0..5 {
        let deal = Deal::create(&t.db, Deal::new(&format!("D{i}"), i))
            .await
            .unwrap();
        created.push(deal.id);
    }
    let mut seen = Vec::new();
    Deal::query()
        .chunk_by_id(&t.db, 2, |deals| {
            seen.extend(deals.model_keys());
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(seen, created, "UUID v7 keys are time-ordered");
}

#[tokio::test]
async fn lazy_streams_records_page_by_page() {
    let t = sqlite().await;
    companies(&t.db, 5).await;
    let all: Vec<String> = Company::query()
        .lazy(&t.db, 2)
        .map_ok(|company| company.name.clone())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(all, ["C1", "C2", "C3", "C4", "C5"]);

    // Stream adapters stop early: only what is pulled is read.
    let first: Vec<i64> = Company::query()
        .lazy(&t.db, 2)
        .take(3)
        .map(|company| company.map(|company| company.id))
        .try_collect()
        .await
        .unwrap();
    assert_eq!(first, [1, 2, 3]);

    let evens: Vec<i32> = Company::query()
        .lazy(&t.db, 10)
        .try_filter(|company| futures::future::ready(company.employees % 2 == 0))
        .map_ok(|company| company.employees)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(evens, [2, 4]);

    let chunks: Vec<Vec<i64>> = Company::query()
        .lazy(&t.db, 2)
        .map(|company| company.unwrap().id)
        .chunks(4)
        .collect()
        .await;
    assert_eq!(chunks, [vec![1, 2, 3, 4], vec![5]]);
}

#[tokio::test]
async fn lazy_by_id_streams_by_key_in_either_direction() {
    let t = sqlite().await;
    companies(&t.db, 5).await;
    let up: Vec<i64> = Company::query()
        .lazy_by_id(&t.db, 2)
        .map_ok(|company| company.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(up, [1, 2, 3, 4, 5]);
    let down: Vec<i64> = Company::query()
        .lazy_by_id_desc(&t.db, 2)
        .map_ok(|company| company.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(down, [5, 4, 3, 2, 1]);
    let limited: Vec<i64> = Company::query()
        .take(3)
        .lazy_by_id_desc(&t.db, 2)
        .map_ok(|company| company.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(limited, [5, 4, 3]);
}

#[tokio::test]
async fn lazy_streams_skip_soft_deleted_records() {
    let t = sqlite().await;
    let kept = Deal::create(&t.db, Deal::new("kept", 1)).await.unwrap();
    let mut gone = Deal::create(&t.db, Deal::new("gone", 2)).await.unwrap();
    gone.delete(&t.db).await.unwrap();
    let ids: Vec<_> = Deal::query()
        .lazy_by_id(&t.db, 1)
        .map_ok(|deal| deal.id)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(ids, [kept.id]);
    let all: Vec<_> = Deal::query()
        .with_trashed()
        .cursor(&t.db)
        .map_ok(|deal| deal.title.clone())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn cursors_stream_records_of_one_query() {
    let t = sqlite().await;
    companies(&t.db, 4).await;
    let mut cursor = Company::query()
        .where_(Company::EMPLOYEES.gte(2))
        .order_by_desc(Company::ID)
        .cursor(&t.db);
    let mut names = Vec::new();
    while let Some(company) = cursor.try_next().await.unwrap() {
        assert!(company.exists());
        names.push(company.name.clone());
    }
    assert_eq!(names, ["C4", "C3", "C2"]);
    assert!(
        cursor.next().await.is_none(),
        "a finished cursor stays finished"
    );
}

#[tokio::test]
async fn cursors_report_query_errors_as_their_first_item() {
    let t = sqlite().await;
    let mut cursor = Company::query()
        .where_(col("name").op("=>", "x"))
        .cursor(&t.db);
    let error = cursor.next().await.unwrap().unwrap_err();
    assert!(
        error.to_string().contains("`=>` is not a known operator"),
        "{error}"
    );
    assert!(cursor.next().await.is_none());
}

/// Rows stream as the database produces them: an endless result set still
/// yields its first rows.
#[tokio::test]
async fn cursors_do_not_wait_for_the_whole_result() {
    let t = sqlite().await;
    let endless = raw(
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n) SELECT x FROM n",
        Vec::<Value>::new(),
    );
    let first = tokio::time::timeout(Duration::from_secs(30), async {
        navree_orm::cursor(&t.db, endless)
            .take(3)
            .map_ok(|row| row.get::<i64>("x").unwrap())
            .try_collect::<Vec<_>>()
            .await
    })
    .await
    .expect("the first rows arrive without reading the rest")
    .unwrap();
    assert_eq!(first, [1, 2, 3]);
}

#[tokio::test]
async fn raw_cursors_bind_their_values() {
    let t = sqlite().await;
    companies(&t.db, 3).await;
    let names: Vec<String> = navree_orm::cursor(
        &t.db,
        raw(
            "SELECT name FROM companies WHERE employees >= ? ORDER BY id",
            [2],
        ),
    )
    .map_ok(|row| row.get::<String>("name").unwrap())
    .try_collect()
    .await
    .unwrap();
    assert_eq!(names, ["C2", "C3"]);
    let error = navree_orm::cursor(&t.db, raw("SELECT ?", [1, 2]))
        .try_collect::<Vec<_>>()
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("1 placeholder(s) but 2 value(s)")
    );
}

#[tokio::test]
async fn a_cursor_in_a_unit_of_work_reads_its_writes_and_guards_its_connection() {
    let t = sqlite().await;
    let uow = t.db.uow(None, None).await;
    companies_in(&uow, 3).await;
    {
        let mut cursor = Company::query().cursor(&uow);
        let first = cursor.try_next().await.unwrap().unwrap();
        assert_eq!(first.name, "C1", "the cursor sees the uncommitted rows");
        // The cursor holds the transaction's connection: a write through
        // the same unit of work is refused instead of waiting forever.
        let error = Company::create(
            &uow,
            Company {
                name: "C4".into(),
                ..Company::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid query: this transaction's connection is busy streaming a cursor; finish or drop the cursor first (lazy() and chunk_by_id() read in pages and allow writes in between)"
        );
        let rest: Vec<String> = cursor
            .map_ok(|company| company.name.clone())
            .try_collect()
            .await
            .unwrap();
        assert_eq!(rest, ["C2", "C3"]);
    }
    // Dropped: the connection is free again.
    Company::create(
        &uow,
        Company {
            name: "C4".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(Company::query().count(&t.db).await.unwrap(), 4);
}

#[tokio::test]
async fn lazy_streams_allow_writes_between_pages() {
    let t = sqlite().await;
    let uow = t.db.uow(None, None).await;
    companies_in(&uow, 4).await;
    let mut stream = Company::query().lazy_by_id(&uow, 2);
    while let Some(mut company) = stream.try_next().await.unwrap() {
        company.employees += 100;
        company.save(&uow).await.unwrap();
    }
    drop(stream);
    uow.commit().await.unwrap();
    let employees: Vec<i32> = Company::query()
        .pluck(&t.db, Company::EMPLOYEES)
        .await
        .unwrap();
    assert_eq!(employees, [101, 102, 103, 104]);
}

async fn companies_in(uow: &navree_db::Uow, n: usize) {
    for i in 1..=n {
        Company::create(
            uow,
            Company {
                name: format!("C{i}"),
                employees: i32::try_from(i).unwrap(),
                active: true,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn table_queries_chunk_stream_and_page_by_key() {
    let t = sqlite().await;
    companies(&t.db, 5).await;
    let error = table("companies")
        .chunk(&t.db, 2, |_| async { Ok(ControlFlow::Continue(())) })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: chunk needs an order on a table query (add order_by(..) so pages neither overlap nor skip rows)"
    );

    let mut pages = Vec::new();
    table("companies")
        .select(["id", "name"])
        .order_by_desc("id")
        .chunk(&t.db, 2, |rows| {
            pages.push(
                rows.iter()
                    .map(|row| row.get::<String>("name").unwrap())
                    .collect::<Vec<_>>(),
            );
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(pages, [vec!["C5", "C4"], vec!["C3", "C2"], vec!["C1"]]);

    let mut ids = Vec::new();
    table("companies")
        .chunk_by_id(&t.db, 2, |rows| {
            ids.extend(rows.iter().map(|row| row.get::<i64>("id").unwrap()));
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(ids, [1, 2, 3, 4, 5]);

    let names: Vec<String> = table("companies")
        .lazy_by_column(&t.db, 2, "name", "name")
        .map_ok(|row| row.get::<String>("name").unwrap())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(names, ["C1", "C2", "C3", "C4", "C5"]);

    let streamed: Vec<i64> = table("companies")
        .where_(col("employees").gt(3))
        .cursor(&t.db)
        .map_ok(|row| row.get::<i64>("employees").unwrap())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(streamed, [4, 5]);

    let mut each = Vec::new();
    table("companies")
        .order_by("id")
        .each(&t.db, 2, |row| {
            each.push(row.get::<i64>("id").unwrap());
            async { Ok(ControlFlow::Continue(())) }
        })
        .await
        .unwrap();
    assert_eq!(each, [1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn streams_are_send_and_live_as_long_as_their_executor() {
    let t = sqlite().await;
    companies(&t.db, 3).await;
    let db = Arc::new(t.db.clone());
    let task = tokio::spawn(async move {
        let names: Vec<String> = Company::query()
            .cursor(&*db)
            .map_ok(|company| company.name.clone())
            .try_collect()
            .await?;
        let lazy: Vec<String> = Company::query()
            .lazy_by_id(&*db, 1)
            .map_ok(|company| company.name.clone())
            .try_collect()
            .await?;
        let mut total = 0;
        Company::query()
            .chunk(&*db, 2, |records| {
                total += records.len();
                async { Ok(ControlFlow::Continue(())) }
            })
            .await?;
        Ok::<_, navree_orm::OrmError>((names, lazy, total))
    });
    let (names, lazy, total) = task.await.unwrap().unwrap();
    assert_eq!(names, lazy);
    assert_eq!(total, 3);
}

/// Notes the companies it sees retrieved.
struct Retrieved(Arc<Mutex<Vec<String>>>);

impl ModelHook<Company> for Retrieved {
    fn after<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        record: &'a mut Record<Company>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if event == ModelEvent::Retrieved {
                self.0.lock().unwrap().push(record.name.clone());
            }
            Ok(())
        })
    }
}

struct RetrievedHook(Arc<Mutex<Vec<String>>>);

impl Module for RetrievedHook {
    fn register(&self, m: &mut ModuleBuilder) {
        let seen = self.0.clone();
        m.multi(Factory::<dyn ModelHook<Company>>::new(move |_| {
            let seen = seen.clone();
            async move { Ok(Arc::new(Retrieved(seen)) as Arc<dyn ModelHook<Company>>) }
        }));
    }
}

#[tokio::test]
async fn retrieved_hooks_run_for_every_streamed_record() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (_app, db, _clock) = app(vec![Box::new(RetrievedHook(seen.clone()))]).await;
    companies(&db, 3).await;
    let mut cursor = Company::query().cursor(&*db);
    cursor.try_next().await.unwrap().unwrap();
    assert_eq!(*seen.lock().unwrap(), ["C1"], "one row read, one hook run");
    drop(cursor);
    seen.lock().unwrap().clear();
    let _: Vec<_> = Company::query().lazy(&*db, 2).try_collect().await.unwrap();
    assert_eq!(*seen.lock().unwrap(), ["C1", "C2", "C3"]);
}

#[tokio::test]
async fn deals_stream_with_their_casts() {
    let t = sqlite().await;
    Deal::create(
        &t.db,
        Deal {
            stage: Stage::Qualified,
            ..Deal::new("Cast", 10)
        },
    )
    .await
    .unwrap();
    let stages: Vec<Stage> = Deal::query()
        .cursor(&t.db)
        .map_ok(|deal| deal.stage)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(stages, [Stage::Qualified]);
}

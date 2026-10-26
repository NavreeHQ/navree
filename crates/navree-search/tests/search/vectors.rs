//! Vector search: `similar`, `similar_to`, `hybrid` and the embeddings
//! cache, on the memory store.

use std::sync::atomic::Ordering;

use navree_db::{Db, PageQuery, Repo};
use navree_search::{Search, SearchError};
use sea_orm::ActiveValue::Set;
use sea_orm::IntoActiveModel;

use crate::support::{self, Contact, insert_contacts, new_contact, settle};

fn with_notes(
    first: &str,
    last: &str,
    notes: &str,
    status: &str,
) -> crate::support::contact::ActiveModel {
    let mut row = new_contact(first, last);
    row.notes = Set(Some(notes.to_owned()));
    row.status = Set(status.to_owned());
    row
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn similar_finds_records_by_meaning() {
    let (app, embeddings) = support::vector_app().await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let rows = insert_contacts(
        &db,
        vec![
            with_notes("Ada", "Lovelace", "struggled with onboarding", "customer"),
            with_notes("Grace", "Hopper", "billing questions", "customer"),
            with_notes("Alan", "Turing", "onboarding call about billing", "lead"),
        ],
    )
    .await;
    settle(&app).await;
    let similar = search
        .similar::<Contact>("customers unhappy with onboarding")
        .limit(2)
        .run()
        .await
        .unwrap();
    let names: Vec<&str> = similar
        .iter()
        .map(|h| h.record.first_name.as_str())
        .collect();
    assert_eq!(names, ["Ada", "Alan"]);
    assert!(similar[0].score > 0.9 && similar[0].score <= 1.0);

    let customers = search
        .similar::<Contact>("onboarding")
        .filter("status", "customer")
        .limit(5)
        .run()
        .await
        .unwrap();
    assert_eq!(customers[0].record.id, rows[0].id);
    assert!(customers.iter().all(|h| h.record.status == "customer"));

    let like_ada = search
        .similar_to::<Contact>(rows[0].id)
        .limit(1)
        .run()
        .await
        .unwrap();
    assert_eq!(like_ada.len(), 1);
    assert_eq!(
        like_ada[0].record.id, rows[2].id,
        "the record itself is left out"
    );

    // Re-syncing an unchanged record embeds nothing new.
    let before = embeddings.calls.load(Ordering::SeqCst);
    search.reindex::<Contact>([rows[0].id]).await.unwrap();
    settle(&app).await;
    assert_eq!(embeddings.calls.load(Ordering::SeqCst), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hybrid_blends_keyword_and_vector_relevance() {
    let (app, _) = support::vector_app().await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let rows = insert_contacts(
        &db,
        vec![
            with_notes("Renewal", "Smith", "billing", "customer"),
            with_notes("Pat", "Jones", "renewal renewal renewal", "customer"),
        ],
    )
    .await;
    settle(&app).await;
    let keywords = search
        .query::<Contact>("renewal")
        .paginate(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(support::ids(&keywords), [rows[0].id.to_string()]);
    let vectors = search
        .query::<Contact>("renewal")
        .hybrid(1.0)
        .paginate(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(support::ids(&vectors)[0], rows[1].id.to_string());
    let blended = search
        .query::<Contact>("renewal")
        .hybrid(0.4)
        .hits(PageQuery::default())
        .await
        .unwrap();
    assert_eq!(blended.meta.total, 2);
    assert!(blended.data.iter().all(|h| h.score <= 1.0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleted_records_leave_the_vector_store() {
    let (app, _) = support::vector_app().await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let rows = insert_contacts(
        &db,
        vec![with_notes("Ada", "Lovelace", "onboarding", "lead")],
    )
    .await;
    settle(&app).await;
    let mut row = rows[0].clone().into_active_model();
    row.deleted_at = Set(Some(support::now()));
    support::in_uow(&db, async |uow| {
        Repo::<Contact>::update(uow, row).await.unwrap()
    })
    .await;
    settle(&app).await;
    assert!(
        search
            .similar_to::<Contact>(rows[0].id)
            .run()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        search
            .similar::<Contact>("onboarding")
            .run()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vector_queries_need_vectors() {
    let app = support::app("memory", "inline").await;
    let search = app.get::<Search>().await;
    let error = search.similar::<Contact>("x").run().await.unwrap_err();
    assert!(matches!(error, SearchError::VectorsOff));
    let error = search
        .query::<Contact>("x")
        .hybrid(0.5)
        .ids(1)
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::VectorsOff));
}

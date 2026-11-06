//! Searches reported as telemetry: the engine, the index, the hits and the
//! length of the term, never the term.

use navree_db::{Db, PageQuery};
use navree_search::Search;
use navree_telemetry::{Outcome, SearchMode, Telemetry, TelemetryContext, TelemetryRecorder};

use crate::support::{self, Contact, insert_contacts, new_contact, settle};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_query_reports_its_hits_and_the_length_of_its_term() {
    let recorder = TelemetryRecorder::new();
    let app = support::builder("memory", "inline")
        .module(recorder.module())
        .build()
        .await;
    let db = app.get::<Db>().await;
    insert_contacts(
        &db,
        vec![
            new_contact("Harpreet", "Lakhotia"),
            new_contact("Gurpreet", "Hundal"),
        ],
    )
    .await;
    settle(&app).await;
    let search = app.get::<Search>().await;
    let telemetry = Telemetry::from_app(app.ctx()).await;

    TelemetryContext::new(telemetry)
        .scope(async {
            search
                .query::<Contact>("lakhotia")
                .paginate(PageQuery::new(1, 15))
                .await
                .unwrap();
        })
        .await;

    let searches = recorder.searches();
    let reported = searches.last().expect("a search was reported");
    assert_eq!(reported.engine, "memory");
    assert!(reported.index.contains("contact"), "{}", reported.index);
    assert_eq!(reported.mode, SearchMode::Text);
    assert_eq!(reported.term_length, 8);
    assert_eq!(reported.hits, Some(1));
    assert_eq!(reported.outcome, Outcome::Success);
    let text = serde_json::to_string(reported).unwrap();
    assert!(!text.contains("lakhotia"), "the term stays out: {text}");
}

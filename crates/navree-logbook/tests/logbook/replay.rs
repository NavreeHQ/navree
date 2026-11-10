//! Replaying recorded requests, as the same user or a guest, and "Copy as
//! curl".

use navree_auth::{AuthModule, AuthService, JwtService};
use navree_crypto::CryptoModule;
use navree_db::DatabaseModule;
use navree_logbook::{Content, Logbook, LogbookError, LogbookModule, ReplayRequest};
use serde_json::json;

use crate::app::{FixturesModule, app, builder};

async fn recorded_echo(app: &navree_testing::TestApp) -> (std::sync::Arc<Logbook>, String) {
    app.client()
        .post("/echo")
        .json(&json!({ "n": 1 }))
        .send()
        .await
        .assert_status(200);
    let logbook = app.get::<Logbook>().await;
    let head = logbook
        .last_request()
        .await
        .unwrap()
        .head()
        .unwrap()
        .clone();
    (logbook, head.id)
}

#[tokio::test]
async fn unsafe_methods_replay_only_when_confirmed() {
    let app = app().await;
    let (logbook, id) = recorded_echo(&app).await;
    let refused = logbook.replay(&id, ReplayRequest::default()).await;
    assert!(
        matches!(refused, Err(LogbookError::Unconfirmed { ref method }) if method == "POST"),
        "{refused:?}"
    );
    let replayed = logbook
        .replay(
            &id,
            ReplayRequest {
                confirm: true,
                ..ReplayRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(replayed.status, 200);
    assert!(replayed.body.contains("\"n\":1"), "{}", replayed.body);

    let batch = logbook
        .batch(&replayed.batch_id)
        .await
        .expect("the replay is recorded");
    let request = batch.request().unwrap();
    assert_eq!(request.replay_of.as_deref(), Some(id.as_str()));
    assert!(batch.tags().contains(&"replay".to_owned()));
    assert_eq!(
        replayed.entry_id.as_deref(),
        Some(batch.head().unwrap().id.as_str())
    );
}

#[tokio::test]
async fn edits_change_what_is_sent() {
    let app = app().await;
    let (logbook, id) = recorded_echo(&app).await;
    let replayed = logbook
        .replay(
            &id,
            ReplayRequest {
                body: Some(r#"{"n":2}"#.into()),
                confirm: true,
                ..ReplayRequest::default()
            },
        )
        .await
        .unwrap();
    assert!(replayed.body.contains("\"n\":2"), "{}", replayed.body);

    let missing = logbook
        .replay(
            &id,
            ReplayRequest {
                method: Some("GET".into()),
                path: Some("/nowhere".into()),
                ..ReplayRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(missing.status, 404, "a GET needs no confirmation");

    let bad = logbook
        .replay(
            &id,
            ReplayRequest {
                method: Some("GET".into()),
                path: Some("nowhere".into()),
                ..ReplayRequest::default()
            },
        )
        .await;
    assert!(
        matches!(bad, Err(LogbookError::InvalidReplay(_))),
        "{bad:?}"
    );
}

#[tokio::test]
async fn only_requests_replay_and_only_when_replay_is_on() {
    let app = app().await;
    let (logbook, _) = recorded_echo(&app).await;
    logbook.dump("not a request", &1);
    let dump = logbook
        .entries(&navree_logbook::ListQuery::new(
            navree_logbook::Filter::parse("type:dump").unwrap(),
            1,
        ))
        .await
        .unwrap()
        .remove(0);
    assert!(matches!(
        logbook.replay(&dump.id, ReplayRequest::default()).await,
        Err(LogbookError::NotARequest(_))
    ));
    assert!(matches!(
        logbook.replay("nope", ReplayRequest::default()).await,
        Err(LogbookError::NotFound(_))
    ));

    let off = builder()
        .config("logbook.replay", false)
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    let (logbook, id) = recorded_echo(&off).await;
    assert!(matches!(
        logbook.replay(&id, ReplayRequest::default()).await,
        Err(LogbookError::ReplayOff)
    ));
}

#[tokio::test]
async fn the_api_replays_and_answers_with_the_right_errors() {
    let app = app().await;
    let (_, id) = recorded_echo(&app).await;
    app.client()
        .post(&format!("/_logbook/api/requests/{id}/replay"))
        .json(&json!({}))
        .send()
        .await
        .assert_error(409, None);
    let replayed = app
        .client()
        .post(&format!("/_logbook/api/requests/{id}/replay"))
        .json(&json!({ "confirm": true }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(replayed["status"], 200);
    assert!(replayed["batch_id"].is_string());
    app.client()
        .post("/_logbook/api/requests/nope/replay")
        .json(&json!({ "confirm": true }))
        .send()
        .await
        .assert_status(404);

    let curl = app
        .client()
        .get(&format!("/_logbook/api/requests/{id}/curl"))
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(curl.starts_with("curl"), "{curl}");
    assert!(curl.contains("-X POST"), "{curl}");
    assert!(curl.contains("/echo'"), "{curl}");
    assert!(curl.contains(r#"--data-raw '{"n":1}'"#), "{curl}");
}

#[tokio::test]
async fn replays_run_as_the_same_user_with_a_fresh_token() {
    let app = crate::servers::database(builder())
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(LogbookModule::from_config())
        .module(FixturesModule)
        .build()
        .await;
    let user = app
        .get::<AuthService>()
        .await
        .create_user("Priya Verma", "priya@example.com", "password123")
        .await
        .unwrap();
    let token = app.get::<JwtService>().await.sign_access(user.id).unwrap();
    app.client()
        .get("/me")
        .bearer(&token)
        .send()
        .await
        .assert_status(200);
    let logbook = app.get::<Logbook>().await;
    let head = logbook
        .last_request()
        .await
        .unwrap()
        .head()
        .unwrap()
        .clone();
    assert_eq!(head.user_id, Some(user.id.to_string()));
    let Content::Request(request) = &head.content else {
        panic!("a request");
    };
    assert_eq!(
        request.request.as_ref().unwrap().header("authorization"),
        None
    );

    let as_user = logbook
        .replay(&head.id, ReplayRequest::default())
        .await
        .unwrap();
    assert_eq!(as_user.status, 200, "{}", as_user.body);
    assert!(as_user.body.contains(&user.id.to_string()));

    let as_guest = logbook
        .replay(
            &head.id,
            ReplayRequest {
                as_guest: true,
                ..ReplayRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(as_guest.status, 401);
    app.shutdown().await.unwrap();
}

//! A Navree UI page's live stream follows the browser's session (with
//! the app's live hub): a browser signed in on the session cookie switches
//! workspace, and its stream on the hub hears only the workspace it switched
//! to; the switch and signing out end the open streams, so every page
//! reconnects in the right place or not at all.
//!
//! `cargo test -p navree --features sqlite --test ui_live_session`

use std::time::Duration;

use http_body_util::BodyExt;
use navree::access::{AccessModule, AccessOptions};
use navree::auth::{AuthModule, AuthPages, AuthService};
use navree::crypto::{CryptoModule, generate_key};
use navree::db::{DatabaseModule, Db};
use navree::http::body::Body;
use navree::prelude::*;
use navree::realtime::{Live, STREAM_PATH, Topic};
use navree::session::{SessionModule, SessionOptions};
use navree::testing::{TestApp, TestClient, TestResponse};
use navree::ui::UiModule;
use navree::workspace::{NewWorkspace, Workspaces};
use serde_json::{Value, json};

const PASSWORD: &str = "correct horse battery";
const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(300);

async fn app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.resolvers", json!(["session"]))
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(RealtimeModule::from_config())
        .module(UiModule)
        .module(AuthPages::new())
        .build()
        .await
}

/// A stream read off a response, event by event.
struct Stream {
    body: Body,
    buffer: String,
}

impl Stream {
    fn new(response: http::Response<Body>) -> Self {
        assert_eq!(response.status(), 200);
        Stream {
            body: response.into_body(),
            buffer: String::new(),
        }
    }

    /// The `event:` of the next event, or `None` when nothing came within
    /// `wait` or the stream ended.
    async fn next(&mut self, wait: Duration) -> Option<String> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..end + 2).collect();
                if let Some(name) = block.lines().find_map(|line| line.strip_prefix("event: ")) {
                    return Some(name.to_owned());
                }
                continue;
            }
            let frame = tokio::time::timeout_at(deadline, self.body.frame())
                .await
                .ok()??;
            if let Ok(data) = frame.ok()?.into_data() {
                self.buffer.push_str(&String::from_utf8_lossy(&data));
            }
        }
    }

    /// Whether the stream ended (the server closed it) within `wait`.
    async fn ended(&mut self, wait: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            match tokio::time::timeout_at(deadline, self.body.frame()).await {
                Err(_) => return false,
                Ok(None) => return true,
                Ok(Some(Err(_))) => return true,
                Ok(Some(Ok(_))) => {}
            }
        }
    }
}

/// Posts a form with the session's CSRF token.
async fn post(client: &TestClient, path: &str, form: &[(&str, &str)]) -> TestResponse {
    let token = client.cookie("XSRF-TOKEN").expect("the CSRF token");
    let mut form = form.to_vec();
    form.push(("_csrf", token.as_str()));
    client
        .post(path)
        .header("accept", "text/html")
        .form(&form)
        .send()
        .await
}

async fn open(client: &TestClient) -> Stream {
    Stream::new(
        client
            .get(&format!("{STREAM_PATH}?topics=deals"))
            .send_stream()
            .await,
    )
}

// The hub resolves a browser's stream with the session
// resolver, a switch ends the open stream and the next one follows the new
// workspace, and signing out ends it too.
#[tokio::test]
async fn a_browsers_stream_follows_its_session_workspace() {
    let app = app().await;
    let auth = app.get::<AuthService>().await;
    let ada = auth
        .create_user("Anu Lakhotia", "anu@example.com", PASSWORD)
        .await
        .unwrap();
    let db = app.get::<Db>().await;
    let workspaces = app.get::<Workspaces>().await;
    let uow = db.uow(None, None).await;
    let agra = workspaces
        .create(&uow, ada.id, NewWorkspace::named("Agra"))
        .await
        .unwrap();
    let gulati = workspaces
        .create(&uow, ada.id, NewWorkspace::named("Gulati"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let live = app.get::<Live>().await;

    let browser = app.client();
    browser
        .get("/sign-in")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200);
    post(
        &browser,
        "/sign-in",
        &[("email", "anu@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_status(303);
    post(&browser, "/workspace/switch", &[("workspace", "agra")])
        .await
        .assert_status(303);

    let mut in_agra = open(&browser).await;
    assert_eq!(in_agra.next(WAIT).await.as_deref(), Some("ui:live"));
    live.nudge([Topic::workspace(agra.id, "deals")])
        .await
        .unwrap();
    assert_eq!(in_agra.next(WAIT).await.as_deref(), Some("deals"));
    live.nudge([Topic::workspace(gulati.id, "deals")])
        .await
        .unwrap();
    assert_eq!(in_agra.next(QUIET).await, None);

    // The switch closes the open stream; the page's next one is in Gulati.
    post(&browser, "/workspace/switch", &[("workspace", "gulati")])
        .await
        .assert_status(303);
    assert!(in_agra.ended(WAIT).await, "the old stream is closed");
    let mut in_gulati = open(&browser).await;
    assert_eq!(in_gulati.next(WAIT).await.as_deref(), Some("ui:live"));
    live.nudge([Topic::workspace(agra.id, "deals")])
        .await
        .unwrap();
    assert_eq!(in_gulati.next(QUIET).await, None);
    live.nudge([Topic::workspace(gulati.id, "deals")])
        .await
        .unwrap();
    assert_eq!(in_gulati.next(WAIT).await.as_deref(), Some("deals"));

    // Signing out closes it as well.
    post(&browser, "/sign-out", &[]).await.assert_status(303);
    assert!(in_gulati.ended(WAIT).await, "signing out closes the stream");
}

//! Feature `notify`: connections give the tokens of navree-notify's Slack
//! and Teams bot routes.

use navree_integrations::{FakeGrant, Owner};
use navree_notify::{
    ConnectionTokens, Notifiable, Notification, Notifier, NotifyModule, NotifyOptions,
    TeamsMessage, TeamsOptions, TeamsRoute,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::harness;

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

#[tokio::test]
async fn the_module_provides_connection_tokens() {
    let h = harness(|b| b).await;
    h.fake
        .on_token("microsoft", FakeGrant::new().access_token("tenant-token"));
    let connection = h.connect_microsoft(Owner::App, TENANT).await;

    let tokens = h.app.get::<dyn ConnectionTokens>().await;
    let token = tokens.token(connection.id).await.unwrap();

    assert_eq!(token.expose(), "tenant-token");
    assert!(
        tokens.token(Uuid::now_v7()).await.is_err(),
        "an unknown connection has no token"
    );
}

struct Member;

impl Notifiable for Member {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        "1".to_owned()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }
}

#[derive(Serialize, Deserialize)]
struct Nudge {
    route: TeamsRoute,
}

impl Notification for Nudge {
    const TYPE: &'static str = "nudge";

    fn via(&self, _n: &dyn Notifiable) -> Vec<navree_notify::Channel> {
        vec![navree_notify::Channel::MicrosoftTeams]
    }

    fn to_teams(&self, _n: &dyn Notifiable) -> Option<TeamsMessage> {
        Some(TeamsMessage::new("Your training is due today").to(self.route.clone()))
    }
}

#[tokio::test]
async fn a_teams_route_with_a_connection_sends_with_its_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/emea/v3/conversations/a:1Xyz/activities"))
        .and(header("authorization", "Bearer tenant-bot-token"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "1|0001" })))
        .expect(1)
        .mount(&server)
        .await;
    let options = NotifyOptions {
        routes: false,
        teams: TeamsOptions {
            // Only so the driver may call the local server.
            token_url: Some(format!("{}/token", server.uri())),
            ..TeamsOptions::default()
        },
        ..NotifyOptions::default()
    };
    let h = harness(move |b| b.module(NotifyModule::for_root(options))).await;
    h.fake.on_token(
        "microsoft",
        FakeGrant::new().access_token("tenant-bot-token"),
    );
    let connection = h.connect_microsoft(Owner::App, TENANT).await;

    let route = TeamsRoute::conversation(format!("{}/emea/", server.uri()), "a:1Xyz")
        .connection(connection.id);
    h.app
        .get::<Notifier>()
        .await
        .send(&Member, Nudge { route })
        .await
        .unwrap();
    // The mock checks on drop that the activity was posted once, with the
    // connection's token.
}

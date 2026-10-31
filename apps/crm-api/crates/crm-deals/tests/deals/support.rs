//! Helpers shared by the deals tests: a CRM with `DealsModule`, companies
//! and contacts made with their model factories (their own module is
//! another crate), and deals created through the API.

#![allow(dead_code)]

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::time::Duration;

use crm_core::models::{Company, Contact};
use crm_core::{DEAL_UPDATED, DEALS_CHANNEL, REALTIME_KEY};
use crm_deals::DealsModule;
use crm_testkit::{CrmClient, TestCrm, WEB_ORIGIN};
use futures_util::{SinkExt, StreamExt};
use navree::db::Db;
use navree::http::build_router;
use navree::orm::Model;
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

/// A CRM with the deals module and the demo accounts.
pub async fn crm() -> TestCrm {
    TestCrm::builder().module(DealsModule).build().await
}

/// Creates a company owned by `owner`; returns its id.
pub async fn company(crm: &TestCrm, name: &str, owner: Uuid) -> Uuid {
    let db = crm.get::<Db>().await;
    Company::factory()
        .set(Company::NAME, name)
        .set(Company::OWNER_ID, owner)
        .create_one(&*db)
        .await
        .expect("the company is created")
        .id
}

/// Soft-deletes company `id`.
pub async fn delete_company(crm: &TestCrm, id: Uuid) {
    let db = crm.get::<Db>().await;
    Company::query()
        .where_key(id)
        .delete(&*db)
        .await
        .expect("the company is soft-deleted");
}

/// Creates a contact owned by `owner`, at `company_id`; returns its id.
pub async fn contact(
    crm: &TestCrm,
    first_name: &str,
    last_name: &str,
    company_id: Option<Uuid>,
    owner: Uuid,
) -> Uuid {
    let db = crm.get::<Db>().await;
    Contact::factory()
        .set(Contact::FIRST_NAME, first_name)
        .set(Contact::LAST_NAME, last_name)
        .set(Contact::COMPANY_ID, company_id)
        .set(Contact::OWNER_ID, owner)
        .create_one(&*db)
        .await
        .expect("the contact is created")
        .id
}

/// The smallest valid `POST /api/v1/deals` body.
pub fn new_deal(title: &str, company_id: Uuid, amount_cents: i64) -> Value {
    json!({ "title": title, "company_id": company_id, "amount_cents": amount_cents })
}

/// Creates a deal through the API as `client` (asserting a 201); returns
/// the response body.
pub async fn create_deal(client: &CrmClient, body: Value) -> Value {
    client
        .post("/api/v1/deals")
        .json(&body)
        .send()
        .await
        .assert_status(201)
        .json_value()
}

/// The `id` of a deal resource.
pub fn id_of(deal: &Value) -> Uuid {
    serde_json::from_value(deal["id"].clone()).expect("a deal id")
}

/// `POST /api/v1/deals/{id}/move` as `client` (asserting a 200); returns
/// the deal.
pub async fn move_deal(client: &CrmClient, id: Uuid, body: Value) -> Value {
    client
        .post(&format!("/api/v1/deals/{id}/move"))
        .json(&body)
        .send()
        .await
        .assert_status(200)
        .json_value()
}

/// The board as `client` sees it.
pub async fn board(client: &CrmClient) -> Value {
    client
        .get("/api/v1/deals/board")
        .send()
        .await
        .assert_status(200)
        .json_value()
}

/// The titles in board column `stage`, top to bottom.
pub fn column_titles(board: &Value, stage: &str) -> Vec<String> {
    let column = board["stages"]
        .as_array()
        .expect("stages")
        .iter()
        .find(|column| column["stage"] == stage)
        .unwrap_or_else(|| panic!("no `{stage}` column in {board}"));
    column["deals"]
        .as_array()
        .expect("deals")
        .iter()
        .map(|deal| deal["title"].as_str().expect("a title").to_owned())
        .collect()
}

/// The `position` of every deal in board column `stage`, top to bottom.
pub fn column_positions(board: &Value, stage: &str) -> Vec<i64> {
    let column = board["stages"]
        .as_array()
        .expect("stages")
        .iter()
        .find(|column| column["stage"] == stage)
        .unwrap_or_else(|| panic!("no `{stage}` column in {board}"));
    column["deals"]
        .as_array()
        .expect("deals")
        .iter()
        .map(|deal| deal["position"].as_i64().expect("a position"))
        .collect()
}

/// The realtime constants the broadcast tests use.
pub const CHANNEL: &str = DEALS_CHANNEL;
/// The broadcast event.
pub const EVENT: &str = DEAL_UPDATED;
/// The realtime app key.
pub const APP_KEY: &str = REALTIME_KEY;
/// The allowed origin of WebSocket handshakes.
pub const ORIGIN: &str = WEB_ORIGIN;

/// How long a socket waits for an expected frame.
const WAIT: Duration = Duration::from_secs(5);

/// Serves `crm` on `127.0.0.1:0` until it shuts down; returns the address
/// (the in-process test client does not speak WebSocket).
pub async fn serve(crm: &TestCrm) -> SocketAddr {
    let router = build_router(crm.ctx()).await.expect("the router builds");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let addr = listener.local_addr().expect("the bound address");
    let shutdown = crm.ctx().shutdown_token();
    tokio::spawn(
        navree::http::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .into_future(),
    );
    addr
}

/// A received Pusher frame, with `data` decoded.
#[derive(Debug)]
pub struct Frame {
    pub event: String,
    pub channel: Option<String>,
    pub data: Value,
}

/// A Pusher-protocol WebSocket client, as pusher-js in crm-web.
pub struct Socket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    pub socket_id: String,
}

impl Socket {
    /// Connects to the CRM served at `addr` from crm-web's origin and reads
    /// `pusher:connection_established`.
    pub async fn connect(addr: SocketAddr) -> Socket {
        let url = format!("ws://{addr}/app/{APP_KEY}?protocol=7&client=js&version=8.4.0");
        let mut request = url.into_client_request().expect("a WebSocket request");
        request
            .headers_mut()
            .insert("origin", HeaderValue::from_static(ORIGIN));
        let (ws, _) = tokio::time::timeout(WAIT, tokio_tungstenite::connect_async(request))
            .await
            .expect("the handshake in time")
            .expect("the handshake succeeds");
        let mut socket = Socket {
            ws,
            socket_id: String::new(),
        };
        let established = socket.expect("pusher:connection_established").await;
        socket.socket_id = established.data["socket_id"]
            .as_str()
            .expect("a socket id")
            .to_owned();
        socket
    }

    /// Subscribes to private `channel` with a signature from
    /// `POST /broadcasting/auth` as `client`.
    pub async fn join(&mut self, client: &CrmClient, channel: &str) {
        let auth = client
            .post("/broadcasting/auth")
            .json(&json!({ "socket_id": self.socket_id, "channel_name": channel }))
            .send()
            .await
            .assert_status(200)
            .json_value()["auth"]
            .as_str()
            .expect("an auth signature")
            .to_owned();
        let subscribe = json!({
            "event": "pusher:subscribe",
            "data": { "channel": channel, "auth": auth },
        });
        self.ws
            .send(Message::text(subscribe.to_string()))
            .await
            .expect("the subscribe frame is sent");
        let joined = self.expect("pusher_internal:subscription_succeeded").await;
        assert_eq!(joined.channel.as_deref(), Some(channel));
    }

    /// The next frame (WebSocket pings skipped); panics after 5 s.
    pub async fn recv(&mut self) -> Frame {
        let text = tokio::time::timeout(WAIT, async {
            loop {
                match self.ws.next().await {
                    Some(Ok(Message::Text(text))) => return text.to_string(),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    other => panic!("expected a text frame, got {other:?}"),
                }
            }
        })
        .await
        .expect("a frame within 5 s");
        let mut frame: Value = serde_json::from_str(&text).expect("frames are JSON");
        let data = match frame["data"].take() {
            Value::String(encoded) => {
                serde_json::from_str(&encoded).unwrap_or(Value::String(encoded))
            }
            other => other,
        };
        Frame {
            event: frame["event"].as_str().expect("an event").to_owned(),
            channel: frame["channel"].as_str().map(str::to_owned),
            data,
        }
    }

    /// The next frame, which must be `event`.
    pub async fn expect(&mut self, event: &str) -> Frame {
        let frame = self.recv().await;
        assert_eq!(frame.event, event, "{frame:?}");
        frame
    }

    /// No frame arrives within `wait`.
    pub async fn expect_silence(&mut self, wait: Duration) {
        let next = tokio::time::timeout(wait, async {
            loop {
                match self.ws.next().await {
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    other => return other,
                }
            }
        })
        .await;
        if let Ok(frame) = next {
            panic!("expected silence, got {frame:?}");
        }
    }

    /// Closes the connection.
    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}

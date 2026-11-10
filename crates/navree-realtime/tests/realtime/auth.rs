//! `POST /broadcasting/auth`: channel authorizers and signatures.

use http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Server, signature};

const SOCKET: &str = "1234.5678";

#[tokio::test]
async fn auth_endpoint_signs_for_authorized_channel_and_403_otherwise() {
    let server = Server::start().await;
    let user = Uuid::now_v7();
    let stranger = Uuid::now_v7();

    // `crm.deals`: any signed-in user.
    let channel = "private-crm.deals";
    let reply = server.auth(Some(user), SOCKET, channel).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    assert_eq!(
        reply.json,
        json!({"auth": signature(SOCKET, channel, None)})
    );

    // `users.{id}`: only the user themself.
    let own = format!("private-users.{user}");
    let reply = server.auth(Some(user), SOCKET, &own).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    assert_eq!(reply.json["auth"], signature(SOCKET, &own, None));
    let reply = server.auth(Some(stranger), SOCKET, &own).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.json);
    assert_eq!(reply.json["statusCode"], 403, "error body: {}", reply.json);
    assert_eq!(reply.json["error"], "Forbidden", "{}", reply.json);
    assert!(reply.json.get("auth").is_none());

    // No authorizer matches, or the channel is public: nothing to sign.
    for channel in ["private-unknown", "private-users.1.2", "news"] {
        let reply = server.auth(Some(user), SOCKET, channel).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{channel}: {}",
            reply.json
        );
    }
}

#[tokio::test]
async fn auth_endpoint_requires_authentication() {
    let server = Server::start().await;
    let reply = server.auth(None, SOCKET, "private-crm.deals").await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{}", reply.json);
}

#[tokio::test]
async fn auth_endpoint_accepts_json_and_validates_fields() {
    let server = Server::start().await;
    let user = Uuid::now_v7();

    let reply = server
        .auth_json(
            user,
            json!({"socket_id": SOCKET, "channel_name": "private-crm.deals"}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    assert_eq!(
        reply.json["auth"],
        signature(SOCKET, "private-crm.deals", None)
    );

    let reply = server.auth_json(user, json!({})).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.json);
    assert_eq!(
        reply.json,
        json!({
            "statusCode": 400,
            "message": ["channel_name should not be empty", "socket_id should not be empty"],
            "error": "Bad Request",
            "code": "validation_failed",
            "errors": {
                "channel_name": ["channel_name should not be empty"],
                "socket_id": ["socket_id should not be empty"],
            },
        })
    );

    let reply = server
        .auth_json(
            user,
            json!({"socket_id": "abc", "channel_name": "private-crm.deals"}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.json);
    assert_eq!(
        reply.json["errors"],
        json!({"socket_id": ["socket_id must look like 1234.5678"]})
    );

    let reply = server
        .auth_json(
            user,
            json!({"socket_id": SOCKET, "channel_name": "private-bad name"}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.json);
    assert_eq!(
        reply.json["errors"],
        json!({"channel_name": ["channel_name must be a valid channel name"]})
    );
}

#[tokio::test]
async fn presence_auth_returns_signed_channel_data() {
    let server = Server::start().await;
    let user = Uuid::now_v7();
    let channel = "presence-room.7";

    let reply = server.auth(Some(user), SOCKET, channel).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    let channel_data = reply.json["channel_data"].as_str().expect("channel_data");
    let data: Value = serde_json::from_str(channel_data).unwrap();
    assert_eq!(data["user_id"], user.to_string());
    assert_eq!(
        data["user_info"]["name"],
        format!("user {}", &user.to_string()[..4])
    );
    assert_eq!(
        reply.json["auth"],
        signature(SOCKET, channel, Some(channel_data))
    );

    // `Allow` on a presence channel: a member without user_info.
    let reply = server.auth(Some(user), SOCKET, "presence-crm.deals").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    let data: Value = serde_json::from_str(reply.json["channel_data"].as_str().unwrap()).unwrap();
    assert_eq!(data, json!({"user_id": user.to_string()}));
}

#[tokio::test]
async fn failing_authorizer_is_a_server_error() {
    let server = Server::start().await;
    let reply = server
        .auth(Some(Uuid::now_v7()), SOCKET, "private-broken")
        .await;
    assert_eq!(
        reply.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        reply.json
    );
    assert!(
        !reply.json.to_string().contains("the authorizer failed"),
        "internal details stay out of the response: {}",
        reply.json
    );
}

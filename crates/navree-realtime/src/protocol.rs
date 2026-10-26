//! The Pusher protocol (version 7) on the wire: frames the server sends,
//! messages clients send, error and close codes, and channel signatures.

use navree_crypto::{constant_time_eq, hmac_sha256};
use serde::Serialize;
use serde_json::{Map, Value};

/// `pusher:error` code: the app key in the URL is unknown (no reconnect).
pub(crate) const APP_DOES_NOT_EXIST: u16 = 4001;
/// `pusher:error` / close code: the connection or subscription is not
/// authorized (bad origin, bad signature, disconnected by the app).
pub(crate) const UNAUTHORIZED: u16 = 4009;
/// Close code: the client did not read its messages fast enough
/// (reconnect after a back-off).
pub(crate) const OVER_CAPACITY: u16 = 4100;
/// Close code: reconnect now (the server is shutting down).
pub(crate) const RECONNECT: u16 = 4200;
/// Close code: the client did not answer the server's ping (reconnect now).
pub(crate) const PONG_TIMEOUT: u16 = 4201;

/// A server frame as sent on the wire.
#[derive(Serialize)]
struct Frame<'a> {
    event: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<&'a str>,
    data: &'a str,
}

/// A server frame: `{"event", "channel"?, "data"}` with `data` a JSON-encoded string.
pub(crate) fn frame(event: &str, channel: Option<&str>, data: &str) -> String {
    serde_json::to_string(&Frame {
        event,
        channel,
        data,
    })
    .expect("a frame of strings always serializes")
}

/// A `pusher:error` frame (`code` is `null` for errors without a code).
pub(crate) fn error_frame(code: Option<u16>, message: &str) -> String {
    let data = serde_json::json!({ "code": code, "message": message });
    frame("pusher:error", None, &data.to_string())
}

/// The string the subscription signature covers.
fn signed_string(socket_id: &str, channel: &str, channel_data: Option<&str>) -> String {
    match channel_data {
        Some(data) => format!("{socket_id}:{channel}:{data}"),
        None => format!("{socket_id}:{channel}"),
    }
}

/// The `auth` string for a private or presence subscription:
/// `<key>:<hex HMAC-SHA256(secret, socket_id:channel[:channel_data])>`.
pub(crate) fn sign(
    key: &str,
    secret: &[u8],
    socket_id: &str,
    channel: &str,
    channel_data: Option<&str>,
) -> String {
    let signature = hmac_sha256(
        secret,
        signed_string(socket_id, channel, channel_data).as_bytes(),
    );
    format!("{key}:{}", hex::encode(signature))
}

/// Whether `auth` is the valid signature for the subscription (constant time).
pub(crate) fn verify(
    key: &str,
    secret: &[u8],
    socket_id: &str,
    channel: &str,
    channel_data: Option<&str>,
    auth: &str,
) -> bool {
    let expected = sign(key, secret, socket_id, channel, channel_data);
    constant_time_eq(expected.as_bytes(), auth.as_bytes())
}

/// Whether `socket_id` has the form `<digits>.<digits>`.
pub(crate) fn is_valid_socket_id(socket_id: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    socket_id
        .split_once('.')
        .is_some_and(|(a, b)| digits(a) && digits(b))
}

/// A message from a client.
#[derive(Debug, PartialEq)]
pub(crate) struct ClientMessage {
    /// The event name, e.g. `pusher:subscribe`.
    pub(crate) event: String,
    /// The data as an object (a JSON-encoded string is decoded; anything
    /// else becomes an empty object).
    pub(crate) data: Map<String, Value>,
}

impl ClientMessage {
    /// Parses a text frame; `None` when it is not a JSON object with an `event`.
    pub(crate) fn parse(text: &str) -> Option<ClientMessage> {
        let Value::Object(mut message) = serde_json::from_str(text).ok()? else {
            return None;
        };
        let Some(Value::String(event)) = message.remove("event") else {
            return None;
        };
        let data = match message.remove("data") {
            Some(Value::Object(data)) => data,
            Some(Value::String(encoded)) => match serde_json::from_str(&encoded) {
                Ok(Value::Object(data)) => data,
                _ => Map::new(),
            },
            _ => Map::new(),
        };
        Some(ClientMessage { event, data })
    }

    /// The string field `name` of the data.
    pub(crate) fn str(&self, name: &str) -> Option<&str> {
        self.data.get(name).and_then(Value::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The example credentials from the Pusher protocol documentation.
    const KEY: &str = "278d425bdf160c739803";
    const SECRET: &[u8] = b"7ad3773142a6692b25b8";

    #[test]
    fn private_signature_matches_the_pusher_documentation() {
        let auth = sign(KEY, SECRET, "1234.1234", "private-foobar", None);
        assert_eq!(
            auth,
            "278d425bdf160c739803:58df8b0c36d6982b82c3ecf6b4662e34fe8c25bba48f5369f135bf843651c3a4"
        );
        assert!(verify(
            KEY,
            SECRET,
            "1234.1234",
            "private-foobar",
            None,
            &auth
        ));
    }

    #[test]
    fn presence_signature_covers_the_channel_data() {
        let data = r#"{"user_id":10,"user_info":{"name":"Mr. Channels"}}"#;
        let auth = sign(KEY, SECRET, "1234.1234", "presence-foobar", Some(data));
        // Computed independently with Python's `hmac` module.
        assert_eq!(
            auth,
            "278d425bdf160c739803:31935e7d86dba64c2a90aed31fdc61869f9b22ba9d8863bba239c03ca481bc80"
        );
        assert!(verify(
            KEY,
            SECRET,
            "1234.1234",
            "presence-foobar",
            Some(data),
            &auth
        ));
        let other = r#"{"user_id":11,"user_info":{"name":"Mr. Channels"}}"#;
        assert!(!verify(
            KEY,
            SECRET,
            "1234.1234",
            "presence-foobar",
            Some(other),
            &auth
        ));
    }

    #[test]
    fn verify_rejects_other_sockets_channels_keys_and_garbage() {
        let auth = sign(KEY, SECRET, "1234.1234", "private-foobar", None);
        assert!(!verify(
            KEY,
            SECRET,
            "1234.1235",
            "private-foobar",
            None,
            &auth
        ));
        assert!(!verify(
            KEY,
            SECRET,
            "1234.1234",
            "private-foobaz",
            None,
            &auth
        ));
        assert!(!verify(
            "other-key",
            SECRET,
            "1234.1234",
            "private-foobar",
            None,
            &auth
        ));
        assert!(!verify(
            KEY,
            b"other",
            "1234.1234",
            "private-foobar",
            None,
            &auth
        ));
        for garbage in [
            "",
            ":",
            "278d425bdf160c739803",
            "278d425bdf160c739803:",
            "x:y:z",
        ] {
            assert!(!verify(
                KEY,
                SECRET,
                "1234.1234",
                "private-foobar",
                None,
                garbage
            ));
        }
    }

    #[test]
    fn socket_ids_are_two_numbers() {
        assert!(is_valid_socket_id("1234.1234"));
        assert!(is_valid_socket_id("0.9"));
        for bad in [
            "", "1234", "1234.", ".1234", "12a4.1", "1.2.3", "1 .2", "-1.2",
        ] {
            assert!(!is_valid_socket_id(bad), "{bad}");
        }
    }

    #[test]
    fn frames_encode_data_as_a_json_string() {
        let text = frame("deal.updated", Some("private-crm.deals"), r#"{"id":1}"#);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value,
            json!({"event": "deal.updated", "channel": "private-crm.deals", "data": "{\"id\":1}"})
        );
        let value: Value = serde_json::from_str(&frame("pusher:pong", None, "{}")).unwrap();
        assert_eq!(value, json!({"event": "pusher:pong", "data": "{}"}));
    }

    #[test]
    fn error_frames_carry_code_and_message() {
        let value: Value = serde_json::from_str(&error_frame(Some(4009), "nope")).unwrap();
        assert_eq!(value["event"], "pusher:error");
        let data: Value = serde_json::from_str(value["data"].as_str().unwrap()).unwrap();
        assert_eq!(data, json!({"code": 4009, "message": "nope"}));
        let value: Value = serde_json::from_str(&error_frame(None, "bad")).unwrap();
        let data: Value = serde_json::from_str(value["data"].as_str().unwrap()).unwrap();
        assert_eq!(data, json!({"code": null, "message": "bad"}));
    }

    #[test]
    fn client_messages_accept_object_or_encoded_data() {
        let message =
            ClientMessage::parse(r#"{"event":"pusher:subscribe","data":{"channel":"news"}}"#)
                .unwrap();
        assert_eq!(message.event, "pusher:subscribe");
        assert_eq!(message.str("channel"), Some("news"));

        let message = ClientMessage::parse(
            r#"{"event":"pusher:subscribe","data":"{\"channel\":\"news\",\"auth\":\"k:s\"}"}"#,
        )
        .unwrap();
        assert_eq!(message.str("channel"), Some("news"));
        assert_eq!(message.str("auth"), Some("k:s"));

        let message = ClientMessage::parse(r#"{"event":"pusher:ping"}"#).unwrap();
        assert!(message.data.is_empty());
        let message = ClientMessage::parse(r#"{"event":"pusher:ping","data":"nope"}"#).unwrap();
        assert!(message.data.is_empty());

        for bad in ["", "null", "[]", "{}", r#"{"event":1}"#, "{not json"] {
            assert_eq!(ClientMessage::parse(bad), None, "{bad}");
        }
    }
}

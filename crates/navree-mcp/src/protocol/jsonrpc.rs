//! JSON-RPC 2.0: parsing incoming messages (with a nesting depth limit),
//! building answers and notifications, and the error codes MCP uses.

use std::fmt;

use serde_json::{Map, Value, json};

/// Invalid JSON.
pub(crate) const PARSE_ERROR: i64 = -32700;
/// Not a valid JSON-RPC message.
pub(crate) const INVALID_REQUEST: i64 = -32600;
/// No such method (or not on this protocol version).
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
/// Invalid parameters.
pub(crate) const INVALID_PARAMS: i64 = -32602;
/// An error inside the server.
pub(crate) const INTERNAL_ERROR: i64 = -32603;
/// No such resource (protocols before 2026-07-28).
pub(crate) const RESOURCE_NOT_FOUND: i64 = -32002;
/// A standard HTTP header does not match the body.
pub(crate) const HEADER_MISMATCH: i64 = -32020;
/// The request names a protocol version the server does not speak.
pub(crate) const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// How deeply arrays and objects may nest in an incoming message. Real MCP
/// messages stay well under it; a deeper one is refused before it is parsed,
/// so it cannot exhaust the stack or the parser.
pub const MAX_JSON_DEPTH: usize = 64;

/// A JSON-RPC error object.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RpcError {
    pub(crate) code: i64,
    pub(crate) message: String,
    pub(crate) data: Option<Value>,
}

impl RpcError {
    pub(crate) fn new(code: i64, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }

    pub(crate) fn parse_error(message: impl Into<String>) -> Self {
        Self::new(PARSE_ERROR, message, None)
    }

    pub(crate) fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message, None)
    }

    pub(crate) fn method_not_found(method: &str) -> Self {
        Self::new(
            METHOD_NOT_FOUND,
            "Method not found",
            Some(json!({ "method": method })),
        )
    }

    pub(crate) fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message, None)
    }

    /// An internal error with a fixed message: what went wrong stays in the
    /// server's log.
    pub(crate) fn internal() -> Self {
        Self::new(INTERNAL_ERROR, "Internal error", None)
    }

    pub(crate) fn to_value(&self) -> Value {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        error
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

/// A request: it gets an answer.
#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub(crate) id: Value,
    pub(crate) method: String,
    pub(crate) params: Map<String, Value>,
}

/// A notification: no answer.
#[derive(Clone, Debug)]
pub(crate) struct Notification {
    pub(crate) method: String,
    pub(crate) params: Map<String, Value>,
}

/// One incoming message.
#[derive(Clone, Debug)]
pub(crate) enum Message {
    Request(Request),
    Notification(Notification),
    /// A response or error from the client. The server sends no requests,
    /// so these answer nothing.
    Reply,
}

/// A parsed body: one message or a batch (a JSON array). Batch entries that
/// are not valid messages are kept as the error to answer them with.
#[derive(Debug)]
pub(crate) enum Parsed {
    Single(Message),
    Batch(Vec<Result<Message, (Value, RpcError)>>),
}

/// Parses `bytes` as one JSON-RPC message or a batch. Fails with the error
/// to answer (and the id to answer it to, `null` when unknown).
pub(crate) fn parse(bytes: &[u8]) -> Result<Parsed, (Value, RpcError)> {
    if exceeds_depth(bytes, MAX_JSON_DEPTH) {
        return Err((
            Value::Null,
            RpcError::invalid_request(format!(
                "Invalid Request: JSON nested deeper than {MAX_JSON_DEPTH} levels"
            )),
        ));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| (Value::Null, RpcError::parse_error("Parse error")))?;
    match value {
        Value::Array(items) if items.is_empty() => Err((
            Value::Null,
            RpcError::invalid_request("Invalid Request: empty batch"),
        )),
        Value::Array(items) => Ok(Parsed::Batch(items.into_iter().map(message).collect())),
        value => message(value).map(Parsed::Single),
    }
}

/// One message from its JSON value.
pub(crate) fn message(value: Value) -> Result<Message, (Value, RpcError)> {
    let Value::Object(mut object) = value else {
        return Err((
            Value::Null,
            RpcError::invalid_request("Invalid Request: not a JSON-RPC message"),
        ));
    };
    let id = object.remove("id");
    let answer_id = id.clone().filter(valid_id).unwrap_or(Value::Null);
    if object.get("jsonrpc") != Some(&Value::String("2.0".to_owned())) {
        return Err((
            answer_id,
            RpcError::invalid_request("Invalid Request: jsonrpc must be \"2.0\""),
        ));
    }
    let Some(method) = object.remove("method") else {
        if object.contains_key("result") || object.contains_key("error") {
            return Ok(Message::Reply);
        }
        return Err((
            answer_id,
            RpcError::invalid_request("Invalid Request: no method"),
        ));
    };
    let Value::String(method) = method else {
        return Err((
            answer_id,
            RpcError::invalid_request("Invalid Request: the method must be a string"),
        ));
    };
    let params = match object.remove("params") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(params)) => params,
        Some(_) => {
            return Err((
                answer_id,
                RpcError::invalid_params("Invalid params: params must be an object"),
            ));
        }
    };
    match id {
        None => Ok(Message::Notification(Notification { method, params })),
        Some(id) if valid_id(&id) => Ok(Message::Request(Request { id, method, params })),
        Some(_) => Err((
            Value::Null,
            RpcError::invalid_request("Invalid Request: the id must be a string or an integer"),
        )),
    }
}

/// MCP request ids are strings or integers (never null).
fn valid_id(id: &Value) -> bool {
    match id {
        Value::String(_) => true,
        Value::Number(number) => number.is_i64() || number.is_u64(),
        _ => false,
    }
}

/// Whether arrays and objects in `bytes` nest deeper than `max` (outside
/// strings). Runs before parsing, in one pass.
pub(crate) fn exceeds_depth(bytes: &[u8], max: usize) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > max {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// The answer to request `id`.
pub(crate) fn result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The error answer to request `id` (`null` when it is unknown).
pub(crate) fn error(id: &Value, error: &RpcError) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": error.to_value() })
}

/// A notification; `params` is left out when `None`.
pub(crate) fn notification(method: &str, params: Option<Value>) -> Value {
    let mut message = json!({ "jsonrpc": "2.0", "method": method });
    if let Some(params) = params {
        message["params"] = params;
    }
    message
}

/// Whether a message is an answer (a result or an error).
pub(crate) fn is_answer(message: &Value) -> bool {
    message.get("method").is_none()
        && (message.get("result").is_some() || message.get("error").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(text: &str) -> Message {
        match parse(text.as_bytes()).unwrap() {
            Parsed::Single(message) => message,
            Parsed::Batch(_) => panic!("a batch"),
        }
    }

    #[test]
    fn requests_notifications_and_replies() {
        let Message::Request(request) =
            single(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list","params":{"cursor":"x"}}"#)
        else {
            panic!("a request");
        };
        assert_eq!(
            (request.id, request.method.as_str()),
            (json!(7), "tools/list")
        );
        assert_eq!(request.params["cursor"], "x");
        assert!(matches!(
            single(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            Message::Notification(_)
        ));
        assert!(matches!(
            single(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#),
            Message::Reply
        ));
        let Message::Request(request) = single(r#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#)
        else {
            panic!("a request");
        };
        assert!(request.params.is_empty());
    }

    #[test]
    fn invalid_messages_name_their_error() {
        let code = |text: &str| parse(text.as_bytes()).unwrap_err().1.code;
        assert_eq!(code("{"), PARSE_ERROR);
        assert_eq!(code("[]"), INVALID_REQUEST);
        assert_eq!(code("3"), INVALID_REQUEST);
        assert_eq!(code(r#"{"id":1,"method":"ping"}"#), INVALID_REQUEST);
        assert_eq!(
            code(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#),
            INVALID_REQUEST
        );
        assert_eq!(
            code(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#),
            INVALID_REQUEST
        );
        assert_eq!(
            code(r#"{"jsonrpc":"2.0","id":1.5,"method":"ping"}"#),
            INVALID_REQUEST
        );
        assert_eq!(
            code(r#"{"jsonrpc":"2.0","id":1,"method":3}"#),
            INVALID_REQUEST
        );
        assert_eq!(
            code(r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":[1]}"#),
            INVALID_PARAMS
        );
        // The id of an invalid request is kept when it is valid.
        let (id, _) = parse(br#"{"jsonrpc":"1.0","id":"x","method":"ping"}"#).unwrap_err();
        assert_eq!(id, "x");
    }

    #[test]
    fn batches_keep_each_entry() {
        let Parsed::Batch(entries) =
            parse(br#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"bad":true}]"#).unwrap()
        else {
            panic!("a batch");
        };
        assert!(entries[0].is_ok());
        assert!(entries[1].is_err());
    }

    #[test]
    fn deep_nesting_is_refused_before_parsing() {
        let deep = format!("{}{}", "[".repeat(65), "]".repeat(65));
        assert!(exceeds_depth(deep.as_bytes(), MAX_JSON_DEPTH));
        assert_eq!(parse(deep.as_bytes()).unwrap_err().1.code, INVALID_REQUEST);
        let fine = format!("{}{}", "[".repeat(64), "]".repeat(64));
        assert!(!exceeds_depth(fine.as_bytes(), MAX_JSON_DEPTH));
        // Brackets inside strings do not count, escaped quotes included.
        let text = format!(r#"{{"a":"{}\"{}"}}"#, "[".repeat(100), "{".repeat(100));
        assert!(!exceeds_depth(text.as_bytes(), MAX_JSON_DEPTH));
    }

    #[test]
    fn answers_and_notifications() {
        assert_eq!(
            result(&json!(1), json!({})),
            json!({ "jsonrpc": "2.0", "id": 1, "result": {} })
        );
        let answer = error(&Value::Null, &RpcError::parse_error("Parse error"));
        assert_eq!(
            answer["error"],
            json!({ "code": -32700, "message": "Parse error" })
        );
        assert!(is_answer(&answer));
        let note = notification("notifications/tools/list_changed", None);
        assert!(note.get("params").is_none());
        assert!(!is_answer(&note));
    }
}

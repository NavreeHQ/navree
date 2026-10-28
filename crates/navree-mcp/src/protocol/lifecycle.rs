//! The lifecycle rules every transport shares: what `initialize` and
//! `server/discover` answer, which requests need `_meta` and which
//! protocol version they run at, which methods exist on which protocol,
//! and how results and errors are shaped for that protocol.

use serde_json::{Map, Value, json};

use super::jsonrpc::{
    INVALID_PARAMS, RESOURCE_NOT_FOUND, Request, RpcError, UNSUPPORTED_PROTOCOL_VERSION,
};
use super::{Implementation, STATELESS_VERSION, SUPPORTED_VERSIONS, is_supported};

/// `_meta` key: the request's protocol version (stateless protocol).
pub(crate) const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// `_meta` key: the client's name and version (stateless protocol).
pub(crate) const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
/// `_meta` key: the client's capabilities (stateless protocol).
pub(crate) const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
/// `_meta` key of results: the server's name and version.
pub(crate) const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
/// `_meta` key of `subscriptions/listen` messages: the listen request's id.
pub(crate) const META_SUBSCRIPTION_ID: &str = "io.modelcontextprotocol/subscriptionId";

/// What `initialize` and `server/discover` say about the server.
#[derive(Clone, Debug)]
pub(crate) struct ServerInfo {
    pub(crate) implementation: Implementation,
    pub(crate) instructions: Option<String>,
    /// The `capabilities` object.
    pub(crate) capabilities: Value,
}

impl ServerInfo {
    /// The `initialize` result at the agreed `version`.
    pub(crate) fn initialize_result(&self, version: &str) -> Value {
        let mut result = json!({
            "protocolVersion": version,
            "capabilities": self.capabilities,
            "serverInfo": self.implementation,
        });
        if let Some(instructions) = &self.instructions {
            result["instructions"] = json!(instructions);
        }
        result
    }

    /// The `server/discover` result: every version, the capabilities, and
    /// the server's name in `_meta`.
    pub(crate) fn discover_result(&self) -> Value {
        let mut result = json!({
            "resultType": "complete",
            "supportedVersions": SUPPORTED_VERSIONS,
            "capabilities": self.capabilities,
            "ttlMs": 0,
            "cacheScope": "private",
            "_meta": { META_SERVER_INFO: self.implementation },
        });
        if let Some(instructions) = &self.instructions {
            result["instructions"] = json!(instructions);
        }
        result
    }

    /// Whether the capabilities say `capability.flag` is true
    /// (`tools.listChanged`, `resources.subscribe`, ...).
    pub(crate) fn advertises(&self, capability: &str, flag: &str) -> bool {
        self.capabilities[capability][flag] == json!(true)
    }
}

/// The request's `_meta` object.
pub(crate) fn meta(params: &Map<String, Value>) -> Option<&Map<String, Value>> {
    params.get("_meta").and_then(Value::as_object)
}

/// The protocol version in the request's `_meta`.
pub(crate) fn meta_version(params: &Map<String, Value>) -> Option<&str> {
    meta(params)?.get(META_PROTOCOL_VERSION)?.as_str()
}

/// The `_meta` keys the stateless protocol needs that the request lacks
/// (or has malformed).
pub(crate) fn missing_meta(params: &Map<String, Value>) -> Vec<&'static str> {
    let meta = meta(params);
    let mut missing = Vec::new();
    if meta
        .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
        .and_then(Value::as_str)
        .is_none()
    {
        missing.push(META_PROTOCOL_VERSION);
    }
    if !meta
        .and_then(|meta| meta.get(META_CLIENT_CAPABILITIES))
        .is_some_and(Value::is_object)
    {
        missing.push(META_CLIENT_CAPABILITIES);
    }
    missing
}

/// Whether the request carries everything the stateless protocol needs.
pub(crate) fn has_complete_meta(params: &Map<String, Value>) -> bool {
    missing_meta(params).is_empty()
}

/// The client's name and version from `clientInfo` (an `initialize`
/// parameter, or the stateless protocol's `_meta` entry).
pub(crate) fn client_info(info: Option<&Value>) -> Option<(String, String)> {
    let info = info?;
    Some((
        info.get("name")?.as_str()?.to_owned(),
        info.get("version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    ))
}

/// The client's name and version from the request's `_meta`.
pub(crate) fn meta_client(params: &Map<String, Value>) -> Option<(String, String)> {
    client_info(meta(params)?.get(META_CLIENT_INFO))
}

/// The progress token the request asked for, a string or a number.
pub(crate) fn progress_token(params: &Map<String, Value>) -> Option<Value> {
    meta(params)?
        .get("progressToken")
        .filter(|token| token.is_string() || token.is_number())
        .cloned()
}

/// How a request runs: the protocol version its answer follows and whether
/// it belongs to a handshake lifecycle (`ping`, `resources/subscribe`) or
/// the stateless one (`subscriptions/listen`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub(crate) version: String,
    pub(crate) legacy: bool,
}

impl Prepared {
    /// Whether the answer follows the stateless protocol's shapes.
    pub(crate) fn stateless_shapes(&self) -> bool {
        self.version.as_str() >= STATELESS_VERSION
    }
}

/// The checks every request (other than `initialize`) passes before it
/// runs. `version` is the session's agreed version (or, for a request
/// outside any session, the `MCP-Protocol-Version` header's);
/// `inline` says the connection has no handshake, so every request must
/// carry its own `_meta`.
pub(crate) fn prepare(
    request: &Request,
    version: Option<&str>,
    inline: bool,
) -> Result<Prepared, RpcError> {
    let requested = meta_version(&request.params);
    if let Some(requested) = requested
        && !is_supported(requested)
    {
        return Err(unsupported_version(requested));
    }
    let needs_meta = request.method == "server/discover"
        || inline
        || requested.is_some_and(|version| version >= STATELESS_VERSION);
    if needs_meta {
        let missing = missing_meta(&request.params);
        if !missing.is_empty() {
            return Err(missing_meta_error(&missing));
        }
    }
    let version = requested.or(version).unwrap_or(STATELESS_VERSION);
    Ok(Prepared {
        version: version.to_owned(),
        legacy: !needs_meta && version < STATELESS_VERSION,
    })
}

/// The `-32602` answer to a request without its stateless `_meta`.
pub(crate) fn missing_meta_error(missing: &[&str]) -> RpcError {
    RpcError::invalid_params(format!(
        "request _meta is missing or has malformed required fields: {}",
        missing.join(", ")
    ))
}

/// The `-32022` answer to a request naming a version the server does not
/// speak.
pub(crate) fn unsupported_version(requested: &str) -> RpcError {
    RpcError::new(
        UNSUPPORTED_PROTOCOL_VERSION,
        "Unsupported protocol version",
        Some(json!({ "requested": requested, "supported": SUPPORTED_VERSIONS })),
    )
}

/// Refuses methods that do not exist on the request's lifecycle: `ping`,
/// `resources/subscribe` and `resources/unsubscribe` belong to the
/// handshake protocols, `subscriptions/listen` to the stateless one.
pub(crate) fn gate(method: &str, prepared: &Prepared) -> Result<(), RpcError> {
    let exists = match method {
        "ping" | "resources/subscribe" | "resources/unsubscribe" => prepared.legacy,
        "subscriptions/listen" => !prepared.legacy,
        _ => true,
    };
    if exists {
        Ok(())
    } else {
        Err(RpcError::method_not_found(method))
    }
}

/// Results whose `resultType` the stateless protocol requires.
const TYPED_RESULTS: &[&str] = &[
    "tools/list",
    "tools/call",
    "prompts/list",
    "prompts/get",
    "resources/list",
    "resources/templates/list",
    "resources/read",
    "completion/complete",
];

/// Results the stateless protocol lets clients cache.
const CACHEABLE_RESULTS: &[&str] = &[
    "tools/list",
    "prompts/list",
    "resources/list",
    "resources/templates/list",
    "resources/read",
];

/// Shapes a result for the request's protocol: on the stateless protocol,
/// `resultType: "complete"` and, on cacheable results, `ttlMs: 0` and
/// `cacheScope: "private"` unless the handler set them.
pub(crate) fn shape_result(method: &str, prepared: &Prepared, result: &mut Value) {
    if !prepared.stateless_shapes() {
        return;
    }
    let Some(object) = result.as_object_mut() else {
        return;
    };
    if TYPED_RESULTS.contains(&method) {
        object
            .entry("resultType")
            .or_insert_with(|| json!("complete"));
    }
    if CACHEABLE_RESULTS.contains(&method) {
        object.entry("ttlMs").or_insert_with(|| json!(0));
        object
            .entry("cacheScope")
            .or_insert_with(|| json!("private"));
    }
}

/// Shapes an error for the request's protocol: the stateless protocol
/// reports a missing resource as invalid params.
pub(crate) fn shape_error(prepared: &Prepared, error: &mut RpcError) {
    if prepared.stateless_shapes() && error.code == RESOURCE_NOT_FOUND {
        error.code = INVALID_PARAMS;
    }
}

/// The notifications a `subscriptions/listen` request asks for (and, once
/// accepted, gets).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    pub(crate) tools: bool,
    pub(crate) prompts: bool,
    pub(crate) resources: bool,
    pub(crate) resource_subscriptions: Vec<String>,
}

impl Filter {
    /// The filter in a listen request's `notifications`.
    pub(crate) fn requested(params: &Map<String, Value>) -> Filter {
        let notifications = params.get("notifications");
        let flag = |key: &str| {
            notifications
                .and_then(|value| value.get(key))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        };
        Filter {
            tools: flag("toolsListChanged"),
            prompts: flag("promptsListChanged"),
            resources: flag("resourcesListChanged"),
            resource_subscriptions: notifications
                .and_then(|value| value.get("resourceSubscriptions"))
                .and_then(Value::as_array)
                .map(|uris| {
                    uris.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// What the server accepts of it: what its capabilities advertise.
    pub(crate) fn accepted(&self, info: &ServerInfo) -> Filter {
        Filter {
            tools: self.tools && info.advertises("tools", "listChanged"),
            prompts: self.prompts && info.advertises("prompts", "listChanged"),
            resources: self.resources && info.advertises("resources", "listChanged"),
            resource_subscriptions: if info.advertises("resources", "subscribe") {
                self.resource_subscriptions.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// Its protocol form (the acknowledgment's `notifications`).
    pub(crate) fn to_value(&self) -> Value {
        let mut value = Map::new();
        if self.tools {
            value.insert("toolsListChanged".to_owned(), json!(true));
        }
        if self.prompts {
            value.insert("promptsListChanged".to_owned(), json!(true));
        }
        if self.resources {
            value.insert("resourcesListChanged".to_owned(), json!(true));
        }
        if !self.resource_subscriptions.is_empty() {
            value.insert(
                "resourceSubscriptions".to_owned(),
                json!(self.resource_subscriptions),
            );
        }
        Value::Object(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::jsonrpc::METHOD_NOT_FOUND;

    fn request(method: &str, params: Value) -> Request {
        Request {
            id: json!(1),
            method: method.to_owned(),
            params: params.as_object().cloned().unwrap_or_default(),
        }
    }

    fn full_meta(version: &str) -> Value {
        json!({ "_meta": {
            META_PROTOCOL_VERSION: version,
            META_CLIENT_CAPABILITIES: {},
        }})
    }

    #[test]
    fn session_requests_run_at_the_session_version() {
        let prepared = prepare(&request("tools/list", json!({})), Some("2025-06-18"), false);
        assert_eq!(
            prepared.unwrap(),
            Prepared {
                version: "2025-06-18".to_owned(),
                legacy: true
            }
        );
    }

    #[test]
    fn stateless_requests_need_their_meta() {
        let error = prepare(&request("tools/list", json!({})), None, true).unwrap_err();
        assert_eq!(error.code, INVALID_PARAMS);
        assert!(error.message.contains(META_PROTOCOL_VERSION), "{error}");
        assert!(error.message.contains(META_CLIENT_CAPABILITIES), "{error}");
        let prepared =
            prepare(&request("tools/list", full_meta(STATELESS_VERSION)), None, true).unwrap();
        assert!(!prepared.legacy);
        assert!(prepared.stateless_shapes());
        // Discovery always needs it.
        assert!(prepare(&request("server/discover", json!({})), Some("2025-11-25"), false).is_err());
    }

    #[test]
    fn unknown_versions_are_refused() {
        let error = prepare(&request("tools/list", full_meta("2099-01-01")), None, true).unwrap_err();
        assert_eq!(error.code, UNSUPPORTED_PROTOCOL_VERSION);
        assert_eq!(error.data.unwrap()["requested"], "2099-01-01");
    }

    #[test]
    fn methods_belong_to_their_lifecycle() {
        let legacy = Prepared {
            version: "2025-11-25".to_owned(),
            legacy: true,
        };
        let stateless = Prepared {
            version: STATELESS_VERSION.to_owned(),
            legacy: false,
        };
        assert!(gate("ping", &legacy).is_ok());
        assert_eq!(gate("ping", &stateless).unwrap_err().code, METHOD_NOT_FOUND);
        assert!(gate("subscriptions/listen", &stateless).is_ok());
        assert!(gate("subscriptions/listen", &legacy).is_err());
        assert!(gate("resources/subscribe", &stateless).is_err());
    }

    #[test]
    fn stateless_results_get_their_type_and_cache_hints() {
        let stateless = Prepared {
            version: STATELESS_VERSION.to_owned(),
            legacy: false,
        };
        let mut listed = json!({ "tools": [] });
        shape_result("tools/list", &stateless, &mut listed);
        assert_eq!(
            listed,
            json!({ "tools": [], "resultType": "complete", "ttlMs": 0, "cacheScope": "private" })
        );
        let mut called = json!({ "content": [] });
        shape_result("tools/call", &stateless, &mut called);
        assert_eq!(called, json!({ "content": [], "resultType": "complete" }));
        let mut legacy = json!({ "tools": [] });
        shape_result(
            "tools/list",
            &Prepared {
                version: "2025-11-25".to_owned(),
                legacy: true,
            },
            &mut legacy,
        );
        assert_eq!(legacy, json!({ "tools": [] }));
        let mut missing = RpcError::new(RESOURCE_NOT_FOUND, "Resource not found", None);
        shape_error(&stateless, &mut missing);
        assert_eq!(missing.code, INVALID_PARAMS);
    }

    #[test]
    fn listen_filters_keep_what_is_advertised() {
        let info = ServerInfo {
            implementation: Implementation::new("x", "1"),
            instructions: None,
            capabilities: json!({ "tools": { "listChanged": true }, "resources": { "subscribe": true } }),
        };
        let requested = Filter::requested(
            json!({ "notifications": {
                "toolsListChanged": true,
                "promptsListChanged": true,
                "resourceSubscriptions": ["a://1"],
            }})
            .as_object()
            .unwrap(),
        );
        let accepted = requested.accepted(&info);
        assert_eq!(
            accepted.to_value(),
            json!({ "toolsListChanged": true, "resourceSubscriptions": ["a://1"] })
        );
    }
}

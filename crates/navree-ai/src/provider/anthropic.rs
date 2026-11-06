//! The Anthropic Messages API (`POST /v1/messages`).

use std::collections::BTreeMap;

use futures_util::StreamExt;
use navree_contracts::Secret;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde_json::{Value, json};

use super::wire::{self, SseEvent};
use super::{
    Capabilities, ChatRequest, ChatResponse, ChunkStream, Driver, StreamChunk, ToolChoice,
};
use crate::error::ProviderError;
use crate::message::{AttachmentKind, Message, Part, Role, ToolCall};
use crate::options::ProviderConfig;
use crate::response::{Finish, Usage};
use crate::schema;

/// The API version sent with every request.
pub(crate) const API_VERSION: &str = "2026-10-21";

/// The vendor's address.
const DEFAULT_URL: &str = "https://api.anthropic.com";

/// The driver name reasoning parts are tagged with.
pub(crate) const DRIVER: &str = "anthropic";

/// An Anthropic account.
pub(crate) struct Anthropic {
    name: String,
    api_key: Option<Secret<String>>,
    url: String,
    http: HttpClient,
}

impl Anthropic {
    pub(crate) fn new(name: &str, config: &ProviderConfig, http: HttpClient) -> Self {
        Self {
            name: name.to_owned(),
            api_key: config
                .api_key
                .clone()
                .filter(|key| !key.expose().is_empty()),
            url: config
                .url
                .clone()
                .unwrap_or_else(|| DEFAULT_URL.to_owned())
                .trim_end_matches('/')
                .to_owned(),
            http,
        }
    }

    /// The request, or a key error.
    fn request(&self, body: &Value) -> Result<navree_http_client::RequestBuilder, ProviderError> {
        let key = self.api_key.as_ref().ok_or_else(|| ProviderError::Auth {
            message: format!(
                "no API key: set APP_AI__PROVIDERS__{}__API_KEY",
                self.name.to_uppercase()
            ),
        })?;
        Ok(self
            .http
            .post(format!("{}/v1/messages", self.url))
            .header("x-api-key", key.expose().as_str())
            .header("anthropic-version", API_VERSION)
            .json(body))
    }
}

impl Driver for Anthropic {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            images: true,
            pdfs: true,
            embeddings: false,
            native_structured: true,
        }
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let body = body(request, false);
            let answer =
                wire::send_json(self.request(&body)?, request.timeout, wire::common_message)
                    .await?;
            parse_response(&answer)
        })
    }

    fn stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChunkStream, ProviderError>> {
        Box::pin(async move {
            let body = body(request, true);
            let response =
                wire::open_stream(self.request(&body)?, request.timeout, wire::common_message)
                    .await?;
            let events = wire::sse_events(response);
            let stream = events
                .scan(StreamState::default(), |state, event| {
                    let chunks = match event {
                        Ok(event) => state.on_event(&event),
                        Err(error) => vec![Err(error)],
                    };
                    futures_util::future::ready(Some(futures_util::stream::iter(chunks)))
                })
                .flatten();
            Ok(Box::pin(stream) as ChunkStream)
        })
    }
}

/// The request body.
pub(crate) fn body(request: &ChatRequest, stream: bool) -> Value {
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens,
        "messages": messages(&request.messages),
    });
    if let Some(system) = &request.system {
        body["system"] = json!(system);
    }
    if !request.stop.is_empty() {
        body["stop_sequences"] = json!(request.stop);
    }
    if !request.tools.is_empty() {
        body["tools"] = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.parameters,
                })
            })
            .collect();
        body["tool_choice"] = match request.tool_choice {
            ToolChoice::Auto => json!({ "type": "auto" }),
            ToolChoice::None => json!({ "type": "none" }),
        };
    }
    if let Some(output) = &request.output {
        body["output_config"] = json!({
            "format": { "type": "json_schema", "schema": schema::for_anthropic(output.schema.clone()) }
        });
    }
    if stream {
        body["stream"] = json!(true);
    }
    body
}

/// The messages, consecutive ones of one role merged (the API alternates
/// roles), attachments before text.
fn messages(messages: &[Message]) -> Vec<Value> {
    let mut out: Vec<(Role, Vec<Value>)> = Vec::new();
    for message in messages {
        let mut blocks: Vec<Value> = Vec::new();
        let mut text_blocks = Vec::new();
        for part in &message.parts {
            match part {
                Part::Text { text } if !text.is_empty() => {
                    text_blocks.push(json!({ "type": "text", "text": text }));
                }
                Part::Text { .. } => {}
                // The API takes images and documents from the user only.
                Part::Attachment(_) if message.role == Role::Assistant => {}
                Part::Attachment(attachment) => {
                    let source = json!({
                        "type": "base64",
                        "media_type": attachment.media_type(),
                        "data": attachment.base64(),
                    });
                    match attachment.kind() {
                        AttachmentKind::Image => {
                            blocks.push(json!({ "type": "image", "source": source }));
                        }
                        _ => blocks.push(json!({ "type": "document", "source": source })),
                    }
                }
                Part::ToolCall(call) => {
                    blocks.append(&mut text_blocks);
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": object_or_empty(&call.arguments),
                    }));
                }
                Part::ToolResult(result) => blocks.push(json!({
                    "type": "tool_result",
                    "tool_use_id": result.call_id,
                    "content": result.content.to_string(),
                    "is_error": result.is_error,
                })),
                Part::Reasoning { provider, data } if provider == DRIVER => {
                    blocks.push(data.clone());
                }
                Part::Reasoning { .. } => {}
            }
        }
        blocks.extend(text_blocks);
        if blocks.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some((role, existing)) if *role == message.role => existing.extend(blocks),
            _ => out.push((message.role, blocks)),
        }
    }
    out.into_iter()
        .map(|(role, content)| {
            json!({
                "role": match role { Role::User => "user", Role::Assistant => "assistant" },
                "content": content,
            })
        })
        .collect()
}

/// Tool inputs must be objects.
fn object_or_empty(value: &Value) -> Value {
    if value.is_object() {
        value.clone()
    } else {
        json!({})
    }
}

/// The parts, usage and finish of a whole answer.
pub(crate) fn parse_response(answer: &Value) -> Result<ChatResponse, ProviderError> {
    let content = answer
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::unavailable("an answer without content"))?;
    let mut parts = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => parts.push(Part::Text {
                text: block["text"].as_str().unwrap_or_default().to_owned(),
            }),
            Some("tool_use") => parts.push(Part::ToolCall(ToolCall {
                id: block["id"].as_str().unwrap_or_default().to_owned(),
                name: block["name"].as_str().unwrap_or_default().to_owned(),
                arguments: block.get("input").cloned().unwrap_or_else(|| json!({})),
                meta: None,
            })),
            Some("thinking" | "redacted_thinking") => parts.push(Part::Reasoning {
                provider: DRIVER.to_owned(),
                data: block.clone(),
            }),
            _ => {}
        }
    }
    Ok(ChatResponse {
        parts,
        usage: usage(answer.get("usage")),
        finish: finish(answer.get("stop_reason").and_then(Value::as_str)),
    })
}

/// Usage: input tokens include cache writes and reads.
fn usage(usage: Option<&Value>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let cached = count("cache_read_input_tokens");
    Usage {
        input_tokens: count("input_tokens") + count("cache_creation_input_tokens") + cached,
        output_tokens: count("output_tokens"),
        cached_input_tokens: cached,
    }
}

fn finish(stop_reason: Option<&str>) -> Finish {
    match stop_reason {
        Some("max_tokens" | "model_context_window_exceeded") => Finish::MaxTokens,
        Some("refusal") => Finish::Refused,
        Some("tool_use") => Finish::ToolCalls,
        _ => Finish::Stop,
    }
}

/// A content block being streamed.
#[derive(Debug)]
enum Block {
    Text,
    Tool {
        id: String,
        name: String,
        json: String,
    },
    Reasoning(Value),
}

/// The state of a streamed answer.
#[derive(Debug, Default)]
struct StreamState {
    blocks: BTreeMap<u64, Block>,
    usage: Usage,
    finish: Finish,
}

impl StreamState {
    fn on_event(&mut self, event: &SseEvent) -> Vec<Result<StreamChunk, ProviderError>> {
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            return Vec::new();
        };
        let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
        match data.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                self.usage = usage(data.pointer("/message/usage"));
                Vec::new()
            }
            Some("content_block_start") => {
                let block = &data["content_block"];
                let state = match block.get("type").and_then(Value::as_str) {
                    Some("tool_use") => Block::Tool {
                        id: block["id"].as_str().unwrap_or_default().to_owned(),
                        name: block["name"].as_str().unwrap_or_default().to_owned(),
                        json: String::new(),
                    },
                    Some("thinking" | "redacted_thinking") => Block::Reasoning(block.clone()),
                    _ => Block::Text,
                };
                self.blocks.insert(index, state);
                match block.get("text").and_then(Value::as_str) {
                    Some(text) if !text.is_empty() => vec![Ok(StreamChunk::Text(text.to_owned()))],
                    _ => Vec::new(),
                }
            }
            Some("content_block_delta") => {
                let delta = &data["delta"];
                match (
                    delta.get("type").and_then(Value::as_str),
                    self.blocks.get_mut(&index),
                ) {
                    (Some("text_delta"), _) => {
                        let text = delta["text"].as_str().unwrap_or_default();
                        if text.is_empty() {
                            Vec::new()
                        } else {
                            vec![Ok(StreamChunk::Text(text.to_owned()))]
                        }
                    }
                    (Some("input_json_delta"), Some(Block::Tool { json, .. })) => {
                        json.push_str(delta["partial_json"].as_str().unwrap_or_default());
                        Vec::new()
                    }
                    (Some("thinking_delta"), Some(Block::Reasoning(block))) => {
                        let more = delta["thinking"].as_str().unwrap_or_default();
                        let thinking =
                            format!("{}{more}", block["thinking"].as_str().unwrap_or_default());
                        block["thinking"] = json!(thinking);
                        Vec::new()
                    }
                    (Some("signature_delta"), Some(Block::Reasoning(block))) => {
                        block["signature"] = delta["signature"].clone();
                        Vec::new()
                    }
                    _ => Vec::new(),
                }
            }
            Some("content_block_stop") => match self.blocks.remove(&index) {
                Some(Block::Tool { id, name, json }) => {
                    let arguments = if json.trim().is_empty() {
                        Ok(json!({}))
                    } else {
                        serde_json::from_str(&json)
                    };
                    match arguments {
                        Ok(arguments) => vec![Ok(StreamChunk::ToolCall(ToolCall {
                            id,
                            name,
                            arguments,
                            meta: None,
                        }))],
                        Err(error) => vec![Err(ProviderError::unavailable(format!(
                            "unreadable tool arguments: {error}"
                        )))],
                    }
                }
                Some(Block::Reasoning(block)) => {
                    vec![Ok(StreamChunk::Reasoning(Part::Reasoning {
                        provider: DRIVER.to_owned(),
                        data: block,
                    }))]
                }
                _ => Vec::new(),
            },
            Some("message_delta") => {
                if let Some(reason) = data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.finish = finish(Some(reason));
                }
                if let Some(output) = data.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                    self.usage.output_tokens = output;
                }
                Vec::new()
            }
            Some("message_stop") => vec![Ok(StreamChunk::Done {
                usage: self.usage,
                finish: self.finish,
            })],
            Some("error") => {
                let message = wire::common_message(&data).unwrap_or_else(|| data.to_string());
                let error = match data.pointer("/error/type").and_then(Value::as_str) {
                    Some("rate_limit_error") => ProviderError::RateLimited { retry_after: None },
                    Some("invalid_request_error") => ProviderError::request(message),
                    _ => ProviderError::unavailable(message),
                };
                vec![Err(error)]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::message::ToolResult;
    use crate::provider::{OutputFormat, ToolDefinition};

    fn request() -> ChatRequest {
        ChatRequest {
            model_name: "fast".into(),
            model: "claude-sonnet-5-5".into(),
            system: Some("Be brief.".into()),
            messages: vec![Message::user("Hi")],
            max_tokens: 100,
            temperature: Some(0.2),
            stop: vec!["END".into()],
            tools: Vec::new(),
            tool_choice: ToolChoice::Auto,
            output: None,
            user: None,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn the_body_has_the_documented_shape() {
        let body = body(&request(), false);
        assert_eq!(body["model"], "claude-sonnet-5-5");
        assert_eq!(body["max_tokens"], 100);
        assert_eq!(body["system"], "Be brief.");
        assert_eq!(body["stop_sequences"], json!(["END"]));
        assert_eq!(
            body["messages"],
            json!([{ "role": "user", "content": [{ "type": "text", "text": "Hi" }] }])
        );
        // The current API version rejects sampling parameters.
        assert!(body.get("temperature").is_none());
        assert!(body.get("stream").is_none());
    }

    #[test]
    fn tools_structured_output_and_tool_turns() {
        let mut request = request();
        request.tools = vec![ToolDefinition {
            name: "search_deals".into(),
            description: "Finds deals.".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        }];
        request.tool_choice = ToolChoice::None;
        request.output = Some(OutputFormat {
            name: "Triage".into(),
            schema: json!({ "type": "object", "properties": { "a": { "type": "string", "maxLength": 3 } } }),
        });
        request.messages.push(Message::new(
            Role::Assistant,
            vec![
                Part::Reasoning {
                    provider: "anthropic".into(),
                    data: json!({ "type": "thinking", "thinking": "", "signature": "sig" }),
                },
                Part::Text {
                    text: "Looking.".into(),
                },
                Part::ToolCall(ToolCall {
                    id: "toolu_1".into(),
                    name: "search_deals".into(),
                    arguments: json!({ "q": "acme" }),
                    meta: None,
                }),
            ],
        ));
        request.messages.push(Message::new(
            Role::User,
            vec![Part::ToolResult(ToolResult {
                call_id: "toolu_1".into(),
                name: "search_deals".into(),
                content: json!([{ "id": 1 }]),
                is_error: false,
            })],
        ));
        let body = body(&request, true);
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["tool_choice"], json!({ "type": "none" }));
        let format = &body["output_config"]["format"];
        assert_eq!(format["type"], "json_schema");
        assert_eq!(format["schema"]["additionalProperties"], false);
        assert!(
            format["schema"]["properties"]["a"]
                .get("maxLength")
                .is_none()
        );
        let assistant = &body["messages"][1]["content"];
        assert_eq!(assistant[0]["type"], "thinking");
        assert_eq!(assistant[1]["type"], "text");
        assert_eq!(assistant[2]["type"], "tool_use");
        assert_eq!(assistant[2]["input"], json!({ "q": "acme" }));
        let result = &body["messages"][2]["content"][0];
        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_1");
        assert_eq!(result["content"], "[{\"id\":1}]");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn attachments_go_in_user_turns_only() {
        use crate::message::Attachment;

        let mut request = request();
        request.messages = vec![
            Message::new(
                Role::User,
                vec![
                    Part::Text {
                        text: "Summarize.".into(),
                    },
                    Part::Attachment(Attachment::pdf(&b"%PDF-1.7"[..])),
                ],
            ),
            Message::new(
                Role::Assistant,
                vec![
                    Part::Attachment(Attachment::image(&b"png"[..], "image/png")),
                    Part::Text {
                        text: "Done.".into(),
                    },
                ],
            ),
        ];
        let body = body(&request, false);
        let user = &body["messages"][0]["content"];
        assert_eq!(user[0]["type"], "document", "attachments before text");
        assert_eq!(user[0]["source"]["media_type"], "application/pdf");
        assert_eq!(user[1]["type"], "text");
        assert_eq!(
            body["messages"][1]["content"],
            json!([{ "type": "text", "text": "Done." }])
        );
    }

    #[test]
    fn responses_parse() {
        let answer = json!({
            "content": [
                { "type": "thinking", "thinking": "", "signature": "abc" },
                { "type": "text", "text": "Calling." },
                { "type": "tool_use", "id": "toolu_9", "name": "lookup", "input": { "id": 3 } }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "cache_read_input_tokens": 90, "cache_creation_input_tokens": 0, "output_tokens": 7 }
        });
        let response = parse_response(&answer).unwrap();
        assert_eq!(response.text(), "Calling.");
        assert_eq!(response.tool_calls()[0].name, "lookup");
        assert_eq!(response.finish, Finish::ToolCalls);
        assert_eq!(
            response.usage,
            Usage {
                input_tokens: 100,
                output_tokens: 7,
                cached_input_tokens: 90
            }
        );
        assert!(
            matches!(&response.parts[0], Part::Reasoning { provider, .. } if provider == "anthropic")
        );
        assert_eq!(finish(Some("refusal")), Finish::Refused);
        assert_eq!(finish(Some("max_tokens")), Finish::MaxTokens);
        assert_eq!(finish(Some("end_turn")), Finish::Stop);
    }

    #[test]
    fn stream_events_assemble_text_tools_and_usage() {
        let mut state = StreamState::default();
        let events = [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":12,"output_tokens":1}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"id\":"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"3}"}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}"#,
            r#"{"type":"message_stop"}"#,
        ];
        let chunks: Vec<StreamChunk> = events
            .iter()
            .flat_map(|data| {
                state.on_event(&SseEvent {
                    event: None,
                    data: (*data).to_owned(),
                })
            })
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            chunks,
            [
                StreamChunk::Text("Hel".into()),
                StreamChunk::Text("lo".into()),
                StreamChunk::ToolCall(ToolCall {
                    id: "toolu_1".into(),
                    name: "lookup".into(),
                    arguments: json!({ "id": 3 }),
                    meta: None
                }),
                StreamChunk::Done {
                    usage: Usage::new(12, 9),
                    finish: Finish::ToolCalls
                },
            ]
        );
    }

    #[test]
    fn stream_errors_classify() {
        let mut state = StreamState::default();
        let chunks = state.on_event(&SseEvent {
            event: Some("error".into()),
            data: r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
                .into(),
        });
        assert!(matches!(&chunks[0], Err(ProviderError::Unavailable { .. })));
    }
}

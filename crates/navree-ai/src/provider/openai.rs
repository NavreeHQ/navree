//! The OpenAI chat completions API (`POST /chat/completions`,
//! `POST /embeddings`), which Mistral and OpenAI-compatible servers speak
//! too.

use std::collections::BTreeMap;

use futures_util::StreamExt;
use navree_http_client::{HttpClient, RequestBuilder};
use navree_kernel::BoxFuture;
use serde_json::{Value, json};

use super::wire;
use super::{
    Capabilities, ChatRequest, ChatResponse, ChunkStream, Driver, EmbedRequest, EmbedResponse,
    StreamChunk, ToolChoice,
};
use crate::error::ProviderError;
use crate::message::{AttachmentKind, Message, Part, Role, ToolCall};
use crate::options::ProviderConfig;
use crate::response::{Finish, Usage};

/// Which vendor's dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavor {
    /// OpenAI, or a compatible server (with `url`).
    OpenAi,
    /// Mistral.
    Mistral,
}

/// An OpenAI, Mistral or OpenAI-compatible account.
pub(crate) struct OpenAi {
    name: String,
    flavor: Flavor,
    api_key: Option<String>,
    organization: Option<String>,
    base: String,
    /// A compatible server (custom `url` with the `openai` driver).
    compatible: bool,
    http: HttpClient,
}

impl OpenAi {
    pub(crate) fn new(
        name: &str,
        config: &ProviderConfig,
        http: HttpClient,
        flavor: Flavor,
    ) -> Self {
        let default = match flavor {
            Flavor::OpenAi => "https://api.openai.com/v1",
            Flavor::Mistral => "https://api.mistral.ai/v1",
        };
        Self {
            name: name.to_owned(),
            flavor,
            api_key: config.api_key.clone().filter(|key| !key.is_empty()),
            organization: config.organization.clone(),
            base: config
                .url
                .clone()
                .unwrap_or_else(|| default.to_owned())
                .trim_end_matches('/')
                .to_owned(),
            compatible: flavor == Flavor::OpenAi && config.url.is_some(),
            http,
        }
    }

    fn post(&self, path: &str, body: &Value) -> Result<RequestBuilder, ProviderError> {
        let mut request = self.http.post(format!("{}{path}", self.base)).json(body);
        match &self.api_key {
            Some(key) => request = request.bearer(key),
            None if !self.compatible => {
                return Err(ProviderError::Auth {
                    message: format!(
                        "no API key: set APP_AI__PROVIDERS__{}__API_KEY",
                        self.name.to_uppercase()
                    ),
                });
            }
            None => {}
        }
        if let Some(organization) = &self.organization {
            request = request.header("OpenAI-Organization", organization.as_str());
        }
        Ok(request)
    }

    /// The chat completions body.
    pub(crate) fn body(&self, request: &ChatRequest, stream: bool) -> Value {
        let mut body = json!({
            "model": request.model,
            "messages": messages(request.system.as_deref(), &request.messages),
        });
        let limit_key = if self.flavor == Flavor::OpenAi && !self.compatible {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        body[limit_key] = json!(request.max_tokens);
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if !request.stop.is_empty() {
            body["stop"] = json!(request.stop);
        }
        if !request.tools.is_empty() {
            body["tools"] = request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect();
            body["tool_choice"] = json!(match request.tool_choice {
                ToolChoice::Auto => "auto",
                ToolChoice::None => "none",
            });
        }
        if let Some(output) = &request.output {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": schema_name(&output.name),
                    "schema": output.schema,
                    "strict": false,
                }
            });
        }
        if stream {
            body["stream"] = json!(true);
            if self.flavor == Flavor::OpenAi {
                body["stream_options"] = json!({ "include_usage": true });
            }
        }
        body
    }
}

impl Driver for OpenAi {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            images: self.flavor == Flavor::OpenAi,
            pdfs: false,
            embeddings: true,
            native_structured: true,
        }
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let body = self.body(request, false);
            let answer = wire::send_json(
                self.post("/chat/completions", &body)?,
                request.timeout,
                wire::common_message,
            )
            .await?;
            parse_response(&answer)
        })
    }

    fn stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChunkStream, ProviderError>> {
        Box::pin(async move {
            let body = self.body(request, true);
            let response = wire::open_stream(
                self.post("/chat/completions", &body)?,
                request.timeout,
                wire::common_message,
            )
            .await?;
            let events = wire::sse_events(response).map(|event| event.map(|e| e.data));
            Ok(Box::pin(chunks(events)) as ChunkStream)
        })
    }

    fn embed<'a>(
        &'a self,
        request: &'a EmbedRequest,
    ) -> BoxFuture<'a, Result<EmbedResponse, ProviderError>> {
        Box::pin(async move {
            let mut body = json!({ "model": request.model, "input": request.texts });
            if let (Some(dimensions), Flavor::OpenAi) = (request.dimensions, self.flavor) {
                body["dimensions"] = json!(dimensions);
            }
            let answer = wire::send_json(
                self.post("/embeddings", &body)?,
                request.timeout,
                wire::common_message,
            )
            .await?;
            parse_embeddings(&answer)
        })
    }
}

/// `json_schema.name` allows `[a-zA-Z0-9_-]`, at most 64 characters.
fn schema_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if name.is_empty() {
        "output".to_owned()
    } else {
        name
    }
}

/// The messages: the system prompt first; tool results become `tool`
/// messages.
fn messages(system: Option<&str>, messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(system) = system {
        out.push(json!({ "role": "system", "content": system }));
    }
    for message in messages {
        match message.role {
            Role::Assistant => {
                let text = message.text();
                let calls: Vec<Value> = message
                    .tool_calls()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": { "name": call.name, "arguments": call.arguments.to_string() },
                        })
                    })
                    .collect();
                let mut entry = json!({
                    "role": "assistant",
                    "content": if text.is_empty() && !calls.is_empty() { Value::Null } else { json!(text) },
                });
                if !calls.is_empty() {
                    entry["tool_calls"] = Value::Array(calls);
                }
                out.push(entry);
            }
            Role::User => {
                for result in message.tool_results() {
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": result.call_id,
                        "content": result.content.to_string(),
                    }));
                }
                let text = message.text();
                let attachments: Vec<Value> = message
                    .attachments()
                    .filter(|a| a.kind() == AttachmentKind::Image)
                    .map(|a| {
                        json!({
                            "type": "image_url",
                            "image_url": { "url": format!("data:{};base64,{}", a.media_type(), a.base64()) },
                        })
                    })
                    .collect();
                if attachments.is_empty() {
                    if !text.is_empty() {
                        out.push(json!({ "role": "user", "content": text }));
                    }
                } else {
                    let mut content = vec![json!({ "type": "text", "text": text })];
                    content.extend(attachments);
                    out.push(json!({ "role": "user", "content": content }));
                }
            }
        }
    }
    out
}

/// A message's text: a string, or an array of content chunks (Mistral).
fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(chunks) => chunks
            .iter()
            .filter(|chunk| chunk.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|chunk| chunk.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

/// A whole answer.
pub(crate) fn parse_response(answer: &Value) -> Result<ChatResponse, ProviderError> {
    let choice = answer
        .pointer("/choices/0")
        .ok_or_else(|| ProviderError::unavailable("an answer without choices"))?;
    let message = &choice["message"];
    let mut parts = Vec::new();
    let text = content_text(&message["content"]);
    if !text.is_empty() {
        parts.push(Part::Text { text });
    }
    for call in message["tool_calls"].as_array().into_iter().flatten() {
        parts.push(Part::ToolCall(ToolCall {
            id: call["id"].as_str().unwrap_or_default().to_owned(),
            name: call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            arguments: arguments(call.pointer("/function/arguments")),
            meta: None,
        }));
    }
    let refused = message.get("refusal").is_some_and(|r| !r.is_null());
    let finish = if refused {
        Finish::Refused
    } else {
        finish(choice["finish_reason"].as_str())
    };
    Ok(ChatResponse {
        parts,
        usage: usage(answer.get("usage")),
        finish,
    })
}

/// Tool arguments: a JSON string (OpenAI) or an object.
fn arguments(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) if text.trim().is_empty() => json!({}),
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_else(|_| json!({})),
        Some(Value::Object(map)) => Value::Object(map.clone()),
        _ => json!({}),
    }
}

fn finish(reason: Option<&str>) -> Finish {
    match reason {
        Some("length" | "model_length") => Finish::MaxTokens,
        Some("tool_calls" | "function_call") => Finish::ToolCalls,
        Some("content_filter") => Finish::Refused,
        _ => Finish::Stop,
    }
}

fn usage(usage: Option<&Value>) -> Usage {
    let Some(usage) = usage.filter(|u| !u.is_null()) else {
        return Usage::default();
    };
    Usage {
        input_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0),
        output_tokens: usage["completion_tokens"].as_u64().unwrap_or(0),
        cached_input_tokens: usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }
}

/// Embedding vectors in input order.
fn parse_embeddings(answer: &Value) -> Result<EmbedResponse, ProviderError> {
    let data = answer["data"]
        .as_array()
        .ok_or_else(|| ProviderError::unavailable("an embeddings answer without data"))?;
    let mut indexed: Vec<(u64, Vec<f32>)> = data
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let index = item["index"].as_u64().unwrap_or(position as u64);
            let vector = item["embedding"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_f64)
                        .map(|v| v as f32)
                        .collect()
                })
                .unwrap_or_default();
            (index, vector)
        })
        .collect();
    indexed.sort_by_key(|(index, _)| *index);
    Ok(EmbedResponse {
        vectors: indexed.into_iter().map(|(_, vector)| vector).collect(),
        input_tokens: answer
            .pointer("/usage/prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

/// A tool call being streamed.
#[derive(Debug, Default)]
struct PendingCall {
    id: String,
    name: String,
    arguments: String,
}

/// The chunks of a streamed answer, from the `data:` payloads.
fn chunks(
    data: impl futures_util::Stream<Item = Result<String, ProviderError>> + Send + 'static,
) -> impl futures_util::Stream<Item = Result<StreamChunk, ProviderError>> + Send {
    #[derive(Default)]
    struct State {
        calls: BTreeMap<u64, PendingCall>,
        usage: Usage,
        finish: Finish,
        refused: bool,
        done: bool,
    }

    impl State {
        fn end(&mut self) -> Vec<Result<StreamChunk, ProviderError>> {
            if self.done {
                return Vec::new();
            }
            self.done = true;
            let mut out: Vec<Result<StreamChunk, ProviderError>> = std::mem::take(&mut self.calls)
                .into_values()
                .map(|call| {
                    Ok(StreamChunk::ToolCall(ToolCall {
                        id: call.id,
                        name: call.name,
                        arguments: arguments(Some(&Value::String(call.arguments))),
                        meta: None,
                    }))
                })
                .collect();
            let finish = if self.refused {
                Finish::Refused
            } else {
                self.finish
            };
            out.push(Ok(StreamChunk::Done {
                usage: self.usage,
                finish,
            }));
            out
        }

        fn on_data(&mut self, data: &str) -> Vec<Result<StreamChunk, ProviderError>> {
            if data.trim() == "[DONE]" {
                return self.end();
            }
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                return Vec::new();
            };
            if let Some(error) = value.get("error") {
                let message = error["message"]
                    .as_str()
                    .unwrap_or("stream error")
                    .to_owned();
                return vec![Err(ProviderError::unavailable(message))];
            }
            if let Some(usage_value) = value.get("usage").filter(|u| !u.is_null()) {
                self.usage = usage(Some(usage_value));
            }
            let mut out = Vec::new();
            if let Some(choice) = value.pointer("/choices/0") {
                let delta = &choice["delta"];
                let text = content_text(&delta["content"]);
                if !text.is_empty() {
                    out.push(Ok(StreamChunk::Text(text)));
                }
                if delta.get("refusal").is_some_and(|r| !r.is_null()) {
                    self.refused = true;
                }
                for call in delta["tool_calls"].as_array().into_iter().flatten() {
                    let index = call["index"].as_u64().unwrap_or(0);
                    let pending = self.calls.entry(index).or_default();
                    if let Some(id) = call["id"].as_str() {
                        pending.id = id.to_owned();
                    }
                    if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                        pending.name.push_str(name);
                    }
                    match call.pointer("/function/arguments") {
                        Some(Value::String(more)) => pending.arguments.push_str(more),
                        Some(Value::Object(map)) => {
                            pending.arguments = Value::Object(map.clone()).to_string();
                        }
                        _ => {}
                    }
                }
                if let Some(reason) = choice["finish_reason"].as_str() {
                    self.finish = finish(Some(reason));
                }
            }
            out
        }
    }

    let data = Box::pin(data);
    futures_util::stream::unfold(
        (data, State::default(), std::collections::VecDeque::new()),
        |(mut data, mut state, mut pending)| async move {
            loop {
                if let Some(item) = pending.pop_front() {
                    return Some((item, (data, state, pending)));
                }
                if state.done {
                    return None;
                }
                match data.next().await {
                    Some(Ok(payload)) => pending.extend(state.on_data(&payload)),
                    Some(Err(error)) => {
                        state.done = true;
                        pending.push_back(Err(error));
                    }
                    // The stream ended without `[DONE]` (Mistral).
                    None => pending.extend(state.end()),
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::StreamExt;

    use super::*;
    use crate::message::{Attachment, ToolResult};
    use crate::provider::OutputFormat;

    fn driver(flavor: Flavor, url: Option<&str>) -> OpenAi {
        let config = ProviderConfig {
            api_key: Some("k".into()),
            url: url.map(str::to_owned),
            ..ProviderConfig::default()
        };
        OpenAi::new("openai", &config, HttpClient::fake().0, flavor)
    }

    fn request() -> ChatRequest {
        ChatRequest {
            model_name: "backup".into(),
            model: "gpt-5".into(),
            system: Some("Be brief.".into()),
            messages: vec![Message::user("Hi")],
            max_tokens: 50,
            temperature: None,
            stop: Vec::new(),
            tools: Vec::new(),
            tool_choice: ToolChoice::Auto,
            output: None,
            user: None,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn bodies_per_flavor() {
        let body = driver(Flavor::OpenAi, None).body(&request(), false);
        assert_eq!(body["max_completion_tokens"], 50);
        assert_eq!(
            body["messages"][0],
            json!({ "role": "system", "content": "Be brief." })
        );
        assert_eq!(
            body["messages"][1],
            json!({ "role": "user", "content": "Hi" })
        );
        let body = driver(Flavor::OpenAi, Some("http://127.0.0.1:8000/v1")).body(&request(), true);
        assert_eq!(body["max_tokens"], 50);
        assert_eq!(body["stream_options"]["include_usage"], true);
        let body = driver(Flavor::Mistral, None).body(&request(), true);
        assert_eq!(body["max_tokens"], 50);
        assert!(body.get("stream_options").is_none());
    }

    #[test]
    fn tool_turns_images_and_structured_output() {
        let mut request = request();
        request.messages = vec![
            Message::new(
                Role::User,
                vec![
                    Part::Text {
                        text: "What is this?".into(),
                    },
                    Part::Attachment(Attachment::image(vec![1u8, 2], "image/png")),
                ],
            ),
            Message::new(
                Role::Assistant,
                vec![Part::ToolCall(ToolCall {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    arguments: json!({ "id": 3 }),
                    meta: None,
                })],
            ),
            Message::new(
                Role::User,
                vec![Part::ToolResult(ToolResult {
                    call_id: "call_1".into(),
                    name: "lookup".into(),
                    content: json!({ "ok": true }),
                    is_error: false,
                })],
            ),
        ];
        request.output = Some(OutputFormat {
            name: "Follow up".into(),
            schema: json!({ "type": "object" }),
        });
        let body = driver(Flavor::OpenAi, None).body(&request, false);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(
            messages[1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AQI="
        );
        assert_eq!(messages[2]["content"], Value::Null);
        assert_eq!(
            messages[2]["tool_calls"][0]["function"]["arguments"],
            "{\"id\":3}"
        );
        assert_eq!(
            messages[3],
            json!({ "role": "tool", "tool_call_id": "call_1", "content": "{\"ok\":true}" })
        );
        assert_eq!(body["response_format"]["json_schema"]["name"], "Follow_up");
    }

    #[test]
    fn responses_parse() {
        let answer = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "lookup", "arguments": "{\"id\":3}" } }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 20, "completion_tokens": 5, "prompt_tokens_details": { "cached_tokens": 8 } }
        });
        let response = parse_response(&answer).unwrap();
        assert_eq!(response.tool_calls()[0].arguments, json!({ "id": 3 }));
        assert_eq!(response.finish, Finish::ToolCalls);
        assert_eq!(response.usage.cached_input_tokens, 8);
        let refused = json!({
            "choices": [{ "message": { "content": null, "refusal": "I can't help." }, "finish_reason": "stop" }]
        });
        assert_eq!(parse_response(&refused).unwrap().finish, Finish::Refused);
    }

    #[tokio::test]
    async fn streams_assemble_tool_calls() {
        let payloads = [
            r#"{"choices":[{"delta":{"role":"assistant","content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"id\""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":3}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":4}}"#,
            "[DONE]",
        ];
        let stream = futures_util::stream::iter(payloads.map(|p| Ok(p.to_owned())));
        let chunks: Vec<StreamChunk> = chunks(stream).map(Result::unwrap).collect().await;
        assert_eq!(
            chunks,
            [
                StreamChunk::Text("Hi".into()),
                StreamChunk::ToolCall(ToolCall {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    arguments: json!({ "id": 3 }),
                    meta: None
                }),
                StreamChunk::Done {
                    usage: Usage::new(9, 4),
                    finish: Finish::ToolCalls
                },
            ]
        );
    }

    #[test]
    fn embeddings_come_back_in_input_order() {
        let answer = json!({
            "data": [
                { "index": 1, "embedding": [0.0, 1.0] },
                { "index": 0, "embedding": [1.0, 0.0] }
            ],
            "usage": { "prompt_tokens": 6 }
        });
        let response = parse_embeddings(&answer).unwrap();
        assert_eq!(response.vectors, [vec![1.0, 0.0], vec![0.0, 1.0]]);
        assert_eq!(response.input_tokens, 6);
    }
}

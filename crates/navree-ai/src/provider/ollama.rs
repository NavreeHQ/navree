//! The Ollama API (`POST /api/chat`, `POST /api/embed`), on the machine.

use futures_util::StreamExt;
use navree_http_client::HttpClient;
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
use crate::schema;

/// Ollama's default address.
pub(crate) const DEFAULT_URL: &str = "http://127.0.0.1:11434";

/// An Ollama server.
pub(crate) struct Ollama {
    base: String,
    http: HttpClient,
}

impl Ollama {
    pub(crate) fn new(_name: &str, config: &ProviderConfig, http: HttpClient) -> Self {
        Self {
            base: config
                .url
                .clone()
                .unwrap_or_else(|| DEFAULT_URL.to_owned())
                .trim_end_matches('/')
                .to_owned(),
            http,
        }
    }
}

impl Driver for Ollama {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            images: true,
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
            let request_body = body(request, false);
            let answer = wire::send_json(
                self.http.post(format!("{}/api/chat", self.base)).json(&request_body),
                request.timeout,
                wire::common_message,
            )
            .await?;
            let mut state = State::default();
            let mut parts = Vec::new();
            let mut text = String::new();
            for chunk in state.on_line(&answer) {
                match chunk {
                    StreamChunk::Text(more) => text.push_str(&more),
                    StreamChunk::ToolCall(call) => parts.push(Part::ToolCall(call)),
                    _ => {}
                }
            }
            if !text.is_empty() {
                parts.insert(0, Part::Text { text });
            }
            Ok(ChatResponse {
                parts,
                usage: state.usage,
                finish: state.finish(),
            })
        })
    }

    fn stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChunkStream, ProviderError>> {
        Box::pin(async move {
            let request_body = body(request, true);
            let response = wire::open_stream(
                self.http.post(format!("{}/api/chat", self.base)).json(&request_body),
                request.timeout,
                wire::common_message,
            )
            .await?;
            let lines = wire::lines(response);
            let stream = lines
                .scan(State::default(), |state, line| {
                    let chunks: Vec<Result<StreamChunk, ProviderError>> = match line {
                        Ok(line) => match serde_json::from_str::<Value>(&line) {
                            Ok(value) => state.on_line(&value).into_iter().map(Ok).collect(),
                            Err(_) => Vec::new(),
                        },
                        Err(error) => vec![Err(error)],
                    };
                    futures_util::future::ready(Some(futures_util::stream::iter(chunks)))
                })
                .flatten();
            Ok(Box::pin(stream) as ChunkStream)
        })
    }

    fn embed<'a>(
        &'a self,
        request: &'a EmbedRequest,
    ) -> BoxFuture<'a, Result<EmbedResponse, ProviderError>> {
        Box::pin(async move {
            let mut body = json!({ "model": request.model, "input": request.texts });
            if let Some(dimensions) = request.dimensions {
                body["dimensions"] = json!(dimensions);
            }
            let answer = wire::send_json(
                self.http.post(format!("{}/api/embed", self.base)).json(&body),
                request.timeout,
                wire::common_message,
            )
            .await?;
            let vectors = answer["embeddings"]
                .as_array()
                .ok_or_else(|| ProviderError::unavailable("an embeddings answer without embeddings"))?
                .iter()
                .map(|vector| {
                    vector
                        .as_array()
                        .map(|values| values.iter().filter_map(Value::as_f64).map(|v| v as f32).collect())
                        .unwrap_or_default()
                })
                .collect();
            Ok(EmbedResponse {
                vectors,
                input_tokens: answer["prompt_eval_count"].as_u64().unwrap_or(0),
            })
        })
    }
}

/// The request body.
pub(crate) fn body(request: &ChatRequest, stream: bool) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = &request.system {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in &request.messages {
        messages.extend(message_entries(message));
    }
    let mut options = json!({ "num_predict": request.max_tokens });
    if let Some(temperature) = request.temperature {
        options["temperature"] = json!(temperature);
    }
    if !request.stop.is_empty() {
        options["stop"] = json!(request.stop);
    }
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": stream,
        "options": options,
    });
    if !request.tools.is_empty() && request.tool_choice == ToolChoice::Auto {
        body["tools"] = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": schema::inline_refs(&tool.parameters),
                    }
                })
            })
            .collect();
    }
    if let Some(output) = &request.output {
        body["format"] = schema::inline_refs(&output.schema);
    }
    body
}

fn message_entries(message: &Message) -> Vec<Value> {
    match message.role {
        Role::Assistant => {
            let calls: Vec<Value> = message
                .tool_calls()
                .map(|call| json!({ "function": { "name": call.name, "arguments": call.arguments } }))
                .collect();
            let mut entry = json!({ "role": "assistant", "content": message.text() });
            if !calls.is_empty() {
                entry["tool_calls"] = Value::Array(calls);
            }
            vec![entry]
        }
        Role::User => {
            let mut entries: Vec<Value> = message
                .tool_results()
                .map(|result| {
                    json!({ "role": "tool", "tool_name": result.name, "content": result.content.to_string() })
                })
                .collect();
            let text = message.text();
            let images: Vec<String> = message
                .attachments()
                .filter(|a| a.kind() == AttachmentKind::Image)
                .map(|a| a.base64())
                .collect();
            if !text.is_empty() || !images.is_empty() {
                let mut entry = json!({ "role": "user", "content": text });
                if !images.is_empty() {
                    entry["images"] = json!(images);
                }
                entries.push(entry);
            }
            entries
        }
    }
}

/// What a (streamed) answer has said so far.
#[derive(Debug, Default)]
struct State {
    usage: Usage,
    done_reason: Option<String>,
    calls: usize,
}

impl State {
    fn on_line(&mut self, value: &Value) -> Vec<StreamChunk> {
        let mut chunks = Vec::new();
        if let Some(text) = value.pointer("/message/content").and_then(Value::as_str)
            && !text.is_empty()
        {
            chunks.push(StreamChunk::Text(text.to_owned()));
        }
        for call in value
            .pointer("/message/tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.calls += 1;
            let id = call["id"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("ollama_{}", self.calls));
            chunks.push(StreamChunk::ToolCall(ToolCall {
                id,
                name: call.pointer("/function/name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                arguments: call.pointer("/function/arguments").cloned().unwrap_or_else(|| json!({})),
                meta: None,
            }));
        }
        if value["done"].as_bool() == Some(true) {
            self.usage = Usage::new(
                value["prompt_eval_count"].as_u64().unwrap_or(0),
                value["eval_count"].as_u64().unwrap_or(0),
            );
            self.done_reason = value["done_reason"].as_str().map(str::to_owned);
            chunks.push(StreamChunk::Done {
                usage: self.usage,
                finish: self.finish(),
            });
        }
        chunks
    }

    fn finish(&self) -> Finish {
        match self.done_reason.as_deref() {
            Some("length") => Finish::MaxTokens,
            _ if self.calls > 0 => Finish::ToolCalls,
            _ => Finish::Stop,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::message::{Attachment, ToolResult};
    use crate::provider::ToolDefinition;

    #[test]
    fn the_body_has_the_documented_shape() {
        let request = ChatRequest {
            model_name: "local".into(),
            model: "llama3.3".into(),
            system: Some("Be brief.".into()),
            messages: vec![
                Message::new(
                    Role::User,
                    vec![
                        Part::Text { text: "What is it?".into() },
                        Part::Attachment(Attachment::image(vec![1u8], "image/png")),
                    ],
                ),
                Message::new(
                    Role::User,
                    vec![Part::ToolResult(ToolResult {
                        call_id: "ollama_1".into(),
                        name: "lookup".into(),
                        content: json!({ "ok": true }),
                        is_error: false,
                    })],
                ),
            ],
            max_tokens: 64,
            temperature: None,
            stop: Vec::new(),
            tools: vec![ToolDefinition {
                name: "lookup".into(),
                description: "Looks up.".into(),
                parameters: json!({ "type": "object" }),
            }],
            tool_choice: ToolChoice::None,
            output: None,
            user: None,
            timeout: Duration::from_secs(5),
        };
        let body = body(&request, true);
        assert_eq!(body["stream"], true);
        assert_eq!(body["options"]["num_predict"], 64);
        assert_eq!(body["messages"][1]["images"], json!(["AQ=="]));
        assert_eq!(body["messages"][2]["role"], "tool");
        // `ToolChoice::None`: no tools offered.
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn lines_parse() {
        let mut state = State::default();
        let mut chunks = state.on_line(&json!({ "message": { "content": "Hi" }, "done": false }));
        chunks.extend(state.on_line(&json!({
            "message": { "content": "", "tool_calls": [{ "function": { "name": "lookup", "arguments": { "id": 1 } } }] },
            "done": false
        })));
        chunks.extend(state.on_line(&json!({
            "message": { "content": "" }, "done": true, "done_reason": "stop",
            "prompt_eval_count": 7, "eval_count": 3
        })));
        assert_eq!(chunks[0], StreamChunk::Text("Hi".into()));
        assert!(matches!(&chunks[1], StreamChunk::ToolCall(call) if call.id == "ollama_1"));
        assert_eq!(
            chunks[2],
            StreamChunk::Done {
                usage: Usage::new(7, 3),
                finish: Finish::ToolCalls
            }
        );
    }
}

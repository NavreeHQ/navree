//! The Google Gemini API (`models/{model}:generateContent`,
//! `:streamGenerateContent`, `:batchEmbedContents`).

use futures_util::StreamExt;
use navree_contracts::Secret;
use navree_http_client::{HttpClient, RequestBuilder};
use navree_kernel::BoxFuture;
use serde_json::{Value, json};

use super::wire;
use super::{
    Capabilities, ChatRequest, ChatResponse, ChunkStream, Driver, EmbedRequest, EmbedResponse,
    StreamChunk, ToolChoice,
};
use crate::error::ProviderError;
use crate::message::{Message, Part, Role, ToolCall};
use crate::options::ProviderConfig;
use crate::response::{Finish, Usage};
use crate::schema;

const DEFAULT_URL: &str = "https://generativelanguage.googleapis.com";

/// A Gemini account.
pub(crate) struct Gemini {
    name: String,
    api_key: Option<Secret<String>>,
    base: String,
    http: HttpClient,
}

impl Gemini {
    pub(crate) fn new(name: &str, config: &ProviderConfig, http: HttpClient) -> Self {
        Self {
            name: name.to_owned(),
            api_key: config
                .api_key
                .clone()
                .filter(|key| !key.expose().is_empty()),
            base: config
                .url
                .clone()
                .unwrap_or_else(|| DEFAULT_URL.to_owned())
                .trim_end_matches('/')
                .to_owned(),
            http,
        }
    }

    fn post(&self, url: String, body: &Value) -> Result<RequestBuilder, ProviderError> {
        let key = self.api_key.as_ref().ok_or_else(|| ProviderError::Auth {
            message: format!(
                "no API key: set APP_AI__PROVIDERS__{}__API_KEY",
                self.name.to_uppercase()
            ),
        })?;
        Ok(self
            .http
            .post(url)
            .header("x-goog-api-key", key.expose().as_str())
            .json(body))
    }

    fn url(&self, model: &str, method: &str) -> String {
        format!("{}/v1beta/models/{model}:{method}", self.base)
    }
}

impl Driver for Gemini {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            images: true,
            pdfs: true,
            embeddings: true,
            native_structured: true,
        }
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let request_body = body(request);
            let answer = wire::send_json(
                self.post(self.url(&request.model, "generateContent"), &request_body)?,
                request.timeout,
                wire::common_message,
            )
            .await?;
            let mut state = State::default();
            let mut parts = Vec::new();
            for chunk in state.on_response(&answer)? {
                match chunk {
                    StreamChunk::Text(text) => parts.push(Part::Text { text }),
                    StreamChunk::ToolCall(call) => parts.push(Part::ToolCall(call)),
                    StreamChunk::Reasoning(part) => parts.push(part),
                    StreamChunk::Done { .. } => {}
                }
            }
            let (usage, finish) = state.end();
            Ok(ChatResponse {
                parts: merge_text(parts),
                usage,
                finish,
            })
        })
    }

    fn stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChunkStream, ProviderError>> {
        Box::pin(async move {
            let request_body = body(request);
            let url = format!(
                "{}?alt=sse",
                self.url(&request.model, "streamGenerateContent")
            );
            let response = wire::open_stream(
                self.post(url, &request_body)?,
                request.timeout,
                wire::common_message,
            )
            .await?;
            let events = Box::pin(wire::sse_events(response));
            let stream = futures_util::stream::unfold(
                (
                    events,
                    State::default(),
                    std::collections::VecDeque::new(),
                    false,
                ),
                |(mut events, mut state, mut pending, mut done)| async move {
                    loop {
                        if let Some(item) = pending.pop_front() {
                            return Some((item, (events, state, pending, done)));
                        }
                        if done {
                            return None;
                        }
                        match events.next().await {
                            Some(Ok(event)) => {
                                if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
                                    match state.on_response(&value) {
                                        Ok(chunks) => pending.extend(chunks.into_iter().map(Ok)),
                                        Err(error) => {
                                            done = true;
                                            pending.push_back(Err(error));
                                        }
                                    }
                                }
                            }
                            Some(Err(error)) => {
                                done = true;
                                pending.push_back(Err(error));
                            }
                            None => {
                                done = true;
                                let (usage, finish) = state.end();
                                pending.push_back(Ok(StreamChunk::Done { usage, finish }));
                            }
                        }
                    }
                },
            );
            Ok(Box::pin(stream) as ChunkStream)
        })
    }

    fn embed<'a>(
        &'a self,
        request: &'a EmbedRequest,
    ) -> BoxFuture<'a, Result<EmbedResponse, ProviderError>> {
        Box::pin(async move {
            let requests: Vec<Value> = request
                .texts
                .iter()
                .map(|text| {
                    let mut item = json!({
                        "model": format!("models/{}", request.model),
                        "content": { "parts": [{ "text": text }] },
                    });
                    if let Some(dimensions) = request.dimensions {
                        item["outputDimensionality"] = json!(dimensions);
                    }
                    item
                })
                .collect();
            let answer = wire::send_json(
                self.post(
                    self.url(&request.model, "batchEmbedContents"),
                    &json!({ "requests": requests }),
                )?,
                request.timeout,
                wire::common_message,
            )
            .await?;
            let vectors = answer["embeddings"]
                .as_array()
                .ok_or_else(|| {
                    ProviderError::unavailable("an embeddings answer without embeddings")
                })?
                .iter()
                .map(|item| {
                    item["values"]
                        .as_array()
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(Value::as_f64)
                                .map(|v| v as f32)
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect();
            Ok(EmbedResponse {
                vectors,
                input_tokens: 0,
            })
        })
    }
}

/// The request body.
pub(crate) fn body(request: &ChatRequest) -> Value {
    let mut body = json!({ "contents": contents(&request.messages) });
    if let Some(system) = &request.system {
        body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }
    let mut generation = json!({ "maxOutputTokens": request.max_tokens });
    if let Some(temperature) = request.temperature {
        generation["temperature"] = json!(temperature);
    }
    if !request.stop.is_empty() {
        generation["stopSequences"] = json!(request.stop);
    }
    if let Some(output) = &request.output {
        generation["responseMimeType"] = json!("application/json");
        generation["responseJsonSchema"] = schema::inline_refs(&output.schema);
    }
    body["generationConfig"] = generation;
    if !request.tools.is_empty() {
        let declarations: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parametersJsonSchema": schema::inline_refs(&tool.parameters),
                })
            })
            .collect();
        body["tools"] = json!([{ "functionDeclarations": declarations }]);
        body["toolConfig"] = json!({
            "functionCallingConfig": {
                "mode": match request.tool_choice { ToolChoice::Auto => "AUTO", ToolChoice::None => "NONE" }
            }
        });
    }
    body
}

fn contents(messages: &[Message]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for message in messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "model",
        };
        let mut parts = Vec::new();
        for part in &message.parts {
            match part {
                Part::Text { text } if !text.is_empty() => parts.push(json!({ "text": text })),
                Part::Attachment(attachment) => parts.push(json!({
                    "inlineData": { "mimeType": attachment.media_type(), "data": attachment.base64() }
                })),
                Part::ToolCall(call) => {
                    let mut function = json!({ "name": call.name, "args": call.arguments });
                    if let Some(id) = call.meta.as_ref().and_then(|m| m.get("id")) {
                        function["id"] = id.clone();
                    }
                    let mut entry = json!({ "functionCall": function });
                    if let Some(signature) = call.meta.as_ref().and_then(|m| m.get("thoughtSignature")) {
                        entry["thoughtSignature"] = signature.clone();
                    }
                    parts.push(entry);
                }
                Part::ToolResult(result) => {
                    let response = if result.content.is_object() {
                        result.content.clone()
                    } else {
                        json!({ "result": result.content })
                    };
                    let mut function = json!({ "name": result.name, "response": response });
                    if !result.call_id.starts_with("gemini_") {
                        function["id"] = json!(result.call_id);
                    }
                    parts.push(json!({ "functionResponse": function }));
                }
                _ => {}
            }
        }
        if parts.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last["role"] == role => {
                if let Some(existing) = last["parts"].as_array_mut() {
                    existing.extend(parts);
                }
            }
            _ => out.push(json!({ "role": role, "parts": parts })),
        }
    }
    out
}

/// Joins adjacent text parts.
fn merge_text(parts: Vec<Part>) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for part in parts {
        match (out.last_mut(), part) {
            (Some(Part::Text { text }), Part::Text { text: more }) => text.push_str(&more),
            (_, part) => out.push(part),
        }
    }
    out
}

/// What a (streamed) answer has said so far.
#[derive(Debug, Default)]
struct State {
    usage: Usage,
    finish: Option<Finish>,
    calls: usize,
}

impl State {
    /// The chunks of one response object.
    fn on_response(&mut self, value: &Value) -> Result<Vec<StreamChunk>, ProviderError> {
        if let Some(error) = value.get("error") {
            return Err(ProviderError::unavailable(
                error["message"]
                    .as_str()
                    .unwrap_or("stream error")
                    .to_owned(),
            ));
        }
        if let Some(meta) = value.get("usageMetadata") {
            let count = |key: &str| meta.get(key).and_then(Value::as_u64).unwrap_or(0);
            self.usage = Usage {
                input_tokens: count("promptTokenCount"),
                output_tokens: count("candidatesTokenCount") + count("thoughtsTokenCount"),
                cached_input_tokens: count("cachedContentTokenCount"),
            };
        }
        if value.pointer("/promptFeedback/blockReason").is_some() {
            self.finish = Some(Finish::Refused);
        }
        let mut chunks = Vec::new();
        let Some(candidate) = value.pointer("/candidates/0") else {
            return Ok(chunks);
        };
        for part in candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if part.get("thought").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    chunks.push(StreamChunk::Text(text.to_owned()));
                }
            } else if let Some(call) = part.get("functionCall") {
                self.calls += 1;
                let mut meta = serde_json::Map::new();
                if let Some(signature) = part.get("thoughtSignature") {
                    meta.insert("thoughtSignature".to_owned(), signature.clone());
                }
                let id = match call.get("id").and_then(Value::as_str) {
                    Some(id) => {
                        meta.insert("id".to_owned(), json!(id));
                        id.to_owned()
                    }
                    None => format!("gemini_{}", self.calls),
                };
                chunks.push(StreamChunk::ToolCall(ToolCall {
                    id,
                    name: call["name"].as_str().unwrap_or_default().to_owned(),
                    arguments: call.get("args").cloned().unwrap_or_else(|| json!({})),
                    meta: (!meta.is_empty()).then_some(Value::Object(meta)),
                }));
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            self.finish = Some(match reason {
                "MAX_TOKENS" => Finish::MaxTokens,
                "SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII"
                | "IMAGE_SAFETY" => Finish::Refused,
                _ => Finish::Stop,
            });
        }
        Ok(chunks)
    }

    /// The usage and finish at the end.
    fn end(&self) -> (Usage, Finish) {
        let finish = match self.finish {
            Some(Finish::Stop) | None if self.calls > 0 => Finish::ToolCalls,
            Some(finish) => finish,
            None => Finish::Stop,
        };
        (self.usage, finish)
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
            model_name: "g".into(),
            model: "gemini-3-flash".into(),
            system: Some("Be brief.".into()),
            messages: vec![
                Message::user("Look it up"),
                Message::new(
                    Role::Assistant,
                    vec![Part::ToolCall(ToolCall {
                        id: "fc_1".into(),
                        name: "lookup".into(),
                        arguments: json!({ "id": 3 }),
                        meta: Some(json!({ "id": "fc_1", "thoughtSignature": "sig" })),
                    })],
                ),
                Message::new(
                    Role::User,
                    vec![Part::ToolResult(ToolResult {
                        call_id: "fc_1".into(),
                        name: "lookup".into(),
                        content: json!([1, 2]),
                        is_error: false,
                    })],
                ),
            ],
            max_tokens: 64,
            temperature: Some(0.1),
            stop: Vec::new(),
            tools: vec![ToolDefinition {
                name: "lookup".into(),
                description: "Looks up.".into(),
                parameters: json!({ "type": "object", "properties": {} }),
            }],
            tool_choice: ToolChoice::Auto,
            output: Some(OutputFormat {
                name: "T".into(),
                schema: json!({ "type": "object" }),
            }),
            user: None,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn the_body_has_the_documented_shape() {
        let body = body(&request());
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be brief.");
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 64);
        assert_eq!(
            body["generationConfig"]["responseMimeType"],
            "application/json"
        );
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(
            body["contents"][1]["parts"][0]["functionCall"]["id"],
            "fc_1"
        );
        assert_eq!(body["contents"][1]["parts"][0]["thoughtSignature"], "sig");
        assert_eq!(
            body["contents"][2]["parts"][0]["functionResponse"]["response"],
            json!({ "result": [1, 2] })
        );
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["name"],
            "lookup"
        );
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "AUTO");
    }

    #[test]
    fn responses_parse() {
        let mut state = State::default();
        let chunks = state
            .on_response(&json!({
                "candidates": [{
                    "content": { "parts": [
                        { "text": "Thinking", "thought": true },
                        { "text": "Calling" },
                        { "functionCall": { "name": "lookup", "args": { "id": 3 } }, "thoughtSignature": "s" }
                    ]},
                    "finishReason": "STOP"
                }],
                "usageMetadata": { "promptTokenCount": 10, "candidatesTokenCount": 4, "thoughtsTokenCount": 2 }
            }))
            .unwrap();
        assert_eq!(chunks[0], StreamChunk::Text("Calling".into()));
        match &chunks[1] {
            StreamChunk::ToolCall(call) => {
                assert_eq!(call.id, "gemini_1");
                assert_eq!(call.meta, Some(json!({ "thoughtSignature": "s" })));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(state.end(), (Usage::new(10, 6), Finish::ToolCalls));
        let mut state = State::default();
        state
            .on_response(&json!({ "candidates": [{ "finishReason": "SAFETY" }] }))
            .unwrap();
        assert_eq!(state.end().1, Finish::Refused);
    }
}

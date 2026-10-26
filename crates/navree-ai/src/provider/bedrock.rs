//! AWS Bedrock: the Converse API (`/model/{id}/converse`,
//! `/converse-stream`) for chat and `InvokeModel` for Titan and Cohere
//! embeddings, signed with SigV4.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use chrono::Utc;
use futures_util::StreamExt;
use navree_http_client::{HttpClient, RequestBuilder};
use navree_kernel::BoxFuture;
use serde_json::{Value, json};
use url::Url;

use super::eventstream::{Decoder, EventMessage};
use super::sigv4::{self, Credentials, Signable};
use super::wire;
use super::{
    Capabilities, ChatRequest, ChatResponse, ChunkStream, Driver, EmbedRequest, EmbedResponse,
    StreamChunk, ToolChoice,
};
use crate::error::ProviderError;
use crate::message::{AttachmentKind, Message, Part, Role, ToolCall};
use crate::options::ProviderConfig;
use crate::response::{Finish, Usage};

/// The driver name reasoning parts are tagged with.
const DRIVER: &str = "bedrock";

/// A Bedrock account in one region.
pub(crate) struct Bedrock {
    name: String,
    region: Option<String>,
    base: Option<String>,
    explicit: (Option<String>, Option<String>, Option<String>),
    profile: Option<String>,
    credentials: OnceLock<Option<Credentials>>,
    http: HttpClient,
}

impl Bedrock {
    pub(crate) fn new(name: &str, config: &ProviderConfig, http: HttpClient) -> Self {
        let region = config
            .region
            .clone()
            .or_else(|| std::env::var("AWS_REGION").ok())
            .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
            .filter(|r| !r.is_empty());
        Self {
            name: name.to_owned(),
            region,
            base: config.url.clone().map(|u| u.trim_end_matches('/').to_owned()),
            explicit: (
                config.access_key_id.clone(),
                config.secret_access_key.clone(),
                config.session_token.clone(),
            ),
            profile: config.profile.clone(),
            credentials: OnceLock::new(),
            http,
        }
    }

    fn credentials(&self) -> Option<&Credentials> {
        self.credentials
            .get_or_init(|| {
                sigv4::resolve_credentials(
                    (
                        self.explicit.0.as_deref(),
                        self.explicit.1.as_deref(),
                        self.explicit.2.as_deref(),
                    ),
                    self.profile.as_deref(),
                    |name| std::env::var(name).ok(),
                    |path| std::fs::read_to_string(path).ok(),
                )
            })
            .as_ref()
    }

    /// A signed `POST {base}/model/{model}/{action}` with `body`.
    fn post(&self, model: &str, action: &str, body: &Value) -> Result<RequestBuilder, ProviderError> {
        let region = self.region.as_deref().ok_or_else(|| ProviderError::Auth {
            message: format!(
                "no region: set APP_AI__PROVIDERS__{}__REGION or AWS_REGION",
                self.name.to_uppercase()
            ),
        })?;
        let credentials = self.credentials().ok_or_else(|| ProviderError::Auth {
            message: "no AWS credentials: set access_key_id and secret_access_key, \
                      AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY, or a shared credentials profile"
                .to_owned(),
        })?;
        let base = self
            .base
            .clone()
            .unwrap_or_else(|| format!("https://bedrock-runtime.{region}.amazonaws.com"));
        let url = Url::parse(&format!("{base}/model/{}/{action}", sigv4::uri_encode(model)))
            .map_err(|error| ProviderError::request(format!("invalid Bedrock URL: {error}")))?;
        let host = match url.port() {
            Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
            None => url.host_str().unwrap_or_default().to_owned(),
        };
        let bytes = serde_json::to_vec(body).unwrap_or_default();
        let amz_date = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let now = chrono::NaiveDateTime::parse_from_str(&amz_date, "%Y%m%dT%H%M%SZ")
            .map(|n| n.and_utc())
            .unwrap_or_else(|_| Utc::now());
        let mut headers = vec![
            ("content-type", "application/json"),
            ("host", host.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];
        if let Some(token) = &credentials.session_token {
            headers.push(("x-amz-security-token", token.as_str()));
        }
        let authorization = sigv4::authorization(
            &Signable {
                method: "POST",
                path: url.path(),
                query: &[],
                headers: &headers,
                body: &bytes,
            },
            credentials,
            region,
            "bedrock",
            now,
        );
        let mut request = self
            .http
            .post(url.as_str())
            .header("content-type", "application/json")
            .header("x-amz-date", amz_date.as_str())
            .header("authorization", authorization.as_str())
            .body(bytes);
        if let Some(token) = &credentials.session_token {
            request = request.header("x-amz-security-token", token.as_str());
        }
        Ok(request)
    }
}

impl Driver for Bedrock {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            images: true,
            pdfs: true,
            embeddings: true,
            native_structured: false,
        }
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let body = body(request);
            let answer = wire::send_json(
                self.post(&request.model, "converse", &body)?,
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
            let body = body(request);
            let response = wire::open_stream(
                self.post(&request.model, "converse-stream", &body)?,
                request.timeout,
                wire::common_message,
            )
            .await?;
            let messages = wire::framed(
                response,
                (Decoder::default(), None::<String>),
                |(decoder, failure), bytes| match decoder.feed(bytes) {
                    Ok(messages) => messages.into_iter().map(Ok).collect(),
                    Err(error) => {
                        *failure = Some(error.clone());
                        vec![Err(error)]
                    }
                },
                |_| None,
            );
            let stream = messages
                .scan(StreamState::default(), |state, message| {
                    let chunks = match message {
                        Ok(Ok(message)) => state.on_message(&message),
                        Ok(Err(error)) => vec![Err(ProviderError::unavailable(error))],
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
            if request.model.starts_with("cohere.") {
                let answer = wire::send_json(
                    self.post(
                        &request.model,
                        "invoke",
                        &json!({ "texts": request.texts, "input_type": "search_document" }),
                    )?,
                    request.timeout,
                    wire::common_message,
                )
                .await?;
                let vectors = answer["embeddings"]
                    .as_array()
                    .ok_or_else(|| ProviderError::unavailable("an answer without embeddings"))?
                    .iter()
                    .map(floats)
                    .collect();
                return Ok(EmbedResponse {
                    vectors,
                    input_tokens: 0,
                });
            }
            if !request.model.starts_with("amazon.titan-embed") {
                return Err(ProviderError::unsupported(format!(
                    "Bedrock embeddings support Amazon Titan and Cohere models, not `{}`",
                    request.model
                )));
            }
            let mut vectors = Vec::with_capacity(request.texts.len());
            let mut tokens = 0;
            for text in &request.texts {
                let mut body = json!({ "inputText": text });
                if let Some(dimensions) = request.dimensions {
                    body["dimensions"] = json!(dimensions);
                }
                let answer = wire::send_json(
                    self.post(&request.model, "invoke", &body)?,
                    request.timeout,
                    wire::common_message,
                )
                .await?;
                vectors.push(floats(&answer["embedding"]));
                tokens += answer["inputTextTokenCount"].as_u64().unwrap_or(0);
            }
            Ok(EmbedResponse {
                vectors,
                input_tokens: tokens,
            })
        })
    }
}

fn floats(value: &Value) -> Vec<f32> {
    value
        .as_array()
        .map(|values| values.iter().filter_map(Value::as_f64).map(|v| v as f32).collect())
        .unwrap_or_default()
}

/// The Converse request body.
pub(crate) fn body(request: &ChatRequest) -> Value {
    let mut body = json!({ "messages": messages(&request.messages) });
    if let Some(system) = &request.system {
        body["system"] = json!([{ "text": system }]);
    }
    let mut inference = json!({ "maxTokens": request.max_tokens });
    if let Some(temperature) = request.temperature {
        inference["temperature"] = json!(temperature);
    }
    if !request.stop.is_empty() {
        inference["stopSequences"] = json!(request.stop);
    }
    body["inferenceConfig"] = inference;
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "toolSpec": {
                        "name": tool.name,
                        "description": tool.description,
                        "inputSchema": { "json": tool.parameters },
                    }
                })
            })
            .collect();
        // Tool blocks in the messages need the tool config.
        body["toolConfig"] = json!({ "tools": tools, "toolChoice": { "auto": {} } });
        // Converse has no "none" choice: a note after the last message asks
        // for an answer without tools (the earlier messages and the system
        // prompt stay as they were).
        if request.tool_choice == ToolChoice::None
            && let Some(last) = body["messages"].as_array_mut().and_then(|m| m.last_mut())
            && last["role"] == "user"
            && let Some(content) = last["content"].as_array_mut()
        {
            content.push(json!({ "text": NO_TOOLS_NOTE }));
        }
    }
    body
}

/// What the last step of an agent run asks on Converse.
const NO_TOOLS_NOTE: &str = "You have reached the step limit of this run: answer now, without \
                             calling tools.";

fn messages(messages: &[Message]) -> Vec<Value> {
    let mut out: Vec<(Role, Vec<Value>)> = Vec::new();
    for message in messages {
        let mut blocks = Vec::new();
        let mut texts = Vec::new();
        for part in &message.parts {
            match part {
                Part::Text { text } if !text.is_empty() => texts.push(json!({ "text": text })),
                Part::Attachment(attachment) => {
                    let format = attachment
                        .media_type()
                        .rsplit('/')
                        .next()
                        .unwrap_or("png")
                        .replace("jpg", "jpeg");
                    match attachment.kind() {
                        AttachmentKind::Image => blocks.push(json!({
                            "image": { "format": format, "source": { "bytes": attachment.base64() } }
                        })),
                        _ => blocks.push(json!({
                            "document": {
                                "format": if attachment.kind() == AttachmentKind::Pdf { "pdf".to_owned() } else { format },
                                "name": document_name(attachment.name()),
                                "source": { "bytes": attachment.base64() },
                            }
                        })),
                    }
                }
                Part::ToolCall(call) => {
                    blocks.append(&mut texts);
                    blocks.push(json!({
                        "toolUse": { "toolUseId": call.id, "name": call.name, "input": call.arguments }
                    }));
                }
                Part::ToolResult(result) => blocks.push(json!({
                    "toolResult": {
                        "toolUseId": result.call_id,
                        "content": [{ "text": result.content.to_string() }],
                        "status": if result.is_error { "error" } else { "success" },
                    }
                })),
                Part::Reasoning { provider, data } if provider == DRIVER => blocks.push(data.clone()),
                _ => {}
            }
        }
        blocks.extend(texts);
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

/// Document names allow letters, digits, spaces, hyphens, parentheses and
/// square brackets.
fn document_name(name: Option<&str>) -> String {
    let cleaned: String = name
        .unwrap_or("document")
        .rsplit_once('.')
        .map_or(name.unwrap_or("document"), |(stem, _)| stem)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " -()[]".contains(c) { c } else { '-' }
        })
        .collect();
    if cleaned.trim().is_empty() { "document".to_owned() } else { cleaned }
}

/// A whole Converse answer.
pub(crate) fn parse_response(answer: &Value) -> Result<ChatResponse, ProviderError> {
    let content = answer
        .pointer("/output/message/content")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::unavailable("an answer without content"))?;
    let mut parts = Vec::new();
    for block in content {
        if let Some(text) = block.get("text").and_then(Value::as_str) {
            parts.push(Part::Text { text: text.to_owned() });
        } else if let Some(tool) = block.get("toolUse") {
            parts.push(Part::ToolCall(ToolCall {
                id: tool["toolUseId"].as_str().unwrap_or_default().to_owned(),
                name: tool["name"].as_str().unwrap_or_default().to_owned(),
                arguments: tool.get("input").cloned().unwrap_or_else(|| json!({})),
                meta: None,
            }));
        } else if block.get("reasoningContent").is_some() {
            parts.push(Part::Reasoning {
                provider: DRIVER.to_owned(),
                data: block.clone(),
            });
        }
    }
    Ok(ChatResponse {
        parts,
        usage: usage(answer.get("usage")),
        finish: finish(answer["stopReason"].as_str()),
    })
}

fn usage(usage: Option<&Value>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let cached = count("cacheReadInputTokens");
    Usage {
        input_tokens: count("inputTokens") + cached + count("cacheWriteInputTokens"),
        output_tokens: count("outputTokens"),
        cached_input_tokens: cached,
    }
}

fn finish(reason: Option<&str>) -> Finish {
    match reason {
        Some("max_tokens" | "model_context_window_exceeded") => Finish::MaxTokens,
        Some("tool_use") => Finish::ToolCalls,
        Some("guardrail_intervened" | "content_filtered") => Finish::Refused,
        _ => Finish::Stop,
    }
}

/// A tool call being streamed.
#[derive(Debug)]
enum Block {
    Tool { id: String, name: String, input: String },
    Reasoning { text: String, signature: Option<String>, redacted: Option<String> },
}

/// The state of a streamed answer.
#[derive(Debug, Default)]
struct StreamState {
    blocks: BTreeMap<u64, Block>,
    usage: Usage,
    finish: Finish,
    done: bool,
}

impl StreamState {
    fn on_message(&mut self, message: &EventMessage) -> Vec<Result<StreamChunk, ProviderError>> {
        let payload: Value = serde_json::from_slice(&message.payload).unwrap_or(Value::Null);
        if message.header(":message-type") == Some("exception") {
            let text = wire::common_message(&payload).unwrap_or_else(|| payload.to_string());
            let kind = message.header(":exception-type").unwrap_or_default();
            let error = match kind {
                "throttlingException" => ProviderError::RateLimited { retry_after: None },
                "validationException" => ProviderError::request(format!("{kind}: {text}")),
                _ => ProviderError::unavailable(format!("{kind}: {text}")),
            };
            return vec![Err(error)];
        }
        let index = payload["contentBlockIndex"].as_u64().unwrap_or(0);
        match message.header(":event-type") {
            Some("contentBlockStart") => {
                if let Some(tool) = payload.pointer("/start/toolUse") {
                    self.blocks.insert(
                        index,
                        Block::Tool {
                            id: tool["toolUseId"].as_str().unwrap_or_default().to_owned(),
                            name: tool["name"].as_str().unwrap_or_default().to_owned(),
                            input: String::new(),
                        },
                    );
                }
                Vec::new()
            }
            Some("contentBlockDelta") => {
                let delta = &payload["delta"];
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    return if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![Ok(StreamChunk::Text(text.to_owned()))]
                    };
                }
                if let Some(more) = delta.pointer("/toolUse/input").and_then(Value::as_str)
                    && let Some(Block::Tool { input, .. }) = self.blocks.get_mut(&index)
                {
                    input.push_str(more);
                }
                if let Some(reasoning) = delta.get("reasoningContent") {
                    let block = self.blocks.entry(index).or_insert(Block::Reasoning {
                        text: String::new(),
                        signature: None,
                        redacted: None,
                    });
                    if let Block::Reasoning { text, signature, redacted } = block {
                        if let Some(more) = reasoning["text"].as_str() {
                            text.push_str(more);
                        }
                        if let Some(sig) = reasoning["signature"].as_str() {
                            *signature = Some(sig.to_owned());
                        }
                        if let Some(data) = reasoning["redactedContent"].as_str() {
                            *redacted = Some(data.to_owned());
                        }
                    }
                }
                Vec::new()
            }
            Some("contentBlockStop") => match self.blocks.remove(&index) {
                Some(Block::Tool { id, name, input }) => {
                    let arguments = if input.trim().is_empty() {
                        Ok(json!({}))
                    } else {
                        serde_json::from_str(&input)
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
                Some(Block::Reasoning { text, signature, redacted }) => {
                    let data = match redacted {
                        Some(redacted) => json!({ "reasoningContent": { "redactedContent": redacted } }),
                        None => json!({
                            "reasoningContent": { "reasoningText": { "text": text, "signature": signature } }
                        }),
                    };
                    vec![Ok(StreamChunk::Reasoning(Part::Reasoning {
                        provider: DRIVER.to_owned(),
                        data,
                    }))]
                }
                None => Vec::new(),
            },
            Some("messageStop") => {
                self.finish = finish(payload["stopReason"].as_str());
                Vec::new()
            }
            Some("metadata") => {
                self.usage = usage(payload.get("usage"));
                if self.done {
                    return Vec::new();
                }
                self.done = true;
                vec![Ok(StreamChunk::Done {
                    usage: self.usage,
                    finish: self.finish,
                })]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::message::{Attachment, ToolResult};
    use crate::provider::eventstream::encode;
    use crate::provider::ToolDefinition;

    #[test]
    fn the_body_has_the_converse_shape() {
        let request = ChatRequest {
            model_name: "aws".into(),
            model: "anthropic.claude-sonnet-5-5".into(),
            system: Some("Be brief.".into()),
            messages: vec![
                Message::new(
                    Role::User,
                    vec![
                        Part::Text { text: "Read this".into() },
                        Part::Attachment(Attachment::pdf(vec![1u8]).named("q4 report.pdf")),
                    ],
                ),
                Message::new(
                    Role::Assistant,
                    vec![Part::ToolCall(ToolCall {
                        id: "t1".into(),
                        name: "lookup".into(),
                        arguments: json!({ "id": 1 }),
                        meta: None,
                    })],
                ),
                Message::new(
                    Role::User,
                    vec![Part::ToolResult(ToolResult {
                        call_id: "t1".into(),
                        name: "lookup".into(),
                        content: json!({ "ok": true }),
                        is_error: true,
                    })],
                ),
            ],
            max_tokens: 10,
            temperature: None,
            stop: Vec::new(),
            tools: vec![ToolDefinition {
                name: "lookup".into(),
                description: "Looks up.".into(),
                parameters: json!({ "type": "object" }),
            }],
            tool_choice: ToolChoice::Auto,
            output: None,
            user: None,
            timeout: Duration::from_secs(5),
        };
        let body = body(&request);
        assert_eq!(body["system"][0]["text"], "Be brief.");
        assert_eq!(body["inferenceConfig"]["maxTokens"], 10);
        let first = &body["messages"][0]["content"];
        assert_eq!(first[0]["document"]["format"], "pdf");
        assert_eq!(first[0]["document"]["name"], "q4 report");
        assert_eq!(first[1]["text"], "Read this");
        assert_eq!(body["messages"][1]["content"][0]["toolUse"]["toolUseId"], "t1");
        assert_eq!(body["messages"][2]["content"][0]["toolResult"]["status"], "error");
        assert_eq!(body["toolConfig"]["tools"][0]["toolSpec"]["inputSchema"]["json"]["type"], "object");
        assert_eq!(body["messages"][2]["content"].as_array().unwrap().len(), 1);

        // Converse has no "none" tool choice: the request asks for an answer
        // without tools in a note after the last message, and the system
        // prompt and earlier messages stay as they were.
        let mut last_step = request.clone();
        last_step.tool_choice = ToolChoice::None;
        let body_none = super::body(&last_step);
        assert_eq!(body_none["system"], body["system"]);
        assert_eq!(
            body_none["messages"].as_array().unwrap()[..2],
            body["messages"].as_array().unwrap()[..2]
        );
        let tail = &body_none["messages"][2]["content"];
        assert_eq!(tail[0], body["messages"][2]["content"][0]);
        assert!(tail[1]["text"].as_str().unwrap().contains("without calling tools"), "{tail}");
        assert!(body_none.get("toolConfig").is_some(), "tool blocks need the tool config");
    }

    #[test]
    fn responses_parse() {
        let answer = json!({
            "output": { "message": { "role": "assistant", "content": [
                { "text": "Hi" },
                { "toolUse": { "toolUseId": "t9", "name": "lookup", "input": { "id": 2 } } }
            ]}},
            "stopReason": "tool_use",
            "usage": { "inputTokens": 11, "outputTokens": 3 }
        });
        let response = parse_response(&answer).unwrap();
        assert_eq!(response.text(), "Hi");
        assert_eq!(response.tool_calls()[0].id, "t9");
        assert_eq!(response.finish, Finish::ToolCalls);
        assert_eq!(response.usage, Usage::new(11, 3));
    }

    #[test]
    fn stream_messages_assemble() {
        let frames = [
            encode(&[(":event-type", "messageStart"), (":message-type", "event")], br#"{"role":"assistant"}"#),
            encode(&[(":event-type", "contentBlockDelta"), (":message-type", "event")], br#"{"contentBlockIndex":0,"delta":{"text":"Hi"}}"#),
            encode(&[(":event-type", "contentBlockStart"), (":message-type", "event")], br#"{"contentBlockIndex":1,"start":{"toolUse":{"toolUseId":"t1","name":"lookup"}}}"#),
            encode(&[(":event-type", "contentBlockDelta"), (":message-type", "event")], br#"{"contentBlockIndex":1,"delta":{"toolUse":{"input":"{\"id\":"}}}"#),
            encode(&[(":event-type", "contentBlockDelta"), (":message-type", "event")], br#"{"contentBlockIndex":1,"delta":{"toolUse":{"input":"4}"}}}"#),
            encode(&[(":event-type", "contentBlockStop"), (":message-type", "event")], br#"{"contentBlockIndex":1}"#),
            encode(&[(":event-type", "messageStop"), (":message-type", "event")], br#"{"stopReason":"tool_use"}"#),
            encode(&[(":event-type", "metadata"), (":message-type", "event")], br#"{"usage":{"inputTokens":5,"outputTokens":2}}"#),
        ];
        let mut decoder = Decoder::default();
        let mut state = StreamState::default();
        let chunks: Vec<StreamChunk> = frames
            .iter()
            .flat_map(|frame| decoder.feed(frame).unwrap())
            .flat_map(|message| state.on_message(&message))
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            chunks,
            [
                StreamChunk::Text("Hi".into()),
                StreamChunk::ToolCall(ToolCall {
                    id: "t1".into(),
                    name: "lookup".into(),
                    arguments: json!({ "id": 4 }),
                    meta: None
                }),
                StreamChunk::Done {
                    usage: Usage::new(5, 2),
                    finish: Finish::ToolCalls
                },
            ]
        );
        let throttled = encode(
            &[(":message-type", "exception"), (":exception-type", "throttlingException")],
            br#"{"message":"Too many requests"}"#,
        );
        let message = &Decoder::default().feed(&throttled).unwrap()[0];
        assert!(matches!(
            StreamState::default().on_message(message)[0],
            Err(ProviderError::RateLimited { .. })
        ));
    }
}

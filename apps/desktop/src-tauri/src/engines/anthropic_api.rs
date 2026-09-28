//! The Claude API engine (docs/PLAN.md 6.4): the Messages API with streaming,
//! the user's own API key from the keychain, and the server-side web search
//! tool. Request shapes checked against Anthropic's docs on 28 Sep 2026.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::sse::{SseEvent, SseParser};
use super::{
    AskOutcome, AskRequest, Block, Effort, Engine, EngineError, EngineEvent, EngineInfo, Limits,
    Message, Readiness, Role, StopReason, Usage,
};
use crate::secrets::Secrets;

pub const ENGINE_ID: &str = "anthropic_api";
const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A stream that stays silent this long is treated as dropped.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Retries for overload and server errors, before any output arrives.
const MAX_RETRIES: u32 = 2;
/// Waiting longer than this for a rate limit isn't worth it; say so instead.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(20);

/// The `anthropic_api` section of `config/models.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    low: Level,
    medium: Level,
    high: Level,
    max: Level,
}

#[derive(Clone, Debug, Deserialize)]
struct Level {
    model: String,
    max_tokens: u32,
    tool_budget: u32,
    web_search_max_uses: u32,
    /// The web search tool's type string, which depends on the model.
    web_search_tool: String,
    /// `output_config.effort`; left out for models that don't take it.
    effort: Option<String>,
    /// Server-side fallback mode for refused requests, with its beta header.
    fallbacks: Option<String>,
    #[serde(default)]
    betas: Vec<String>,
    /// USD per million tokens, for cost estimates in dev mode.
    price_input: f64,
    price_output: f64,
}

impl Config {
    fn level(&self, effort: Effort) -> &Level {
        match effort {
            Effort::Low => &self.low,
            Effort::Medium => &self.medium,
            Effort::High => &self.high,
            Effort::Max => &self.max,
        }
    }
}

pub struct AnthropicApi {
    config: Config,
    secrets: Arc<Secrets>,
    http: reqwest::Client,
}

impl AnthropicApi {
    pub fn new(config: Config, secrets: Arc<Secrets>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("building the HTTP client");
        Self {
            config,
            secrets,
            http,
        }
    }

    /// Sends the request, retrying overloads and server errors a few times.
    async fn send(
        &self,
        key: &str,
        level: &Level,
        body: &Value,
        cancel: &CancellationToken,
    ) -> Result<reqwest::Response, EngineError> {
        let mut attempt = 0;
        loop {
            let mut request = self
                .http
                .post(ENDPOINT)
                .header("x-api-key", key)
                .header("anthropic-version", API_VERSION)
                .json(body);
            if !level.betas.is_empty() {
                request = request.header("anthropic-beta", level.betas.join(","));
            }
            let response = tokio::select! {
                _ = cancel.cancelled() => return Err(EngineError::Cancelled),
                r = request.send() => r.map_err(|e| EngineError::Offline(e.to_string()))?,
            };
            if response.status().is_success() {
                return Ok(response);
            }
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let body = response.text().await.unwrap_or_default();
            let error = http_error(status, &headers, &body);
            let wait = retry_wait(&error, attempt);
            match wait {
                Some(wait) if attempt < MAX_RETRIES => {
                    log::info!("retrying after {error} in {wait:?}");
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(EngineError::Cancelled),
                        _ = tokio::time::sleep(wait) => {}
                    }
                    attempt += 1;
                }
                _ => return Err(error),
            }
        }
    }
}

#[async_trait]
impl Engine for AnthropicApi {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            id: ENGINE_ID,
            name: "Claude API",
            key_label: "Claude API key",
            key_help: "Create a dedicated key with a spend limit in the Claude Console.",
            key_help_url: "https://platform.claude.com/settings/keys",
        }
    }

    fn limits(&self, effort: Effort) -> Limits {
        Limits {
            tool_budget: self.config.level(effort).tool_budget,
        }
    }

    async fn check_ready(&self) -> Readiness {
        if self.secrets.get(ENGINE_ID).is_some() {
            Readiness::Ready
        } else {
            Readiness::NeedsSetup {
                reason: "Add your Claude API key in Settings.".into(),
            }
        }
    }

    async fn ask(
        &self,
        req: AskRequest,
        events: mpsc::UnboundedSender<EngineEvent>,
        cancel: CancellationToken,
    ) -> Result<AskOutcome, EngineError> {
        let key = self
            .secrets
            .get(ENGINE_ID)
            .ok_or(EngineError::NotConfigured)?;
        let level = self.config.level(req.effort);
        let body = request_body(level, &req);
        let response = self.send(&key, level, &body, &cancel).await?;

        let mut stream = response.bytes_stream();
        let mut parser = SseParser::default();
        let mut state = StreamState::default();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(EngineError::Cancelled),
                next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()) => match next {
                    Err(_) => return Err(EngineError::Offline("the stream went silent".into())),
                    Ok(None) => return Err(EngineError::Offline("the stream ended early".into())),
                    Ok(Some(Err(e))) => return Err(EngineError::Offline(e.to_string())),
                    Ok(Some(Ok(chunk))) => chunk,
                },
            };
            for event in parser.push(&chunk) {
                if state.apply(&event, &events)? {
                    let mut outcome = state.finish()?;
                    outcome.usage.cost_usd = Some(cost(level, &outcome.usage));
                    let _ = events.send(EngineEvent::Usage(outcome.usage));
                    return Ok(outcome);
                }
            }
        }
    }
}

fn request_body(level: &Level, req: &AskRequest) -> Value {
    let mut tools: Vec<Value> = req
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
                // Stream tool inputs as they're written; validated on arrival.
                "eager_input_streaming": true,
            })
        })
        .collect();
    if req.web_search {
        tools.push(json!({
            "type": level.web_search_tool,
            "name": "web_search",
            "max_uses": level.web_search_max_uses,
        }));
    }

    let mut body = json!({
        "model": level.model,
        "max_tokens": level.max_tokens,
        "stream": true,
        "system": [{ "type": "text", "text": req.system }],
        "messages": req.messages.iter().map(to_wire).collect::<Vec<_>>(),
        // Caches the whole prompt, so follow-ups reuse it.
        "cache_control": { "type": "ephemeral" },
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if let Some(effort) = &level.effort {
        body["output_config"] = json!({ "effort": effort });
    }
    if let Some(fallbacks) = &level.fallbacks {
        body["fallbacks"] = json!(fallbacks);
    }
    body
}

fn to_wire(message: &Message) -> Value {
    let content: Vec<Value> = message
        .content
        .iter()
        .filter_map(|block| match block {
            Block::Text(text) if text.is_empty() => None,
            Block::Text(text) => Some(json!({ "type": "text", "text": text })),
            Block::Image { media_type, data } => Some(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media_type, "data": STANDARD.encode(data) },
            })),
            Block::ToolUse { id, name, input } => {
                Some(json!({ "type": "tool_use", "id": id, "name": name, "input": input }))
            }
            Block::ToolResult {
                id,
                content,
                is_error,
            } => Some(json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": content,
                "is_error": is_error,
            })),
            Block::Opaque { engine, data } if *engine == ENGINE_ID => Some(data.clone()),
            Block::Opaque { .. } => None,
        })
        .collect();
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    json!({ "role": role, "content": content })
}

/// Builds the response message from the stream's events.
#[derive(Default)]
struct StreamState {
    blocks: Vec<Value>,
    /// Tool inputs arrive as JSON fragments, per block index.
    partial_json: HashMap<usize, String>,
    stop_reason: Option<String>,
    usage: Usage,
}

impl StreamState {
    /// Applies one event. Returns true when the message is complete.
    fn apply(
        &mut self,
        event: &SseEvent,
        out: &mpsc::UnboundedSender<EngineEvent>,
    ) -> Result<bool, EngineError> {
        if event.event == "ping" {
            return Ok(false);
        }
        let data: Value = serde_json::from_str(&event.data)
            .map_err(|e| EngineError::Protocol(format!("bad event data: {e}")))?;
        let emit = |e: EngineEvent| {
            let _ = out.send(e);
        };
        match data["type"].as_str().unwrap_or(&event.event) {
            "message_start" => {
                let usage = &data["message"]["usage"];
                self.usage.input_tokens = count(&usage["input_tokens"]);
                self.usage.cache_read_tokens = count(&usage["cache_read_input_tokens"]);
                self.usage.cache_write_tokens = count(&usage["cache_creation_input_tokens"]);
            }
            "content_block_start" => {
                let index = index(&data)?;
                let block = data["content_block"].clone();
                match block["type"].as_str().unwrap_or_default() {
                    "thinking" | "redacted_thinking" => {
                        emit(EngineEvent::Status(Some("Thinking…".into())));
                    }
                    "text" => emit(EngineEvent::Status(None)),
                    "server_tool_use" if block["name"] == "web_search" => {
                        emit(EngineEvent::ToolStarted {
                            name: "web_search".into(),
                            summary: "Searching the web…".into(),
                        });
                    }
                    "web_search_tool_result" => emit(EngineEvent::ToolFinished {
                        name: "web_search".into(),
                    }),
                    "tool_use" => {
                        let name = block["name"].as_str().unwrap_or_default().to_string();
                        emit(EngineEvent::ToolStarted {
                            summary: name.clone(),
                            name,
                        });
                    }
                    "fallback" => emit(EngineEvent::Status(Some("Trying again…".into()))),
                    _ => {}
                }
                if self.blocks.len() <= index {
                    self.blocks.resize(index + 1, Value::Null);
                }
                self.blocks[index] = block;
            }
            "content_block_delta" => {
                let index = index(&data)?;
                let delta = &data["delta"];
                let block = self
                    .blocks
                    .get_mut(index)
                    .ok_or_else(|| EngineError::Protocol("delta for a missing block".into()))?;
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let text = delta["text"].as_str().unwrap_or_default();
                        append(block, "text", text);
                        emit(EngineEvent::TextDelta(text.to_string()));
                    }
                    "thinking_delta" => {
                        append(
                            block,
                            "thinking",
                            delta["thinking"].as_str().unwrap_or_default(),
                        );
                    }
                    "signature_delta" => block["signature"] = delta["signature"].clone(),
                    "input_json_delta" => self
                        .partial_json
                        .entry(index)
                        .or_default()
                        .push_str(delta["partial_json"].as_str().unwrap_or_default()),
                    "citations_delta" => {
                        if !block["citations"].is_array() {
                            block["citations"] = json!([]);
                        }
                        if let Some(list) = block["citations"].as_array_mut() {
                            list.push(delta["citation"].clone());
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = index(&data)?;
                if let Some(json) = self.partial_json.remove(&index)
                    && let Some(block) = self.blocks.get_mut(index)
                {
                    // Eager streaming skips server-side validation, so a bad
                    // fragment becomes an input the tool will reject.
                    block["input"] = if json.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&json).unwrap_or(Value::Null)
                    };
                    if block["type"] == "server_tool_use"
                        && block["name"] == "web_search"
                        && let Some(query) = block["input"]["query"].as_str()
                    {
                        emit(EngineEvent::ToolStarted {
                            name: "web_search".into(),
                            summary: format!("Searching the web for “{query}”…"),
                        });
                    }
                }
            }
            "message_delta" => {
                if let Some(reason) = data["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(reason.to_string());
                }
                let output = count(&data["usage"]["output_tokens"]);
                if output > 0 {
                    self.usage.output_tokens = output;
                }
            }
            "message_stop" => return Ok(true),
            "error" => return Err(stream_error(&data["error"])),
            _ => {}
        }
        Ok(false)
    }

    fn finish(self) -> Result<AskOutcome, EngineError> {
        let stop = match self.stop_reason.as_deref() {
            Some("end_turn") | Some("stop_sequence") => StopReason::EndTurn,
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("pause_turn") => StopReason::PauseTurn,
            Some("refusal") => StopReason::Refusal,
            Some(_) => StopReason::Other,
            None => {
                return Err(EngineError::Protocol(
                    "the stream ended without a stop reason".into(),
                ));
            }
        };
        let content = echoable(self.blocks)
            .into_iter()
            .map(|block| match block["type"].as_str() {
                Some("text") => Block::Text(block["text"].as_str().unwrap_or_default().to_string()),
                Some("tool_use") => Block::ToolUse {
                    id: block["id"].as_str().unwrap_or_default().to_string(),
                    name: block["name"].as_str().unwrap_or_default().to_string(),
                    input: block["input"].clone(),
                },
                _ => Block::Opaque {
                    engine: ENGINE_ID,
                    data: block,
                },
            })
            .collect();
        Ok(AskOutcome {
            message: Message {
                role: Role::Assistant,
                content,
            },
            stop,
            usage: self.usage,
        })
    }
}

/// The blocks that may be sent back in later requests. After a server-side
/// fallback, the declined attempt's reasoning and unfinished tool calls
/// (everything before the last `fallback` block but text and paired
/// server-tool blocks) must be left out. The `fallback` marker itself is an
/// audit record the API ignores, so it goes too.
fn echoable(blocks: Vec<Value>) -> Vec<Value> {
    let blocks: Vec<Value> = blocks.into_iter().filter(|b| !b.is_null()).collect();
    let Some(boundary) = blocks.iter().rposition(|b| b["type"] == "fallback") else {
        return blocks;
    };
    let result_ids: Vec<&str> = blocks
        .iter()
        .filter(|b| {
            b["type"]
                .as_str()
                .is_some_and(|t| t.ends_with("_tool_result"))
        })
        .filter_map(|b| b["tool_use_id"].as_str())
        .collect();
    let keep_before = |b: &Value| match b["type"].as_str() {
        Some("text") => true,
        Some("server_tool_use") => b["id"].as_str().is_some_and(|id| result_ids.contains(&id)),
        Some(t) => t.ends_with("_tool_result"),
        None => false,
    };
    blocks
        .iter()
        .enumerate()
        .filter(|(i, b)| *i > boundary || keep_before(b))
        .map(|(_, b)| b.clone())
        .collect()
}

fn append(block: &mut Value, field: &str, text: &str) {
    let current = block[field].as_str().unwrap_or_default();
    block[field] = Value::String(format!("{current}{text}"));
}

fn index(data: &Value) -> Result<usize, EngineError> {
    data["index"]
        .as_u64()
        .map(|i| i as usize)
        .ok_or_else(|| EngineError::Protocol("event without a block index".into()))
}

fn count(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0)
}

fn cost(level: &Level, usage: &Usage) -> f64 {
    // Cache writes cost 1.25× and reads 0.1× the input price (5-minute TTL).
    let input = usage.input_tokens as f64
        + usage.cache_write_tokens as f64 * 1.25
        + usage.cache_read_tokens as f64 * 0.1;
    (input * level.price_input + usage.output_tokens as f64 * level.price_output) / 1_000_000.0
}

/// Maps an HTTP error response to an engine error.
fn http_error(status: u16, headers: &HeaderMap, body: &str) -> EngineError {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let message = parsed["error"]["message"]
        .as_str()
        .unwrap_or(body)
        .chars()
        .take(300)
        .collect::<String>();
    match status {
        401 => EngineError::InvalidKey,
        402 => EngineError::NoCredits,
        400 if message.to_lowercase().contains("credit balance") => EngineError::NoCredits,
        429 => EngineError::RateLimited {
            retry_after: headers
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<f64>().ok())
                .map(Duration::from_secs_f64),
        },
        529 => EngineError::Overloaded,
        _ => EngineError::Api { status, message },
    }
}

/// An `error` event in the middle of a stream.
fn stream_error(error: &Value) -> EngineError {
    let message = error["message"].as_str().unwrap_or_default().to_string();
    match error["type"].as_str() {
        Some("overloaded_error") => EngineError::Overloaded,
        Some("rate_limit_error") => EngineError::RateLimited { retry_after: None },
        Some("authentication_error") => EngineError::InvalidKey,
        Some("billing_error") => EngineError::NoCredits,
        _ => EngineError::Api {
            status: 500,
            message,
        },
    }
}

/// How long to wait before retrying, or None if it isn't worth retrying.
fn retry_wait(error: &EngineError, attempt: u32) -> Option<Duration> {
    let backoff = Duration::from_secs(1 << attempt);
    match error {
        EngineError::Overloaded => Some(backoff),
        EngineError::Api { status, .. } if *status >= 500 => Some(backoff),
        EngineError::RateLimited { retry_after } => match retry_after {
            Some(wait) if *wait <= MAX_RETRY_WAIT => Some(*wait),
            Some(_) => None,
            None => Some(backoff),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::ToolSpec;

    fn config() -> Config {
        let models: Value =
            serde_json::from_str(include_str!("../../../../../config/models.json")).unwrap();
        serde_json::from_value(models[ENGINE_ID].clone()).unwrap()
    }

    fn request(effort: Effort) -> AskRequest {
        AskRequest {
            effort,
            system: "You explain things.".into(),
            messages: vec![Message::user(vec![
                Block::Image {
                    media_type: "image/png".into(),
                    data: vec![1, 2, 3],
                },
                Block::Text("What is this?".into()),
                Block::Text(String::new()),
            ])],
            tools: vec![ToolSpec {
                name: "read_file".into(),
                description: "Reads a file".into(),
                input_schema: json!({"type": "object"}),
            }],
            web_search: true,
        }
    }

    #[test]
    fn the_config_covers_every_effort() {
        let config = config();
        for effort in Effort::ALL {
            let level = config.level(effort);
            assert!(
                !level.model.is_empty() && level.max_tokens > 0,
                "{effort:?}"
            );
        }
    }

    #[test]
    fn low_effort_sends_no_effort_setting() {
        let config = config();
        let body = request_body(config.level(Effort::Low), &request(Effort::Low));
        assert!(body.get("output_config").is_none());
        assert!(body.get("fallbacks").is_none());
        assert_eq!(body["stream"], true);
        assert_eq!(body["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn the_body_carries_images_tools_and_web_search() {
        let config = config();
        let level = config.level(Effort::High);
        let body = request_body(level, &request(Effort::High));
        assert_eq!(body["model"], level.model.as_str());
        assert_eq!(body["output_config"]["effort"], "high");
        assert_eq!(body["fallbacks"], "default");

        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2, "the empty text block is dropped");
        assert_eq!(content[0]["source"]["data"], "AQID");
        assert_eq!(content[0]["source"]["type"], "base64");

        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["eager_input_streaming"], true);
        assert_eq!(tools[1]["name"], "web_search");
        assert_eq!(tools[1]["type"], level.web_search_tool.as_str());
        assert_eq!(tools[1]["max_uses"], level.web_search_max_uses);
    }

    #[test]
    fn only_this_engines_opaque_blocks_are_sent_back() {
        let message = Message {
            role: Role::Assistant,
            content: vec![
                Block::Opaque {
                    engine: ENGINE_ID,
                    data: json!({"type": "thinking", "thinking": "", "signature": "s"}),
                },
                Block::Opaque {
                    engine: "other",
                    data: json!({"type": "whatever"}),
                },
                Block::Text("Hi".into()),
            ],
        };
        let wire = to_wire(&message);
        assert_eq!(wire["role"], "assistant");
        assert_eq!(wire["content"].as_array().unwrap().len(), 2);
        assert_eq!(wire["content"][0]["signature"], "s");
    }

    /// Runs a recorded stream through the parser and state machine.
    fn replay(stream: &str) -> (Result<AskOutcome, EngineError>, Vec<EngineEvent>) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut parser = SseParser::default();
        let mut state = StreamState::default();
        let mut result = Err(EngineError::Protocol("unfinished".into()));
        'outer: for chunk in stream.as_bytes().chunks(17) {
            for event in parser.push(chunk) {
                match state.apply(&event, &tx) {
                    Ok(true) => {
                        result = std::mem::take(&mut state).finish();
                        break 'outer;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        result = Err(e);
                        break 'outer;
                    }
                }
            }
        }
        drop(tx);
        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        (result, events)
    }

    const ANSWER_WITH_SEARCH: &str = include_str!("fixtures/answer_with_search.sse");
    const TOOL_CALL: &str = include_str!("fixtures/tool_call.sse");
    const MID_STREAM_ERROR: &str = include_str!("fixtures/overloaded.sse");
    const FALLBACK: &str = include_str!("fixtures/fallback.sse");

    #[test]
    fn a_streamed_answer_with_web_search_comes_back_whole() {
        let (outcome, events) = replay(ANSWER_WITH_SEARCH);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.stop, StopReason::EndTurn);
        assert_eq!(
            outcome.message.text(),
            "TARGET: Echo Dot product title\nA smart speaker."
        );
        assert_eq!(outcome.usage.input_tokens, 2100);
        assert_eq!(outcome.usage.cache_read_tokens, 1800);
        assert_eq!(outcome.usage.output_tokens, 42);

        // Thinking and the search round-trip are kept for follow-ups.
        let opaque: Vec<&str> = outcome
            .message
            .content
            .iter()
            .filter_map(|b| match b {
                Block::Opaque { data, .. } => data["type"].as_str(),
                _ => None,
            })
            .collect();
        assert_eq!(
            opaque,
            ["thinking", "server_tool_use", "web_search_tool_result"]
        );
        let thinking = outcome.message.content.iter().find_map(|b| match b {
            Block::Opaque { data, .. } if data["type"] == "thinking" => Some(data),
            _ => None,
        });
        assert_eq!(thinking.unwrap()["signature"], "sig-123");

        assert!(events.contains(&EngineEvent::Status(Some("Thinking…".into()))));
        assert!(events.contains(&EngineEvent::ToolStarted {
            name: "web_search".into(),
            summary: "Searching the web for “echo dot 5th gen”…".into(),
        }));
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, outcome.message.text());
    }

    #[test]
    fn tool_calls_arrive_with_parsed_input() {
        let (outcome, _) = replay(TOOL_CALL);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.stop, StopReason::ToolUse);
        let calls: Vec<_> = outcome.message.tool_uses().collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "toolu_1");
        assert_eq!(calls[0].1, "read_file");
        assert_eq!(calls[0].2, &json!({"path": "package.json"}));
    }

    #[test]
    fn an_error_event_mid_stream_is_reported() {
        let (outcome, _) = replay(MID_STREAM_ERROR);
        assert_eq!(outcome.unwrap_err(), EngineError::Overloaded);
    }

    #[test]
    fn after_a_fallback_the_declined_attempts_reasoning_is_dropped() {
        let (outcome, _) = replay(FALLBACK);
        let outcome = outcome.unwrap();
        let types: Vec<String> = outcome
            .message
            .content
            .iter()
            .map(|b| match b {
                Block::Text(_) => "text".to_string(),
                Block::Opaque { data, .. } => data["type"].as_str().unwrap().to_string(),
                other => format!("{other:?}"),
            })
            .collect();
        // The fallback marker itself is an audit record the API ignores; dropped too.
        assert_eq!(types, ["text", "text"]);
        assert_eq!(outcome.message.text(), "Partial. Continued answer.");
    }

    #[test]
    fn http_errors_map_to_friendly_kinds() {
        let mut headers = HeaderMap::new();
        assert_eq!(http_error(401, &headers, "{}"), EngineError::InvalidKey);
        assert_eq!(http_error(402, &headers, "{}"), EngineError::NoCredits);
        assert_eq!(
            http_error(
                400,
                &headers,
                r#"{"error":{"type":"invalid_request_error","message":"Your credit balance is too low"}}"#
            ),
            EngineError::NoCredits
        );
        assert_eq!(http_error(529, &headers, "{}"), EngineError::Overloaded);
        headers.insert(RETRY_AFTER, "3".parse().unwrap());
        assert_eq!(
            http_error(429, &headers, "{}"),
            EngineError::RateLimited {
                retry_after: Some(Duration::from_secs(3))
            }
        );
        assert!(matches!(
            http_error(404, &HeaderMap::new(), r#"{"error":{"message":"model: x"}}"#),
            EngineError::Api { status: 404, ref message } if message == "model: x"
        ));
    }

    #[test]
    fn only_temporary_failures_are_retried() {
        assert_eq!(
            retry_wait(&EngineError::Overloaded, 0),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            retry_wait(&EngineError::Overloaded, 1),
            Some(Duration::from_secs(2))
        );
        let soon = EngineError::RateLimited {
            retry_after: Some(Duration::from_secs(5)),
        };
        assert_eq!(retry_wait(&soon, 0), Some(Duration::from_secs(5)));
        let late = EngineError::RateLimited {
            retry_after: Some(Duration::from_secs(120)),
        };
        assert_eq!(retry_wait(&late, 0), None);
        assert_eq!(retry_wait(&EngineError::InvalidKey, 0), None);
    }

    #[test]
    fn cost_counts_cache_reads_cheaper() {
        let level = config().low.clone();
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            cache_read_tokens: 1_000_000,
            cache_write_tokens: 0,
            cost_usd: None,
        };
        let expected = level.price_input * 1.1;
        assert!((cost(&level, &usage) - expected).abs() < 1e-9);
    }
}

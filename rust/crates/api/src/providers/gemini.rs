use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::http_client::build_http_client_or_default;
use crate::types::{
    InputContentBlock, MessageRequest, MessageResponse, MessageStartEvent, MessageStopEvent,
    OutputContentBlock, StreamEvent, ToolChoice, ToolDefinition, ToolResultContentBlock, Usage,
};

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl GeminiClient {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            http: build_http_client_or_default(),
            api_key,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    pub async fn send_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse, ApiError> {
        let endpoint_model = normalize_model_name(&request.model);
        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url.trim_end_matches('/'),
            endpoint_model,
            self.api_key
        );

        let payload = build_generate_content_payload(request)?;
        let response = self
            .http
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(ApiError::from)?;

        let status = response.status();
        let body = response.text().await.map_err(ApiError::from)?;
        if !status.is_success() {
            return Err(ApiError::Api {
                status,
                error_type: None,
                message: None,
                request_id: None,
                retryable: status.is_server_error() || status.as_u16() == 429,
                body,
            });
        }

        let decoded: GenerateContentResponse =
            serde_json::from_str(&body).map_err(|error| {
                ApiError::json_deserialize("Gemini", &request.model, &body, error)
            })?;
        decode_message_response(request, decoded)
    }

    pub async fn stream_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageStream, ApiError> {
        // "Quick wiring" stream: perform a normal request and surface it as a
        // tiny stream that emits MessageStart/Stop with the complete content.
        let message = self.send_message(request).await?;
        Ok(MessageStream::single(message))
    }
}

#[derive(Debug)]
pub struct MessageStream {
    request_id: Option<String>,
    state: StreamState,
}

#[derive(Debug)]
enum StreamState {
    Start(MessageResponse),
    Stop,
    Done,
}

impl MessageStream {
    fn single(message: MessageResponse) -> Self {
        Self {
            request_id: message.request_id.clone(),
            state: StreamState::Start(message),
        }
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub async fn next_event(&mut self) -> Result<Option<StreamEvent>, ApiError> {
        match std::mem::replace(&mut self.state, StreamState::Done) {
            StreamState::Start(message) => {
                self.state = StreamState::Stop;
                Ok(Some(StreamEvent::MessageStart(MessageStartEvent {
                    message,
                })))
            }
            StreamState::Stop => {
                self.state = StreamState::Done;
                Ok(Some(StreamEvent::MessageStop(MessageStopEvent {})))
            }
            StreamState::Done => Ok(None),
        }
    }
}

fn normalize_model_name(model: &str) -> String {
    model
        .trim()
        .strip_prefix("gemini/")
        .unwrap_or(model.trim())
        .to_string()
}

fn build_generate_content_payload(request: &MessageRequest) -> Result<Value, ApiError> {
    let (contents, tools, tool_config) = convert_messages_and_tools(request)?;
    let mut root = json!({
        "contents": contents,
        "generationConfig": generation_config(request),
    });

    if let Some(system) = request.system.as_ref().filter(|value| !value.is_empty()) {
        root.as_object_mut()
            .expect("json object")
            .insert("systemInstruction".to_string(), json!({"parts": [{"text": system}]}));
    }

    if let Some(tools) = tools {
        root.as_object_mut()
            .expect("json object")
            .insert("tools".to_string(), tools);
    }
    if let Some(tool_config) = tool_config {
        root.as_object_mut()
            .expect("json object")
            .insert("toolConfig".to_string(), tool_config);
    }

    Ok(root)
}

fn generation_config(request: &MessageRequest) -> Value {
    let mut config = serde_json::Map::new();
    config.insert(
        "maxOutputTokens".to_string(),
        json!(request.max_tokens),
    );
    if let Some(value) = request.temperature {
        config.insert("temperature".to_string(), json!(value));
    }
    if let Some(value) = request.top_p {
        config.insert("topP".to_string(), json!(value));
    }
    if let Some(value) = request.stop.as_ref().filter(|items| !items.is_empty()) {
        config.insert("stopSequences".to_string(), json!(value));
    }
    Value::Object(config)
}

fn convert_messages_and_tools(
    request: &MessageRequest,
) -> Result<(Vec<Value>, Option<Value>, Option<Value>), ApiError> {
    let mut tool_id_to_name: HashMap<String, String> = HashMap::new();
    let mut contents: Vec<Value> = Vec::new();

    for message in &request.messages {
        let role = normalize_role(&message.role);
        let mut parts: Vec<Value> = Vec::new();

        for block in &message.content {
            match block {
                InputContentBlock::Text { text } => {
                    if !text.is_empty() {
                        parts.push(json!({ "text": text }));
                    }
                }
                InputContentBlock::ToolUse { id, name, input } => {
                    tool_id_to_name.insert(id.clone(), name.clone());
                    parts.push(json!({ "functionCall": { "name": name, "args": input } }));
                }
                InputContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let Some(name) = tool_id_to_name.get(tool_use_id) else {
                        // Fallback: we cannot emit a functionResponse without a name.
                        parts.push(json!({
                            "text": format!("[tool_result missing tool name for id={tool_use_id} is_error={is_error}]")
                        }));
                        continue;
                    };
                    let response = tool_result_payload(content, *is_error);
                    parts.push(json!({
                        "functionResponse": {
                            "name": name,
                            "response": response
                        }
                    }));
                }
            }
        }

        if parts.is_empty() {
            continue;
        }
        contents.push(json!({ "role": role, "parts": parts }));
    }

    let (tools, tool_config) = convert_tools_and_choice(request.tools.as_ref(), request.tool_choice.as_ref())?;
    Ok((contents, tools, tool_config))
}

fn tool_result_payload(content: &[ToolResultContentBlock], is_error: bool) -> Value {
    let mut value = serde_json::Map::new();
    if is_error {
        value.insert("is_error".to_string(), json!(true));
    }
    if content.len() == 1 {
        match &content[0] {
            ToolResultContentBlock::Text { text } => {
                value.insert("content".to_string(), json!(text));
            }
            ToolResultContentBlock::Json { value: json_value } => {
                value.insert("content".to_string(), json_value.clone());
            }
        }
    } else {
        let items: Vec<Value> = content
            .iter()
            .map(|block| match block {
                ToolResultContentBlock::Text { text } => json!({ "type": "text", "text": text }),
                ToolResultContentBlock::Json { value } => json!({ "type": "json", "value": value }),
            })
            .collect();
        value.insert("content".to_string(), Value::Array(items));
    }
    Value::Object(value)
}

fn convert_tools_and_choice(
    tools: Option<&Vec<ToolDefinition>>,
    tool_choice: Option<&ToolChoice>,
) -> Result<(Option<Value>, Option<Value>), ApiError> {
    let Some(tools) = tools else {
        return Ok((None, None));
    };
    if tools.is_empty() {
        return Ok((None, None));
    }

    let function_declarations: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema,
            })
        })
        .collect();

    let tools_value = json!([{
        "functionDeclarations": function_declarations
    }]);

    let (mode, allowed) = match tool_choice {
        None | Some(ToolChoice::Auto) => ("AUTO", None),
        Some(ToolChoice::Any) => ("ANY", None),
        Some(ToolChoice::Tool { name }) => ("ANY", Some(vec![name.clone()])),
    };

    let mut tool_config = serde_json::Map::new();
    let mut calling = serde_json::Map::new();
    calling.insert("mode".to_string(), json!(mode));
    if let Some(allowed) = allowed.filter(|items| !items.is_empty()) {
        calling.insert("allowedFunctionNames".to_string(), json!(allowed));
    }
    tool_config.insert(
        "functionCallingConfig".to_string(),
        Value::Object(calling),
    );

    Ok((Some(tools_value), Some(Value::Object(tool_config))))
}

fn normalize_role(role: &str) -> &'static str {
    match role {
        "assistant" | "model" => "model",
        _ => "user",
    }
}

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default, rename = "usageMetadata")]
    usage: Option<UsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    #[serde(default, rename = "finishReason")]
    finish_reason: Option<String>,
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GeminiPart {
    Text { text: String },
    FunctionCall { #[serde(rename = "functionCall")] function_call: GeminiFunctionCall },
    Other(Value),
}

#[derive(Debug, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    #[serde(default)]
    args: Value,
}

#[derive(Debug, Deserialize)]
struct UsageMetadata {
    #[serde(default, rename = "promptTokenCount")]
    prompt_tokens: u32,
    #[serde(default, rename = "candidatesTokenCount")]
    candidates_tokens: u32,
    #[serde(default, rename = "totalTokenCount")]
    total_tokens: u32,
}

fn decode_message_response(
    request: &MessageRequest,
    decoded: GenerateContentResponse,
) -> Result<MessageResponse, ApiError> {
    let candidate = decoded
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::Auth("Gemini response missing candidates".to_string()))?;

    let mut content_blocks: Vec<OutputContentBlock> = Vec::new();
    if let Some(content) = candidate.content {
        for part in content.parts {
            match part {
                GeminiPart::Text { text } => {
                    if !text.is_empty() {
                        content_blocks.push(OutputContentBlock::Text { text });
                    }
                }
                GeminiPart::FunctionCall { function_call } => {
                    let id = new_id("toolu_");
                    content_blocks.push(OutputContentBlock::ToolUse {
                        id,
                        name: function_call.name,
                        input: function_call.args,
                    });
                }
                GeminiPart::Other(value) => {
                    content_blocks.push(OutputContentBlock::Text {
                        text: format!("[unhandled gemini part: {}]", value),
                    });
                }
            }
        }
    }

    let usage = decoded.usage.map_or_else(
        Usage::default,
        |usage| Usage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage
                .candidates_tokens
                .max(usage.total_tokens.saturating_sub(usage.prompt_tokens)),
            ..Usage::default()
        },
    );

    Ok(MessageResponse {
        id: new_id("msg_"),
        kind: "message".to_string(),
        role: "assistant".to_string(),
        content: content_blocks,
        model: request.model.clone(),
        stop_reason: candidate.finish_reason,
        stop_sequence: None,
        usage,
        request_id: None,
    })
}

fn new_id(prefix: &str) -> String {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{InputContentBlock, InputMessage};

    #[test]
    fn normalize_model_strips_gemini_namespace() {
        assert_eq!(normalize_model_name("gemini/gemini-3-flash-preview"), "gemini-3-flash-preview");
    }

    #[test]
    fn build_payload_converts_simple_user_message() {
        let request = MessageRequest {
            model: "gemini-3-flash-preview".to_string(),
            max_tokens: 32,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "hi".to_string(),
                }],
            }],
            system: Some("sys".to_string()),
            stream: false,
            ..Default::default()
        };
        let payload = build_generate_content_payload(&request).expect("payload");
        assert!(payload.get("contents").is_some());
        assert!(payload.get("systemInstruction").is_some());
    }
}

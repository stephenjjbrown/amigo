use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::Translation;
use crate::error::{ApiError, ErrorKind};
use crate::pairs::Language;

use super::model::{McpSession, MCP_MAX_TEXT_CHARS};

const PROTOCOL_VERSION: &str = "2025-06-18";
const TOOL_NAME: &str = "translate-text";

#[derive(Clone, Debug)]
struct TranslateShape {
    text_key: String,
    text_is_array: bool,
    source_key: Option<String>,
    target_key: String,
    target_is_array: bool,
    allow_preserve_formatting: bool,
    allow_split_sentences: bool,
}

impl TranslateShape {
    fn from_tool(tool: &Value) -> Self {
        let properties = tool
            .get("inputSchema")
            .and_then(|schema| schema.get("properties"))
            .and_then(Value::as_object);

        let Some(properties) = properties else {
            return Self::default();
        };

        let has = |name: &str| properties.contains_key(name);
        let pick = |names: &[&str]| {
            names
                .iter()
                .find(|name| has(name))
                .map(|name| (*name).to_string())
        };

        let text_key = pick(&["text", "texts"]).unwrap_or_else(|| "text".to_string());
        let text_is_array = schema_is_array(properties, &text_key);
        let source_key = pick(&["sourceLang", "sourceLangCode", "sourceLanguage"]);
        let target_key = pick(&[
            "targetLangs",
            "targetLangCodes",
            "targetLanguages",
            "targetLangCode",
        ])
        .unwrap_or_else(|| "targetLangs".to_string());
        let target_is_array = schema_is_array(properties, &target_key);

        Self {
            text_key,
            text_is_array,
            source_key,
            target_key,
            target_is_array,
            allow_preserve_formatting: has("preserveFormatting"),
            allow_split_sentences: has("splitSentences"),
        }
    }
}

impl Default for TranslateShape {
    fn default() -> Self {
        Self {
            text_key: "text".to_string(),
            text_is_array: true,
            source_key: Some("sourceLang".to_string()),
            target_key: "targetLangs".to_string(),
            target_is_array: true,
            allow_preserve_formatting: false,
            allow_split_sentences: false,
        }
    }
}

fn schema_is_array(properties: &serde_json::Map<String, Value>, key: &str) -> bool {
    properties
        .get(key)
        .map(|schema| {
            schema.get("type").and_then(Value::as_str) == Some("array")
                || schema.get("items").is_some()
                || schema
                    .get("anyOf")
                    .and_then(Value::as_array)
                    .map(|variants| {
                        variants.iter().any(|variant| {
                            variant.get("type").and_then(Value::as_str) == Some("array")
                                || variant.get("items").is_some()
                        })
                    })
                    .unwrap_or(false)
        })
        .unwrap_or(false)
}

pub struct McpClient {
    http: reqwest::blocking::Client,
    endpoint: String,
    access_token: String,
    session_id: Option<String>,
    protocol_version: String,
    initialized: bool,
    next_id: u64,
    translate_shape: Option<TranslateShape>,
}

impl McpClient {
    pub fn new(session: &McpSession) -> Result<Self, ApiError> {
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("TranslationAmigo/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| {
                ApiError::network(format!("Could not start the MCP client: {error}"))
            })?;

        Ok(Self {
            http,
            endpoint: if session.resource.is_empty() {
                super::model::MCP_ENDPOINT.to_string()
            } else {
                session.resource.clone()
            },
            access_token: session.access_token.clone(),
            session_id: None,
            protocol_version: PROTOCOL_VERSION.to_string(),
            initialized: false,
            next_id: 0,
            translate_shape: None,
        })
    }

    /// Initialize the MCP session and confirm the account can call the
    /// translation tool. Used right after sign-in to validate the session.
    pub fn verify(&mut self) -> Result<(), ApiError> {
        self.ensure_translate_shape()?;
        Ok(())
    }

    pub fn translate(
        &mut self,
        text: &str,
        source: Language,
        target: Language,
    ) -> Result<Translation, ApiError> {
        let characters = text.chars().count();
        if characters > MCP_MAX_TEXT_CHARS {
            return Err(ApiError::input(format!(
                "DeepL's MCP Free tier accepts up to {MCP_MAX_TEXT_CHARS} characters per request; this text is {characters} characters long."
            )));
        }

        self.initialize()?;

        log::info!(
            "translate: {} -> {}, {} chars",
            source.code(),
            target.code(),
            characters
        );

        let shape = self.ensure_translate_shape()?.clone();

        let mut arguments = serde_json::Map::new();
        arguments.insert(
            shape.text_key.clone(),
            if shape.text_is_array {
                json!([text])
            } else {
                json!(text)
            },
        );
        if let Some(source_key) = &shape.source_key {
            arguments.insert(source_key.clone(), json!(source.mcp_source_code()));
        }
        arguments.insert(
            shape.target_key.clone(),
            if shape.target_is_array {
                json!([target.mcp_target_code()])
            } else {
                json!(target.mcp_target_code())
            },
        );
        if shape.allow_preserve_formatting {
            arguments.insert("preserveFormatting".to_string(), json!(true));
        }
        if shape.allow_split_sentences {
            arguments.insert("splitSentences".to_string(), json!("nonewlines"));
        }
        let arguments = Value::Object(arguments);

        let result = match self.call_tool(TOOL_NAME, arguments.clone()) {
            Ok(result) => result,
            Err(error) if error.kind == ErrorKind::SessionExpired => {
                // The MCP session may have expired; start a fresh one once.
                self.reset_connection();
                self.initialize()?;
                self.call_tool(TOOL_NAME, arguments)?
            }
            Err(error) => return Err(error),
        };

        parse_translation_result(&result)
    }

    fn initialize(&mut self) -> Result<(), ApiError> {
        if self.initialized {
            return Ok(());
        }

        let id = self.next_id();
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "Amigo",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            },
        });

        let response = self.post(&payload, Some(id))?;
        let result = rpc_result(response, Some(id))?;

        if let Some(version) = result.get("protocolVersion").and_then(Value::as_str) {
            self.protocol_version = version.to_string();
        }
        log::debug!(
            "MCP initialized (protocol {}, session id: {})",
            self.protocol_version,
            if self.session_id.is_some() {
                "yes"
            } else {
                "no"
            }
        );

        self.notify_initialized()?;
        self.initialized = true;

        Ok(())
    }

    fn notify_initialized(&mut self) -> Result<(), ApiError> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });

        let response = self
            .base_request()
            .json(&payload)
            .send()
            .map_err(ApiError::from)?;

        self.capture_session(&response);

        let status = response.status();
        if status.is_success() {
            log::debug!("MCP initialized notification accepted");
            return Ok(());
        }

        let body = response.text().map_err(ApiError::from)?;
        Err(map_http_error(status.as_u16(), &body))
    }

    fn list_tools(&mut self) -> Result<Vec<Value>, ApiError> {
        let id = self.next_id();
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": {},
        });

        let response = self.post(&payload, Some(id))?;
        let result = rpc_result(response, Some(id))?;

        Ok(result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    fn ensure_translate_shape(&mut self) -> Result<&TranslateShape, ApiError> {
        self.initialize()?;

        if self.translate_shape.is_none() {
            let tools = self.list_tools()?;
            let names: Vec<&str> = tools
                .iter()
                .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                .collect();
            log::info!("MCP tools available: {}", names.join(", "));

            let tool = tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(TOOL_NAME))
                .ok_or_else(|| {
                    log::warn!("translate-text tool is not available for this account");
                    ApiError::api("This DeepL account does not expose the translate-text tool.")
                })?;

            if let Ok(schema) = serde_json::to_string(tool) {
                log::debug!("translate-text schema: {schema}");
            }
            self.translate_shape = Some(TranslateShape::from_tool(tool));

            if let Some(shape) = &self.translate_shape {
                log::debug!(
                    "translate-text arguments: text={}({}), source={:?}, target={}({})",
                    shape.text_key,
                    if shape.text_is_array {
                        "array"
                    } else {
                        "string"
                    },
                    shape.source_key,
                    shape.target_key,
                    if shape.target_is_array {
                        "array"
                    } else {
                        "string"
                    },
                );
            }
        }

        Ok(self
            .translate_shape
            .as_ref()
            .expect("translate shape initialized"))
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, ApiError> {
        let id = self.next_id();
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            },
        });

        let response = self.post(&payload, Some(id))?;
        rpc_result(response, Some(id))
    }

    fn post(&mut self, payload: &Value, expected_id: Option<u64>) -> Result<Value, ApiError> {
        let method = payload.get("method").and_then(Value::as_str).unwrap_or("?");
        let tool = payload.pointer("/params/name").and_then(Value::as_str);
        let started = Instant::now();

        match tool {
            Some(tool) => log::debug!("MCP -> {method} ({tool})"),
            None => log::debug!("MCP -> {method}"),
        }

        let response = self
            .base_request()
            .json(payload)
            .send()
            .map_err(ApiError::from)?;

        self.capture_session(&response);

        let status = response.status();
        let trace_id = response
            .headers()
            .get("x-trace-id")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-")
            .to_string();

        if !status.is_success() {
            let body = response.text().map_err(ApiError::from)?;
            log::warn!(
                "MCP HTTP {} for {method}: trace={trace_id} body={}",
                status.as_u16(),
                truncate(&body, 300)
            );
            return Err(map_http_error(status.as_u16(), &body));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();

        log::debug!(
            "MCP <- {method} status={} in {}ms trace={trace_id}",
            status.as_u16(),
            started.elapsed().as_millis()
        );

        if content_type.contains("text/event-stream") {
            return read_sse(response, expected_id);
        }

        let body = response.text().map_err(ApiError::from)?;
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }

        serde_json::from_str(&body).map_err(|error| {
            ApiError::api(format!(
                "DeepL sent an MCP response this app could not read ({error})."
            ))
        })
    }

    fn base_request(&self) -> reqwest::blocking::RequestBuilder {
        let mut request = self
            .http
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .header("MCP-Protocol-Version", &self.protocol_version);

        if let Some(session_id) = &self.session_id {
            request = request.header("Mcp-Session-Id", session_id);
        }

        request
    }

    fn capture_session(&mut self, response: &reqwest::blocking::Response) {
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            self.session_id = Some(session_id.to_string());
        }
    }

    fn reset_connection(&mut self) {
        self.initialized = false;
        self.session_id = None;
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

fn read_sse(
    response: reqwest::blocking::Response,
    expected_id: Option<u64>,
) -> Result<Value, ApiError> {
    let reader = BufReader::new(response);
    let mut data_lines: Vec<String> = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|error| {
            ApiError::network(format!("Reading the MCP event stream failed: {error}"))
        })?;

        if line.is_empty() {
            if data_lines.is_empty() {
                continue;
            }

            let payload = data_lines.join("\n");
            data_lines.clear();

            if let Ok(value) = serde_json::from_str::<Value>(&payload) {
                if matches_expected(&value, expected_id) {
                    return Ok(value);
                }
            }
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.strip_prefix(' ').unwrap_or(data).to_string());
        }
    }

    if !data_lines.is_empty() {
        let payload = data_lines.join("\n");
        if let Ok(value) = serde_json::from_str::<Value>(&payload) {
            if matches_expected(&value, expected_id) {
                return Ok(value);
            }
        }
    }

    Err(ApiError::api(
        "DeepL closed the MCP event stream before sending a response.",
    ))
}

fn matches_expected(value: &Value, expected: Option<u64>) -> bool {
    match value {
        Value::Array(items) => items.iter().any(|item| id_matches(item, expected)),
        _ => id_matches(value, expected),
    }
}

fn id_matches(value: &Value, expected: Option<u64>) -> bool {
    let Some(expected) = expected else {
        return true;
    };

    match value.get("id") {
        Some(Value::Number(number)) => number.as_u64() == Some(expected),
        Some(Value::String(text)) => text.parse::<u64>().ok() == Some(expected),
        _ => false,
    }
}

fn rpc_result(value: Value, expected: Option<u64>) -> Result<Value, ApiError> {
    let value = match value {
        Value::Array(items) => items
            .into_iter()
            .find(|item| id_matches(item, expected))
            .ok_or_else(|| {
                ApiError::api("DeepL returned an MCP batch without a matching response.")
            })?,
        other => other,
    };

    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("DeepL returned an MCP error.");
        let code = error.get("code").and_then(Value::as_i64);

        let detail = error.get("data").and_then(|data| {
            data.get("message")
                .and_then(Value::as_str)
                .or_else(|| data.get("detail").and_then(Value::as_str))
                .or_else(|| data.as_str())
                .map(str::to_string)
                .or_else(|| {
                    let rendered = data.to_string();
                    (!rendered.is_empty() && rendered != "null").then_some(rendered)
                })
        });

        log::warn!(
            "MCP JSON-RPC error: code={:?} message={} data={}",
            code,
            message,
            detail.as_deref().unwrap_or("-")
        );

        let mut text = message.to_string();
        if let Some(detail) = detail.filter(|detail| !detail.is_empty()) {
            text.push_str(" — ");
            text.push_str(&truncate(&detail, 400));
        }
        if let Some(code) = code {
            text.push_str(&format!(" (code {code})"));
        }

        let kind = if code == Some(-32602) {
            ErrorKind::Input
        } else {
            ErrorKind::Api
        };
        return Err(ApiError::new(kind, text));
    }

    value
        .get("result")
        .cloned()
        .ok_or_else(|| ApiError::api("DeepL returned an empty MCP response."))
}

fn map_http_error(status: u16, body: &str) -> ApiError {
    let detail = error_detail(body);

    match status {
        401 | 404 => ApiError::new(
            ErrorKind::SessionExpired,
            "Your DeepL session expired. Sign in again.",
        ),
        403 => ApiError::auth("DeepL denied this request. Check your DeepL plan and permissions."),
        429 => {
            ApiError::rate_limit("DeepL is rate-limiting requests. Wait a moment and try again.")
        }
        400 | 413 | 422 => ApiError::input(format!("DeepL rejected the request. {detail}")),
        500..=599 => ApiError::api(format!(
            "DeepL is having trouble right now (HTTP {status}). Try again shortly."
        )),
        other => ApiError::api(format!(
            "DeepL returned an unexpected error (HTTP {other}). {detail}"
        )),
    }
}

fn error_detail(body: &str) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        message: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "No details were provided.".to_string();
    }

    if let Ok(parsed) = serde_json::from_str::<ErrorBody>(trimmed) {
        if let Some(description) = parsed
            .error_description
            .filter(|value| !value.trim().is_empty())
        {
            return truncate(description.trim(), 240);
        }
        if let Some(message) = parsed.message.filter(|value| !value.trim().is_empty()) {
            return truncate(message.trim(), 240);
        }
        if let Some(error) = parsed.error.filter(|value| !value.trim().is_empty()) {
            return truncate(error.trim(), 240);
        }
    }

    truncate(trimmed, 240)
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let shortened: String = value.chars().take(max).collect();
        format!("{shortened}…")
    }
}

pub(crate) fn parse_translation_result(result: &Value) -> Result<Translation, ApiError> {
    let blocks = content_texts(result);
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if is_error {
        let message = blocks.join("\n");
        return Err(ApiError::api(if message.is_empty() {
            "DeepL could not translate this text.".to_string()
        } else {
            message
        }));
    }

    if let Some(structured) = result.get("structuredContent") {
        if let Some(message) = structured_failure(structured) {
            return Err(ApiError::api(message));
        }
        if let Some(translation) = translation_from_structured(structured) {
            return Ok(translation);
        }
    }

    // Some servers return a JSON payload as a text block instead of using
    // structuredContent.
    for block in &blocks {
        if let Ok(value) = serde_json::from_str::<Value>(block) {
            if let Some(message) = structured_failure(&value) {
                return Err(ApiError::api(message));
            }
            if let Some(translation) = translation_from_structured(&value) {
                return Ok(translation);
            }
            if let Some(translations) = value.get("translations").and_then(Value::as_array) {
                if let Some(first) = translations.first() {
                    if let Some(translation) = translation_from_structured(first) {
                        return Ok(translation);
                    }
                }
            }
        }
    }

    let mut translation = None;
    let mut detected = None;

    for block in blocks {
        let mut kept = Vec::new();

        for line in block.lines() {
            if let Some(language) = line.strip_prefix("Detected source language:") {
                detected = Some(language.trim().to_string());
                continue;
            }
            if line.starts_with("Target language used:") {
                continue;
            }
            kept.push(line);
        }

        let cleaned = kept.join("\n").trim().to_string();
        if !cleaned.is_empty() && translation.is_none() {
            translation = Some(cleaned);
        }
    }

    let text = translation.ok_or_else(|| ApiError::api("DeepL returned no translation text."))?;

    Ok(Translation {
        text,
        detected_source_language: detected,
        billed_characters: None,
    })
}

fn translation_from_structured(value: &Value) -> Option<Translation> {
    // Hosted MCP shape:
    // { "translations": [ { "targetLang": "EN",
    //     "translations": [ { "detectedSourceLanguage": "PT", "text": "…" } ] } ] }
    if let Some(targets) = value.get("translations").and_then(Value::as_array) {
        for target in targets {
            if let Some(items) = target.get("translations").and_then(Value::as_array) {
                if let Some(item) = items.first() {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        return Some(Translation {
                            text: text.to_string(),
                            detected_source_language: detected_language(item),
                            billed_characters: None,
                        });
                    }
                }
            }
        }

        // Older flat shape: { "translations": [ { "text": "…" } ] }
        if let Some(first) = targets.first() {
            if let Some(text) = first.get("text").and_then(Value::as_str) {
                return Some(Translation {
                    text: text.to_string(),
                    detected_source_language: detected_language(first),
                    billed_characters: None,
                });
            }
        }
    }

    let text = value
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| value.get("translation").and_then(Value::as_str))?;

    Some(Translation {
        text: text.to_string(),
        detected_source_language: detected_language(value),
        billed_characters: None,
    })
}

fn detected_language(value: &Value) -> Option<String> {
    value
        .get("detectedSourceLanguage")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("detected_source_language")
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

fn structured_failure(value: &Value) -> Option<String> {
    let failure = value.get("failures").and_then(Value::as_array)?.first()?;
    let target = failure
        .get("targetLang")
        .and_then(Value::as_str)
        .unwrap_or("the target language");
    let error = failure
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");

    Some(format!("DeepL could not translate to {target}: {error}"))
}

fn content_texts(result: &Value) -> Vec<String> {
    result
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_blocks_and_detected_language() {
        let result = json!({
            "content": [
                { "type": "text", "text": "Olá, mundo." },
                { "type": "text", "text": "Detected source language: PT" }
            ]
        });

        let translation = parse_translation_result(&result).unwrap();
        assert_eq!(translation.text, "Olá, mundo.");
        assert_eq!(translation.detected_source_language.as_deref(), Some("PT"));
    }

    #[test]
    fn parses_structured_translations() {
        let result = json!({
            "structuredContent": {
                "translations": [
                    {
                        "text": "Hallo, Welt.",
                        "detected_source_language": "EN"
                    }
                ]
            }
        });

        let translation = parse_translation_result(&result).unwrap();
        assert_eq!(translation.text, "Hallo, Welt.");
        assert_eq!(translation.detected_source_language.as_deref(), Some("EN"));
    }

    #[test]
    fn adapts_to_the_hosted_tool_schema() {
        let tool = json!({
            "name": "translate-text",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "array", "items": { "type": "string" } },
                    "targetLangs": { "type": "array", "items": { "type": "string" } },
                    "sourceLang": {
                        "anyOf": [{ "type": "string" }, { "type": "null" }]
                    }
                },
                "required": ["text", "targetLangs"]
            }
        });

        let shape = TranslateShape::from_tool(&tool);
        assert_eq!(shape.text_key, "text");
        assert!(shape.text_is_array);
        assert_eq!(shape.source_key.as_deref(), Some("sourceLang"));
        assert_eq!(shape.target_key, "targetLangs");
        assert!(shape.target_is_array);
        assert!(!shape.allow_preserve_formatting);
        assert!(!shape.allow_split_sentences);
    }

    #[test]
    fn parses_the_hosted_structured_response() {
        let result = json!({
            "structuredContent": {
                "translations": [{
                    "targetLang": "EN",
                    "translations": [{
                        "detectedSourceLanguage": "PT",
                        "text": "I wanted to date you."
                    }]
                }]
            }
        });

        let translation = parse_translation_result(&result).unwrap();
        assert_eq!(translation.text, "I wanted to date you.");
        assert_eq!(translation.detected_source_language.as_deref(), Some("PT"));
    }

    #[test]
    fn parses_json_content_blocks() {
        let result = json!({
            "content": [{
                "type": "text",
                "text": "{\"translations\":[{\"text\":\"Hola\",\"detected_source_language\":\"EN\"}]}"
            }]
        });

        let translation = parse_translation_result(&result).unwrap();
        assert_eq!(translation.text, "Hola");
        assert_eq!(translation.detected_source_language.as_deref(), Some("EN"));
    }

    #[test]
    fn surfaces_tool_errors() {
        let result = json!({
            "isError": true,
            "content": [{ "type": "text", "text": "Language pair not supported" }]
        });

        let error = parse_translation_result(&result).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Api);
        assert_eq!(error.message, "Language pair not supported");
    }

    #[test]
    fn matches_json_rpc_ids() {
        assert!(id_matches(&json!({ "id": 3 }), Some(3)));
        assert!(id_matches(&json!({ "id": "3" }), Some(3)));
        assert!(!id_matches(&json!({ "id": 4 }), Some(3)));
    }
}

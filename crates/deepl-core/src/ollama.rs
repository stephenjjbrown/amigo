use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::api::Translation;
use crate::error::ApiError;
use crate::pairs::Language;
use crate::provider::MAX_TRANSLATION_CHARS;

pub const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434";
pub const OLLAMA_CHAT_URL: &str = "http://127.0.0.1:11434/api/chat";
pub const OLLAMA_TAGS_URL: &str = "http://127.0.0.1:11434/api/tags";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampling {
    pub temperature: f32,
    pub top_p: f32,
}

impl Default for Sampling {
    fn default() -> Self {
        // Greedy decoding is the right default for a single translation.
        Self {
            temperature: 0.0,
            top_p: 0.95,
        }
    }
}

pub const MAX_VARIANTS: usize = 10;

/// Ollama Cloud allows 10 concurrent requests on the highest plan; local
/// Ollama is bounded by `OLLAMA_NUM_PARALLEL`. Settings clamp to this.
pub const MAX_PARALLEL_LIMIT: usize = 10;

/// Split `count` requests into waves of at most `parallel` indices.
fn batch_indices(count: usize, parallel: usize) -> Vec<Vec<usize>> {
    let parallel = parallel.max(1);
    (0..count)
        .collect::<Vec<_>>()
        .chunks(parallel)
        .map(|wave| wave.to_vec())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OllamaModel {
    #[default]
    TranslateGemma4b,
    TranslateGemma12b,
    TranslateGemma27b,
    Gemma4Cloud,
    DeepSeekV41FlashCloud,
}

impl OllamaModel {
    pub const ALL: [OllamaModel; 5] = [
        Self::TranslateGemma4b,
        Self::TranslateGemma12b,
        Self::TranslateGemma27b,
        Self::Gemma4Cloud,
        Self::DeepSeekV41FlashCloud,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::TranslateGemma4b => "translategemma:4b",
            Self::TranslateGemma12b => "translategemma:12b",
            Self::TranslateGemma27b => "translategemma:27b",
            Self::Gemma4Cloud => "gemma4:cloud",
            Self::DeepSeekV41FlashCloud => "deepseek-v4.1-flash:cloud",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TranslateGemma4b => "TranslateGemma 4B",
            Self::TranslateGemma12b => "TranslateGemma 12B",
            Self::TranslateGemma27b => "TranslateGemma 27B",
            Self::Gemma4Cloud => "Gemma 4 31B (cloud)",
            Self::DeepSeekV41FlashCloud => "DeepSeek V4.1 Flash (cloud)",
        }
    }

    pub fn is_cloud(self) -> bool {
        self.id().ends_with(":cloud")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Register {
    #[default]
    Neutral,
    Casual,
    Formal,
    Conversational,
    Academic,
}

impl Register {
    pub const ALL: [Register; 5] = [
        Self::Neutral,
        Self::Casual,
        Self::Formal,
        Self::Conversational,
        Self::Academic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Neutral => "Neutral",
            Self::Casual => "Casual",
            Self::Formal => "Formal",
            Self::Conversational => "Conversational",
            Self::Academic => "Academic",
        }
    }

    pub fn prompt_hint(self) -> Option<&'static str> {
        match self {
            Self::Neutral => None,
            Self::Casual => Some("Use a casual, informal register."),
            Self::Formal => Some("Use a formal, polite register."),
            Self::Conversational => {
                Some("Use a natural, conversational register suitable for spoken dialogue.")
            }
            Self::Academic => {
                Some("Use a precise, academic register suitable for scholarly writing.")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaStatus {
    pub running: bool,
    pub models: Vec<String>,
}

impl OllamaStatus {
    pub fn not_running() -> Self {
        Self {
            running: false,
            models: Vec::new(),
        }
    }

    pub fn has_model(&self, model: &str) -> bool {
        self.models.iter().any(|installed| installed == model)
    }

    /// Whether `model` can be sent to the daemon right now.
    ///
    /// Cloud models are hosted by Ollama, so they need no download and may not
    /// be listed by `/api/tags` until they have been used once.
    pub fn can_use(&self, model: OllamaModel) -> bool {
        self.running && (model.is_cloud() || self.has_model(model.id()))
    }
}

/// Check whether the local Ollama daemon is running and list installed models.
/// A refused connection is a normal "not running" state, not an error.
pub fn fetch_status() -> Result<OllamaStatus, ApiError> {
    let http = reqwest::blocking::Client::builder()
        .user_agent(concat!("TranslationAmigo/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|error| {
            ApiError::network(format!("Could not start the Ollama client: {error}"))
        })?;

    let response = match http
        .get(OLLAMA_TAGS_URL)
        .header("Accept", "application/json")
        .send()
    {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => {
            return Ok(OllamaStatus::not_running());
        }
        Err(error) => {
            return Err(ApiError::network(format!(
                "Could not reach Ollama at {OLLAMA_BASE_URL}: {error}"
            )));
        }
    };

    let status = response.status();
    let body = response.text().map_err(ApiError::from)?;

    if !status.is_success() {
        return Err(map_ollama_error(status.as_u16(), &body, ""));
    }

    let parsed: TagsResponse = serde_json::from_str(&body).map_err(|error| {
        ApiError::api(format!(
            "Ollama sent a model list this app could not read ({error})."
        ))
    })?;

    let mut models: Vec<String> = parsed
        .models
        .into_iter()
        .filter_map(|model| model.name.or(model.model))
        .collect();
    models.sort();
    models.dedup();

    Ok(OllamaStatus {
        running: true,
        models,
    })
}

pub struct OllamaClient {
    http: reqwest::blocking::Client,
    model: String,
}

impl OllamaClient {
    pub fn new(model: &str) -> Result<Self, ApiError> {
        let model = model.trim().to_string();
        if model.is_empty() {
            return Err(ApiError::input("Choose an Ollama model in Settings."));
        }

        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("TranslationAmigo/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|error| {
                ApiError::network(format!("Could not start the Ollama client: {error}"))
            })?;

        Ok(Self { http, model })
    }

    pub fn translate(
        &self,
        text: &str,
        source: Language,
        target: Language,
        register: Register,
        context: Option<&str>,
    ) -> Result<Translation, ApiError> {
        let mut variants = self.translate_variants(
            text,
            source,
            target,
            register,
            context,
            Sampling::default(),
            1,
            1,
        )?;
        Ok(variants.remove(0))
    }

    /// Ask for `count` samples of the same translation. With `temperature` at 0
    /// every sample is identical, so variation needs a sampling temperature.
    ///
    /// At most `parallel` requests are in flight at once. Ollama Cloud allows a
    /// plan-dependent number of concurrent requests (1 Free, 3 Pro, 10 Max) and
    /// queues anything beyond it, while a local daemon is bounded by
    /// `OLLAMA_NUM_PARALLEL`.
    #[allow(clippy::too_many_arguments)]
    pub fn translate_variants(
        &self,
        text: &str,
        source: Language,
        target: Language,
        register: Register,
        context: Option<&str>,
        sampling: Sampling,
        count: usize,
        parallel: usize,
    ) -> Result<Vec<Translation>, ApiError> {
        let characters = text.chars().count();
        if characters > MAX_TRANSLATION_CHARS {
            return Err(ApiError::input(format!(
                "That text is {characters} characters long. Ollama requests are limited to {MAX_TRANSLATION_CHARS} characters."
            )));
        }

        let prompt = build_prompt(text, source, target, register, context);
        let count = count.clamp(1, MAX_VARIANTS);
        let parallel = parallel.clamp(1, MAX_PARALLEL_LIMIT);

        if count == 1 || parallel == 1 {
            let mut translations = Vec::with_capacity(count);
            for _ in 0..count {
                translations.push(self.request_once(&prompt, source, sampling)?);
            }
            return Ok(translations);
        }

        let mut slots: Vec<Option<Result<Translation, ApiError>>> =
            (0..count).map(|_| None).collect();

        let prompt_ref = &prompt;
        for wave in batch_indices(count, parallel) {
            std::thread::scope(|scope| {
                let handles: Vec<_> = wave
                    .into_iter()
                    .map(|index| {
                        scope
                            .spawn(move || (index, self.request_once(prompt_ref, source, sampling)))
                    })
                    .collect();

                for handle in handles {
                    if let Ok((index, result)) = handle.join() {
                        slots[index] = Some(result);
                    }
                }
            });
        }

        let mut translations = Vec::with_capacity(count);
        for slot in slots {
            match slot {
                Some(Ok(translation)) => translations.push(translation),
                Some(Err(error)) => return Err(error),
                None => {
                    return Err(ApiError::api(
                        "A translation request stopped unexpectedly. Try again.",
                    ))
                }
            }
        }

        Ok(translations)
    }

    fn request_once(
        &self,
        prompt: &str,
        source: Language,
        sampling: Sampling,
    ) -> Result<Translation, ApiError> {
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": prompt,
            }],
            "stream": false,
            "think": false,
            "options": {
                "temperature": sampling.temperature,
                "top_p": sampling.top_p,
            },
        });

        let response = match self
            .http
            .post(OLLAMA_CHAT_URL)
            .header("Accept", "application/json")
            .json(&payload)
            .send()
        {
            Ok(response) => response,
            Err(error) if error.is_connect() || error.is_timeout() => {
                return Err(ApiError::network(
                    "Ollama isn't running. Start it with `ollama serve` and try again.",
                ));
            }
            Err(error) => {
                return Err(ApiError::network(format!(
                    "The request to Ollama failed: {error}"
                )));
            }
        };

        let status = response.status();
        let body = response.text().map_err(ApiError::from)?;

        if !status.is_success() {
            return Err(map_ollama_error(status.as_u16(), &body, &self.model));
        }

        let parsed: ChatResponse = serde_json::from_str(&body).map_err(|error| {
            ApiError::api(format!(
                "Ollama sent a response this app could not read ({error})."
            ))
        })?;

        let translation = parsed.message.content.trim();
        if translation.is_empty() {
            return Err(ApiError::api(
                "Ollama returned an empty translation. Try a smaller model or shorter text.",
            ));
        }

        Ok(Translation {
            text: translation.to_string(),
            detected_source_language: Some(source.code().to_string()),
            billed_characters: None,
        })
    }
}

pub fn build_prompt(
    text: &str,
    source: Language,
    target: Language,
    register: Register,
    context: Option<&str>,
) -> String {
    let source_name = source.label();
    let target_name = target.label();
    let source_code = source.ollama_source_code();
    let target_code = target.ollama_target_code();

    let mut instructions = String::new();
    if let Some(hint) = register.prompt_hint() {
        instructions.push(' ');
        instructions.push_str(hint);
    }

    if let Some(context) = context.map(str::trim).filter(|value| !value.is_empty()) {
        let context = context.replace(['\r', '\n'], " ");
        instructions.push_str(&format!(
            " The user provided this context for the translation: \"{}\".",
            context.trim()
        ));
    }

    format!(
        "You are a professional {source_name} to {target_name} translator. Your goal is to accurately convey the meaning and nuances of the original text while adhering to the target language's grammar, vocabulary, and cultural sensitivities.{instructions}\nSource language code: {source_code}. Target language code: {target_code}.\nProduce only the {target_name} translation, without any additional explanations or commentary. Please translate the following {source_name} text into {target_name}:\n\n{text}"
    )
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(Debug, Deserialize)]
struct TagModel {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

fn map_ollama_error(status: u16, body: &str, model: &str) -> ApiError {
    let detail = error_detail(body);

    match status {
        404 => {
            if model.is_empty() {
                ApiError::api(
                    "The selected Ollama model isn't installed. Pull it with `ollama pull <model>`.",
                )
            } else {
                ApiError::api(format!(
                    "{model} isn't installed in Ollama. Run `ollama pull {model}` and try again."
                ))
            }
        }
        401 | 403 => {
            ApiError::api("Ollama refused the request. Check the local Ollama configuration.")
        }
        429 => ApiError::rate_limit("Ollama is busy. Wait a moment and try again."),
        400 | 413 | 422 => ApiError::input(with_detail(
            "Ollama rejected the request. Try a shorter text.",
            &detail,
        )),
        500..=599 => ApiError::api(format!(
            "Ollama is having trouble right now (HTTP {status}). Try again shortly."
        )),
        other => ApiError::api(with_detail(
            &format!("Ollama returned an unexpected error (HTTP {other})."),
            &detail,
        )),
    }
}

fn with_detail(message: &str, detail: &str) -> String {
    if detail.is_empty() {
        message.to_string()
    } else {
        format!("{message} ({detail})")
    }
}

fn error_detail(body: &str) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: Option<String>,
        message: Option<String>,
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(parsed) = serde_json::from_str::<ErrorBody>(trimmed) {
        if let Some(error) = parsed.error.filter(|value| !value.trim().is_empty()) {
            return truncate(error.trim(), 240);
        }
        if let Some(message) = parsed.message.filter(|value| !value.trim().is_empty()) {
            return truncate(message.trim(), 240);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_uses_generic_language_metadata() {
        let prompt = build_prompt(
            "Olá, mundo.",
            Language::Portuguese,
            Language::English,
            Register::Neutral,
            None,
        );

        assert!(prompt.contains("Portuguese to English translator"));
        assert!(prompt.contains("Source language code: pt. Target language code: en."));
        assert!(prompt.contains("Produce only the English translation"));
        assert!(prompt.ends_with("\n\nOlá, mundo."));
        assert!(!prompt.contains("Brazilian"));
        assert!(!prompt.contains("European Portuguese"));
    }

    #[test]
    fn register_hints_are_generic() {
        let casual = build_prompt(
            "Hello.",
            Language::English,
            Language::PortuguesePortugal,
            Register::Casual,
            None,
        );
        assert!(casual.contains("Use a casual, informal register."));

        let formal = build_prompt(
            "Hello.",
            Language::English,
            Language::PortuguesePortugal,
            Register::Formal,
            None,
        );
        assert!(formal.contains("Use a formal, polite register."));

        let neutral = build_prompt(
            "Hello.",
            Language::English,
            Language::PortuguesePortugal,
            Register::Neutral,
            None,
        );
        assert!(!neutral.contains("register."));
    }

    #[test]
    fn context_hint_is_appended_verbatim() {
        let prompt = build_prompt(
            "Hello.",
            Language::English,
            Language::Spanish,
            Register::Neutral,
            Some("flirty conversation between friends"),
        );
        assert!(prompt.contains(
            "The user provided this context for the translation: \"flirty conversation between friends\"."
        ));
    }

    #[test]
    fn no_language_specific_instructions_are_added() {
        for target in [
            Language::PortuguesePortugal,
            Language::PortugueseBrazil,
            Language::English,
            Language::Spanish,
        ] {
            let prompt = build_prompt("Hello.", Language::English, target, Register::Casual, None);
            assert!(!prompt.contains("Brazilian"));
            assert!(!prompt.contains("European Portuguese"));
            assert!(!prompt.contains("not Brazilian"));
            assert!(!prompt.contains("not European"));
        }
    }

    #[test]
    fn regional_portuguese_codes_are_used() {
        assert_eq!(Language::PortuguesePortugal.ollama_target_code(), "pt-PT");
        assert_eq!(Language::PortugueseBrazil.ollama_target_code(), "pt-BR");
    }

    #[test]
    fn default_model_is_the_local_4b() {
        assert_eq!(OllamaModel::default().id(), "translategemma:4b");
    }

    #[test]
    fn parses_tags_response() {
        let parsed: TagsResponse = serde_json::from_str(
            r#"{"models":[{"name":"translategemma:4b"},{"model":"llama3.2:latest"}]}"#,
        )
        .unwrap();
        let models: Vec<String> = parsed
            .models
            .into_iter()
            .filter_map(|model| model.name.or(model.model))
            .collect();

        assert_eq!(models, vec!["translategemma:4b", "llama3.2:latest"]);
    }

    #[test]
    fn parses_chat_response() {
        let parsed: ChatResponse =
            serde_json::from_str(r#"{"message":{"role":"assistant","content":"Hello!"}}"#).unwrap();
        assert_eq!(parsed.message.content, "Hello!");
    }

    #[test]
    fn status_knows_installed_models() {
        let status = OllamaStatus {
            running: true,
            models: vec!["translategemma:4b".to_string()],
        };
        assert!(status.has_model("translategemma:4b"));
        assert!(!status.has_model("translategemma:27b"));
    }

    #[test]
    fn cloud_models_need_no_download() {
        let status = OllamaStatus {
            running: true,
            models: vec!["translategemma:4b".to_string()],
        };

        assert!(status.can_use(OllamaModel::Gemma4Cloud));
        assert!(status.can_use(OllamaModel::TranslateGemma4b));
        assert!(!status.can_use(OllamaModel::TranslateGemma12b));
    }

    #[test]
    fn stopped_daemon_can_use_nothing() {
        let status = OllamaStatus::not_running();

        assert!(!status.can_use(OllamaModel::Gemma4Cloud));
        assert!(!status.can_use(OllamaModel::TranslateGemma4b));
    }

    #[test]
    fn deepseek_cloud_is_selectable() {
        let model = OllamaModel::DeepSeekV41FlashCloud;
        assert_eq!(model.id(), "deepseek-v4.1-flash:cloud");
        assert!(model.is_cloud());
        assert!(!OllamaModel::TranslateGemma4b.is_cloud());
    }

    #[test]
    fn batches_requests_into_waves() {
        assert_eq!(batch_indices(0, 3), Vec::<Vec<usize>>::new());
        assert_eq!(batch_indices(1, 3), vec![vec![0]]);
        assert_eq!(batch_indices(3, 3), vec![vec![0, 1, 2]]);
        assert_eq!(
            batch_indices(10, 3),
            vec![vec![0, 1, 2], vec![3, 4, 5], vec![6, 7, 8], vec![9]]
        );
        assert_eq!(batch_indices(2, 0), vec![vec![0], vec![1]]);
    }

    #[test]
    fn gemma4_cloud_is_selectable() {
        let model = OllamaModel::Gemma4Cloud;
        assert_eq!(model.id(), "gemma4:cloud");
        assert_eq!(model.label(), "Gemma 4 31B (cloud)");
        assert!(model.is_cloud());
    }

    #[test]
    fn explains_missing_models() {
        let error = map_ollama_error(404, "", "translategemma:4b");
        assert!(error.message.contains("ollama pull translategemma:4b"));
    }
}

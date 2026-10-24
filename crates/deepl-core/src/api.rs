use std::time::Duration;

use serde::Deserialize;

use crate::error::ApiError;
use crate::pairs::Language;

pub const FREE_BASE_URL: &str = "https://api-free.deepl.com/v2";
pub const PRO_BASE_URL: &str = "https://api.deepl.com/v2";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiMode {
    Free,
    Pro,
}

impl ApiMode {
    /// DeepL free keys always end in ":fx"; everything else is a Pro key.
    pub fn from_key(key: &str) -> Self {
        if key.trim().to_ascii_lowercase().ends_with(":fx") {
            Self::Free
        } else {
            Self::Pro
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            Self::Free => FREE_BASE_URL,
            Self::Pro => PRO_BASE_URL,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Pro => "pro",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub character_count: u64,
    pub character_limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub text: String,
    pub detected_source_language: Option<String>,
    pub billed_characters: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct UsageResponse {
    character_count: u64,
    character_limit: u64,
}

#[derive(Debug, Deserialize)]
struct TranslateResponse {
    translations: Vec<TranslateResponseItem>,
}

#[derive(Debug, Deserialize)]
struct TranslateResponseItem {
    text: String,
    #[serde(default)]
    detected_source_language: Option<String>,
    #[serde(default)]
    billed_characters: Option<u64>,
}

/// A blocking DeepL client. It is owned by the engine thread, never by the UI.
pub struct ApiClient {
    client: reqwest::blocking::Client,
}

impl ApiClient {
    pub fn new() -> Result<Self, ApiError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("TranslationAmigo/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|error| {
                ApiError::network(format!("Could not start the HTTP client: {error}"))
            })?;

        Ok(Self { client })
    }

    pub fn translate(
        &self,
        api_key: &str,
        source: Language,
        target: Language,
        text: &str,
    ) -> Result<Translation, ApiError> {
        let url = format!("{}/translate", ApiMode::from_key(api_key).base_url());
        let payload = serde_json::json!({
            "text": [text],
            "source_lang": source.code(),
            "target_lang": target.code(),
            "preserve_formatting": true,
        });

        let response = self
            .request(self.client.post(url), api_key)
            .json(&payload)
            .send()
            .map_err(ApiError::from)?;

        let status = response.status();
        let body = response.text().map_err(ApiError::from)?;

        if !status.is_success() {
            return Err(map_api_error(status.as_u16(), &body));
        }

        let parsed: TranslateResponse = serde_json::from_str(&body).map_err(|error| {
            ApiError::api(format!(
                "DeepL sent a translation response this app could not read ({error})."
            ))
        })?;

        let translation = parsed
            .translations
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::api("DeepL returned no translation for this text."))?;

        Ok(Translation {
            text: translation.text,
            detected_source_language: translation.detected_source_language,
            billed_characters: translation.billed_characters,
        })
    }

    pub fn usage(&self, api_key: &str) -> Result<Usage, ApiError> {
        let url = format!("{}/usage", ApiMode::from_key(api_key).base_url());
        let response = self
            .request(self.client.get(url), api_key)
            .send()
            .map_err(ApiError::from)?;

        let status = response.status();
        let body = response.text().map_err(ApiError::from)?;

        if !status.is_success() {
            return Err(map_api_error(status.as_u16(), &body));
        }

        let parsed: UsageResponse = serde_json::from_str(&body).map_err(|error| {
            ApiError::api(format!(
                "DeepL sent a usage response this app could not read ({error})."
            ))
        })?;

        Ok(Usage {
            character_count: parsed.character_count,
            character_limit: parsed.character_limit,
        })
    }

    fn request(
        &self,
        request: reqwest::blocking::RequestBuilder,
        api_key: &str,
    ) -> reqwest::blocking::RequestBuilder {
        request
            .header("Authorization", format!("DeepL-Auth-Key {api_key}"))
            .header("Content-Type", "application/json")
    }
}

/// Map DeepL's HTTP status codes onto errors the UI can explain.
pub fn map_api_error(status: u16, body: &str) -> ApiError {
    let detail = api_detail(body);
    let with_detail = |fallback: &str| {
        if detail.is_empty() {
            fallback.to_string()
        } else {
            format!("{fallback} ({detail})")
        }
    };

    match status {
        400 => ApiError::input(with_detail(
            "DeepL rejected the request. Check the text and language pair.",
        )),
        403 => ApiError::auth(with_detail(
            "DeepL rejected the API key. Make sure the key is active and belongs to the right plan — Free keys end in :fx.",
        )),
        404 => ApiError::api(with_detail(
            "DeepL could not find that API endpoint. Check the key type.",
        )),
        413 => ApiError::input(
            "This text is too large for a single DeepL request. Try a shorter selection.",
        ),
        429 => ApiError::rate_limit(
            "DeepL is receiving too many requests. Wait a moment, then try again.",
        ),
        456 => ApiError::quota(
            "Your DeepL character quota is used up for this billing period. Check your plan on deepl.com.",
        ),
        500..=599 => ApiError::api(format!(
            "DeepL is having trouble right now (HTTP {status}). Try again shortly."
        )),
        other => ApiError::api(with_detail(&format!(
            "DeepL returned an unexpected error (HTTP {other})."
        ))),
    }
}

/// DeepL error bodies are usually `{"message": "..."}`.
fn api_detail(body: &str) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        message: Option<String>,
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(parsed) = serde_json::from_str::<ErrorBody>(trimmed) {
        if let Some(message) = parsed.message {
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

/// Last four meaningful characters of a key, safe to display in settings.
pub fn key_last4(key: &str) -> String {
    let trimmed = key.trim();
    let body = trimmed
        .strip_suffix(":fx")
        .or_else(|| trimmed.strip_suffix(":FX"))
        .unwrap_or(trimmed);
    let characters: Vec<char> = body
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();
    let start = characters.len().saturating_sub(4);
    characters[start..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn detects_free_keys() {
        assert_eq!(ApiMode::from_key("abc-123:fx"), ApiMode::Free);
        assert_eq!(ApiMode::from_key("  abc-123:FX "), ApiMode::Free);
        assert_eq!(ApiMode::from_key("abc-123"), ApiMode::Pro);
    }

    #[test]
    fn picks_the_right_endpoint() {
        assert_eq!(
            ApiMode::from_key("abc:fx").base_url(),
            "https://api-free.deepl.com/v2"
        );
        assert_eq!(
            ApiMode::from_key("abc").base_url(),
            "https://api.deepl.com/v2"
        );
    }

    #[test]
    fn extracts_last_four_characters() {
        assert_eq!(key_last4("12345678-abcd:fx"), "abcd");
        assert_eq!(key_last4("ABCDEF"), "CDEF");
        assert_eq!(key_last4(""), "");
    }

    #[test]
    fn explains_auth_errors() {
        let error = map_api_error(403, r#"{"message":"Forbidden"}"#);
        assert_eq!(error.kind, ErrorKind::Auth);
        assert!(error.message.contains("Forbidden"));
    }

    #[test]
    fn explains_quota_and_rate_limits() {
        assert_eq!(map_api_error(456, "").kind, ErrorKind::Quota);
        assert_eq!(map_api_error(429, "").kind, ErrorKind::RateLimit);
        assert_eq!(map_api_error(503, "").kind, ErrorKind::Api);
    }

    #[test]
    fn falls_back_to_plain_text_error_bodies() {
        let error = map_api_error(400, "Bad request");
        assert_eq!(error.kind, ErrorKind::Input);
        assert!(error.message.contains("Bad request"));
    }
}

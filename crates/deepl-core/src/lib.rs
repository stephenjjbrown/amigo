pub mod api;
pub mod error;
pub mod mcp;
pub mod ollama;
pub mod pairs;
pub mod provider;
pub mod session_store;
pub mod types;

pub use api::{ApiClient, ApiMode, Translation, Usage};
pub use error::{ApiError, ErrorKind};
pub use mcp::{McpClient, McpSession, SessionStatus, MCP_ENDPOINT, MCP_MAX_TEXT_CHARS};
pub use ollama::{
    fetch_status, OllamaClient, OllamaModel, OllamaStatus, Register, Sampling, MAX_VARIANTS,
    OLLAMA_BASE_URL, OLLAMA_CHAT_URL, OLLAMA_TAGS_URL,
};
pub use pairs::{
    allowed_targets, allowed_targets_for, is_supported_pair, swap_pair, target_for_source,
    target_for_source_with, LangPair, Language, MAX_TEXT_CHARS, SOURCE_LANGUAGES, TARGET_LANGUAGES,
};
pub use provider::{Provider, MAX_TRANSLATION_CHARS};
pub use types::{KeyOrigin, KeyStatus};

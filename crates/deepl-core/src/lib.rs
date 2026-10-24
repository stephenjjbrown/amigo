pub mod api;
pub mod error;
pub mod keychain;
pub mod mcp;
pub mod pairs;
pub mod types;

pub use api::{ApiClient, ApiMode, Translation, Usage};
pub use error::{ApiError, ErrorKind};
pub use mcp::{McpClient, McpSession, SessionStatus, MCP_ENDPOINT, MCP_MAX_TEXT_CHARS};
pub use pairs::{
    allowed_targets, is_supported_pair, swap_pair, target_for_source, LangPair, Language,
    MAX_TEXT_CHARS, SOURCE_LANGUAGES, TARGET_LANGUAGES,
};
pub use types::{KeyOrigin, KeyStatus};

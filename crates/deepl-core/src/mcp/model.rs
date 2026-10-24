use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const MCP_ENDPOINT: &str = "https://mcp.deepl.com/v1/mcp";

/// DeepL's MCP Free tier accepts 5,000 characters per translation request.
pub const MCP_MAX_TEXT_CHARS: usize = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStatus {
    pub signed_in: bool,
}

impl SessionStatus {
    pub fn signed_in() -> Self {
        Self { signed_in: true }
    }

    pub fn signed_out() -> Self {
        Self { signed_in: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSession {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub token_endpoint_auth_method: Option<String>,
    #[serde(default = "default_resource")]
    pub resource: String,
    #[serde(default)]
    pub scope: Option<String>,
}

impl McpSession {
    pub fn is_expiring(&self) -> bool {
        self.expires_at - now_unix() <= 60
    }
}

fn default_resource() -> String {
    MCP_ENDPOINT.to_string()
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

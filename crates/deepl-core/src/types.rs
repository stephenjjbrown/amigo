use crate::api::{key_last4, ApiMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOrigin {
    Keychain,
    Environment,
}

impl KeyOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keychain => "keychain",
            Self::Environment => "environment",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyStatus {
    pub configured: bool,
    pub mode: Option<ApiMode>,
    pub last4: Option<String>,
    pub origin: Option<KeyOrigin>,
}

impl KeyStatus {
    pub fn for_key(key: &str, origin: KeyOrigin) -> Self {
        Self {
            configured: true,
            mode: Some(ApiMode::from_key(key)),
            last4: Some(key_last4(key)),
            origin: Some(origin),
        }
    }

    pub fn empty() -> Self {
        Self {
            configured: false,
            mode: None,
            last4: None,
            origin: None,
        }
    }
}

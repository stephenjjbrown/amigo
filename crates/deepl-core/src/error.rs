use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NotConfigured,
    Auth,
    Quota,
    RateLimit,
    Input,
    Network,
    Storage,
    System,
    Api,
    Cancelled,
    SessionExpired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ApiError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn input(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Input, message)
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Auth, message)
    }

    pub fn quota(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Quota, message)
    }

    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::RateLimit, message)
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Storage, message)
    }

    pub fn system(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::System, message)
    }

    pub fn api(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Api, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Network, message)
    }

    pub fn not_configured() -> Self {
        Self::new(
            ErrorKind::NotConfigured,
            "Sign in with your DeepL account to start translating.",
        )
    }

    pub fn auth_expired() -> Self {
        Self::new(
            ErrorKind::SessionExpired,
            "Your DeepL session expired. Sign in again.",
        )
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "Sign-in cancelled.")
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        let message = if error.is_timeout() {
            "DeepL took too long to respond. Check your connection and try again.".to_string()
        } else if error.is_connect() {
            "Could not reach DeepL. Check your internet connection.".to_string()
        } else {
            format!("The request to DeepL failed: {error}")
        };
        Self::network(message)
    }
}

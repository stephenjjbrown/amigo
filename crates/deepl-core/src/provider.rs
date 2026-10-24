use serde::{Deserialize, Serialize};

/// A translation backend the app can call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Provider {
    DeepL,
    Ollama,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::DeepL, Provider::Ollama];

    pub fn label(self) -> &'static str {
        match self {
            Self::DeepL => "DeepL",
            Self::Ollama => "Ollama",
        }
    }

    pub fn settings_key(self) -> &'static str {
        match self {
            Self::DeepL => "deepl",
            Self::Ollama => "ollama",
        }
    }
}

/// Shared per-request text limit for the two hosted backends.
pub const MAX_TRANSLATION_CHARS: usize = 5_000;

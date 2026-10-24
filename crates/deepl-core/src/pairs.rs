use serde::{Deserialize, Serialize};

/// DeepL accepts up to 128 KiB of text per request; keep a margin below that.
pub const MAX_TEXT_CHARS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Portuguese,
    English,
    Spanish,
    PortuguesePortugal,
    PortugueseBrazil,
}

impl Language {
    /// Code sent to DeepL.
    pub fn code(self) -> &'static str {
        match self {
            Self::Portuguese => "PT",
            Self::English => "EN",
            Self::Spanish => "ES",
            Self::PortuguesePortugal => "PT-PT",
            Self::PortugueseBrazil => "PT-BR",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Portuguese => "Portuguese",
            Self::English => "English",
            Self::Spanish => "Spanish",
            Self::PortuguesePortugal => "Portuguese (Portugal)",
            Self::PortugueseBrazil => "Portuguese (Brazil)",
        }
    }

    /// DeepL language code accepted by the hosted MCP `translate-text` tool.
    pub fn mcp_source_code(self) -> &'static str {
        match self {
            Self::Portuguese | Self::PortuguesePortugal | Self::PortugueseBrazil => "PT",
            Self::English => "EN",
            Self::Spanish => "ES",
        }
    }

    /// DeepL language code accepted by the hosted MCP tool as a target. The
    /// region must be explicit for PT, otherwise the service defaults to
    /// Brazilian Portuguese.
    pub fn mcp_target_code(self) -> &'static str {
        match self {
            Self::Portuguese | Self::PortuguesePortugal => "PT-PT",
            Self::PortugueseBrazil => "PT-BR",
            Self::English => "EN",
            Self::Spanish => "ES",
        }
    }
}

pub const SOURCE_LANGUAGES: &[Language] =
    &[Language::Portuguese, Language::English, Language::Spanish];

pub const TARGET_LANGUAGES: &[Language] = &[
    Language::English,
    Language::PortuguesePortugal,
    Language::PortugueseBrazil,
    Language::Spanish,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LangPair {
    pub source: Language,
    pub target: Language,
}

impl Default for LangPair {
    fn default() -> Self {
        Self {
            source: Language::Portuguese,
            target: Language::English,
        }
    }
}

/// The five pairs this build exposes. DeepL supports far more; this app
/// deliberately ships a curated set.
pub const SUPPORTED_PAIRS: &[(Language, Language)] = &[
    (Language::Portuguese, Language::English),
    (Language::English, Language::PortuguesePortugal),
    (Language::English, Language::PortugueseBrazil),
    (Language::English, Language::Spanish),
    (Language::Spanish, Language::English),
];

pub fn is_supported_pair(source: Language, target: Language) -> bool {
    SUPPORTED_PAIRS
        .iter()
        .any(|(supported_source, supported_target)| {
            *supported_source == source && *supported_target == target
        })
}

pub fn allowed_targets(source: Language) -> &'static [Language] {
    match source {
        Language::Portuguese => &[Language::English],
        Language::English => &[
            Language::PortuguesePortugal,
            Language::PortugueseBrazil,
            Language::Spanish,
        ],
        Language::Spanish => &[Language::English],
        Language::PortuguesePortugal | Language::PortugueseBrazil => &[],
    }
}

/// Pick the target that should follow a newly chosen source language.
pub fn target_for_source(source: Language, current_target: Language) -> Language {
    let allowed = allowed_targets(source);
    if allowed.contains(&current_target) {
        current_target
    } else {
        allowed.first().copied().unwrap_or(current_target)
    }
}

/// Swap direction when the reverse is unambiguous. PT→EN has no single reverse
/// (Portugal vs. Brazil), so it defaults to European Portuguese.
pub fn swap_pair(pair: LangPair) -> Option<LangPair> {
    if is_supported_pair(pair.target, pair.source) {
        return Some(LangPair {
            source: pair.target,
            target: pair.source,
        });
    }
    if pair.target == Language::English {
        return Some(LangPair {
            source: Language::English,
            target: Language::PortuguesePortugal,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_every_advertised_pair() {
        for (source, target) in SUPPORTED_PAIRS {
            assert!(is_supported_pair(*source, *target));
        }
    }

    #[test]
    fn rejects_pairs_outside_the_curated_set() {
        assert!(!is_supported_pair(
            Language::Portuguese,
            Language::PortugueseBrazil
        ));
        assert!(!is_supported_pair(Language::English, Language::English));
        assert!(!is_supported_pair(
            Language::Spanish,
            Language::PortuguesePortugal
        ));
        assert!(!is_supported_pair(
            Language::PortuguesePortugal,
            Language::English
        ));
    }

    #[test]
    fn swaps_ambiguous_portuguese_to_pt_pt() {
        let swapped = swap_pair(LangPair {
            source: Language::Portuguese,
            target: Language::English,
        })
        .unwrap();

        assert_eq!(swapped.source, Language::English);
        assert_eq!(swapped.target, Language::PortuguesePortugal);
    }

    #[test]
    fn swaps_unambiguous_pairs_exactly() {
        let swapped = swap_pair(LangPair {
            source: Language::English,
            target: Language::Spanish,
        })
        .unwrap();

        assert_eq!(swapped.source, Language::Spanish);
        assert_eq!(swapped.target, Language::English);
    }

    #[test]
    fn maps_languages_to_deepl_codes() {
        assert_eq!(Language::Portuguese.code(), "PT");
        assert_eq!(Language::English.code(), "EN");
        assert_eq!(Language::Spanish.code(), "ES");
        assert_eq!(Language::PortuguesePortugal.code(), "PT-PT");
        assert_eq!(Language::PortugueseBrazil.code(), "PT-BR");
    }

    #[test]
    fn maps_languages_to_mcp_codes() {
        assert_eq!(Language::Portuguese.mcp_source_code(), "PT");
        assert_eq!(Language::English.mcp_source_code(), "EN");
        assert_eq!(Language::Spanish.mcp_source_code(), "ES");
        assert_eq!(Language::PortuguesePortugal.mcp_target_code(), "PT-PT");
        assert_eq!(Language::PortugueseBrazil.mcp_target_code(), "PT-BR");
        assert_eq!(Language::English.mcp_target_code(), "EN");
        assert_eq!(Language::Spanish.mcp_target_code(), "ES");
    }

    #[test]
    fn adjusts_target_when_source_changes() {
        assert_eq!(
            target_for_source(Language::English, Language::English),
            Language::PortuguesePortugal
        );
        assert_eq!(
            target_for_source(Language::Spanish, Language::English),
            Language::English
        );
        assert_eq!(
            target_for_source(Language::English, Language::Spanish),
            Language::Spanish
        );
    }
}

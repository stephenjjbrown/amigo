use serde::{Deserialize, Serialize};

use crate::provider::Provider;

/// DeepL accepts up to 128 KiB of text per request; keep a margin below that.
pub const MAX_TEXT_CHARS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Portuguese,
    English,
    Spanish,
    PortuguesePortugal,
    PortugueseBrazil,
    PortugueseAngola,
    PortugueseAzores,
    SpanishArgentina,
    SpanishColombia,
    SpanishMexico,
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
            Self::PortugueseAngola => "PT-AO",
            Self::PortugueseAzores => "PT-20",
            Self::SpanishArgentina => "ES-AR",
            Self::SpanishColombia => "ES-CO",
            Self::SpanishMexico => "ES-MX",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Portuguese => "Portuguese",
            Self::English => "English",
            Self::Spanish => "Spanish",
            Self::PortuguesePortugal => "Portuguese (Portugal)",
            Self::PortugueseBrazil => "Portuguese (Brazil)",
            Self::PortugueseAngola => "Portuguese (Angola)",
            Self::PortugueseAzores => "Portuguese (Azores)",
            Self::SpanishArgentina => "Spanish (Argentina)",
            Self::SpanishColombia => "Spanish (Colombia)",
            Self::SpanishMexico => "Spanish (Mexico)",
        }
    }

    /// DeepL language code accepted by the hosted MCP `translate-text` tool.
    pub fn mcp_source_code(self) -> &'static str {
        match self {
            Self::Portuguese
            | Self::PortuguesePortugal
            | Self::PortugueseBrazil
            | Self::PortugueseAngola
            | Self::PortugueseAzores => "PT",
            Self::English => "EN",
            Self::Spanish
            | Self::SpanishArgentina
            | Self::SpanishColombia
            | Self::SpanishMexico => "ES",
        }
    }

    /// DeepL language code accepted by the hosted MCP tool as a target. The
    /// region must be explicit for PT, otherwise the service defaults to
    /// Brazilian Portuguese.
    ///
    /// DeepL has no Angolan or Azorean Portuguese target and a single Spanish,
    /// so those regional pairs are never offered for a DeepL provider.
    pub fn mcp_target_code(self) -> &'static str {
        match self {
            Self::Portuguese
            | Self::PortuguesePortugal
            | Self::PortugueseAngola
            | Self::PortugueseAzores => "PT-PT",
            Self::PortugueseBrazil => "PT-BR",
            Self::English => "EN",
            Self::Spanish => "ES",
            Self::SpanishArgentina | Self::SpanishColombia | Self::SpanishMexico => "ES",
        }
    }

    /// Lowercase code used in the TranslateGemma prompt.
    pub fn ollama_source_code(self) -> &'static str {
        match self {
            Self::Portuguese
            | Self::PortuguesePortugal
            | Self::PortugueseBrazil
            | Self::PortugueseAngola
            | Self::PortugueseAzores => "pt",
            Self::English => "en",
            Self::Spanish
            | Self::SpanishArgentina
            | Self::SpanishColombia
            | Self::SpanishMexico => "es",
        }
    }

    /// Lowercase target code used in the TranslateGemma prompt.
    pub fn ollama_target_code(self) -> &'static str {
        match self {
            Self::Portuguese | Self::PortuguesePortugal => "pt-PT",
            Self::PortugueseBrazil => "pt-BR",
            Self::PortugueseAngola => "pt-AO",
            // ISO 3166-2:PT-20, the Azores as a Portuguese subdivision.
            Self::PortugueseAzores => "pt-20",
            Self::English => "en",
            Self::Spanish => "es",
            Self::SpanishArgentina => "es-AR",
            Self::SpanishColombia => "es-CO",
            Self::SpanishMexico => "es-MX",
        }
    }
}

pub const SOURCE_LANGUAGES: &[Language] =
    &[Language::Portuguese, Language::English, Language::Spanish];

pub const TARGET_LANGUAGES: &[Language] = &[
    Language::English,
    Language::PortuguesePortugal,
    Language::PortugueseBrazil,
    Language::PortugueseAngola,
    Language::PortugueseAzores,
    Language::SpanishArgentina,
    Language::SpanishColombia,
    Language::SpanishMexico,
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

/// The pairs this build exposes. DeepL supports far more; this app
/// deliberately ships a curated set.
pub const SUPPORTED_PAIRS: &[(Language, Language)] = &[
    (Language::Portuguese, Language::English),
    (Language::English, Language::PortuguesePortugal),
    (Language::English, Language::PortugueseBrazil),
    (Language::English, Language::PortugueseAngola),
    (Language::English, Language::PortugueseAzores),
    (Language::English, Language::SpanishArgentina),
    (Language::English, Language::SpanishColombia),
    (Language::English, Language::SpanishMexico),
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
        Language::PortuguesePortugal
        | Language::PortugueseBrazil
        | Language::PortugueseAngola
        | Language::PortugueseAzores
        | Language::SpanishArgentina
        | Language::SpanishColombia
        | Language::SpanishMexico => &[],
    }
}

/// Targets a specific provider can produce. Angolan and Azorean Portuguese and
/// regional Spanish are only available through Ollama, because DeepL has no such
/// target languages.
pub fn allowed_targets_for(provider: Provider, source: Language) -> &'static [Language] {
    match provider {
        Provider::DeepL => allowed_targets(source),
        Provider::Ollama => match source {
            Language::Portuguese => &[Language::English],
            Language::English => &[
                Language::PortuguesePortugal,
                Language::PortugueseBrazil,
                Language::PortugueseAngola,
                Language::PortugueseAzores,
                Language::Spanish,
                Language::SpanishArgentina,
                Language::SpanishColombia,
                Language::SpanishMexico,
            ],
            Language::Spanish => &[Language::English],
            Language::PortuguesePortugal
            | Language::PortugueseBrazil
            | Language::PortugueseAngola
            | Language::PortugueseAzores
            | Language::SpanishArgentina
            | Language::SpanishColombia
            | Language::SpanishMexico => &[],
        },
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

pub fn target_for_source_with(
    provider: Provider,
    source: Language,
    current_target: Language,
) -> Language {
    let allowed = allowed_targets_for(provider, source);
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
        assert_eq!(Language::PortugueseAngola.code(), "PT-AO");
        assert_eq!(Language::PortugueseAzores.code(), "PT-20");
        assert_eq!(Language::SpanishArgentina.code(), "ES-AR");
        assert_eq!(Language::SpanishColombia.code(), "ES-CO");
        assert_eq!(Language::SpanishMexico.code(), "ES-MX");
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
    fn maps_languages_to_ollama_codes() {
        assert_eq!(Language::Portuguese.ollama_source_code(), "pt");
        assert_eq!(Language::English.ollama_source_code(), "en");
        assert_eq!(Language::Spanish.ollama_source_code(), "es");
        assert_eq!(Language::PortuguesePortugal.ollama_target_code(), "pt-PT");
        assert_eq!(Language::PortugueseBrazil.ollama_target_code(), "pt-BR");
        assert_eq!(Language::PortugueseAngola.ollama_target_code(), "pt-AO");
        assert_eq!(Language::PortugueseAzores.ollama_target_code(), "pt-20");
        assert_eq!(Language::English.ollama_target_code(), "en");
        assert_eq!(Language::Spanish.ollama_target_code(), "es");
        assert_eq!(Language::SpanishArgentina.ollama_target_code(), "es-AR");
        assert_eq!(Language::SpanishColombia.ollama_target_code(), "es-CO");
        assert_eq!(Language::SpanishMexico.ollama_target_code(), "es-MX");
    }

    #[test]
    fn angolan_portuguese_is_ollama_only() {
        assert!(!allowed_targets(Language::English).contains(&Language::PortugueseAngola));
        assert!(allowed_targets_for(Provider::DeepL, Language::English)
            .contains(&Language::PortugueseBrazil));
        assert!(!allowed_targets_for(Provider::DeepL, Language::English)
            .contains(&Language::PortugueseAngola));
        assert!(allowed_targets_for(Provider::Ollama, Language::English)
            .contains(&Language::PortugueseAngola));
        assert!(is_supported_pair(
            Language::English,
            Language::PortugueseAngola
        ));
    }

    #[test]
    fn regional_spanish_is_ollama_only() {
        for regional in [
            Language::SpanishArgentina,
            Language::SpanishColombia,
            Language::SpanishMexico,
        ] {
            assert!(!allowed_targets(Language::English).contains(&regional));
            assert!(!allowed_targets_for(Provider::DeepL, Language::English).contains(&regional));
            assert!(allowed_targets_for(Provider::Ollama, Language::English).contains(&regional));
            assert!(is_supported_pair(Language::English, regional));
        }
    }

    #[test]
    fn azorean_portuguese_is_ollama_only() {
        assert!(!allowed_targets(Language::English).contains(&Language::PortugueseAzores));
        assert!(!allowed_targets_for(Provider::DeepL, Language::English)
            .contains(&Language::PortugueseAzores));
        assert!(allowed_targets_for(Provider::Ollama, Language::English)
            .contains(&Language::PortugueseAzores));
        assert!(is_supported_pair(
            Language::English,
            Language::PortugueseAzores
        ));
    }

    #[test]
    fn provider_specific_target_fallback() {
        assert_eq!(
            target_for_source_with(
                Provider::Ollama,
                Language::English,
                Language::PortugueseAngola
            ),
            Language::PortugueseAngola
        );
        assert_eq!(
            target_for_source_with(
                Provider::DeepL,
                Language::English,
                Language::PortugueseAngola
            ),
            Language::PortuguesePortugal
        );
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

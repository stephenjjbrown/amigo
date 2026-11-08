//! The language catalogue: every language the app knows, which provider can use
//! it and in which role, and the coarser variant to fall back to when one cannot.
//!
//! DeepL's lists are the larger of the two — 114 source and 121 target codes —
//! and its source side never takes a region (`PT`, not `PT-PT`).
//! TranslateGemma covers its 55 verified languages plus the same regional
//! variants, using BCP-47 tags in both roles. The table is the single source of
//! truth: a `None` code means that role is not offered.

use serde::{Deserialize, Serialize};

use crate::provider::Provider;

/// DeepL accepts up to 128 KiB of text per request; keep a margin below that.
pub const MAX_TEXT_CHARS: usize = 100_000;

/// Which end of a translation a language is being used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Source,
    Target,
}

/// One entry in the catalogue.
#[derive(Debug, Clone, Copy)]
pub struct LanguageDef {
    pub language: Language,
    pub label: &'static str,
    /// The identifier shown in diagnostics.
    pub code: &'static str,
    /// Tag used in the TranslateGemma prompt, when the model's verified set
    /// (the WMT24++ languages) covers this one.
    pub gemma: Option<&'static str>,
    /// The coarser variant to use when a provider does not support this one.
    pub fallback: Option<Language>,
    /// DeepL code as a source, or `None` when DeepL cannot read this language.
    pub deepl_source: Option<&'static str>,
    /// DeepL code as a target, or `None` when DeepL cannot produce it.
    pub deepl_target: Option<&'static str>,
}

macro_rules! catalogue {
    ($($variant:ident, $label:literal, $code:literal, $gemma:tt, $fallback:tt, $dsrc:tt, $dtgt:tt;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum Language { $($variant),* }

        impl Language {
            /// Stable, unique identifier for this language: its variant name.
            /// Codes can repeat (`ES` is both the source and the Spain target),
            /// element ids cannot.
            pub fn id(self) -> &'static str {
                match self { $(Self::$variant => stringify!($variant)),* }
            }
        }

        pub const CATALOGUE: &[LanguageDef] = &[
            $(LanguageDef {
                language: Language::$variant,
                label: $label,
                code: $code,
                gemma: opt!($gemma),
                fallback: fallback!($fallback),
                deepl_source: opt!($dsrc),
                deepl_target: opt!($dtgt),
            }),*
        ];
    };
}

macro_rules! fallback {
    (none) => {
        None
    };
    ($variant:ident) => {
        Some(Language::$variant)
    };
}

macro_rules! opt {
    (none) => {
        None
    };
    ($value:literal) => {
        Some($value)
    };
}

catalogue! {
    Acehnese, "Acehnese", "ACE", none, none, "ACE", "ACE";
    Afrikaans, "Afrikaans", "AF", none, none, "AF", "AF";
    Aragonese, "Aragonese", "AN", none, none, "AN", "AN";
    Arabic, "Arabic", "AR", none, none, "AR", "AR";
    Assamese, "Assamese", "AS", "as", none, "AS", "AS";
    Aymara, "Aymara", "AY", none, none, "AY", "AY";
    Azerbaijani, "Azerbaijani", "AZ", none, none, "AZ", "AZ";
    Bashkir, "Bashkir", "BA", none, none, "BA", "BA";
    Belarusian, "Belarusian", "BE", "be", none, "BE", "BE";
    Bulgarian, "Bulgarian", "BG", "bg", none, "BG", "BG";
    Bhojpuri, "Bhojpuri", "BHO", none, none, "BHO", "BHO";
    Bengali, "Bengali", "BN", "bn", none, "BN", "BN";
    Breton, "Breton", "BR", none, none, "BR", "BR";
    Bosnian, "Bosnian", "BS", "bs", none, "BS", "BS";
    Catalan, "Catalan", "CA", "ca", none, "CA", "CA";
    Cebuano, "Cebuano", "CEB", none, none, "CEB", "CEB";
    KurdishSorani, "Kurdish (Sorani)", "CKB", none, none, "CKB", "CKB";
    Czech, "Czech", "CS", "cs", none, "CS", "CS";
    Welsh, "Welsh", "CY", "cy", none, "CY", "CY";
    Danish, "Danish", "DA", "da", none, "DA", "DA";
    German, "German", "DE", "de", none, "DE", none;
    Greek, "Greek", "EL", "el", none, "EL", "EL";
    English, "English", "EN", "en", none, "EN", none;
    Esperanto, "Esperanto", "EO", none, none, "EO", "EO";
    Spanish, "Spanish", "ES", "es", none, "ES", none;
    Estonian, "Estonian", "ET", "et", none, "ET", "ET";
    Basque, "Basque", "EU", none, none, "EU", "EU";
    Persian, "Persian", "FA", "fa", none, "FA", "FA";
    Finnish, "Finnish", "FI", "fi", none, "FI", "FI";
    French, "French", "FR", "fr", none, "FR", none;
    Irish, "Irish", "GA", none, none, "GA", "GA";
    Galician, "Galician", "GL", "gl", none, "GL", "GL";
    Guarani, "Guarani", "GN", none, none, "GN", "GN";
    Konkani, "Konkani", "GOM", none, none, "GOM", "GOM";
    Gujarati, "Gujarati", "GU", "gu", none, "GU", "GU";
    Hausa, "Hausa", "HA", none, none, "HA", "HA";
    Hebrew, "Hebrew", "HE", "he", none, "HE", "HE";
    Hindi, "Hindi", "HI", "hi", none, "HI", "HI";
    Croatian, "Croatian", "HR", "hr", none, "HR", "HR";
    HaitianCreole, "Haitian Creole", "HT", none, none, "HT", "HT";
    Hungarian, "Hungarian", "HU", "hu", none, "HU", "HU";
    Armenian, "Armenian", "HY", none, none, "HY", "HY";
    Indonesian, "Indonesian", "ID", "id", none, "ID", "ID";
    Igbo, "Igbo", "IG", none, none, "IG", "IG";
    Icelandic, "Icelandic", "IS", "is", none, "IS", "IS";
    Italian, "Italian", "IT", "it", none, "IT", "IT";
    Japanese, "Japanese", "JA", "ja", none, "JA", "JA";
    Javanese, "Javanese", "JV", none, none, "JV", "JV";
    Georgian, "Georgian", "KA", none, none, "KA", "KA";
    Kazakh, "Kazakh", "KK", none, none, "KK", "KK";
    KurdishKurmanji, "Kurdish (Kurmanji)", "KMR", none, none, "KMR", "KMR";
    Korean, "Korean", "KO", "ko", none, "KO", "KO";
    Kyrgyz, "Kyrgyz", "KY", none, none, "KY", "KY";
    Latin, "Latin", "LA", none, none, "LA", "LA";
    Luxembourgish, "Luxembourgish", "LB", none, none, "LB", "LB";
    Lombard, "Lombard", "LMO", none, none, "LMO", "LMO";
    Lingala, "Lingala", "LN", none, none, "LN", "LN";
    Lithuanian, "Lithuanian", "LT", "lt", none, "LT", "LT";
    Latvian, "Latvian", "LV", "lv", none, "LV", "LV";
    Maithili, "Maithili", "MAI", none, none, "MAI", "MAI";
    Malagasy, "Malagasy", "MG", none, none, "MG", "MG";
    Maori, "Maori", "MI", none, none, "MI", "MI";
    Macedonian, "Macedonian", "MK", "mk", none, "MK", "MK";
    Malayalam, "Malayalam", "ML", "ml", none, "ML", "ML";
    Mongolian, "Mongolian", "MN", "mn", none, "MN", "MN";
    Marathi, "Marathi", "MR", "mr", none, "MR", "MR";
    Malay, "Malay", "MS", none, none, "MS", "MS";
    Maltese, "Maltese", "MT", none, none, "MT", "MT";
    Burmese, "Burmese", "MY", none, none, "MY", "MY";
    Norwegian, "Norwegian (bokmål)", "NB", "nb", none, "NB", "NB";
    Nepali, "Nepali", "NE", none, none, "NE", "NE";
    Dutch, "Dutch", "NL", "nl", none, "NL", "NL";
    Occitan, "Occitan", "OC", none, none, "OC", "OC";
    Oromo, "Oromo", "OM", none, none, "OM", "OM";
    Punjabi, "Punjabi", "PA", "pa", none, "PA", "PA";
    Pangasinan, "Pangasinan", "PAG", none, none, "PAG", "PAG";
    Kapampangan, "Kapampangan", "PAM", none, none, "PAM", "PAM";
    Polish, "Polish", "PL", "pl", none, "PL", "PL";
    Dari, "Dari", "PRS", none, none, "PRS", "PRS";
    Pashto, "Pashto", "PS", none, none, "PS", "PS";
    Portuguese, "Portuguese", "PT", "pt", none, "PT", none;
    Quechua, "Quechua", "QU", none, none, "QU", "QU";
    Romanian, "Romanian", "RO", "ro", none, "RO", "RO";
    Russian, "Russian", "RU", "ru", none, "RU", "RU";
    Sanskrit, "Sanskrit", "SA", none, none, "SA", "SA";
    Sicilian, "Sicilian", "SCN", none, none, "SCN", "SCN";
    Slovak, "Slovak", "SK", "sk", none, "SK", "SK";
    Slovenian, "Slovenian", "SL", "sl", none, "SL", "SL";
    Albanian, "Albanian", "SQ", none, none, "SQ", "SQ";
    Serbian, "Serbian", "SR", "sr", none, "SR", "SR";
    Sesotho, "Sesotho", "ST", none, none, "ST", "ST";
    Sundanese, "Sundanese", "SU", none, none, "SU", "SU";
    Swedish, "Swedish", "SV", "sv", none, "SV", "SV";
    Swahili, "Swahili", "SW", "sw", none, "SW", "SW";
    Tamil, "Tamil", "TA", "ta", none, "TA", "TA";
    Telugu, "Telugu", "TE", "te", none, "TE", "TE";
    Tajik, "Tajik", "TG", none, none, "TG", "TG";
    Thai, "Thai", "TH", "th", none, "TH", "TH";
    Turkmen, "Turkmen", "TK", none, none, "TK", "TK";
    Tagalog, "Tagalog", "TL", none, none, "TL", "TL";
    Tswana, "Tswana", "TN", none, none, "TN", "TN";
    Turkish, "Turkish", "TR", "tr", none, "TR", "TR";
    Tsonga, "Tsonga", "TS", none, none, "TS", "TS";
    Tatar, "Tatar", "TT", none, none, "TT", "TT";
    Ukrainian, "Ukrainian", "UK", "uk", none, "UK", "UK";
    Urdu, "Urdu", "UR", "ur", none, "UR", "UR";
    Uzbek, "Uzbek", "UZ", none, none, "UZ", "UZ";
    Vietnamese, "Vietnamese", "VI", "vi", none, "VI", "VI";
    Wolof, "Wolof", "WO", none, none, "WO", "WO";
    Xhosa, "Xhosa", "XH", none, none, "XH", "XH";
    Yiddish, "Yiddish", "YI", none, none, "YI", "YI";
    Cantonese, "Cantonese", "YUE", none, none, "YUE", "YUE";
    Chinese, "Chinese (Simplified)", "ZH", "zh", none, "ZH", none;
    Zulu, "Zulu", "ZU", none, none, "ZU", "ZU";
    GermanSwiss, "German (Swiss)", "DE-CH", "de-CH", German, none, "DE-CH";
    GermanGermany, "German (Germany)", "DE", "de-DE", German, none, "DE";
    EnglishBritish, "English (British)", "EN-GB", "en-GB", English, none, "EN-GB";
    EnglishAmerican, "English (American)", "EN-US", "en-US", English, none, "EN-US";
    SpanishLatinAmerican, "Spanish (Latin American)", "ES-419", "es-419", Spanish, none, "ES-419";
    // DeepL's bare `ES` target is Spain Spanish (its docs list `ES` and `ES-ES`
    // under the same name, and only `ES` is offered for translation). It is a
    // separate entry from `Spanish` because the base must stay a source-only
    // language: a base can be read generically but resolves to Latin America
    // when a *default* target is needed, while Spain is chosen explicitly.
    SpanishSpain, "Spanish (Spain)", "ES", "es-ES", Spanish, none, "ES";
    FrenchCanadian, "French (Canadian)", "FR-CA", "fr-CA", French, none, "FR-CA";
    FrenchFrance, "French (France)", "FR", "fr-FR", French, none, "FR";
    PortugueseBrazil, "Portuguese (Brazilian)", "PT-BR", "pt-BR", Portuguese, none, "PT-BR";
    PortuguesePortugal, "Portuguese (European)", "PT-PT", "pt-PT", Portuguese, none, "PT-PT";
    ChineseSimplified, "Chinese (Simplified)", "ZH-HANS", "zh-Hans", Chinese, none, "ZH-HANS";
    ChineseTraditional, "Chinese (Traditional)", "ZH-HANT", "zh-Hant", Chinese, none, "ZH-HANT";
    Kannada, "Kannada", "KN", "kn", none, none, none;
}

impl Language {
    /// The catalogue entry for this language.
    pub fn info(self) -> &'static LanguageDef {
        CATALOGUE
            .iter()
            .find(|entry| entry.language == self)
            .expect("every language is in the catalogue")
    }

    pub fn label(self) -> &'static str {
        self.info().label
    }

    pub fn code(self) -> &'static str {
        self.info().code
    }

    pub fn gemma_code(self) -> Option<&'static str> {
        self.info().gemma
    }

    /// The coarser variant to use when a provider lacks this one.
    pub fn fallback(self) -> Option<Language> {
        self.info().fallback
    }

    /// Whether the provider can use this language in this role, unresolved.
    pub fn is_supported_by(self, provider: Provider, role: Role) -> bool {
        match provider {
            // Gemma claims its verified (WMT24++) languages plus the regional
            // variants DeepL offers — but a localisation is a *target* concern:
            // `FR-CA` is offered to write, not to read, and a stored regional
            // source falls back to its base language.
            Provider::Ollama => match role {
                Role::Source => self.info().gemma.is_some() && self.info().fallback.is_none(),
                // A localised language is written, not spoken: `English` is
                // never a target, `English (American)` is.
                Role::Target => self.info().gemma.is_some() && !self.has_variants(),
            },
            Provider::DeepL => match role {
                Role::Source => self.info().deepl_source.is_some(),
                Role::Target => self.info().deepl_target.is_some(),
            },
        }
    }

    /// Whether any catalogue entry is a localisation of this language.
    fn has_variants(self) -> bool {
        CATALOGUE.iter().any(|entry| entry.fallback == Some(self))
    }

    /// The regional variant to use when a base language is wanted as a target but
    /// the provider only accepts its regions ("reverse resolve": coarser to more
    /// specific). The choice is the *most widely used* variant, not the first one
    /// in the catalogue: Brazilian Portuguese, American English, simplified
    /// Chinese. English and the rest already resolve directly today; they are
    /// listed so the rule stays in one place if a provider ever stops taking the
    /// base code as a target.
    fn preferred_target(self) -> Option<Language> {
        match self {
            Language::English => Some(Language::EnglishAmerican),
            Language::Portuguese => Some(Language::PortugueseBrazil),
            Language::Spanish => Some(Language::SpanishLatinAmerican),
            Language::Chinese => Some(Language::ChineseSimplified),
            Language::German => Some(Language::GermanGermany),
            Language::French => Some(Language::FrenchFrance),
            _ => None,
        }
    }

    /// The language a provider should actually receive: this one, its nearest
    /// supported ancestor, or — for a base language that is source-only — its
    /// preferred regional target. `PT-PT` becomes `PT` when DeepL reads it, and
    /// `PT` becomes `PT-PT` when DeepL writes it.
    pub fn resolve(self, provider: Provider, role: Role) -> Option<Language> {
        let mut candidate = Some(self);
        while let Some(language) = candidate {
            if language.is_supported_by(provider, role) {
                return Some(language);
            }
            candidate = language.fallback();
        }

        if role == Role::Target {
            if let Some(variant) = self.preferred_target() {
                return variant.resolve(provider, role);
            }
        }

        None
    }

    /// The DeepL code for this language in a role, if DeepL supports it directly.
    pub fn deepl_code(self, role: Role) -> Option<&'static str> {
        match role {
            Role::Source => self.info().deepl_source,
            Role::Target => self.info().deepl_target,
        }
    }
}

/// Every language in the catalogue, in menu order: alphabetical by label, so a
/// language's regional variants sit directly under it.
pub fn all_languages() -> Vec<Language> {
    let mut languages: Vec<Language> = CATALOGUE.iter().map(|entry| entry.language).collect();
    languages.sort_by_key(|language| language.label());
    languages
}

/// Every language the provider can use in `role`, in menu order.
pub fn languages_for(provider: Provider, role: Role) -> Vec<Language> {
    all_languages()
        .into_iter()
        .filter(|language| language.is_supported_by(provider, role))
        .collect()
}

/// Targets a provider can produce for `source`, excluding the source itself.
pub fn allowed_targets_for(provider: Provider, source: Language) -> Vec<Language> {
    languages_for(provider, Role::Target)
        .into_iter()
        .filter(|target| *target != source)
        .collect()
}

/// Whether the pair can be translated by this provider after fallbacks.
pub fn is_supported_pair(provider: Provider, source: Language, target: Language) -> bool {
    source.resolve(provider, Role::Source).is_some()
        && target.resolve(provider, Role::Target).is_some()
}

/// Pick the target that should follow a newly chosen source language.
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

/// Swap direction, resolving each end for the provider. `EN → PT-PT` becomes
/// `EN → PT` in reverse because DeepL has no regional source language.
pub fn swap_pair(provider: Provider, pair: LangPair) -> Option<LangPair> {
    let source = pair.target.resolve(provider, Role::Source)?;
    let target = pair.source.resolve(provider, Role::Target)?;
    Some(LangPair { source, target })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LangPair {
    pub source: Language,
    pub target: Language,
}

impl Default for LangPair {
    fn default() -> Self {
        Self {
            source: Language::Portuguese,
            // Targets are localised, so the default says which English.
            target: Language::EnglishAmerican,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_unique_and_complete() {
        let mut seen = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for entry in CATALOGUE {
            assert!(
                seen.insert(entry.language),
                "duplicate {:?}",
                entry.language
            );
            // Element ids come from `id()`, so it must be unique even where a
            // code is shared by a source and its home target (ES, DE, FR).
            assert!(ids.insert(entry.language.id()), "duplicate id");
            let _ = entry.language.info();
        }
        assert_eq!(seen.len(), CATALOGUE.len());
    }

    #[test]
    fn deepl_has_distinct_source_and_target_lists() {
        let sources = languages_for(Provider::DeepL, Role::Source);
        let targets = languages_for(Provider::DeepL, Role::Target);

        assert_eq!(sources.len(), 114);
        assert_eq!(targets.len(), 120);

        // Portuguese and Chinese are read under their base code and written
        // under a regional one.
        assert!(sources.contains(&Language::Portuguese));
        assert!(!targets.contains(&Language::Portuguese));
        assert!(targets.contains(&Language::PortuguesePortugal));
        assert!(targets.contains(&Language::PortugueseBrazil));
        assert!(!sources.contains(&Language::PortugueseBrazil));

        // Localised languages are read under their base code and written under
        // a localisation; the bare code is never a target.
        assert!(sources.contains(&Language::English));
        assert!(!targets.contains(&Language::English));
        assert!(targets.contains(&Language::EnglishAmerican));
        assert!(targets.contains(&Language::EnglishBritish));
        assert!(!targets.contains(&Language::Spanish));
        assert!(targets.contains(&Language::SpanishSpain));
        assert!(targets.contains(&Language::SpanishLatinAmerican));

        // The long tail is only in the newer list.
        assert!(sources.contains(&Language::Acehnese));
        assert!(targets.contains(&Language::Acehnese));
    }

    #[test]
    fn gemma_offers_only_its_verified_set() {
        let sources = languages_for(Provider::Ollama, Role::Source);

        // The 55 verified languages: localisations are targets, not sources.
        assert_eq!(sources.len(), 55);
        assert!(sources.contains(&Language::Kannada));
        assert!(sources.contains(&Language::English));
        assert!(sources.contains(&Language::Portuguese));
        assert!(!sources.contains(&Language::PortugueseBrazil));
        assert!(!sources.contains(&Language::Acehnese));

        // Every regional variant DeepL offers is a Gemma *target*, and resolves
        // back to its base language when used as a source.
        for variant in [
            Language::PortuguesePortugal,
            Language::PortugueseBrazil,
            Language::EnglishBritish,
            Language::EnglishAmerican,
            Language::SpanishLatinAmerican,
            Language::FrenchCanadian,
            Language::GermanSwiss,
            Language::ChineseSimplified,
            Language::ChineseTraditional,
        ] {
            assert!(
                !sources.contains(&variant),
                "{variant:?} should not be a Gemma source"
            );
            assert_eq!(
                variant.resolve(Provider::Ollama, Role::Target),
                Some(variant),
                "{variant:?} is not a Gemma target"
            );
        }

        for language in sources {
            assert!(language.gemma_code().is_some());
            // A localised base resolves down to its default localisation.
            assert!(language.resolve(Provider::Ollama, Role::Target).is_some());
        }
    }

    #[test]
    fn regions_are_target_only_for_deepl_and_fall_back_to_the_base() {
        assert_eq!(
            Language::PortugueseBrazil.resolve(Provider::DeepL, Role::Source),
            Some(Language::Portuguese)
        );
        assert_eq!(
            Language::PortugueseBrazil.resolve(Provider::DeepL, Role::Target),
            Some(Language::PortugueseBrazil)
        );
        assert_eq!(
            Language::ChineseTraditional.resolve(Provider::DeepL, Role::Source),
            Some(Language::Chinese)
        );
        assert_eq!(
            Language::EnglishAmerican.resolve(Provider::Ollama, Role::Source),
            Some(Language::English)
        );
    }

    #[test]
    fn swapping_degrades_the_region_away() {
        let pair = LangPair {
            source: Language::English,
            target: Language::PortuguesePortugal,
        };

        let swapped = swap_pair(Provider::DeepL, pair).unwrap();
        assert_eq!(swapped.source, Language::Portuguese);
        assert_eq!(swapped.target, Language::EnglishAmerican);

        // Gemma offers the region as a target, but reads the base language.
        let swapped = swap_pair(Provider::Ollama, pair).unwrap();
        assert_eq!(swapped.source, Language::Portuguese);
    }

    #[test]
    fn resolution_walks_up_for_sources_and_down_for_targets() {
        // Specific -> base: a region collapses to its base language when read.
        assert_eq!(
            Language::PortugueseBrazil.resolve(Provider::DeepL, Role::Source),
            Some(Language::Portuguese)
        );

        // Base -> specific: a base that is not a target falls through to its most
        // widely used variant.
        assert_eq!(
            Language::Portuguese.resolve(Provider::DeepL, Role::Target),
            Some(Language::PortugueseBrazil)
        );

        // An exact target is never rewritten.
        assert_eq!(
            Language::PortugueseBrazil.resolve(Provider::DeepL, Role::Target),
            Some(Language::PortugueseBrazil)
        );
        assert_eq!(
            Language::PortuguesePortugal.resolve(Provider::DeepL, Role::Target),
            Some(Language::PortuguesePortugal)
        );

        // Gemma localises targets the same way.
        assert_eq!(
            Language::Portuguese.resolve(Provider::Ollama, Role::Target),
            Some(Language::PortugueseBrazil)
        );
    }

    #[test]
    fn swapping_a_source_only_base_picks_its_regional_target() {
        let pair = LangPair {
            source: Language::Portuguese,
            target: Language::English,
        };
        let swapped = swap_pair(Provider::DeepL, pair).unwrap();
        assert_eq!(swapped.source, Language::English);
        // Brazilian Portuguese is the popular variant, so it wins the choice.
        assert_eq!(swapped.target, Language::PortugueseBrazil);

        let pair = LangPair {
            source: Language::Chinese,
            target: Language::English,
        };
        let swapped = swap_pair(Provider::DeepL, pair).unwrap();
        assert_eq!(swapped.target, Language::ChineseSimplified);

        // Gemma localises too, so the same choice is made there.
        let swapped = swap_pair(
            Provider::Ollama,
            LangPair {
                source: Language::Portuguese,
                target: Language::English,
            },
        )
        .unwrap();
        assert_eq!(swapped.target, Language::PortugueseBrazil);
    }

    #[test]
    fn changing_the_source_retargets_to_something_the_provider_offers() {
        let target = target_for_source_with(
            Provider::DeepL,
            Language::English,
            Language::ChineseTraditional,
        );
        assert!(target.is_supported_by(Provider::DeepL, Role::Target));
    }
}

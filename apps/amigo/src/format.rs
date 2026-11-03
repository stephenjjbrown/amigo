//! Text derived from a configured slot, so the views do not grow formatting
//! rules of their own.

use deepl_core::config::{ProviderSlot, TranslationMode};
use deepl_core::Provider;

/// "Ollama · PT-PT → EN" — what a result card is titled with.
pub fn slot_title(slot: &ProviderSlot) -> String {
    format!(
        "{} · {} → {}",
        slot.kind.label(),
        slot.pair.source.code(),
        slot.pair.target.code()
    )
}

/// The model and mode under the title: "Gemma 4 31B · 3 variants".
pub fn slot_subtitle(slot: &ProviderSlot) -> String {
    match slot.kind {
        Provider::DeepL => "Single translation".to_string(),
        Provider::Ollama => match slot.mode {
            TranslationMode::Single => slot.model.label().to_string(),
            TranslationMode::Multi => format!("{} · {} variants", slot.model.label(), slot.count()),
        },
    }
}

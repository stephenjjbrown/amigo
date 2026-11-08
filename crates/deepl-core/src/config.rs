//! The user's workspace configuration: provider slots, saved presets and the
//! options that go with them.
//!
//! This lives beside the engine rather than in a UI crate so both front ends share
//! one definition, one file format and one set of migration rules.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ollama::{OllamaModel, Register, Sampling, MAX_PARALLEL_LIMIT, MAX_VARIANTS};
use crate::pairs::{LangPair, Language, Role};
use crate::provider::Provider;
use crate::target_for_source_with;

pub const DEFAULT_VARIANTS: usize = 5;
pub const DEFAULT_MULTI_TEMPERATURE: f32 = 1.7;
pub const DEFAULT_MULTI_TOP_P: f32 = 0.95;

const CONFIG_FILE: &str = "preferences.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TranslationMode {
    #[default]
    Single,
    Multi,
}

/// One provider's configuration. There is exactly one of these per [`Provider`]:
/// providers are a fixed set, so a slot needs no identity of its own — the kind
/// is the identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderSlot {
    pub kind: Provider,
    #[serde(default)]
    pub pair: LangPair,
    #[serde(default)]
    pub model: OllamaModel,
    #[serde(default)]
    pub mode: TranslationMode,
    /// Present a multi translation as diffs against the most typical sample,
    /// rather than as a flat list of samples.
    #[serde(default = "default_true")]
    pub diff_view: bool,
    /// In the diff view, hide the struck-out removals and show only the result.
    #[serde(default)]
    pub hide_deletions: bool,
    #[serde(default)]
    pub register: Register,
    /// Situational context appended to this provider's prompt.
    #[serde(default)]
    pub context: String,
    #[serde(default = "default_variants")]
    pub variants: usize,
    #[serde(default = "default_multi_temperature")]
    pub temperature: f32,
    #[serde(default = "default_multi_top_p")]
    pub top_p: f32,
}

impl Default for ProviderSlot {
    fn default() -> Self {
        Self {
            kind: Provider::Ollama,
            pair: LangPair::default(),
            model: OllamaModel::default(),
            mode: TranslationMode::Single,
            diff_view: true,
            hide_deletions: false,
            register: Register::default(),
            context: String::new(),
            variants: DEFAULT_VARIANTS,
            temperature: DEFAULT_MULTI_TEMPERATURE,
            top_p: DEFAULT_MULTI_TOP_P,
        }
    }
}

impl ProviderSlot {
    /// Whether two slots describe the same provider configuration.
    pub fn same_configuration(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.pair == other.pair
            && self.model == other.model
            && self.mode == other.mode
            && self.diff_view == other.diff_view
            && self.hide_deletions == other.hide_deletions
            && self.register == other.register
            && self.context == other.context
            && self.variants == other.variants
            && self.temperature == other.temperature
            && self.top_p == other.top_p
    }

    /// The context hint, when one is set.
    pub fn context(&self) -> Option<&str> {
        let context = self.context.trim();
        (!context.is_empty()).then_some(context)
    }

    pub fn sampling(&self) -> Sampling {
        match self.mode {
            TranslationMode::Single => Sampling::default(),
            TranslationMode::Multi => Sampling {
                temperature: self.temperature,
                top_p: self.top_p,
            },
        }
    }

    /// Local models are only offered for single translation; multi sampling runs
    /// in the cloud, so a slot that moves to multi falls back to the cloud model.
    pub fn enforce_model_for_mode(&mut self) {
        if self.mode == TranslationMode::Multi && !self.model.is_cloud() {
            self.model = OllamaModel::default();
        }
    }

    /// How many samples this slot asks for.
    pub fn count(&self) -> usize {
        match self.mode {
            TranslationMode::Single => 1,
            TranslationMode::Multi => self.variants.clamp(1, MAX_VARIANTS),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub providers: Vec<ProviderSlot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub providers: Vec<ProviderSlot>,
    /// The provider in use, or `None` for the "Select Provider" state.
    #[serde(default)]
    pub selected_provider: Option<Provider>,
    /// Languages offered in the dropdowns. `None` means every catalogue entry.
    #[serde(default)]
    pub enabled_languages: Option<Vec<Language>>,
    /// Saved provider sets the user can switch between.
    #[serde(default)]
    pub presets: Vec<Preset>,
    #[serde(default)]
    pub verbose_logging: bool,
    #[serde(default = "default_local_parallel")]
    pub local_parallel: usize,
    #[serde(default = "default_cloud_parallel")]
    pub cloud_parallel: usize,

    // Legacy single-provider layout. Read once so existing installs migrate
    // their DeepL/Ollama setup into the provider list.
    #[serde(rename = "deepl_pair", default, alias = "pair")]
    pub legacy_deepl_pair: LangPair,
    #[serde(rename = "ollama_pair", default)]
    pub legacy_ollama_pair: LangPair,
    #[serde(rename = "deepl_enabled", default = "default_true")]
    pub legacy_deepl_enabled: bool,
    #[serde(rename = "ollama_enabled", default)]
    pub legacy_ollama_enabled: bool,
    #[serde(rename = "ollama_model", default)]
    pub legacy_ollama_model: OllamaModel,
    #[serde(rename = "register", default)]
    pub legacy_register: Register,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            providers: Provider::ALL
                .iter()
                .map(|kind| ProviderSlot {
                    kind: *kind,
                    ..ProviderSlot::default()
                })
                .collect(),
            selected_provider: None,
            enabled_languages: None,
            presets: Vec::new(),
            verbose_logging: false,
            local_parallel: default_local_parallel(),
            cloud_parallel: default_cloud_parallel(),
            legacy_deepl_pair: LangPair::default(),
            legacy_ollama_pair: LangPair::default(),
            legacy_deepl_enabled: true,
            legacy_ollama_enabled: false,
            legacy_ollama_model: OllamaModel::default(),
            legacy_register: Register::default(),
        }
    }
}

impl Preferences {
    /// Rebuild the providers from the pre-multi-provider layout when there are
    /// none stored, then fold them into the fixed shape.
    pub fn migrate(&mut self) {
        if self.providers.is_empty() {
            self.providers = vec![
                ProviderSlot {
                    kind: Provider::DeepL,
                    pair: self.legacy_deepl_pair,
                    ..ProviderSlot::default()
                },
                ProviderSlot {
                    kind: Provider::Ollama,
                    pair: self.legacy_ollama_pair,
                    model: self.legacy_ollama_model,
                    register: self.legacy_register,
                    ..ProviderSlot::default()
                },
            ];
            if self.selected_provider.is_none() {
                self.selected_provider = if self.legacy_deepl_enabled {
                    Some(Provider::DeepL)
                } else if self.legacy_ollama_enabled {
                    Some(Provider::Ollama)
                } else {
                    None
                };
            }
        }
        self.normalise();
        // Presets were snapshots of the old free-form list. Fold them too, so a
        // saved single-provider preset still compares equal to the fixed pair.
        for preset in &mut self.presets {
            preset.providers = fold(preset.providers.clone());
        }
    }

    /// Exactly one configuration per kind, in `Provider::ALL` order. The first
    /// entry of a kind wins; a missing kind gets a default, because both
    /// providers are always on offer.
    fn normalise(&mut self) {
        self.providers = fold(std::mem::take(&mut self.providers));
        for slot in &mut self.providers {
            slot.enforce_model_for_mode();
        }
    }

    /// The configuration of one provider.
    pub fn slot(&self, kind: Provider) -> Option<&ProviderSlot> {
        self.providers.iter().find(|slot| slot.kind == kind)
    }

    /// Whether a language is offered in the dropdowns.
    pub fn language_enabled(&self, language: Language) -> bool {
        match &self.enabled_languages {
            None => true,
            Some(enabled) => enabled.contains(&language),
        }
    }

    /// Show or hide a language in the dropdowns.
    pub fn set_language_enabled(&mut self, language: Language, enabled: bool) {
        let mut languages = self
            .enabled_languages
            .clone()
            .unwrap_or_else(crate::all_languages);
        if enabled {
            if !languages.contains(&language) {
                languages.push(language);
            }
        } else {
            languages.retain(|kept| *kept != language);
        }
        self.enabled_languages = Some(languages);
    }

    /// Apply a pair chosen in the UI. The selected provider keeps it verbatim
    /// (its menus only offer what it supports); the others keep the closest
    /// equivalent they can translate, or their current value when there is none.
    /// This is what makes the pair behave as one setting while staying stored per
    /// provider.
    pub fn set_pair_everywhere(&mut self, selected: Provider, pair: LangPair) {
        for kind in Provider::ALL {
            let Some(mut slot) = self.slot(kind).cloned() else {
                continue;
            };

            if kind == selected {
                slot.pair = pair;
            } else {
                slot.pair.source = pair
                    .source
                    .resolve(kind, Role::Source)
                    .unwrap_or(slot.pair.source);
                slot.pair.target = pair
                    .target
                    .resolve(kind, Role::Target)
                    .unwrap_or(slot.pair.target);
            }

            // A provider may collapse a region onto its base language, which can
            // make source and target agree; pick something else in that case.
            if slot.pair.target == slot.pair.source {
                slot.pair.target = target_for_source_with(kind, slot.pair.source, slot.pair.target);
            }

            self.set_slot(slot);
        }
    }

    /// Replace the configuration of one provider.
    pub fn set_slot(&mut self, slot: ProviderSlot) {
        match self
            .providers
            .iter_mut()
            .find(|stored| stored.kind == slot.kind)
        {
            Some(stored) => *stored = slot,
            None => self.providers.push(slot),
        }
        self.normalise();
    }

    /// How many requests a model may have in flight at once.
    pub fn parallel_for(&self, model: OllamaModel) -> usize {
        let limit = if model.is_cloud() {
            self.cloud_parallel
        } else {
            self.local_parallel
        };
        limit.clamp(1, MAX_PARALLEL_LIMIT)
    }

    /// Snapshot the current providers under `name`, replacing any preset that
    /// already has that name.
    pub fn save_preset(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }

        let providers = self.providers.clone();
        match self.presets.iter_mut().find(|preset| preset.name == name) {
            Some(preset) => preset.providers = providers,
            None => self.presets.push(Preset {
                name: name.to_string(),
                providers,
            }),
        }
    }

    /// Replace the current providers with a preset's, one per kind.
    pub fn apply_preset(&mut self, index: usize) -> bool {
        let Some(preset) = self.presets.get(index) else {
            return false;
        };
        if preset.providers.is_empty() {
            return false;
        }

        self.providers = preset.providers.clone();
        self.normalise();
        true
    }

    pub fn delete_preset(&mut self, index: usize) {
        if index < self.presets.len() {
            self.presets.remove(index);
        }
    }

    /// Rename a preset. Fails on a blank name or one another preset already uses.
    pub fn rename_preset(&mut self, index: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let taken = self
            .presets
            .iter()
            .enumerate()
            .any(|(other, preset)| other != index && preset.name == name);
        if taken {
            return false;
        }

        let Some(preset) = self.presets.get_mut(index) else {
            return false;
        };
        preset.name = name.to_string();
        true
    }

    /// Whether a preset's providers still match the current ones.
    pub fn matches_preset(&self, index: usize) -> bool {
        self.presets
            .get(index)
            .map(|preset| same_providers(&preset.providers, &self.providers))
            .unwrap_or(false)
    }

    pub fn load() -> Self {
        match load_from(&config_path()) {
            Some(mut prefs) => {
                prefs.migrate();
                prefs
            }
            None => {
                let mut prefs = Preferences::default();
                prefs.migrate();
                prefs.save();
                prefs
            }
        }
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }
}

pub fn same_providers(left: &[ProviderSlot], right: &[ProviderSlot]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.same_configuration(right))
}

/// Fold any provider list into one configuration per kind, in `Provider::ALL`
/// order. Shared by the live providers and by preset snapshots.
fn fold(stored: Vec<ProviderSlot>) -> Vec<ProviderSlot> {
    let mut remaining = stored;
    Provider::ALL
        .iter()
        .map(
            |kind| match remaining.iter().position(|slot| slot.kind == *kind) {
                Some(index) => remaining.remove(index),
                None => ProviderSlot {
                    kind: *kind,
                    ..ProviderSlot::default()
                },
            },
        )
        .collect()
}

pub fn config_path() -> PathBuf {
    crate::session_store::support_dir().join(CONFIG_FILE)
}

fn load_from(path: &PathBuf) -> Option<Preferences> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn default_variants() -> usize {
    DEFAULT_VARIANTS
}

fn default_multi_temperature() -> f32 {
    DEFAULT_MULTI_TEMPERATURE
}

fn default_multi_top_p() -> f32 {
    DEFAULT_MULTI_TOP_P
}

fn default_local_parallel() -> usize {
    1
}

fn default_cloud_parallel() -> usize {
    3
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_through_json() {
        let mut prefs = Preferences::default();
        prefs.save_preset("European");
        let json = serde_json::to_string(&prefs).unwrap();
        let back: Preferences = serde_json::from_str(&json).unwrap();
        assert_eq!(back.presets.len(), 1);
        assert_eq!(back.presets[0].name, "European");
    }

    #[test]
    fn applying_a_preset_restores_one_slot_per_kind() {
        let mut prefs = Preferences::default();
        prefs.save_preset("one");
        prefs.set_slot(ProviderSlot {
            kind: Provider::Ollama,
            pair: LangPair::default(),
            ..ProviderSlot::default()
        });
        assert!(prefs.apply_preset(0));
        assert_eq!(prefs.providers.len(), 2);
        assert!(prefs.slot(Provider::Ollama).is_some());
    }

    #[test]
    fn duplicate_kinds_fold_to_the_first() {
        let mut prefs = Preferences {
            providers: vec![
                ProviderSlot {
                    kind: Provider::Ollama,
                    register: Register::Neutral,
                    ..ProviderSlot::default()
                },
                ProviderSlot {
                    kind: Provider::DeepL,
                    ..ProviderSlot::default()
                },
                ProviderSlot {
                    kind: Provider::Ollama,
                    register: Register::Casual,
                    ..ProviderSlot::default()
                },
            ],
            ..Preferences::default()
        };

        prefs.migrate();

        assert_eq!(prefs.providers.len(), 2);
        assert_eq!(prefs.providers[0].kind, Provider::DeepL);
        assert_eq!(
            prefs.slot(Provider::Ollama).unwrap().register,
            Register::Neutral
        );
    }

    #[test]
    fn the_pair_is_stored_per_provider_but_behaves_as_one_setting() {
        let mut prefs = Preferences::default();

        // Chosen with Gemma selected, which accepts the region as a source.
        prefs.set_pair_everywhere(
            Provider::Ollama,
            LangPair {
                source: Language::PortuguesePortugal,
                target: Language::English,
            },
        );

        assert_eq!(
            prefs.slot(Provider::Ollama).unwrap().pair.source,
            Language::PortuguesePortugal
        );
        // DeepL never reads a region, so it keeps the base language.
        assert_eq!(
            prefs.slot(Provider::DeepL).unwrap().pair.source,
            Language::Portuguese
        );
        // Targets are localised: `English` resolves to `English (American)`.
        assert_eq!(
            prefs.slot(Provider::DeepL).unwrap().pair.target,
            Language::EnglishAmerican
        );
    }

    #[test]
    fn a_missing_kind_gets_a_default_slot() {
        let mut prefs = Preferences {
            providers: vec![ProviderSlot {
                kind: Provider::DeepL,
                ..ProviderSlot::default()
            }],
            ..Preferences::default()
        };

        prefs.migrate();

        assert_eq!(prefs.providers.len(), 2);
        assert!(prefs.slot(Provider::Ollama).is_some());
    }

    #[test]
    fn migration_folds_preset_snapshots_too() {
        let mut prefs = Preferences {
            presets: vec![Preset {
                name: "old".to_string(),
                providers: vec![ProviderSlot {
                    kind: Provider::Ollama,
                    ..ProviderSlot::default()
                }],
            }],
            ..Preferences::default()
        };

        prefs.migrate();

        assert_eq!(prefs.presets[0].providers.len(), 2);
    }
}

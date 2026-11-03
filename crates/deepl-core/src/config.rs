//! The user's workspace configuration: provider slots, saved presets and the
//! options that go with them.
//!
//! This lives beside the engine rather than in a UI crate so both front ends share
//! one definition, one file format and one set of migration rules.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ollama::{OllamaModel, Register, Sampling, MAX_PARALLEL_LIMIT, MAX_VARIANTS};
use crate::pairs::LangPair;
use crate::provider::Provider;

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

/// One configured translation provider in the workspace. The app can hold any
/// number of these, including several of the same kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderSlot {
    pub id: u64,
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
            id: 0,
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
    /// Whether two slots describe the same provider configuration. The `id` is
    /// runtime identity — loading a preset re-keys it — so it is not part of the
    /// comparison.
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
    /// Saved provider sets the user can switch between.
    #[serde(default)]
    pub presets: Vec<Preset>,
    #[serde(default)]
    pub next_slot_id: u64,
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
            providers: vec![ProviderSlot {
                id: 1,
                kind: Provider::DeepL,
                ..ProviderSlot::default()
            }],
            next_slot_id: 2,
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
    /// Rebuild the provider list from the pre-multi-provider layout.
    pub fn migrate(&mut self) {
        if !self.providers.is_empty() {
            let highest = self.providers.iter().map(|slot| slot.id).max().unwrap_or(0);
            if self.next_slot_id <= highest {
                self.next_slot_id = highest + 1;
            }
            return;
        }

        let mut providers = Vec::new();
        if self.legacy_deepl_enabled {
            providers.push(ProviderSlot {
                id: providers.len() as u64 + 1,
                kind: Provider::DeepL,
                pair: self.legacy_deepl_pair,
                ..ProviderSlot::default()
            });
        }
        if self.legacy_ollama_enabled {
            providers.push(ProviderSlot {
                id: providers.len() as u64 + 1,
                kind: Provider::Ollama,
                pair: self.legacy_ollama_pair,
                model: self.legacy_ollama_model,
                register: self.legacy_register,
                ..ProviderSlot::default()
            });
        }

        self.next_slot_id = providers.len() as u64 + 1;
        self.providers = providers;
    }

    pub fn allocate_slot_id(&mut self) -> u64 {
        let id = self.next_slot_id.max(1);
        self.next_slot_id = id + 1;
        id
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
        if name.is_empty() || self.providers.is_empty() {
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

    /// Replace the current providers with a preset's, re-keying their ids so they
    /// cannot collide with outcomes or with each other.
    pub fn apply_preset(&mut self, index: usize) -> bool {
        let Some(preset) = self.presets.get(index) else {
            return false;
        };
        let providers = preset.providers.clone();
        if providers.is_empty() {
            return false;
        }

        self.providers = providers
            .into_iter()
            .enumerate()
            .map(|(offset, mut slot)| {
                slot.id = offset as u64 + 1;
                slot
            })
            .collect();
        self.next_slot_id = self.providers.len() as u64 + 1;
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
    fn applying_a_preset_rekeys_provider_ids() {
        let mut prefs = Preferences::default();
        prefs.save_preset("one");
        assert!(prefs.apply_preset(0));
        assert_eq!(prefs.providers[0].id, 1);
        assert_eq!(prefs.next_slot_id, 2);
    }
}

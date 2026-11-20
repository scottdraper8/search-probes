use crate::{binding::InputBinding, criteria::Criteria, theme::UiTheme};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BUDGETS: [u64; 6] = [1_000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000];
pub const BUDGET_LABELS: [&str; 6] = [
    "1,000",
    "10,000",
    "100,000",
    "1,000,000",
    "10,000,000",
    "100,000,000",
];
pub const MAX_RESULTS: usize = 100;
pub const MAX_PRESETS: usize = 9;
pub const MAX_SETTINGS_BYTES: usize = 1_048_576;
pub const MAX_WORKERS: usize = 16;

pub fn validate_frame_budget(frame_budget_ms: u8) -> Result<(), &'static str> {
    if !(1..=16).contains(&frame_budget_ms) {
        return Err("frame budget must be between 1 and 16 milliseconds");
    }
    Ok(())
}

pub fn available_workers() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .clamp(1, MAX_WORKERS)
}

pub fn recommended_workers(available: usize) -> usize {
    (available.clamp(1, MAX_WORKERS) / 2).max(1)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchOptions {
    pub criteria: Criteria,
    pub systems: u64,
    pub results: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            criteria: Criteria::default(),
            systems: 10_000,
            results: 5,
        }
    }
}

impl SearchOptions {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.criteria.compile()?;
        if !BUDGETS.contains(&self.systems) {
            return Err("system budget must be a dropdown value");
        }
        if !(1..=MAX_RESULTS).contains(&self.results) {
            return Err("result limit must be between 1 and 100");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    pub search: SearchOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub search: SearchOptions,
    pub presets: Vec<Preset>,
    pub interface: InterfaceSettings,
    pub workers: Option<usize>,
    pub frame_budget_ms: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceSettings {
    pub hotkey: InputBinding,
    pub theme: UiTheme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            search: SearchOptions::default(),
            presets: Vec::new(),
            interface: InterfaceSettings::default(),
            workers: None,
            frame_budget_ms: 4,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.search.validate()?;
        self.interface.hotkey.validate()?;
        if self.presets.len() > MAX_PRESETS {
            return Err("at most nine Guide presets can be saved");
        }
        let mut names = BTreeSet::new();
        for preset in &self.presets {
            validate_name(&preset.name)?;
            if !names.insert(preset.name.to_lowercase()) {
                return Err("preset names must be unique");
            }
            preset.search.validate()?;
        }
        if self
            .workers
            .is_some_and(|count| !(1..=MAX_WORKERS).contains(&count))
        {
            return Err("worker count must be automatic or between 1 and 16");
        }
        validate_frame_budget(self.frame_budget_ms)
    }
}

pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty()
        || name.trim() != name
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
    {
        return Err(
            "preset name must contain 1 to 80 characters without edge whitespace or control characters",
        );
    }
    Ok(())
}

pub fn nearest_budget(value: u64) -> u64 {
    *BUDGETS
        .iter()
        .min_by_key(|&&budget| (budget.abs_diff(value), std::cmp::Reverse(budget)))
        .expect("nonempty budgets")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_use_the_current_interface_defaults() {
        let settings = Settings::default();
        let encoded = serde_json::to_value(&settings).expect("settings serialize");

        assert!(encoded.get("schema").is_none());
        assert_eq!(encoded["interface"]["hotkey"]["device"], "keyboard");
        assert_eq!(encoded["interface"]["hotkey"]["first"], "control_left");
        assert_eq!(encoded["interface"]["hotkey"]["second"], "comma");
        assert_eq!(encoded["interface"]["theme"], "search_probes_dark");
        assert!(settings.validate().is_ok());
    }
}

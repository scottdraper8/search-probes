use crate::criteria::{Criteria, Field, PlanetFlag};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BUDGETS: [u64; 7] = [
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
];
pub const BUDGET_LABELS: [&str; 7] = [
    "1,000",
    "10,000",
    "100,000",
    "1,000,000",
    "10,000,000",
    "100,000,000",
    "1,000,000,000",
];
pub const MAX_RESULTS: usize = 100;
pub const MAX_PRESETS: usize = 9;
pub const MAX_SETTINGS_BYTES: usize = 1_048_576;

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
    pub schema: u32,
    pub search: SearchOptions,
    pub presets: Vec<Preset>,
    pub workers: usize,
    pub frame_budget_ms: u8,
}

impl Default for Settings {
    fn default() -> Self {
        let mut earthlike = SearchOptions::default();
        earthlike.criteria.choices.extend([
            (Field::Biome, 1),
            (Field::Grass, 1 << 3),
            (Field::Sky, 1 << 5),
        ]);
        earthlike
            .criteria
            .planets
            .extend([(PlanetFlag::Paradise, true), (PlanetFlag::Moons, true)]);
        Self {
            schema: 1,
            search: SearchOptions::default(),
            presets: vec![Preset {
                name: "Earthlike".into(),
                search: earthlike,
            }],
            workers: 2,
            frame_budget_ms: 4,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != 1 {
            return Err("unsupported settings schema");
        }
        self.search.validate()?;
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
        if !(1..=16).contains(&self.workers) {
            return Err("worker count must be between 1 and 16");
        }
        if !(1..=16).contains(&self.frame_budget_ms) {
            return Err("frame budget must be between 1 and 16 milliseconds");
        }
        Ok(())
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
    fn budgets_and_presets_use_only_dropdown_values() {
        let settings = Settings::default();
        settings.validate().unwrap();
        assert_eq!(nearest_budget(20_000), 10_000);
        assert_eq!(nearest_budget(55_000), 100_000);
        assert_eq!(nearest_budget(u64::MAX), 1_000_000_000);
        assert_eq!(nearest_budget(0), 1_000);
        for budget in BUDGETS {
            assert_eq!(nearest_budget(budget), budget);
        }
    }
    #[test]
    fn persisted_inputs_are_bounded_and_strict() {
        let mut settings = Settings::default();
        settings.search.results = 100;
        settings.validate().unwrap();
        settings.search.results = 101;
        assert!(settings.validate().is_err());
        settings.search.results = 1;
        settings.presets.push(settings.presets[0].clone());
        assert!(settings.validate().is_err());
        assert!(serde_json::from_str::<Settings>(r#"{"schema":1,"obsolete":true}"#).is_err());
        assert_eq!(
            serde_json::from_str::<Settings>(&serde_json::to_string(&Settings::default()).unwrap())
                .unwrap(),
            Settings::default()
        );
    }
}

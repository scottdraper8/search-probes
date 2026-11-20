use crate::{
    binding::InputBinding,
    criteria::{Criteria, Field, Plan, Predicate},
    theme::UiTheme,
};
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
pub const FLORA_COLOUR_SEARCH_ERROR: &str = "non-grass flora colour search is unavailable until planet-wide occurrence can be established; inspect the current planet for generated recipe observations";

pub fn validate_search_plan(plan: &Plan) -> Result<(), &'static str> {
    if plan.planet.iter().any(|predicate| {
        matches!(predicate, Predicate::Choice(choices) if choices.field() == Field::FloraColour)
    }) {
        return Err(FLORA_COLOUR_SEARCH_ERROR);
    }
    Ok(())
}

/// Literal two-field hours/minutes input. Minutes 60..=99 are intentional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeLimit {
    pub hours: u8,
    pub minutes: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSettings {
    pub workers: Option<usize>,
    pub frame_budget_ms: u8,
}

impl ExecutionSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if self
            .workers
            .is_some_and(|workers| !(1..=MAX_WORKERS).contains(&workers))
        {
            return Err("worker count must be automatic or between 1 and 16");
        }
        validate_frame_budget(self.frame_budget_ms)
    }
}

impl TimeLimit {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.hours > 99 || self.minutes > 99 || (self.hours == 0 && self.minutes == 0) {
            return Err("time limit must be between 00:01 and 99:99");
        }
        Ok(())
    }

    pub fn duration(self) -> std::time::Duration {
        std::time::Duration::from_secs((u64::from(self.hours) * 60 + u64::from(self.minutes)) * 60)
    }
}

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
    pub time_limit: Option<TimeLimit>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            criteria: Criteria::default(),
            systems: 10_000,
            results: 5,
            time_limit: None,
        }
    }
}

impl SearchOptions {
    pub fn validate_search(&self) -> Result<(), &'static str> {
        self.validate()?;
        validate_search_plan(&self.criteria.compile()?)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        self.criteria.compile()?;
        if let Some(limit) = self.time_limit {
            limit.validate()?;
        }
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
    pub notifications: NotificationSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationSettings {
    pub result_found: bool,
    pub search_completed: bool,
    pub automatic_pause: bool,
    pub automatic_resume: bool,
    pub placement: ToastPlacement,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            result_found: true,
            search_completed: true,
            automatic_pause: true,
            automatic_resume: true,
            placement: ToastPlacement::TopRight,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastPlacement {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl ToastPlacement {
    pub const ALL: &[Self] = &[
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "Top left",
            Self::TopCenter => "Top center",
            Self::TopRight => "Top right",
            Self::BottomLeft => "Bottom left",
            Self::BottomCenter => "Bottom center",
            Self::BottomRight => "Bottom right",
        }
    }
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
    pub fn execution(&self) -> ExecutionSettings {
        ExecutionSettings {
            workers: self.workers,
            frame_budget_ms: self.frame_budget_ms,
        }
    }
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
    fn flora_colour_configuration_can_be_inspected_but_cannot_start_a_search() {
        let mut settings = Settings::default();
        settings
            .search
            .criteria
            .choices
            .insert(Field::FloraColour, 1);
        assert!(settings.validate().is_ok());
        assert!(settings.search.criteria.compile().is_ok());
        assert_eq!(
            settings.search.validate_search(),
            Err(FLORA_COLOUR_SEARCH_ERROR)
        );
        settings.search.criteria.choices.remove(&Field::FloraColour);
        assert!(settings.search.validate_search().is_ok());
    }

    #[test]
    fn literal_time_limits_preserve_non_normalized_minutes_and_reject_zero_and_overflow() {
        let maximum = TimeLimit {
            hours: 99,
            minutes: 99,
        };
        assert!(maximum.validate().is_ok());
        assert_eq!(maximum.duration().as_secs(), 362_340);
        assert!(
            TimeLimit {
                hours: 0,
                minutes: 99
            }
            .validate()
            .is_ok()
        );
        for (hours, minutes) in [(0, 0), (100, 0), (0, 100)] {
            assert!(TimeLimit { hours, minutes }.validate().is_err());
        }
        for invalid in [
            r#"{"hours":256,"minutes":0}"#,
            r#"{"hours":-1,"minutes":1}"#,
            r#"{"hours":1.5,"minutes":0}"#,
            r#"{"hours":1,"minutes":0,"seconds":5}"#,
        ] {
            assert!(serde_json::from_str::<TimeLimit>(invalid).is_err());
        }
        let mut settings = Settings::default();
        assert_eq!(
            serde_json::to_value(&settings).unwrap()["search"]["time_limit"],
            serde_json::Value::Null
        );
        settings.search.time_limit = Some(maximum);
        let decoded: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(decoded.search.time_limit, Some(maximum));
        settings.search.time_limit = Some(TimeLimit {
            hours: 0,
            minutes: 0,
        });
        assert!(settings.validate().is_err());
    }

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

    #[test]
    fn notification_preferences_are_a_strict_current_contract() {
        for &placement in ToastPlacement::ALL {
            let mut settings = Settings::default();
            settings.interface.notifications.placement = placement;
            settings.interface.notifications.automatic_pause = false;
            let encoded = serde_json::to_value(&settings).unwrap();
            assert_eq!(
                serde_json::from_value::<Settings>(encoded.clone()).unwrap(),
                settings
            );
            let mut invalid = encoded.clone();
            invalid["interface"]["notifications"]["placement"] = "anywhere".into();
            assert!(serde_json::from_value::<Settings>(invalid).is_err());
            let mut invalid = encoded.clone();
            invalid["interface"]["notifications"]["unknown"] = true.into();
            assert!(serde_json::from_value::<Settings>(invalid).is_err());
            let mut incomplete = encoded;
            incomplete["interface"]["notifications"]
                .as_object_mut()
                .unwrap()
                .remove("automatic_pause");
            assert!(serde_json::from_value::<Settings>(incomplete).is_err());
        }
    }
}

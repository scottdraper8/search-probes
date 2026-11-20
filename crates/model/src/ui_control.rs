use crate::{
    arena::ArenaField,
    binding::{BindingDevice, InputBinding},
    criteria::{
        CompositionField, Criteria, Field, PlanetFlag, SelectionSide, SystemFlag, WaterFilter,
    },
    fauna::{Domain, FaunaField},
    settings::{NotificationSettings, SearchOptions, Settings, TimeLimit},
    theme::UiTheme,
};
use serde::{Deserialize, Serialize};

pub const UI_ACTIONS: &[(&str, &[&str])] = &[
    ("set_overlay_visible", &["visible"]),
    ("set_settings_sidebar", &["open"]),
    ("set_page", &["page"]),
    ("set_draft_settings", &["settings"]),
    ("set_draft_search", &["search"]),
    ("set_criteria", &["criteria"]),
    ("set_choice_filter", &["field", "selected"]),
    ("set_planet_requirement", &["flag", "value"]),
    ("set_system_requirement", &["flag", "value"]),
    ("set_resource_filter", &["selected"]),
    ("set_minimum_planets", &["minimum"]),
    ("set_water_filter", &["scope", "value"]),
    ("set_composition_filter", &["field", "side", "selected"]),
    ("set_fauna_choices", &["domain", "field", "selected"]),
    ("set_fauna_presence", &["domain", "value"]),
    ("set_fauna_height", &["domain", "minimum_cm", "maximum_cm"]),
    ("set_arena_choices", &["field", "selected"]),
    ("set_arena_presence", &["value"]),
    ("set_system_budget", &["systems"]),
    ("set_result_limit", &["results"]),
    ("set_time_limit", &["time_limit"]),
    ("set_workers", &["workers"]),
    ("set_frame_budget", &["frame_budget_ms"]),
    ("set_theme", &["theme"]),
    ("set_hotkey", &["hotkey"]),
    ("set_notifications", &["notifications"]),
    ("set_hotkey_dialog", &["open"]),
    ("set_hotkey_capture", &["device"]),
    ("select_preset", &["index"]),
    ("clear_preset_selection", &[]),
    ("set_preset_name", &["name"]),
    ("save_preset", &[]),
    ("remove_preset", &[]),
    ("clear_criteria", &[]),
    ("apply_settings", &[]),
    ("start_search", &[]),
    ("set_search_paused", &["paused"]),
    ("cancel_search", &[]),
    ("set_viewed_search", &["search_id"]),
    ("step_history", &["direction"]),
    ("navigate_result", &["search_id", "result_index"]),
    ("copy_result", &["search_id", "result_index", "value"]),
    ("export", &["source", "format"]),
    ("show_toast", &["toast"]),
    ("clear_toasts", &[]),
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiPage {
    #[default]
    Search,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaterScope {
    System,
    Planet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryDirection {
    Older,
    Newer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultValue {
    PortalCode,
    GalacticCoordinates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiExportSource {
    Results,
    CurrentSearch,
    ViewedSearch,
    Presets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiExportFormat {
    Text,
    Csv,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TestToast {
    ResultFound { count: usize },
    SearchCompleted { message: String },
    AutomaticPause { reason: String },
    AutomaticResume {},
    ExportCompleted { subject: String, path: String },
}

/// Semantic operations exposed by the rendered Search Probes interface. These are intentionally
/// higher level than pointer coordinates: each operation uses the same application state and
/// validation path as its corresponding UI control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiAction {
    SetOverlayVisible {
        visible: bool,
    },
    SetSettingsSidebar {
        open: bool,
    },
    SetPage {
        page: UiPage,
    },
    SetDraftSettings {
        settings: Settings,
    },
    SetDraftSearch {
        search: SearchOptions,
    },
    SetCriteria {
        criteria: Criteria,
    },
    SetChoiceFilter {
        field: Field,
        selected: Vec<String>,
    },
    SetPlanetRequirement {
        flag: PlanetFlag,
        value: Option<bool>,
    },
    SetSystemRequirement {
        flag: SystemFlag,
        value: Option<bool>,
    },
    SetResourceFilter {
        selected: Vec<String>,
    },
    SetMinimumPlanets {
        minimum: Option<u8>,
    },
    SetWaterFilter {
        scope: WaterScope,
        value: WaterFilter,
    },
    SetCompositionFilter {
        field: CompositionField,
        side: SelectionSide,
        selected: Vec<String>,
    },
    SetFaunaChoices {
        domain: Domain,
        field: FaunaField,
        selected: Vec<String>,
    },
    SetFaunaPresence {
        domain: Domain,
        value: Option<bool>,
    },
    SetFaunaHeight {
        domain: Domain,
        minimum_cm: Option<u32>,
        maximum_cm: Option<u32>,
    },
    SetArenaChoices {
        field: ArenaField,
        selected: Vec<String>,
    },
    SetArenaPresence {
        value: Option<bool>,
    },
    SetSystemBudget {
        systems: u64,
    },
    SetResultLimit {
        results: usize,
    },
    SetTimeLimit {
        time_limit: Option<TimeLimit>,
    },
    SetWorkers {
        workers: Option<usize>,
    },
    SetFrameBudget {
        frame_budget_ms: u8,
    },
    SetTheme {
        theme: UiTheme,
    },
    SetHotkey {
        hotkey: InputBinding,
    },
    SetNotifications {
        notifications: NotificationSettings,
    },
    SetHotkeyDialog {
        open: bool,
    },
    SetHotkeyCapture {
        device: Option<BindingDevice>,
    },
    SelectPreset {
        index: usize,
    },
    ClearPresetSelection {},
    SetPresetName {
        name: String,
    },
    SavePreset {},
    RemovePreset {},
    ClearCriteria {},
    ApplySettings {},
    StartSearch {},
    SetSearchPaused {
        paused: bool,
    },
    CancelSearch {},
    SetViewedSearch {
        search_id: Option<u64>,
    },
    StepHistory {
        direction: HistoryDirection,
    },
    NavigateResult {
        search_id: u64,
        result_index: usize,
    },
    CopyResult {
        search_id: u64,
        result_index: usize,
        value: ResultValue,
    },
    Export {
        source: UiExportSource,
        format: UiExportFormat,
    },
    ShowToast {
        toast: TestToast,
    },
    ClearToasts {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiOutput {
    RuntimeCommand {
        command_id: u64,
    },
    CopiedText {
        value: ResultValue,
        text: String,
    },
    Export {
        source: UiExportSource,
        format: UiExportFormat,
        subject: String,
        suggested_name: String,
        contents: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToastSnapshot {
    pub title: String,
    pub message: String,
    pub created_ms: u64,
    pub expires_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiState {
    pub overlay_visible: bool,
    pub page: UiPage,
    pub settings_sidebar: bool,
    pub hotkey_dialog: bool,
    pub hotkey_capture: Option<BindingDevice>,
    pub selected_preset: Option<usize>,
    pub preset_name: String,
    pub viewed_search: Option<u64>,
    pub draft_settings: Settings,
    pub toasts: Vec<ToastSnapshot>,
    pub last_command: u64,
    pub error: Option<String>,
    pub output: Option<UiOutput>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            overlay_visible: false,
            page: UiPage::Search,
            settings_sidebar: false,
            hotkey_dialog: false,
            hotkey_capture: None,
            selected_preset: None,
            preset_name: String::new(),
            viewed_search: None,
            draft_settings: Settings::default(),
            toasts: Vec::new(),
            last_command: 0,
            error: None,
            output: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiCommand {
    pub generation: u64,
    pub id: u64,
    pub action: UiAction,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn advertised_actions_are_unique_and_have_unique_fields() {
        let mut actions = BTreeSet::new();
        for (kind, fields) in UI_ACTIONS {
            assert!(!kind.is_empty());
            assert!(actions.insert(*kind), "duplicate UI action {kind}");
            let unique: BTreeSet<_> = fields.iter().copied().collect();
            assert_eq!(unique.len(), fields.len(), "duplicate field in {kind}");
        }
    }

    #[test]
    fn action_payloads_reject_unknown_fields() {
        assert!(
            serde_json::from_str::<UiAction>(
                r#"{"type":"set_page","page":"history","legacy":true}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<UiAction>(r#"{"type":"set_page","page":"history"}"#).is_ok()
        );
    }
}

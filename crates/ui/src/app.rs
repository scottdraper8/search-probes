use crate::{
    export::{ExportFormat, ExportRequest},
    notifications::{Notification, Notifications, SearchProgress, result_subtitle},
    portal_glyphs::PortalGlyphs,
    theme::{self, Palette},
};
use egui::{self, RichText, Ui};
use search_probes_model::controls::Controls;
use search_probes_model::{
    arena::ArenaField,
    binding::{BindingDevice, ControllerButton, InputBinding, KeyboardKey},
    cache::CachedMatch,
    criteria::{
        CompositionField, Criteria, Field, PlanetFlag, SelectionSide, SystemFlag, WaterFilter,
        subtype_options_for,
    },
    fauna::{Domain, FaunaCriteria, FaunaField},
    runtime::{Action, Availability, LifecycleEvent, LifecycleEventKind, SearchPause, State},
    session::{Finish, Phase},
    settings::{
        BUDGET_LABELS, BUDGETS, MAX_PRESETS, MAX_RESULTS, NotificationSettings, Preset,
        SearchOptions, Settings, TimeLimit, ToastPlacement, available_workers, recommended_workers,
        validate_custom_theme_name, validate_frame_budget, validate_name,
    },
    target::TargetKind,
    theme::{CustomTheme, MAX_CUSTOM_THEMES, ThemeColor, ThemeSelection, UiTheme},
    ui_control::{
        HistoryDirection, ResultValue, TestToast, UiAction, UiCommand, UiExportFormat,
        UiExportSource, UiOutput, UiState, WaterScope,
    },
    vocabulary::RESOURCE_LABELS,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

const CRITERIA_SPLIT_MIN_WIDTH: f32 = 820.0;
const CRITERIA_THREE_COLUMN_MIN_WIDTH: f32 = 1230.0;
const MAIN_SPLIT_MIN_WIDTH: f32 = 2460.0;
const SEARCH_PANEL_SPLIT_MIN_WIDTH: f32 = 800.0;
const RESULT_CARD_MIN_WIDTH: f32 = 310.0;
const COMBO_POPUP_MAX_HEIGHT: f32 = 360.0;
const NON_SCROLLING_SELECT_MAX_OPTIONS: usize = 10;
const SEARCH_ACTION_HEIGHT: f32 = 32.0;
const PRIMARY_SEARCH_ACTION_WIDTH: f32 = 130.0;
const SECONDARY_SEARCH_ACTION_WIDTH: f32 = 110.0;
const SECTION_CONTENT_MARGIN: f32 = 10.0;
const SETTINGS_SIDEBAR_WIDTH: f32 = 340.0;
const SETTINGS_SIDEBAR_LAYER: &str = "search_probes_settings_layer";

fn search_action_button(button: egui::Button<'_>, width: f32) -> egui::Button<'_> {
    button.min_size(egui::vec2(width, SEARCH_ACTION_HEIGHT))
}

fn export_button(label: &'static str) -> egui::Button<'static> {
    let icon = egui::Image::from_bytes(
        "bytes://search-probes/icons/export.svg",
        include_bytes!("../assets/icons/export.svg"),
    )
    .fit_to_exact_size(egui::vec2(18.0, 18.0));
    egui::Button::new(label)
        .right_text(icon)
        .image_tint_follows_text_color(true)
}

fn section_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::group(style).inner_margin(SECTION_CONTENT_MARGIN)
}

fn palette_swatch(ui: &mut Ui, label: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, color);
        ui.painter().rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            egui::StrokeKind::Inside,
        );
        response.on_hover_text(format!(
            "{label}: #{:02X}{:02X}{:02X}",
            color.r(),
            color.g(),
            color.b()
        ));
        ui.label(label);
    });
}

fn edit_theme_color(ui: &mut Ui, label: &str, color: &mut ThemeColor) {
    ui.label(label);
    let mut rgb = [color.red, color.green, color.blue];
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        *color = ThemeColor {
            red: rgb[0],
            green: rgb[1],
            blue: rgb[2],
        };
    }
    ui.label(format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]));
    ui.end_row();
}

fn select_menu(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    selected_text: impl Into<String>,
    width: f32,
    option_count: usize,
    close_behavior: egui::PopupCloseBehavior,
    contents: impl FnOnce(&mut Ui),
) {
    let selected_text = selected_text.into();
    ui.push_id(id, |ui| {
        let chevron = egui::Image::from_bytes(
            "bytes://search-probes/icons/chevron-down.svg",
            include_bytes!("../assets/icons/chevron-down.svg"),
        )
        .fit_to_exact_size(egui::vec2(12.0, 8.0));
        let button = egui::Button::new(selected_text)
            .right_text(chevron)
            .image_tint_follows_text_color(true)
            .min_size(egui::vec2(width, ui.spacing().interact_size.y))
            .truncate();
        egui::containers::menu::MenuButton::from_button(button)
            .config(egui::containers::menu::MenuConfig::new().close_behavior(close_behavior))
            .ui(ui, |ui| {
                ui.set_min_width(width);
                if option_count <= NON_SCROLLING_SELECT_MAX_OPTIONS {
                    contents(ui);
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(COMBO_POPUP_MAX_HEIGHT)
                        .show(ui, contents);
                }
            });
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CapturedInput {
    Keyboard(KeyboardKey),
    Controller(ControllerButton),
}

#[derive(Clone)]
struct SearchDraft {
    search: SearchOptions,
    workers: Option<usize>,
    frame_budget_ms: u8,
    keep_game_running: bool,
}

#[derive(Clone, Copy)]
struct RuntimeActionState {
    viewing_past: bool,
    ready: bool,
    running: bool,
    active: bool,
    user_paused: bool,
    automatically_paused: bool,
    search_valid: bool,
}

#[derive(Clone, Copy)]
struct RatePoint {
    search_id: u64,
    elapsed_ms: u64,
    systems: u64,
    planets: u64,
    fauna: u64,
}

#[derive(Default)]
struct RollingRates {
    point: Option<RatePoint>,
    values: Option<[f64; 3]>,
}

impl RollingRates {
    fn observe(&mut self, state: &State) {
        let Some(progress) = state.progress.as_ref() else {
            *self = Self::default();
            return;
        };
        let next = RatePoint {
            search_id: progress.id,
            elapsed_ms: state.metrics.elapsed_ms,
            systems: state.metrics.generated_systems,
            planets: state.metrics.generated_planets,
            fauna: state.metrics.generated_fauna,
        };
        let Some(previous) = self
            .point
            .filter(|previous| previous.search_id == next.search_id)
        else {
            self.point = Some(next);
            self.values = None;
            return;
        };
        let elapsed = next.elapsed_ms.saturating_sub(previous.elapsed_ms);
        if elapsed > 0 {
            let scale = 1000.0 / elapsed as f64;
            let sample = [
                next.systems.saturating_sub(previous.systems) as f64 * scale,
                next.planets.saturating_sub(previous.planets) as f64 * scale,
                next.fauna.saturating_sub(previous.fauna) as f64 * scale,
            ];
            self.values = Some(self.values.map_or(sample, |current| {
                std::array::from_fn(|index| current[index] * 0.65 + sample[index] * 0.35)
            }));
        }
        self.point = Some(next);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PresetNameDialog {
    SaveCurrentSearch,
    Rename { index: usize },
}

impl SearchDraft {
    fn capture(settings: &Settings) -> Self {
        Self {
            search: settings.search.clone(),
            workers: settings.workers,
            frame_budget_ms: settings.frame_budget_ms,
            keep_game_running: settings.keep_game_running,
        }
    }

    fn restore(self, settings: &mut Settings) {
        settings.search = self.search;
        settings.workers = self.workers;
        settings.frame_budget_ms = self.frame_budget_ms;
        settings.keep_game_running = self.keep_game_running;
    }
}

impl CapturedInput {
    fn device(self) -> BindingDevice {
        match self {
            Self::Keyboard(_) => BindingDevice::Keyboard,
            Self::Controller(_) => BindingDevice::Controller,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Keyboard(key) => key.label(),
            Self::Controller(button) => button.label(),
        }
    }
}

struct BindingCapture {
    device: BindingDevice,
    controller: Option<u8>,
    held: Vec<CapturedInput>,
    candidate: Vec<CapturedInput>,
    overflowed: bool,
}

impl BindingCapture {
    fn new(device: BindingDevice) -> Self {
        Self {
            device,
            controller: None,
            held: Vec::new(),
            candidate: Vec::new(),
            overflowed: false,
        }
    }

    fn set_device(&mut self, device: BindingDevice) {
        if self.device != device {
            *self = Self::new(device);
        }
    }

    fn input(&mut self, input: CapturedInput, pressed: bool) -> Option<InputBinding> {
        if input.device() != self.device {
            return None;
        }
        if pressed {
            if self.held.contains(&input) {
                return None;
            }
            self.held.push(input);
            if self.candidate.len() < 2 {
                self.candidate.push(input);
            } else {
                self.overflowed = true;
            }
        } else {
            self.held.retain(|held| *held != input);
        }
        if !self.held.is_empty() || self.candidate.is_empty() {
            return None;
        }
        if self.overflowed {
            self.candidate.clear();
            self.overflowed = false;
            self.controller = None;
            return None;
        }
        let first = self.candidate[0];
        let second = self.candidate.get(1).copied();
        match (first, second) {
            (CapturedInput::Keyboard(first), None) => Some(InputBinding::Keyboard {
                first,
                second: None,
            }),
            (CapturedInput::Keyboard(first), Some(CapturedInput::Keyboard(second))) => {
                Some(InputBinding::Keyboard {
                    first,
                    second: Some(second),
                })
            }
            (CapturedInput::Controller(first), None) => Some(InputBinding::Controller {
                first,
                second: None,
            }),
            (CapturedInput::Controller(first), Some(CapturedInput::Controller(second))) => {
                Some(InputBinding::Controller {
                    first,
                    second: Some(second),
                })
            }
            _ => None,
        }
    }
}

pub struct App {
    controls: Arc<Controls>,
    state_revision: u64,
    state: State,
    settings: Settings,
    loaded_generation: Option<u64>,
    preset_name: String,
    selected_preset: Option<usize>,
    preset_dialog: Option<PresetNameDialog>,
    managed_preset: Option<usize>,
    managed_preset_name: String,
    theme_editor: Option<CustomTheme>,
    theme_editor_original_name: Option<String>,
    error: Option<String>,
    published_settings: Option<Settings>,
    pending_search: Option<u64>,
    binding_capture: Option<BindingCapture>,
    export_request: Option<ExportRequest>,
    pending_export_subject: Option<&'static str>,
    notifications: Notifications,
    started: Instant,
    settings_sidebar: bool,
    viewed_search: Option<u64>,
    history_return_draft: Option<SearchDraft>,
    latest_search_id: Option<u64>,
    lifecycle_sequence: u64,
    applied_theme: Option<(ThemeSelection, Palette, Option<UiTheme>)>,
    portal_glyphs: PortalGlyphs,
    overlay_visible: bool,
    ui_last_command: u64,
    ui_error: Option<String>,
    ui_output: Option<UiOutput>,
    rolling_rates: RollingRates,
    published_ui: Option<(UiState, u64)>,
}

fn labels_mask<'a>(
    selected: &[String],
    options: impl IntoIterator<Item = (u64, &'a str)>,
) -> Result<u64, String> {
    let mut available = BTreeMap::new();
    for (mask, label) in options {
        *available.entry(label).or_insert(0) |= mask;
    }
    let mut unique = BTreeSet::new();
    let mut result = 0;
    for label in selected {
        if !unique.insert(label.as_str()) {
            return Err(format!("Filter option is selected more than once: {label}"));
        }
        result |= available
            .get(label.as_str())
            .copied()
            .ok_or_else(|| format!("Filter option is unavailable: {label}"))?;
    }
    Ok(result)
}

fn count_mask(selected: &[u8], maximum: u8) -> Result<u8, String> {
    let mut result = 0u8;
    for &count in selected {
        if !(1..=maximum).contains(&count) {
            return Err(format!("Count selection is unavailable: {count}"));
        }
        let bit = 1 << (count - 1);
        if result & bit != 0 {
            return Err(format!("Count is selected more than once: {count}"));
        }
        result |= bit;
    }
    Ok(result)
}

fn remove_empty_fauna(criteria: &mut Criteria, domain: Domain) {
    let empty = criteria
        .fauna
        .get(&domain)
        .is_some_and(|selected| selected.presence.is_none() && !selected.has_detail_filters());
    if empty {
        criteria.fauna.remove(&domain);
    }
}

impl App {
    pub fn new(ctx: &egui::Context, controls: Arc<Controls>) -> Self {
        egui_extras::install_image_loaders(ctx);
        for egui_theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.style_mut_of(egui_theme, |style| {
                style.spacing.item_spacing = egui::vec2(10.0, 8.0);
                style.spacing.combo_height = COMBO_POPUP_MAX_HEIGHT;
                for font in style.text_styles.values_mut() {
                    font.size += 3.0;
                }
                if let Some(font) = style.text_styles.get_mut(&egui::TextStyle::Small) {
                    font.size += 3.0;
                }
            });
        }
        theme::apply(ctx, UiTheme::default());
        let (state_revision, state) = controls
            .snapshot_if_changed(0)
            .expect("new controls publish an initial revision");
        let settings = state.settings.clone();
        let generation = state.generation;
        let latest_search_id = state.search_history.last().map(|record| record.progress.id);
        let started = Instant::now();
        let mut notifications = Notifications::default();
        if let Some(recovery) = state.settings_recovery {
            notifications.push(
                Notification::CompatibilityRecovered { recovery },
                Duration::ZERO,
                &settings.interface.notifications,
            );
        }
        Self {
            state_revision,
            state,
            controls,
            settings: settings.clone(),
            loaded_generation: Some(generation),
            preset_name: String::new(),
            selected_preset: None,
            preset_dialog: None,
            managed_preset: None,
            managed_preset_name: String::new(),
            theme_editor: None,
            theme_editor_original_name: None,
            error: None,
            published_settings: Some(settings),
            pending_search: None,
            binding_capture: None,
            export_request: None,
            pending_export_subject: None,
            notifications,
            started,
            settings_sidebar: false,
            viewed_search: None,
            history_return_draft: None,
            latest_search_id,
            lifecycle_sequence: 0,
            applied_theme: None,
            portal_glyphs: PortalGlyphs::new(ctx),
            overlay_visible: false,
            ui_last_command: 0,
            ui_error: None,
            ui_output: None,
            rolling_rates: RollingRates::default(),
            published_ui: None,
        }
    }

    fn palette(&self) -> Palette {
        if let Some(draft) = &self.theme_editor {
            return Palette::from_model(&draft.palette);
        }
        Palette::for_selection(
            &self.settings.interface.theme,
            &self.settings.interface.custom_themes,
        )
    }

    fn theme_control(&mut self, ui: &mut Ui) {
        let current = self.settings.interface.theme.clone();
        let mut selected = current.clone();
        let custom_names: Vec<_> = self
            .settings
            .interface
            .custom_themes
            .iter()
            .map(|custom| custom.name.clone())
            .collect();
        select_menu(
            ui,
            "theme",
            current.label(&self.settings.interface.custom_themes),
            ui.available_width().min(190.0),
            UiTheme::ALL.len() + custom_names.len(),
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                for &candidate in UiTheme::ALL {
                    ui.selectable_value(
                        &mut selected,
                        ThemeSelection::BuiltIn { theme: candidate },
                        candidate.label(),
                    );
                }
                if !custom_names.is_empty() {
                    ui.separator();
                    for name in &custom_names {
                        ui.selectable_value(
                            &mut selected,
                            ThemeSelection::Custom { name: name.clone() },
                            name,
                        );
                    }
                }
            },
        );
        if selected != current {
            self.settings.interface.theme = selected;
            self.applied_theme = None;
            self.send(Action::Configure {
                settings: self.settings.clone(),
            });
        }
    }

    fn theme_palette_preview(&self, ui: &mut Ui) {
        let palette = self.palette();
        egui::Grid::new("theme_palette_preview")
            .num_columns(2)
            .spacing(egui::vec2(14.0, 6.0))
            .show(ui, |ui| {
                for (index, (label, color)) in [
                    ("Background", palette.background),
                    ("Surface", palette.surface),
                    ("Text", palette.text),
                    ("Secondary text", palette.secondary_text),
                    ("Primary", palette.primary),
                    ("Selection", palette.selection),
                    ("Outline", palette.outline),
                    ("Link", palette.link),
                    ("Success", palette.success),
                    ("Warning", palette.warning),
                    ("Error", palette.error),
                ]
                .into_iter()
                .enumerate()
                {
                    palette_swatch(ui, label, color);
                    if index % 2 == 1 {
                        ui.end_row();
                    }
                }
                ui.end_row();
            });
    }

    fn open_new_theme_editor(&mut self) {
        self.theme_editor = Some(CustomTheme {
            name: String::new(),
            palette: self.palette().to_model(),
        });
        self.theme_editor_original_name = None;
        self.error = None;
    }

    fn open_selected_theme_editor(&mut self) {
        let ThemeSelection::Custom { name } = &self.settings.interface.theme else {
            return;
        };
        if let Some(custom) = self
            .settings
            .interface
            .custom_themes
            .iter()
            .find(|custom| custom.name == *name)
            .cloned()
        {
            self.theme_editor_original_name = Some(custom.name.clone());
            self.theme_editor = Some(custom);
            self.error = None;
        }
    }

    fn save_custom_theme(&mut self) -> Result<u64, String> {
        let mut draft = self
            .theme_editor
            .clone()
            .ok_or_else(|| "Custom theme editor is not open".to_owned())?;
        draft.name = draft.name.trim().to_owned();
        if draft.name.is_empty() {
            return Err("Enter a custom theme name".into());
        }
        if draft.name.chars().count() > 80 {
            return Err("Theme names can contain at most 80 characters".into());
        }
        if draft.name.chars().any(char::is_control) {
            return Err("Theme name contains unsupported characters".into());
        }
        validate_custom_theme_name(&draft.name).map_err(str::to_owned)?;
        if UiTheme::ALL
            .iter()
            .any(|theme| theme.label().eq_ignore_ascii_case(&draft.name))
        {
            return Err("Theme name already taken".into());
        }
        let original = self.theme_editor_original_name.as_deref();
        if self.settings.interface.custom_themes.iter().any(|custom| {
            custom.name.eq_ignore_ascii_case(&draft.name)
                && original.is_none_or(|original| custom.name != original)
        }) {
            return Err("Theme name already taken".into());
        }
        draft.validate_palette().map_err(str::to_owned)?;
        let mut next_settings = self.settings.clone();
        let index = original.and_then(|original| {
            next_settings
                .interface
                .custom_themes
                .iter()
                .position(|custom| custom.name == original)
        });
        if let Some(index) = index {
            next_settings.interface.custom_themes[index] = draft.clone();
        } else if next_settings.interface.custom_themes.len() < MAX_CUSTOM_THEMES {
            next_settings.interface.custom_themes.push(draft.clone());
        } else {
            return Err("Search Probes can save twenty custom themes. Remove one first.".into());
        }
        next_settings.interface.theme = ThemeSelection::Custom {
            name: draft.name.clone(),
        };
        next_settings.validate().map_err(str::to_owned)?;
        let command_id = self.try_send(Action::Configure {
            settings: next_settings.clone(),
        })?;
        self.settings = next_settings;
        self.applied_theme = None;
        self.theme_editor = None;
        self.theme_editor_original_name = None;
        Ok(command_id)
    }

    fn remove_custom_theme(&mut self, name: &str) -> Result<u64, String> {
        let index = self
            .settings
            .interface
            .custom_themes
            .iter()
            .position(|custom| custom.name == name)
            .ok_or_else(|| "Custom theme is unavailable".to_owned())?;
        self.settings.interface.custom_themes.remove(index);
        if matches!(
            &self.settings.interface.theme,
            ThemeSelection::Custom { name: selected } if selected == name
        ) {
            self.settings.interface.theme = ThemeSelection::default();
        }
        self.applied_theme = None;
        self.try_send(Action::Configure {
            settings: self.settings.clone(),
        })
    }

    fn theme_editor_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.theme_editor.clone() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let title = if self.theme_editor_original_name.is_some() {
            "Edit custom theme"
        } else {
            "Create custom theme"
        };
        let palette = self.palette();
        let (title_fill, title_text) = (palette.link, palette.foreground_on(palette.link));
        let title_style = ctx.style_of(if palette.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        egui::Window::new(RichText::new(title).color(title_text))
            .id(egui::Id::new("custom_theme_editor"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(340.0)
            .min_width(340.0)
            .max_width(340.0)
            .title_frame(egui::Frame::window(&title_style).fill(title_fill))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Theme name");
                ui.add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .hint_text("Custom theme name")
                        .desired_width(ui.available_width())
                        .char_limit(80),
                );
                ui.checkbox(&mut draft.palette.dark, "Use dark window styling");
                ui.separator();
                egui::Grid::new("custom_theme_colors")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        edit_theme_color(ui, "Background", &mut draft.palette.background);
                        edit_theme_color(ui, "Surface", &mut draft.palette.surface);
                        edit_theme_color(
                            ui,
                            "Surface container",
                            &mut draft.palette.surface_container,
                        );
                        edit_theme_color(ui, "Raised surface", &mut draft.palette.raised_surface);
                        edit_theme_color(ui, "Text", &mut draft.palette.text);
                        edit_theme_color(ui, "Secondary text", &mut draft.palette.secondary_text);
                        edit_theme_color(ui, "Primary", &mut draft.palette.primary);
                        edit_theme_color(
                            ui,
                            "Primary container",
                            &mut draft.palette.primary_container,
                        );
                        edit_theme_color(
                            ui,
                            "Primary container text",
                            &mut draft.palette.primary_container_text,
                        );
                        edit_theme_color(ui, "Selection", &mut draft.palette.selection);
                        edit_theme_color(ui, "Selection text", &mut draft.palette.selection_text);
                        edit_theme_color(ui, "Link", &mut draft.palette.link);
                        edit_theme_color(ui, "Outline", &mut draft.palette.outline);
                        edit_theme_color(ui, "Error", &mut draft.palette.error);
                        edit_theme_color(ui, "Success", &mut draft.palette.success);
                        edit_theme_color(
                            ui,
                            "Success container",
                            &mut draft.palette.success_container,
                        );
                        edit_theme_color(ui, "Success outline", &mut draft.palette.success_outline);
                        edit_theme_color(ui, "Warning", &mut draft.palette.warning);
                        edit_theme_color(
                            ui,
                            "Warning container",
                            &mut draft.palette.warning_container,
                        );
                        edit_theme_color(ui, "Warning outline", &mut draft.palette.warning_outline);
                    });
                if let Some(error) = self.error.as_deref() {
                    error_callout(ui, self.palette(), error);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let width =
                        ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(90.0);
                    save = ui
                        .add_sized([width, SEARCH_ACTION_HEIGHT], egui::Button::new("Save"))
                        .clicked();
                    cancel = ui
                        .add_sized([width, SEARCH_ACTION_HEIGHT], egui::Button::new("Cancel"))
                        .clicked();
                });
            });
        self.theme_editor = Some(draft);
        if save {
            if let Err(error) = self.save_custom_theme() {
                self.error = Some(error);
            }
        } else if cancel || !open {
            self.theme_editor = None;
            self.theme_editor_original_name = None;
            self.error = None;
        }
        // The editor draft is the active preview palette. Apply it after processing the
        // editor so color changes are visible immediately; Save only persists the draft,
        // while Cancel restores the selected saved theme.
        self.apply_theme(ctx);
    }

    fn result_card_settings(&mut self, ui: &mut Ui) {
        let previous = self.settings.interface.result_cards;
        let cards = &mut self.settings.interface.result_cards;

        ui.label(RichText::new("Result Details").strong());
        ui.checkbox(&mut cards.location_subtitle, "Location subtitle");
        ui.checkbox(&mut cards.portal_glyphs, "Portal glyphs");
        ui.add_space(8.0);
        ui.label(RichText::new("Action buttons").strong());

        let enabled = !cards.set_destination || cards.action_count() > 1;
        ui.add_enabled(
            enabled,
            egui::Checkbox::new(&mut cards.set_destination, "Set destination"),
        )
        .on_disabled_hover_text("At least one result action must remain visible");

        let enabled = !cards.copy_portal_code || cards.action_count() > 1;
        ui.add_enabled(
            enabled,
            egui::Checkbox::new(&mut cards.copy_portal_code, "Copy portal code"),
        )
        .on_disabled_hover_text("At least one result action must remain visible");

        let enabled = !cards.copy_galactic_coordinates || cards.action_count() > 1;
        ui.add_enabled(
            enabled,
            egui::Checkbox::new(&mut cards.copy_galactic_coordinates, "Copy galactic coords"),
        )
        .on_disabled_hover_text("At least one result action must remain visible");

        if *cards != previous {
            self.send(Action::Configure {
                settings: self.settings.clone(),
            });
        }
    }

    fn settings_contents(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Settings");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = egui::Image::from_bytes(
                    "bytes://search-probes/icons/close.svg",
                    include_bytes!("../assets/icons/close.svg"),
                )
                .fit_to_exact_size(egui::vec2(24.0, 24.0));
                let response = ui
                    .add(
                        egui::Button::image(close)
                            .image_tint_follows_text_color(true)
                            .frame(false)
                            .min_size(egui::vec2(42.0, 36.0)),
                    )
                    .on_hover_text("Close settings");
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close settings")
                });
                if response.clicked() {
                    self.settings_sidebar = false;
                }
            });
        });
        sidebar_separator(ui);
        egui::ScrollArea::vertical()
            .id_salt("settings_sidebar_scroll")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                // ScrollArea animates its reserved scrollbar width. Widgets laid out on the first
                // open frame can otherwise retain the pre-animation width while being clipped to
                // the narrower post-animation rect. A fixed inner gutter keeps centered widget
                // strokes inside every intermediate clip width as the scrollbar appears.
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        right: 8,
                        ..Default::default()
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading("Theme");
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    self.theme_control(ui);
                                },
                            );
                        });
                        self.theme_palette_preview(ui);
                        ui.add_space(4.0);
                        let selected_custom = match &self.settings.interface.theme {
                            ThemeSelection::Custom { name } => Some(name.clone()),
                            ThemeSelection::BuiltIn { .. } => None,
                        };
                        if ui
                            .add_sized(
                                [ui.available_width(), 32.0],
                                egui::Button::new("Create custom theme"),
                            )
                            .clicked()
                        {
                            self.open_new_theme_editor();
                        }
                        ui.horizontal(|ui| {
                            let width = ((ui.available_width() - ui.spacing().item_spacing.x)
                                / 2.0)
                                .max(70.0);
                            if ui
                                .add_enabled(
                                    selected_custom.is_some(),
                                    egui::Button::new("Edit").min_size(egui::vec2(width, 32.0)),
                                )
                                .clicked()
                            {
                                self.open_selected_theme_editor();
                            }
                            if ui
                                .add_enabled(
                                    selected_custom.is_some(),
                                    egui::Button::new("Remove").min_size(egui::vec2(width, 32.0)),
                                )
                                .clicked()
                                && let Some(name) = selected_custom
                                && let Err(error) = self.remove_custom_theme(&name)
                            {
                                self.error = Some(error);
                            }
                        });
                        ui.add_space(12.0);
                        sidebar_separator(ui);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.heading("Preset Manager");
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    self.export_presets_button(ui);
                                },
                            );
                        });
                        self.preset_manager(ui);
                        ui.add_space(12.0);
                        sidebar_separator(ui);
                        ui.add_space(8.0);
                        ui.heading("UI Hotkey");
                        self.hotkey_settings(ui);
                        ui.add_space(12.0);
                        sidebar_separator(ui);
                        ui.add_space(8.0);
                        ui.heading("Notifications");
                        let previous = self.settings.interface.notifications.clone();
                        {
                            let notifications = &mut self.settings.interface.notifications;
                            ui.checkbox(&mut notifications.result_found, "Result found");
                            ui.checkbox(&mut notifications.search_completed, "Search completed");
                            ui.checkbox(&mut notifications.automatic_pause, "Automatic pause");
                            ui.checkbox(&mut notifications.automatic_resume, "Automatic resume");
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label("Duration:");
                                ui.add_sized(
                                    [42.0, ui.spacing().interact_size.y],
                                    egui::DragValue::new(&mut notifications.duration_seconds)
                                        .range(
                                            NotificationSettings::MIN_DURATION_SECONDS
                                                ..=NotificationSettings::MAX_DURATION_SECONDS,
                                        ),
                                );
                                ui.label("seconds");
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label("Screen placement:");
                                select_menu(
                                    ui,
                                    "notification_placement",
                                    notifications.placement.label(),
                                    ui.available_width(),
                                    ToastPlacement::ALL.len(),
                                    egui::PopupCloseBehavior::CloseOnClick,
                                    |ui| {
                                        for &placement in ToastPlacement::ALL {
                                            ui.selectable_value(
                                                &mut notifications.placement,
                                                placement,
                                                placement.label(),
                                            );
                                        }
                                    },
                                );
                            });
                        }
                        if self.settings.interface.notifications != previous {
                            self.notifications.update(
                                self.started.elapsed(),
                                &self.settings.interface.notifications,
                            );
                            self.send(Action::Configure {
                                settings: self.settings.clone(),
                            });
                        }
                        ui.add_space(12.0);
                        sidebar_separator(ui);
                        ui.add_space(8.0);
                        ui.heading("Result cards");
                        self.result_card_settings(ui);
                    });
            });
    }
    fn send(&mut self, action: Action) {
        let _ = self.try_send(action);
    }
    fn try_send(&mut self, action: Action) -> Result<u64, String> {
        let pending = matches!(&action, Action::Start { .. });
        match self.controls.submit(self.state.generation, action) {
            Ok(id) => {
                self.error = None;
                if pending {
                    self.pending_search = Some(id);
                }
                Ok(id)
            }
            Err(error) => {
                self.error = Some(error.into());
                Err(error.into())
            }
        }
    }

    fn start_displayed_search(&mut self) -> Result<u64, String> {
        let viewing_past = self.viewing_past_search();
        let active = self.pending_search.is_some()
            || self
                .state
                .progress
                .as_ref()
                .is_some_and(|progress| progress.phase == Phase::Running);
        if viewing_past && active {
            self.try_send(Action::Cancel {})?;
        }
        let command_id = self.try_send(Action::Start {
            settings: self.settings.clone(),
        })?;
        self.viewed_search = None;
        self.history_return_draft = None;
        Ok(command_id)
    }
    fn preset_selector(&mut self, ui: &mut Ui, width: f32) {
        let names: Vec<_> = self
            .settings
            .presets
            .iter()
            .map(|preset| preset.name.clone())
            .collect();
        let selected_text = self
            .selected_preset
            .and_then(|index| names.get(index))
            .cloned()
            .unwrap_or_else(|| "No preset selected".to_owned());
        let mut choice = None;
        select_menu(
            ui,
            "preset_selector",
            selected_text,
            width,
            names.len() + 1,
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                if ui
                    .selectable_label(self.selected_preset.is_none(), "No preset selected")
                    .clicked()
                {
                    choice = Some(None);
                }
                for (index, name) in names.iter().enumerate() {
                    if ui
                        .selectable_label(self.selected_preset == Some(index), name)
                        .clicked()
                    {
                        choice = Some(Some(index));
                    }
                }
            },
        );
        if let Some(choice) = choice {
            self.selected_preset = choice;
            if let Some(index) = choice {
                let preset = self.settings.presets[index].clone();
                self.preset_name = preset.name.clone();
                self.managed_preset = Some(index);
                self.managed_preset_name = preset.name;
                self.settings.search = preset.search;
            } else {
                self.preset_name.clear();
            }
        }
    }

    fn open_preset_dialog(&mut self) {
        self.preset_name = self
            .selected_preset
            .and_then(|index| self.settings.presets.get(index))
            .map_or_else(String::new, |preset| preset.name.clone());
        self.error = None;
        self.preset_dialog = Some(PresetNameDialog::SaveCurrentSearch);
    }

    fn open_rename_preset_dialog(&mut self, index: usize) -> Result<(), String> {
        let preset = self
            .settings
            .presets
            .get(index)
            .ok_or_else(|| "Preset index is unavailable".to_owned())?;
        self.preset_name = preset.name.clone();
        self.error = None;
        self.preset_dialog = Some(PresetNameDialog::Rename { index });
        Ok(())
    }

    fn preset_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.preset_dialog else {
            return;
        };
        let (title, prompt, confirm_label) = match dialog {
            PresetNameDialog::SaveCurrentSearch => {
                ("Save as preset", Some("Name this search preset"), "Save")
            }
            PresetNameDialog::Rename { .. } => ("Rename preset", None, "Rename"),
        };
        let original_name = match dialog {
            PresetNameDialog::Rename { index } => self
                .settings
                .presets
                .get(index)
                .map(|preset| preset.name.clone()),
            PresetNameDialog::SaveCurrentSearch => None,
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let palette = self.palette();
        let (title_fill, title_text) = (palette.link, palette.foreground_on(palette.link));
        let title_style = ctx.style_of(if palette.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        egui::Window::new(RichText::new(title).color(title_text))
            .id(egui::Id::new("save_as_preset_dialog"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .title_frame(egui::Frame::window(&title_style).fill(title_fill))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                if let Some(prompt) = prompt {
                    ui.vertical_centered(|ui| {
                        ui.label(prompt);
                    });
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.preset_name)
                        .hint_text("Preset name")
                        .desired_width(ui.available_width())
                        .horizontal_align(egui::Align::Center)
                        .char_limit(80),
                );
                if let Some(original_name) = original_name.as_deref() {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Rename “{original_name}” to “{}”",
                                self.preset_name.trim()
                            ))
                            .color(self.palette().secondary_text),
                        );
                    });
                }
                let matching = self
                    .settings
                    .presets
                    .iter()
                    .enumerate()
                    .find(|(_, preset)| preset.name.eq_ignore_ascii_case(self.preset_name.trim()))
                    .map(|(index, preset)| (index, preset.name.clone()));
                let warning = match (dialog, matching) {
                    (PresetNameDialog::SaveCurrentSearch, Some((_, name))) => {
                        Some(("Existing preset", format!("Saving will replace “{name}”.")))
                    }
                    (PresetNameDialog::Rename { index }, Some((matching, name)))
                        if matching != index =>
                    {
                        Some((
                            "Preset name in use",
                            format!("“{name}” is already used by another preset."),
                        ))
                    }
                    _ => None,
                };
                if let Some((warning_title, warning_message)) = warning {
                    callout(
                        ui,
                        self.palette(),
                        CalloutTone::Warning,
                        warning_title,
                        &warning_message,
                    );
                }
                if let Some(error) = self.error.as_deref() {
                    error_callout(ui, self.palette(), error);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let width =
                        ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(90.0);
                    confirm = ui
                        .add_sized(
                            [width, SEARCH_ACTION_HEIGHT],
                            egui::Button::new(confirm_label),
                        )
                        .clicked();
                    cancel = ui
                        .add_sized([width, SEARCH_ACTION_HEIGHT], egui::Button::new("Cancel"))
                        .clicked();
                });
            });
        if confirm {
            let result = match dialog {
                PresetNameDialog::SaveCurrentSearch => self.save_preset(),
                PresetNameDialog::Rename { index } => {
                    self.rename_preset(index, self.preset_name.clone())
                }
            };
            match result {
                Ok(_) => self.preset_dialog = None,
                Err(error) => self.error = Some(error),
            }
        } else if cancel || !open {
            self.preset_dialog = None;
            self.error = None;
        }
    }

    fn preset_manager(&mut self, ui: &mut Ui) {
        let names: Vec<_> = self
            .settings
            .presets
            .iter()
            .map(|preset| preset.name.clone())
            .collect();
        if self
            .managed_preset
            .is_some_and(|index| index >= names.len())
        {
            self.managed_preset = None;
            self.managed_preset_name.clear();
        }
        if names.is_empty() {
            ui.label("No saved presets");
        } else {
            let selected_text = self
                .managed_preset
                .and_then(|index| names.get(index))
                .cloned()
                .unwrap_or_else(|| "Choose a preset".to_owned());
            let mut choice = None;
            select_menu(
                ui,
                "managed_preset",
                selected_text,
                ui.available_width(),
                names.len(),
                egui::PopupCloseBehavior::CloseOnClick,
                |ui| {
                    for (index, name) in names.iter().enumerate() {
                        if ui
                            .selectable_label(self.managed_preset == Some(index), name)
                            .clicked()
                        {
                            choice = Some(index);
                        }
                    }
                },
            );
            if let Some(index) = choice {
                self.managed_preset = Some(index);
                self.managed_preset_name = names[index].clone();
                self.selected_preset = Some(index);
                self.preset_name = names[index].clone();
                self.settings.search = self.settings.presets[index].search.clone();
                self.viewed_search = None;
                self.history_return_draft = None;
            }
            ui.horizontal(|ui| {
                let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(80.0);
                let rename = ui
                    .add_enabled(
                        self.managed_preset.is_some(),
                        egui::Button::new("Rename").min_size(egui::vec2(width, 32.0)),
                    )
                    .clicked();
                let remove = ui
                    .add_enabled(
                        self.managed_preset.is_some(),
                        egui::Button::new("Remove").min_size(egui::vec2(width, 32.0)),
                    )
                    .clicked();
                if rename
                    && let Some(index) = self.managed_preset
                    && let Err(error) = self.open_rename_preset_dialog(index)
                {
                    self.error = Some(error);
                }
                if remove && let Some(index) = self.managed_preset {
                    match self.remove_preset(index) {
                        Ok(_) => {
                            self.managed_preset = None;
                            self.managed_preset_name.clear();
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            });
        }
        if ui
            .add_sized(
                [ui.available_width(), SEARCH_ACTION_HEIGHT],
                egui::Button::new("Save current search as preset"),
            )
            .on_hover_text("Create a preset from the current search settings")
            .clicked()
        {
            self.open_preset_dialog();
        }
    }

    fn export_presets_button(&mut self, ui: &mut Ui) {
        if ui
            .add_enabled(
                !self.settings.presets.is_empty(),
                export_button("Export presets").min_size(egui::vec2(130.0, 32.0)),
            )
            .on_hover_text("Export all saved presets as JSON")
            .clicked()
        {
            match ExportRequest::presets_json(&self.settings) {
                Ok(request) => {
                    self.export_request = Some(request);
                    self.notifications.clear_export();
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
    fn search_controls(&mut self, ui: &mut Ui) {
        if ui.available_width() >= SEARCH_PANEL_SPLIT_MIN_WIDTH {
            ui.columns(2, |columns| {
                self.search_scope(&mut columns[0]);
                self.search_performance(&mut columns[1]);
            });
        } else {
            self.search_scope(ui);
            ui.separator();
            self.search_performance(ui);
        }
    }

    fn runtime_action_buttons(&mut self, ui: &mut Ui, state: RuntimeActionState) {
        let RuntimeActionState {
            viewing_past,
            ready,
            running,
            active,
            user_paused,
            automatically_paused,
            search_valid,
        } = state;
        if ui
            .add_enabled(
                ready && (!active || viewing_past) && search_valid,
                search_action_button(
                    egui::Button::new(
                        RichText::new(if viewing_past {
                            "Search again"
                        } else {
                            "Search"
                        })
                        .strong(),
                    ),
                    PRIMARY_SEARCH_ACTION_WIDTH,
                ),
            )
            .clicked()
        {
            let _ = self.start_displayed_search();
        }
        if ui
            .add_enabled(
                !viewing_past && ready && running && !automatically_paused,
                search_action_button(
                    egui::Button::new(if user_paused { "Resume" } else { "Pause" }),
                    SECONDARY_SEARCH_ACTION_WIDTH,
                ),
            )
            .on_hover_text(if user_paused {
                "Resume the active search"
            } else {
                "Pause the active search"
            })
            .clicked()
        {
            self.send(Action::SetSearchPaused {
                paused: !user_paused,
            });
        }
        if ui
            .add_enabled(
                !viewing_past && active,
                search_action_button(egui::Button::new("Cancel"), SECONDARY_SEARCH_ACTION_WIDTH),
            )
            .on_hover_text("Cancel the active search")
            .clicked()
        {
            self.send(Action::Cancel {});
        }
        if ui
            .add_enabled(
                !viewing_past && self.settings.search.criteria != Criteria::default(),
                search_action_button(
                    egui::Button::new("Clear criteria"),
                    SECONDARY_SEARCH_ACTION_WIDTH,
                ),
            )
            .on_hover_text("Remove all selected search criteria")
            .clicked()
        {
            self.settings.search.criteria = Criteria::default();
            self.selected_preset = None;
            self.preset_name.clear();
        }
    }

    fn storage_action_buttons(
        &mut self,
        ui: &mut Ui,
        current: Option<&search_probes_model::session::SearchRecord>,
    ) {
        if ui
            .add_enabled(
                current.is_some(),
                search_action_button(export_button("Export search"), PRIMARY_SEARCH_ACTION_WIDTH),
            )
            .on_hover_text("Export the current search settings and results as JSON")
            .clicked()
            && let Some(record) = current
        {
            match ExportRequest::search_json(record) {
                Ok(request) => self.export_request = Some(request),
                Err(error) => self.error = Some(error),
            }
            self.notifications.clear_export();
        }
        if ui
            .add_enabled(
                self.settings.search.validate().is_ok(),
                search_action_button(
                    egui::Button::new("Save as preset"),
                    PRIMARY_SEARCH_ACTION_WIDTH,
                ),
            )
            .on_hover_text("Save the current search settings as a preset")
            .clicked()
        {
            self.open_preset_dialog();
        }
    }

    fn search_actions(&mut self, ui: &mut Ui) {
        let state = &self.state;
        let viewing_past = self.viewing_past_search();
        let ready = state.availability == Availability::Ready;
        let running = state
            .progress
            .as_ref()
            .is_some_and(|progress| progress.phase == Phase::Running);
        let active = self.pending_search.is_some() || running;
        let user_paused = state.search_pause == Some(SearchPause::User);
        let automatically_paused = matches!(
            state.search_pause,
            Some(SearchPause::Loading | SearchPause::OtherGalaxy)
        );
        let valid = self
            .settings
            .validate()
            .and_then(|_| self.settings.search.validate_search());
        let action_state = RuntimeActionState {
            viewing_past,
            ready,
            running,
            active,
            user_paused,
            automatically_paused,
            search_valid: valid.is_ok(),
        };
        let current = self.displayed_record();
        let single_row_width = PRIMARY_SEARCH_ACTION_WIDTH * 3.0
            + SECONDARY_SEARCH_ACTION_WIDTH * 3.0
            + ui.spacing().item_spacing.x * 5.0;
        if ui.available_width() >= single_row_width {
            ui.horizontal(|ui| {
                self.runtime_action_buttons(ui, action_state);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.storage_action_buttons(ui, current.as_deref());
                });
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                self.runtime_action_buttons(ui, action_state);
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.storage_action_buttons(ui, current.as_deref());
                });
            });
        }
        animated_visibility(
            ui,
            "pending_search_status",
            self.pending_search.is_some(),
            |ui| {
                ui.label("Starting search…");
            },
        );
        let validation_error = valid.err();
        animated_visibility(
            ui,
            "search_validation_error",
            validation_error.is_some(),
            |ui| {
                if let Some(error) = validation_error {
                    error_callout(ui, self.palette(), error);
                }
            },
        );
    }

    fn search_time_limit(&mut self, ui: &mut Ui, width: f32) {
        let mut disabled = self.settings.search.time_limit.is_none();
        let mut limit = self.settings.search.time_limit.unwrap_or(TimeLimit {
            hours: 1,
            minutes: 0,
        });
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(width);
                ui.checkbox(&mut disabled, "Disable");
                let field_width =
                    ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(36.0);
                ui.add_enabled_ui(!disabled, |ui| {
                    ui.add_sized(
                        [field_width, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut limit.hours)
                            .range(0..=99)
                            .suffix(" h"),
                    );
                    ui.add_sized(
                        [field_width, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut limit.minutes)
                            .range(0..=99)
                            .suffix(" min"),
                    );
                });
            },
        );
        if disabled {
            self.settings.search.time_limit = None;
        } else {
            self.settings.search.time_limit = Some(limit);
        }
    }

    fn apply_binding(&mut self, binding: InputBinding) {
        self.settings.interface.hotkey = binding;
        self.binding_capture = None;
        self.send(Action::Configure {
            settings: self.settings.clone(),
        });
    }

    fn capture_keyboard(&mut self, ctx: &egui::Context) {
        if self
            .binding_capture
            .as_ref()
            .is_none_or(|capture| capture.device != BindingDevice::Keyboard)
        {
            return;
        }
        let events = ctx.input(|input| input.raw.events.clone());
        let mut completed = None;
        for event in events {
            let egui::Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                ..
            } = event
            else {
                continue;
            };
            if repeat {
                continue;
            }
            let Some(key) = keyboard_key(physical_key.unwrap_or(key)) else {
                continue;
            };
            if let Some(binding) = self
                .binding_capture
                .as_mut()
                .and_then(|capture| capture.input(CapturedInput::Keyboard(key), pressed))
            {
                completed = Some(binding);
            }
        }
        if let Some(binding) = completed {
            self.apply_binding(binding);
        }
    }

    /// Supplies controller button transitions to an active binding capture.
    /// Identity keeps a captured chord on one device, but is not stored in the binding.
    pub fn controller_input(&mut self, controller: u8, button: ControllerButton, pressed: bool) {
        let completed = self.binding_capture.as_mut().and_then(|capture| {
            if capture.device != BindingDevice::Controller {
                return None;
            }
            if capture.controller.is_none() && pressed {
                capture.controller = Some(controller);
            }
            if capture.controller != Some(controller) {
                return None;
            }
            capture.input(CapturedInput::Controller(button), pressed)
        });
        if let Some(binding) = completed {
            self.apply_binding(binding);
        }
    }

    /// The device whose runtime input is currently routed into the hotkey recorder.
    pub fn binding_capture_device(&self) -> Option<BindingDevice> {
        self.binding_capture.as_ref().map(|capture| capture.device)
    }

    fn hotkey_settings(&mut self, ui: &mut Ui) {
        let binding = self.settings.interface.hotkey;
        let palette = self.palette();
        let mut start_capture = false;
        let mut cancel_capture = false;
        let mut reset = false;
        ui.label("Use up to two keyboard keys or controller inputs");
        ui.add_space(6.0);
        if let Some(capture) = self.binding_capture.as_mut() {
            let mut selected_device = capture.device;
            ui.horizontal(|ui| {
                let button_width =
                    ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(70.0);
                if ui
                    .add_sized(
                        [button_width, 30.0],
                        egui::Button::selectable(
                            selected_device == BindingDevice::Keyboard,
                            "Keyboard",
                        ),
                    )
                    .clicked()
                {
                    selected_device = BindingDevice::Keyboard;
                }
                if ui
                    .add_sized(
                        [button_width, 30.0],
                        egui::Button::selectable(
                            selected_device == BindingDevice::Controller,
                            "Controller",
                        ),
                    )
                    .clicked()
                {
                    selected_device = BindingDevice::Controller;
                }
            });
            capture.set_device(selected_device);
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(palette.surface)
                .stroke(egui::Stroke::new(1.0, palette.primary))
                .corner_radius(5.0)
                .inner_margin(egui::Margin::symmetric(10, 9))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(match capture.device {
                                BindingDevice::Keyboard => "Press one or two keys together",
                                BindingDevice::Controller => {
                                    "Press one or two controller inputs together"
                                }
                            })
                            .strong(),
                        );
                        ui.add_space(6.0);
                        if capture.candidate.is_empty() {
                            ui.label(
                                RichText::new(match capture.device {
                                    BindingDevice::Keyboard => "Waiting for keyboard input…",
                                    BindingDevice::Controller => "Waiting for any controller…",
                                })
                                .color(palette.secondary_text),
                            );
                        } else {
                            captured_caps(ui, &capture.candidate, palette);
                        }
                        if capture.overflowed {
                            error_callout(
                                ui,
                                palette,
                                "Release the inputs and try again with no more than two",
                            );
                        } else {
                            ui.label("Release all inputs to save.");
                        }
                    });
                });
            if ui
                .add_sized([ui.available_width(), 34.0], egui::Button::new("Cancel"))
                .clicked()
            {
                cancel_capture = true;
            }
        } else {
            binding_caps(ui, binding, palette);
            ui.horizontal(|ui| {
                let button_width =
                    ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(70.0);
                if ui
                    .add_sized([button_width, 34.0], egui::Button::new("Change"))
                    .clicked()
                {
                    start_capture = true;
                }
                if ui
                    .add_enabled(
                        binding != InputBinding::default(),
                        egui::Button::new("Reset to default")
                            .min_size(egui::vec2(button_width, 34.0)),
                    )
                    .clicked()
                {
                    reset = true;
                }
            });
        }
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Recovery: Ctrl + Shift + Backspace").color(palette.secondary_text),
            );
        });
        if cancel_capture {
            self.binding_capture = None;
        } else if start_capture {
            self.binding_capture = Some(BindingCapture::new(binding.device()));
        } else if reset {
            self.apply_binding(InputBinding::default());
        }
    }
    fn search_scope(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Search Scope").size(19.0).strong());
        let selected = BUDGETS
            .iter()
            .position(|&budget| budget == self.settings.search.systems)
            .unwrap_or(1);
        egui::Grid::new("search_scope")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Preset");
                self.preset_selector(ui, 180.0);
                ui.end_row();
                ui.label("Systems to search");
                select_menu(
                    ui,
                    "systems",
                    BUDGET_LABELS[selected],
                    180.0,
                    BUDGETS.len(),
                    egui::PopupCloseBehavior::CloseOnClick,
                    |ui| {
                        for (index, &budget) in BUDGETS.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.settings.search.systems,
                                budget,
                                BUDGET_LABELS[index],
                            );
                        }
                    },
                );
                ui.end_row();
                ui.label("Time limit");
                self.search_time_limit(ui, 180.0);
                ui.end_row();
                ui.label("Results");
                ui.add_sized(
                    [180.0, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut self.settings.search.results).range(1..=MAX_RESULTS),
                );
                ui.end_row();
            });
        callout(
            ui,
            self.palette(),
            CalloutTone::Success,
            "The search will stop after whichever comes first",
            "- Finding the number of requested results\n- Processing the set number of systems\n- Hitting the time limit",
        );
    }
    fn search_performance(&mut self, ui: &mut Ui) {
        let previous_intensity = self.settings.frame_budget_ms;
        let previous_keep_game_running = self.settings.keep_game_running;
        ui.heading(RichText::new("Performance").size(19.0).strong());
        let input_width = filter_control_width(ui);
        let intensity_levels = [
            (2, "Lowest game impact"),
            (4, "Low game impact"),
            (8, "Balanced"),
            (16, "Fastest search"),
        ];
        let available_workers = available_workers();
        let recommended_workers = recommended_workers(available_workers);
        egui::Grid::new("performance_settings")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Search intensity");
                select_menu(
                    ui,
                    "effort",
                    intensity_levels
                        .iter()
                        .find_map(|&(value, label)| {
                            (value == self.settings.frame_budget_ms).then_some(label)
                        })
                        .unwrap_or("Unknown"),
                    input_width,
                    intensity_levels.len(),
                    egui::PopupCloseBehavior::CloseOnClick,
                    |ui| {
                        for (value, label) in intensity_levels {
                            ui.selectable_value(&mut self.settings.frame_budget_ms, value, label);
                        }
                    },
                );
                ui.end_row();
                ui.label("Workers");
                select_menu(
                    ui,
                    "workers",
                    match self.settings.workers {
                        None => format!("Auto ({recommended_workers})"),
                        Some(count) if count <= available_workers => count.to_string(),
                        Some(count) => format!("{count} (limited to {available_workers})"),
                    },
                    input_width,
                    available_workers + 1,
                    egui::PopupCloseBehavior::CloseOnClick,
                    |ui| {
                        ui.selectable_value(
                            &mut self.settings.workers,
                            None,
                            format!("Auto (Recommended · {recommended_workers})"),
                        );
                        for count in 1..=available_workers {
                            ui.selectable_value(
                                &mut self.settings.workers,
                                Some(count),
                                count.to_string(),
                            );
                        }
                    },
                );
                ui.end_row();
                ui.label("Keep game running");
                ui.checkbox(&mut self.settings.keep_game_running, "Enabled");
                ui.end_row();
            });
        if self.settings.frame_budget_ms != previous_intensity {
            self.send(Action::ConfigureIntensity {
                frame_budget_ms: self.settings.frame_budget_ms,
            });
        }
        if self.settings.keep_game_running != previous_keep_game_running {
            self.send(Action::Configure {
                settings: self.settings.clone(),
            });
        }
        callout(
            ui,
            self.palette(),
            CalloutTone::Warning,
            "Performance impact",
            "- Higher search intensity and more workers can find results faster, but may make the game less smooth\n- Keeping the game running prevents NMS from pausing in menus or while unfocused, which uses more system resources",
        );
    }
    fn criteria(&mut self, ui: &mut Ui, input_width: f32) {
        self.settings.search.criteria.enforce_dependencies();
        if ui.available_width() >= CRITERIA_THREE_COLUMN_MIN_WIDTH {
            ui.columns(3, |columns| {
                self.system_criteria(&mut columns[0], input_width);
                self.planet_criteria(&mut columns[1], input_width);
                self.fauna_criteria(&mut columns[2], input_width);
            });
        } else if ui.available_width() >= CRITERIA_SPLIT_MIN_WIDTH {
            ui.columns(2, |columns| {
                self.system_criteria(&mut columns[0], input_width);
                columns[0].add_space(8.0);
                self.fauna_criteria(&mut columns[0], input_width);
                self.planet_criteria(&mut columns[1], input_width);
            });
        } else {
            self.system_criteria(ui, input_width);
            ui.add_space(8.0);
            self.planet_criteria(ui, input_width);
            ui.add_space(8.0);
            self.fauna_criteria(ui, input_width);
        }
        self.settings.search.criteria.enforce_dependencies();
    }
    fn criteria_panel(&mut self, ui: &mut Ui) {
        let viewing_past = self.viewing_past_search();
        section_frame(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.search_actions(ui);
            ui.separator();
            let input_width = filter_control_width(ui);
            ui.add_enabled_ui(!viewing_past, |ui| self.criteria(ui, input_width));
        });
    }
    fn system_criteria(&mut self, ui: &mut Ui, input_width: f32) {
        egui::CollapsingHeader::new(RichText::new("System Criteria").size(19.0).strong())
            .default_open(true)
            .show(ui, |ui| {
                let criteria = &mut self.settings.search.criteria;
                egui::CollapsingHeader::new(RichText::new("System Features").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(ui, criteria, &[Field::Anomaly, Field::Star], input_width);
                        count_choices_row(
                            ui,
                            "Number of stars",
                            "star_counts",
                            &mut criteria.star_counts,
                            3,
                            input_width,
                        );
                        system_flag_rows(ui, criteria, &[SystemFlag::Dissonance], input_width);
                        water_row(
                            ui,
                            "system_water",
                            &mut criteria.system_water,
                            input_width,
                            |_| true,
                        );
                    });

                egui::CollapsingHeader::new(
                    RichText::new("System Demographics").size(16.0).strong(),
                )
                .default_open(true)
                .show(ui, |ui| {
                    choice_rows(ui, criteria, &[Field::Population], input_width);
                    criteria.enforce_population_dependencies();
                    ui.add_enabled_ui(criteria.race_available_mask() != 0, |ui| {
                        choice_rows(ui, criteria, &[Field::Race], input_width);
                    });
                    ui.add_enabled_ui(criteria.outlaw_filter_available(), |ui| {
                        system_flag_rows(ui, criteria, &[SystemFlag::Pirate], input_width);
                    });
                    criteria.enforce_population_dependencies();
                    choice_rows(
                        ui,
                        criteria,
                        &[Field::Wealth, Field::Economy, Field::ConflictLevel],
                        input_width,
                    );
                });

                egui::CollapsingHeader::new(
                    RichText::new("Planetary Composition").size(16.0).strong(),
                )
                .default_open(true)
                .show(ui, |ui| {
                    count_choices_row(
                        ui,
                        "Number of planets",
                        "planet_counts",
                        &mut criteria.planet_counts,
                        6,
                        input_width,
                    );
                    system_flag_rows(ui, criteria, &[SystemFlag::ExtremeStorms], input_width);
                    for (field, side, label) in [
                        (
                            CompositionField::Biomes,
                            SelectionSide::Exclude,
                            "Exclude system by biome",
                        ),
                        (
                            CompositionField::Subtypes,
                            SelectionSide::Exclude,
                            "Exclude system by biome subtype",
                        ),
                        (
                            CompositionField::Sizes,
                            SelectionSide::Exclude,
                            "Exclude system by planet size",
                        ),
                        (
                            CompositionField::Biomes,
                            SelectionSide::Include,
                            "Include system by biome",
                        ),
                        (
                            CompositionField::Subtypes,
                            SelectionSide::Include,
                            "Include system by biome subtype",
                        ),
                        (
                            CompositionField::Sizes,
                            SelectionSide::Include,
                            "Include system by planet size",
                        ),
                    ] {
                        composition_row(ui, criteria, field, side, label, input_width);
                    }
                });
            });
    }
    fn fauna_criteria(&mut self, ui: &mut Ui, input_width: f32) {
        let criteria = &mut self.settings.search.criteria;
        egui::CollapsingHeader::new(RichText::new("Fauna Criteria").size(19.0).strong())
            .default_open(true)
            .show(ui, |ui| {
                egui::CollapsingHeader::new(RichText::new("General").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(ui, criteria, &[Field::Fauna], input_width);
                        ui.add_enabled_ui(criteria.fauna_details_available(), |ui| {
                            tameable_fauna_row(ui, criteria, input_width);
                        });
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Sandworms], input_width);
                        planet_flag_rows(
                            ui,
                            criteria,
                            &[
                                PlanetFlag::TitanWorms,
                                PlanetFlag::VileBrood,
                                PlanetFlag::PlayerAttackingFauna,
                            ],
                            input_width,
                        );
                    });
                fauna_domain_section(ui, criteria, Domain::Airborne, input_width);
                fauna_domain_section(ui, criteria, Domain::Aquatic, input_width);
                fauna_domain_section(ui, criteria, Domain::Land, input_width);
                egui::CollapsingHeader::new(RichText::new("Xeno Arena").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.add_enabled_ui(criteria.arena_available(), |ui| {
                            arena_criteria(ui, criteria, input_width)
                        });
                    });
            });
    }
    fn planet_criteria(&mut self, ui: &mut Ui, input_width: f32) {
        let criteria = &mut self.settings.search.criteria;
        egui::CollapsingHeader::new(RichText::new("Planet Criteria").size(19.0).strong())
            .default_open(true)
            .show(ui, |ui| {
                egui::CollapsingHeader::new(RichText::new("Flora").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(ui, criteria, &[Field::Flora], input_width);
                        ui.add_enabled_ui(criteria.flora_details_available(), |ui| {
                            planet_flag_rows(
                                ui,
                                criteria,
                                &[PlanetFlag::GlowingGrass, PlanetFlag::GlowingFlora],
                                input_width,
                            );
                            choice_rows(
                                ui,
                                criteria,
                                &[Field::Grass, Field::FloraColour],
                                input_width,
                            );
                        });
                    });

                egui::CollapsingHeader::new(RichText::new("Planet Features").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        planet_flag_rows(
                            ui,
                            criteria,
                            &[PlanetFlag::Moons, PlanetFlag::Rings],
                            input_width,
                        );
                        let system_water = criteria.system_water;
                        let water_colour = criteria.choices.contains_key(&Field::WaterColour);
                        let aquatic = criteria.fauna.get(&Domain::Aquatic).is_some_and(|fauna| {
                            fauna.presence == Some(true) || fauna.has_detail_filters()
                        });
                        water_row(
                            ui,
                            "planet_water",
                            &mut criteria.planet_water,
                            input_width,
                            |value| match value {
                                WaterFilter::RequireAny => system_water != WaterFilter::ExcludeAll,
                                WaterFilter::RequireDeep => !matches!(
                                    system_water,
                                    WaterFilter::ExcludeDeep | WaterFilter::ExcludeAll
                                ),
                                WaterFilter::ExcludeAll => !water_colour && !aquatic,
                                _ => true,
                            },
                        );
                        choice_rows(ui, criteria, &[Field::WaterColour], input_width);
                        sentinel_row(ui, criteria, input_width);
                    });

                egui::CollapsingHeader::new(RichText::new("Planet Type").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(
                            ui,
                            criteria,
                            &[Field::Biome, Field::Subtype, Field::Size, Field::Terrain],
                            input_width,
                        );
                    });

                egui::CollapsingHeader::new(RichText::new("Resources").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(ui, criteria, &[Field::ResourceRichness], input_width);
                        filter_row(ui, "Resources", input_width, |ui| {
                            let mut selected = u64::from(criteria.resources.selected);
                            choices(
                                ui,
                                "resources",
                                &mut selected,
                                RESOURCE_LABELS.iter().copied().enumerate(),
                                input_width,
                            );
                            criteria.resources.selected = selected as u32;
                        });
                        choice_rows(ui, criteria, &[Field::SpecialtyResources], input_width);
                    });

                egui::CollapsingHeader::new(
                    RichText::new("Sky & Atmospherics").size(16.0).strong(),
                )
                .default_open(true)
                .show(ui, |ui| {
                    choice_rows(
                        ui,
                        criteria,
                        &[Field::Sky, Field::Horizon, Field::WorldColourFilter],
                        input_width,
                    );
                });

                egui::CollapsingHeader::new(RichText::new("Weather").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(ui, criteria, &[Field::StormFrequency], input_width);
                        criteria.enforce_weather_dependencies();
                        let storms_enabled = !criteria.storm_frequency_is_only_none();
                        ui.add_enabled_ui(storms_enabled, |ui| {
                            planet_flag_rows(
                                ui,
                                criteria,
                                &[PlanetFlag::ExtremeStorms],
                                input_width,
                            );
                            choice_rows(ui, criteria, &[Field::StormType], input_width);
                        });
                    });
            });
    }
    fn displayed_record(&self) -> Option<Arc<search_probes_model::session::SearchRecord>> {
        let records = &self.state.search_history;
        self.viewed_search
            .and_then(|id| {
                records
                    .iter()
                    .find(|record| record.progress.id == id)
                    .cloned()
            })
            .or_else(|| records.last().cloned())
    }

    fn viewing_past_search(&self) -> bool {
        self.viewed_search.is_some()
    }

    fn load_history_record(&mut self, index: usize) -> Result<(), String> {
        let records = &self.state.search_history;
        let record = records
            .get(index)
            .cloned()
            .ok_or_else(|| "Search history index is unavailable".to_owned())?;
        if index + 1 == records.len() {
            self.viewed_search = None;
            if let Some(draft) = self.history_return_draft.take() {
                draft.restore(&mut self.settings);
            } else {
                self.settings.search = record.progress.options.clone();
                self.settings.workers = record.execution.workers;
                self.settings.frame_budget_ms = record.execution.frame_budget_ms;
                self.settings.keep_game_running = record.execution.keep_game_running;
            }
        } else {
            if self.viewed_search.is_none() {
                self.history_return_draft = Some(SearchDraft::capture(&self.settings));
            }
            self.viewed_search = Some(record.progress.id);
            self.settings.search = record.progress.options.clone();
            self.settings.workers = record.execution.workers;
            self.settings.frame_budget_ms = record.execution.frame_budget_ms;
            self.settings.keep_game_running = record.execution.keep_game_running;
            self.selected_preset = None;
            self.preset_name.clear();
        }
        Ok(())
    }

    fn set_viewed_search(&mut self, search_id: Option<u64>) -> Result<(), String> {
        let Some(search_id) = search_id else {
            let latest = self.state.search_history.len().checked_sub(1);
            return latest.map_or(Ok(()), |index| self.load_history_record(index));
        };
        let index = self
            .state
            .search_history
            .iter()
            .position(|record| record.progress.id == search_id)
            .ok_or_else(|| "Search is not retained in history".to_owned())?;
        self.load_history_record(index)
    }

    fn step_history(&mut self, direction: HistoryDirection) -> Result<(), String> {
        let records = &self.state.search_history;
        if records.is_empty() {
            return Err("Search history is empty".into());
        }
        let selected = self
            .viewed_search
            .and_then(|id| records.iter().position(|record| record.progress.id == id))
            .unwrap_or(records.len() - 1);
        let next = match direction {
            HistoryDirection::Older => selected.checked_sub(1),
            HistoryDirection::Newer => (selected + 1 < records.len()).then_some(selected + 1),
        }
        .ok_or_else(|| "There is no search in that history direction".to_owned())?;
        self.load_history_record(next)
    }

    fn history_navigation(&mut self, ui: &mut Ui, rect: egui::Rect) {
        let records = &self.state.search_history;
        let selected = self
            .viewed_search
            .and_then(|id| records.iter().position(|record| record.progress.id == id))
            .or_else(|| records.len().checked_sub(1));
        let older = selected.is_some_and(|index| index > 0);
        let newer = selected.is_some_and(|index| index + 1 < records.len());
        let label = match selected {
            None => "No searches yet".to_owned(),
            Some(index) if index + 1 == records.len() => "Current search".to_owned(),
            Some(index) => format!("Search {} of {}", index + 1, records.len()),
        };
        let label: egui::WidgetText = RichText::new(label).strong().size(18.0).into();
        let galley = label.clone().into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        );
        let label_rect = egui::Rect::from_center_size(rect.center(), galley.size());
        ui.put(label_rect, egui::Label::new(label));

        let button_size = egui::vec2(44.0, 44.0);
        let spacing = 8.0;
        let previous_rect = egui::Rect::from_center_size(
            egui::pos2(
                label_rect.left() - spacing - button_size.x / 2.0,
                rect.center().y,
            ),
            button_size,
        );
        let next_rect = egui::Rect::from_center_size(
            egui::pos2(
                label_rect.right() + spacing + button_size.x / 2.0,
                rect.center().y,
            ),
            button_size,
        );

        let previous = egui::Image::from_bytes(
            "bytes://search-probes/icons/arrow-left.svg",
            include_bytes!("../assets/icons/arrow-left.svg"),
        )
        .fit_to_exact_size(egui::vec2(30.0, 30.0));
        let mut previous_builder = egui::UiBuilder::new()
            .id_salt("previous_search")
            .max_rect(previous_rect);
        if !older {
            previous_builder = previous_builder.disabled();
        }
        let mut previous_ui = ui.new_child(previous_builder);
        let previous_response = previous_ui
            .put(
                previous_rect,
                egui::Button::image(previous)
                    .image_tint_follows_text_color(true)
                    .frame(false),
            )
            .on_hover_text("View the previous search");
        if previous_response.clicked()
            && let Err(error) = self.step_history(HistoryDirection::Older)
        {
            self.error = Some(error);
        }

        let next = egui::Image::from_bytes(
            "bytes://search-probes/icons/arrow-right.svg",
            include_bytes!("../assets/icons/arrow-right.svg"),
        )
        .fit_to_exact_size(egui::vec2(30.0, 30.0));
        let mut next_builder = egui::UiBuilder::new()
            .id_salt("next_search")
            .max_rect(next_rect);
        if !newer {
            next_builder = next_builder.disabled();
        }
        let mut next_ui = ui.new_child(next_builder);
        let next_response = next_ui
            .put(
                next_rect,
                egui::Button::image(next)
                    .image_tint_follows_text_color(true)
                    .frame(false),
            )
            .on_hover_text("View the next search");
        if next_response.clicked()
            && let Err(error) = self.step_history(HistoryDirection::Newer)
        {
            self.error = Some(error);
        }
    }

    fn settings_toggle(&mut self, ui: &mut Ui) {
        let icon = egui::Image::from_bytes(
            "bytes://search-probes/icons/settings.svg",
            include_bytes!("../assets/icons/settings.svg"),
        )
        .fit_to_exact_size(egui::vec2(28.0, 28.0));
        let settings = ui
            .add(
                egui::Button::image(icon)
                    .image_tint_follows_text_color(true)
                    .frame(false)
                    .min_size(egui::vec2(42.0, 40.0)),
            )
            .on_hover_text("Settings");
        settings
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Settings"));
        if settings.clicked() {
            self.settings_sidebar = !self.settings_sidebar;
        }
    }

    fn results(&mut self, ui: &mut Ui) {
        let availability = self.state.availability;
        let search_pause = self.state.search_pause;
        let metrics = self.state.metrics.clone();
        let player = self.state.player;
        let destination = self.state.destination;
        let viewing_past = self.viewing_past_search();
        let progress = self
            .displayed_record()
            .map(|record| Arc::clone(&record.progress));
        let result_cards = self.settings.interface.result_cards;
        let palette = self.palette();
        let results = progress
            .as_ref()
            .map_or(&[][..], |progress| progress.results.as_slice());
        ui.horizontal(|ui| {
            ui.heading("Results");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(!results.is_empty(), |ui| {
                    let (response, _) =
                        egui::containers::menu::MenuButton::from_button(search_action_button(
                            export_button("Export results"),
                            PRIMARY_SEARCH_ACTION_WIDTH,
                        ))
                        .ui(ui, |ui| {
                            for format in [ExportFormat::Text, ExportFormat::Csv] {
                                if ui.button(format.label()).clicked() {
                                    match ExportRequest::new(format, results) {
                                        Ok(request) => self.export_request = Some(request),
                                        Err(error) => self.error = Some(error),
                                    }
                                    self.notifications.clear_export();
                                    ui.close();
                                }
                            }
                        });
                    response.on_hover_text("Export the displayed results as text or CSV");
                })
                .response
                .on_disabled_hover_text("There are no results to export");
            });
        });
        if !viewing_past && availability != Availability::Ready {
            ui.label(match availability {
                Availability::MainMenu => "Load a save to search.",
                Availability::Loading => "Waiting for the game to finish loading…",
                Availability::Unsupported => {
                    "This game build is not supported by the installed mod."
                }
                Availability::Stopped => "The game has closed.",
                Availability::Failed => "The runtime could not start.",
                _ => "Starting the game runtime…",
            });
        }
        let has_progress = progress.is_some();
        animated_visibility(
            ui,
            (
                "search_progress",
                progress.as_ref().map(|progress| progress.id),
            ),
            has_progress,
            |ui| {
                let Some(progress) = &progress else {
                    return;
                };
                ui.label(format!(
                    "{} systems checked · {} results · {} elapsed",
                    number(progress.systems),
                    progress.results.len(),
                    elapsed(progress.elapsed_ms),
                ));
                ui.label(format!(
                    "{} / {} systems",
                    number(progress.systems),
                    number(progress.options.systems)
                ));
                theme::progress_bar(
                    ui,
                    self.palette(),
                    (progress.systems as f64 / progress.options.systems as f64) as f32,
                );
                ui.label(match progress.phase {
                    Phase::Running => match search_pause {
                        Some(SearchPause::Loading) => "Search paused while the game loads.",
                        Some(SearchPause::OtherGalaxy) => {
                            "Search paused: return to its starting galaxy to resume."
                        }
                        Some(SearchPause::User) => "Search paused.",
                        None => "Searching…",
                    },
                    Phase::Completed => "Search complete",
                    Phase::Cancelled => "Search cancelled",
                    Phase::Failed => "Search stopped after an error",
                });
                if !viewing_past
                    && metrics.elapsed_ms > 0
                    && progress.phase == Phase::Running
                    && search_pause.is_none()
                    && self.rolling_rates.values.is_some()
                {
                    let rates = self.rolling_rates.values.unwrap_or([0.0; 3]);
                    let mut labels = vec![format!("{:.0} generated systems/s", rates[0])];
                    if let Ok(plan) = progress.options.criteria.compile() {
                        if !plan.planet.is_empty() {
                            labels.push(format!("{:.0} generated planets/s", rates[1]));
                        }
                        if plan.targets_animals() {
                            labels.push(format!("{:.0} generated fauna/s", rates[2]));
                        }
                    }
                    ui.small(format!("Recent {}", labels.join(" · ")));
                }
                if progress.unresolved_arena_planets > 0 {
                    ui.small(format!(
                        "{} planets could not be checked against the selected Arena ranks.",
                        number(progress.unresolved_arena_planets)
                    ));
                }
                if let Some(error) = &progress.error {
                    error_callout(ui, self.palette(), error);
                }
            },
        );
        animated_visibility(ui, "search_instructions", !has_progress, |ui| {
            ui.label("Choose your criteria and start a search.");
        });
        let has_results = !results.is_empty();
        animated_visibility(ui, "search_result_cards", has_results, |ui| {
            if !has_results {
                return;
            }
            ui.separator();
            let gap = ui.spacing().item_spacing.x;
            let available = ui.available_width();
            let columns = ((available + gap) / (RESULT_CARD_MIN_WIDTH + gap))
                .floor()
                .max(1.0) as usize;
            for (row, results) in results.chunks(columns).enumerate() {
                ui.columns(columns, |cards| {
                    for (column, result) in results.iter().enumerate() {
                        let index = row * columns + column;
                        cards[column].push_id(
                            (
                                result.address.raw(),
                                result.kind,
                                result.fauna.as_ref().map(|fauna| fauna.role),
                            ),
                            |ui| {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let coordinates = result.address.coordinates();
                                    let name = result.title().unwrap_or("Name unavailable");
                                    ui.label(
                                        RichText::new(format!("{}. {name}", index + 1)).strong(),
                                    );
                                    let portal_code = result.target().portal_code();
                                    if result_cards.location_subtitle {
                                        ui.label(
                                            RichText::new(result_subtitle(result))
                                            .small()
                                            .color(palette.secondary_text),
                                        );
                                    }
                                    if result_cards.portal_glyphs {
                                        self.portal_glyphs
                                            .show(ui, &portal_code, palette.secondary_text)
                                            .on_hover_text(format!("Portal code: {portal_code}"));
                                    }
                                    ui.horizontal_wrapped(|ui| {
                                        if result_cards.set_destination {
                                            let set_destination = ui
                                            .add_enabled(
                                                availability == Availability::Ready
                                                    && result.destination_names_complete()
                                                    && player.is_some_and(|player| player.coordinates().galaxy == coordinates.galaxy),
                                                egui::Button::new("Set destination"),
                                            )
                                            .on_hover_text("Set this result as your active in-game destination.");
                                            if set_destination.clicked() {
                                                self.send(Action::Navigate {
                                                    search_id: progress.as_ref().map_or(0, |progress| progress.id),
                                                    target: result.target(),
                                                });
                                            }
                                        }
                                        if result_cards.copy_portal_code
                                            && ui
                                            .button("Copy portal code")
                                            .on_hover_text(format!(
                                                "{}\nUse a portal in Galaxy {}. {}",
                                                portal_code,
                                                u16::from(coordinates.galaxy) + 1,
                                                if result.kind == TargetKind::System { "Arrives through the first planet’s portal in this system." } else { "Arrives on the target planet." }
                                            ))
                                            .clicked()
                                        {
                                            ui.ctx().copy_text(portal_code.clone());
                                        }
                                        if result_cards.copy_galactic_coordinates
                                            && ui
                                            .button("Copy galactic coords")
                                            .on_hover_text(format!(
                                                "{}\nUse with save editors or other third-party tools.",
                                                result.address.galactic_coordinates()
                                            ))
                                            .clicked()
                                        {
                                            ui.ctx().copy_text(result.address.galactic_coordinates());
                                        }
                                        if destination == Some(result.target()) {
                                            ui.label("Active destination");
                                        }
                                    });
                                });
                            },
                        );
                    }
                });
            }
        });
    }
}

impl App {
    fn save_preset(&mut self) -> Result<u64, String> {
        let name = self.preset_name.trim().to_owned();
        validate_name(&name)
            .and_then(|_| self.settings.search.validate())
            .map_err(str::to_owned)?;
        let existing = self
            .settings
            .presets
            .iter()
            .position(|preset| preset.name.to_lowercase() == name.to_lowercase());
        let preset = Preset {
            name,
            search: self.settings.search.clone(),
        };
        if let Some(index) = existing {
            self.settings.presets[index] = preset;
            self.selected_preset = Some(index);
            self.managed_preset = Some(index);
            self.managed_preset_name = self.settings.presets[index].name.clone();
        } else if self.settings.presets.len() < MAX_PRESETS {
            let index = self.settings.presets.len();
            self.selected_preset = Some(index);
            self.managed_preset = Some(index);
            self.managed_preset_name = preset.name.clone();
            self.settings.presets.push(preset);
        } else {
            return Err("Search Probes can save nine presets. Replace or remove one first.".into());
        }
        let command_id = self.try_send(Action::Configure {
            settings: self.settings.clone(),
        })?;
        self.preset_dialog = None;
        Ok(command_id)
    }

    fn rename_preset(&mut self, index: usize, name: String) -> Result<u64, String> {
        if index >= self.settings.presets.len() {
            return Err("Preset is unavailable".into());
        }
        let name = name.trim().to_owned();
        validate_name(&name).map_err(str::to_owned)?;
        if self
            .settings
            .presets
            .iter()
            .enumerate()
            .any(|(candidate, preset)| {
                candidate != index && preset.name.eq_ignore_ascii_case(&name)
            })
        {
            return Err("Preset name already taken".into());
        }
        self.settings.presets[index].name = name.clone();
        self.managed_preset = Some(index);
        self.managed_preset_name = name.clone();
        if self.selected_preset == Some(index) {
            self.preset_name = name;
        }
        self.try_send(Action::Configure {
            settings: self.settings.clone(),
        })
    }

    fn remove_preset(&mut self, index: usize) -> Result<u64, String> {
        if index >= self.settings.presets.len() {
            return Err("Preset is unavailable".into());
        }
        self.settings.presets.remove(index);
        match self.selected_preset {
            Some(selected) if selected == index => {
                self.selected_preset = None;
                self.preset_name.clear();
            }
            Some(selected) if selected > index => self.selected_preset = Some(selected - 1),
            _ => {}
        }
        match self.managed_preset {
            Some(selected) if selected == index => {
                self.managed_preset = None;
                self.managed_preset_name.clear();
            }
            Some(selected) if selected > index => self.managed_preset = Some(selected - 1),
            _ => {}
        }
        self.try_send(Action::Configure {
            settings: self.settings.clone(),
        })
    }

    fn record(
        &self,
        search_id: u64,
    ) -> Result<Arc<search_probes_model::session::SearchRecord>, String> {
        self.state
            .search_history
            .iter()
            .find(|record| record.progress.id == search_id)
            .cloned()
            .ok_or_else(|| "Search is not retained in history".into())
    }

    fn result(&self, search_id: u64, result_index: usize) -> Result<CachedMatch, String> {
        self.record(search_id)?
            .progress
            .results
            .get(result_index)
            .cloned()
            .ok_or_else(|| "Result index is outside the retained search".into())
    }

    fn export_output(
        &self,
        source: UiExportSource,
        format: UiExportFormat,
    ) -> Result<UiOutput, String> {
        let request = match (source, format) {
            (UiExportSource::Results, UiExportFormat::Text | UiExportFormat::Csv) => {
                let record = self.displayed_record();
                let results = record
                    .as_ref()
                    .map_or(&[][..], |record| record.progress.results.as_slice());
                let format = match format {
                    UiExportFormat::Text => ExportFormat::Text,
                    UiExportFormat::Csv => ExportFormat::Csv,
                    UiExportFormat::Json => unreachable!(),
                };
                ExportRequest::new(format, results)?
            }
            (UiExportSource::CurrentSearch, UiExportFormat::Json) => {
                let progress = self
                    .state
                    .progress
                    .as_ref()
                    .ok_or_else(|| "There is no current search to export".to_owned())?;
                let record = self.record(progress.id)?;
                ExportRequest::search_json(&record)?
            }
            (UiExportSource::ViewedSearch, UiExportFormat::Json) => {
                let search_id = self
                    .viewed_search
                    .ok_or_else(|| "No history search is selected".to_owned())?;
                let record = self.record(search_id)?;
                ExportRequest::search_json(&record)?
            }
            (UiExportSource::Presets, UiExportFormat::Json) => {
                ExportRequest::presets_json(&self.state.settings)?
            }
            _ => return Err("Export format is unavailable for the selected source".into()),
        };
        if request.contents.len() > search_probes_model::runtime::MAX_STATE_BYTES / 2 {
            return Err("Export is too large for agent control".into());
        }
        Ok(UiOutput::Export {
            source,
            format,
            subject: request.subject().into(),
            suggested_name: request.suggested_name(),
            contents: request.contents,
        })
    }

    fn apply_agent_action(
        &mut self,
        ctx: &egui::Context,
        action: UiAction,
        set_overlay_visible: &mut impl FnMut(bool) -> Result<(), String>,
    ) -> Result<Option<UiOutput>, String> {
        match action {
            UiAction::SetOverlayVisible { visible } => {
                set_overlay_visible(visible)?;
                self.overlay_visible = visible;
            }
            UiAction::SetSettingsSidebar { open } => self.settings_sidebar = open,
            UiAction::SetDraftSettings { settings } => {
                settings.validate().map_err(str::to_owned)?;
                self.settings = settings;
                self.selected_preset = None;
            }
            UiAction::SetDraftSearch { search } => {
                search.validate().map_err(str::to_owned)?;
                self.settings.search = search;
                self.selected_preset = None;
            }
            UiAction::SetCriteria { criteria } => {
                criteria.compile().map_err(str::to_owned)?;
                self.settings.search.criteria = criteria;
                self.selected_preset = None;
            }
            UiAction::SetChoiceFilter { field, selected } => {
                let dependencies = &self.settings.search.criteria;
                if field == Field::Size && dependencies.derived_planet_size().is_some() {
                    return Err("Planet size is derived from the selected Gas Giant biome".into());
                }
                if field == Field::StormType && dependencies.storm_frequency_is_only_none() {
                    return Err("Storm type is disabled while Storm frequency is only None".into());
                }
                if field == Field::Race && dependencies.race_available_mask() == 0 {
                    return Err("Dominant lifeform cannot narrow the Population selection".into());
                }
                let options: Vec<_> = if field == Field::Subtype {
                    self.settings.search.criteria.subtype_options()
                } else {
                    self.settings
                        .search
                        .criteria
                        .choice_options(field)
                        .into_iter()
                        .map(|(index, label)| (1u64 << index, label))
                        .collect()
                };
                let mask = labels_mask(&selected, options)?;
                let criteria = &mut self.settings.search.criteria;
                match field {
                    Field::Biome => criteria.set_biome_choices(mask),
                    Field::Sentinels => criteria.set_sentinel_choices(mask),
                    _ if mask == 0 => {
                        criteria.choices.remove(&field);
                    }
                    _ => {
                        criteria.choices.insert(field, mask);
                    }
                }
                criteria.enforce_dependencies();
                self.selected_preset = None;
            }
            UiAction::SetPlanetRequirement { flag, value } => {
                if flag.is_sentinel_level_projection() {
                    return Err("Use the unified Sentinels filter for sentinel levels".into());
                }
                if value.is_some()
                    && !self
                        .settings
                        .search
                        .criteria
                        .planet_requirement_available(flag)
                {
                    return Err(format!(
                        "{} is unavailable with the current filters",
                        flag.label()
                    ));
                }
                if let Some(value) = value {
                    self.settings.search.criteria.planets.insert(flag, value);
                } else {
                    self.settings.search.criteria.planets.remove(&flag);
                }
                self.settings.search.criteria.enforce_dependencies();
                self.selected_preset = None;
            }
            UiAction::SetSystemRequirement { flag, value } => {
                if flag == SystemFlag::Pirate
                    && value.is_some()
                    && !self.settings.search.criteria.outlaw_filter_available()
                {
                    return Err(
                        "Outlaw system cannot narrow the selected population and lifeform".into(),
                    );
                }
                if let Some(value) = value {
                    self.settings.search.criteria.systems.insert(flag, value);
                } else {
                    self.settings.search.criteria.systems.remove(&flag);
                }
                self.settings.search.criteria.enforce_dependencies();
                self.selected_preset = None;
            }
            UiAction::SetResourceFilter { selected } => {
                let options = RESOURCE_LABELS
                    .iter()
                    .enumerate()
                    .map(|(index, label)| (1u64 << index, *label));
                self.settings.search.criteria.resources.selected =
                    u32::try_from(labels_mask(&selected, options)?)
                        .map_err(|_| "Resource selection exceeds its UI bounds")?;
                self.selected_preset = None;
            }
            UiAction::SetPlanetCounts { selected } => {
                self.settings.search.criteria.planet_counts = count_mask(&selected, 6)?;
                self.selected_preset = None;
            }
            UiAction::SetStarCounts { selected } => {
                self.settings.search.criteria.star_counts = count_mask(&selected, 3)?;
                self.selected_preset = None;
            }
            UiAction::SetWaterFilter { scope, value } => {
                if scope == WaterScope::Planet
                    && !self
                        .settings
                        .search
                        .criteria
                        .planet_water_option_available(value)
                {
                    return Err(
                        "That planet water option conflicts with the current filters".into(),
                    );
                }
                match scope {
                    WaterScope::System => self.settings.search.criteria.system_water = value,
                    WaterScope::Planet => self.settings.search.criteria.planet_water = value,
                }
                self.settings.search.criteria.enforce_dependencies();
                self.selected_preset = None;
            }
            UiAction::SetCompositionFilter {
                field,
                side,
                selected,
            } => {
                let options: Vec<_> = match field {
                    CompositionField::Biomes => Field::Biome
                        .available_options()
                        .map(|(index, label)| (1u64 << index, label))
                        .collect(),
                    CompositionField::Sizes => Field::Size
                        .available_options()
                        .map(|(index, label)| (1u64 << index, label))
                        .collect(),
                    CompositionField::Subtypes => {
                        let biomes = self
                            .settings
                            .search
                            .criteria
                            .composition
                            .selections(CompositionField::Biomes)
                            .get(side);
                        subtype_options_for(biomes)
                    }
                };
                let mask = labels_mask(&selected, options)?;
                self.settings
                    .search
                    .criteria
                    .composition
                    .set(field, side, mask);
                self.settings.search.criteria.enforce_dependencies();
                self.selected_preset = None;
            }
            UiAction::SetFaunaChoices {
                domain,
                field,
                selected,
            } => {
                if !self.settings.search.criteria.fauna_domain_available(domain) {
                    return Err(format!(
                        "{} filters are unavailable with the current planet filters",
                        domain.label()
                    ));
                }
                if self
                    .settings
                    .search
                    .criteria
                    .fauna
                    .get(&domain)
                    .is_some_and(|criteria| criteria.presence == Some(false))
                {
                    return Err(format!(
                        "{} filters are disabled while fauna presence is Exclude",
                        domain.label()
                    ));
                }
                let options = self
                    .settings
                    .search
                    .criteria
                    .fauna
                    .get(&domain)
                    .cloned()
                    .unwrap_or_default()
                    .choice_options(domain, field)
                    .into_iter()
                    .map(|(index, label)| (1u64 << index, label));
                let mask = labels_mask(&selected, options)?;
                let criteria = self
                    .settings
                    .search
                    .criteria
                    .fauna
                    .entry(domain)
                    .or_default();
                if mask == 0 {
                    criteria.choices.remove(&field);
                } else {
                    criteria.choices.insert(field, mask);
                }
                criteria.enforce_domain_dependencies(domain);
                remove_empty_fauna(&mut self.settings.search.criteria, domain);
                self.selected_preset = None;
            }
            UiAction::SetFaunaPresence { domain, value } => {
                if value.is_some() && !self.settings.search.criteria.fauna_domain_available(domain)
                {
                    return Err(format!(
                        "{} presence is unavailable with the current planet filters",
                        domain.label()
                    ));
                }
                let criteria = self
                    .settings
                    .search
                    .criteria
                    .fauna
                    .entry(domain)
                    .or_default();
                if criteria.has_detail_filters() && value != Some(true) {
                    return Err(format!(
                        "{} presence is locked to Require until its filters are reset",
                        domain.label()
                    ));
                }
                criteria.presence = value;
                criteria.enforce_dependencies();
                remove_empty_fauna(&mut self.settings.search.criteria, domain);
                self.selected_preset = None;
            }
            UiAction::SetFaunaHeight {
                domain,
                minimum_cm,
                maximum_cm,
            } => {
                if !self.settings.search.criteria.fauna_domain_available(domain) {
                    return Err(format!(
                        "{} filters are unavailable with the current planet filters",
                        domain.label()
                    ));
                }
                if self
                    .settings
                    .search
                    .criteria
                    .fauna
                    .get(&domain)
                    .is_some_and(|criteria| criteria.presence == Some(false))
                {
                    return Err(format!(
                        "{} filters are disabled while fauna presence is Exclude",
                        domain.label()
                    ));
                }
                if minimum_cm.is_some_and(|value| value > 100_000)
                    || maximum_cm.is_some_and(|value| value > 100_000)
                    || matches!((minimum_cm, maximum_cm), (Some(a), Some(b)) if a > b)
                {
                    return Err("Fauna height must be ordered between 0 and 100000 cm".into());
                }
                let height = &mut self
                    .settings
                    .search
                    .criteria
                    .fauna
                    .entry(domain)
                    .or_default()
                    .height;
                height.minimum_cm = minimum_cm;
                height.maximum_cm = maximum_cm;
                self.settings
                    .search
                    .criteria
                    .fauna
                    .get_mut(&domain)
                    .expect("fauna criteria was inserted")
                    .enforce_domain_dependencies(domain);
                remove_empty_fauna(&mut self.settings.search.criteria, domain);
                self.selected_preset = None;
            }
            UiAction::SetArenaChoices { field, selected } => {
                if !self.settings.search.criteria.arena_available() {
                    return Err(
                        "Xeno Arena filters are unavailable when planet fauna is Dead".into(),
                    );
                }
                if self.settings.search.criteria.arena.presence == Some(false) {
                    return Err(
                        "Xeno Arena filters are disabled while tameable fauna is excluded".into(),
                    );
                }
                let options = self
                    .settings
                    .search
                    .criteria
                    .arena
                    .choice_options(field)
                    .into_iter()
                    .map(|(index, label)| (1u64 << index, label));
                let mask = labels_mask(&selected, options)?;
                if mask == 0 {
                    self.settings.search.criteria.arena.choices.remove(&field);
                } else {
                    self.settings
                        .search
                        .criteria
                        .arena
                        .choices
                        .insert(field, mask);
                }
                self.settings.search.criteria.arena.enforce_dependencies();
                self.selected_preset = None;
            }
            UiAction::SetArenaPresence { value } => {
                if value.is_some() && !self.settings.search.criteria.arena_available() {
                    return Err("Tameable fauna is unavailable when planet fauna is Dead".into());
                }
                if self.settings.search.criteria.arena.has_detail_filters() && value != Some(true) {
                    return Err(
                        "Tameable fauna is locked to Require until Xeno Arena filters are reset"
                            .into(),
                    );
                }
                self.settings.search.criteria.arena.presence = value;
                self.settings.search.criteria.arena.enforce_dependencies();
                self.selected_preset = None;
            }
            UiAction::SetSystemBudget { systems } => {
                if !BUDGETS.contains(&systems) {
                    return Err("System budget is not a UI dropdown value".into());
                }
                self.settings.search.systems = systems;
            }
            UiAction::SetResultLimit { results } => {
                if !(1..=MAX_RESULTS).contains(&results) {
                    return Err("Result limit must be between 1 and 100".into());
                }
                self.settings.search.results = results;
            }
            UiAction::SetTimeLimit { time_limit } => {
                if let Some(limit) = time_limit {
                    limit.validate().map_err(str::to_owned)?;
                }
                self.settings.search.time_limit = time_limit;
            }
            UiAction::SetWorkers { workers } => {
                if workers.is_some_and(|workers| !(1..=available_workers()).contains(&workers)) {
                    return Err("Worker count is not available on this system".into());
                }
                self.settings.workers = workers;
            }
            UiAction::SetFrameBudget { frame_budget_ms } => {
                validate_frame_budget(frame_budget_ms).map_err(str::to_owned)?;
                self.settings.frame_budget_ms = frame_budget_ms;
                let command_id = self.try_send(Action::ConfigureIntensity { frame_budget_ms })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetKeepGameRunning { keep_game_running } => {
                self.settings.keep_game_running = keep_game_running;
                let command_id = self.try_send(Action::Configure {
                    settings: self.settings.clone(),
                })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetTheme { theme } => {
                if let ThemeSelection::Custom { name } = &theme
                    && !self
                        .settings
                        .interface
                        .custom_themes
                        .iter()
                        .any(|custom| custom.name == *name)
                {
                    return Err("Custom theme is unavailable".into());
                }
                self.settings.interface.theme = theme;
                self.applied_theme = None;
                let command_id = self.try_send(Action::Configure {
                    settings: self.settings.clone(),
                })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetThemeEditor {
                draft,
                original_name,
            } => {
                if original_name.is_some() && draft.is_none() {
                    return Err("An edited custom theme requires a draft".into());
                }
                if let Some(draft) = draft.as_ref() {
                    if draft.name.chars().count() > 80 {
                        return Err("Theme names can contain at most 80 characters".into());
                    }
                    if draft.name.chars().any(char::is_control) {
                        return Err("Theme name contains unsupported characters".into());
                    }
                }
                if let Some(original) = original_name.as_deref()
                    && !self
                        .settings
                        .interface
                        .custom_themes
                        .iter()
                        .any(|custom| custom.name == original)
                {
                    return Err("Custom theme is unavailable".into());
                }
                self.theme_editor = draft;
                self.theme_editor_original_name = original_name;
                self.error = None;
            }
            UiAction::SaveCustomTheme {} => {
                let command_id = self.save_custom_theme()?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::RemoveCustomTheme { name } => {
                let command_id = self.remove_custom_theme(&name)?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetHotkey { hotkey } => {
                hotkey.validate().map_err(str::to_owned)?;
                self.settings.interface.hotkey = hotkey;
                self.binding_capture = None;
                let command_id = self.try_send(Action::Configure {
                    settings: self.settings.clone(),
                })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetNotifications { notifications } => {
                self.settings.interface.notifications = notifications;
                self.notifications.update(
                    self.started.elapsed(),
                    &self.settings.interface.notifications,
                );
                let command_id = self.try_send(Action::Configure {
                    settings: self.settings.clone(),
                })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetResultCards { result_cards } => {
                result_cards.validate().map_err(str::to_owned)?;
                self.settings.interface.result_cards = result_cards;
                let command_id = self.try_send(Action::Configure {
                    settings: self.settings.clone(),
                })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetHotkeyCapture { device } => {
                self.binding_capture = device.map(BindingCapture::new);
            }
            UiAction::SelectPreset { index } => {
                let preset = self
                    .settings
                    .presets
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "Preset index is unavailable".to_owned())?;
                self.selected_preset = Some(index);
                self.preset_name = preset.name.clone();
                self.managed_preset = Some(index);
                self.managed_preset_name = preset.name;
                self.settings.search = preset.search;
            }
            UiAction::ClearPresetSelection {} => {
                self.selected_preset = None;
                self.preset_name.clear();
            }
            UiAction::SetPresetDialog { open } => {
                if open {
                    self.open_preset_dialog();
                } else {
                    self.preset_dialog = None;
                    self.error = None;
                }
            }
            UiAction::SetRenamePresetDialog { index } => {
                if let Some(index) = index {
                    self.open_rename_preset_dialog(index)?;
                } else {
                    self.preset_dialog = None;
                    self.error = None;
                }
            }
            UiAction::SetPresetName { name } => {
                if name.chars().count() > 80 {
                    return Err("Preset names can contain at most 80 characters".into());
                }
                if name.chars().any(char::is_control) {
                    return Err("Preset name contains unsupported characters".into());
                }
                self.preset_name = name;
            }
            UiAction::SelectManagedPreset { index } => {
                if let Some(index) = index {
                    let preset = self
                        .settings
                        .presets
                        .get(index)
                        .ok_or_else(|| "Preset index is unavailable".to_owned())?;
                    self.managed_preset = Some(index);
                    self.managed_preset_name = preset.name.clone();
                    self.selected_preset = Some(index);
                    self.preset_name = preset.name.clone();
                    self.settings.search = preset.search.clone();
                    self.viewed_search = None;
                    self.history_return_draft = None;
                } else {
                    self.managed_preset = None;
                    self.managed_preset_name.clear();
                }
            }
            UiAction::SavePreset {} => {
                let command_id = self.save_preset()?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::RenamePreset { index, name } => {
                let command_id = self.rename_preset(index, name)?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::RemovePreset { index } => {
                let command_id = self.remove_preset(index)?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::ClearCriteria {} => {
                self.settings.search.criteria = Criteria::default();
                self.selected_preset = None;
            }
            UiAction::ApplySettings {} => {
                self.settings.validate().map_err(str::to_owned)?;
                let command_id = self.try_send(Action::Configure {
                    settings: self.settings.clone(),
                })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::StartSearch {} => {
                self.settings.validate().map_err(str::to_owned)?;
                self.settings
                    .search
                    .validate_search()
                    .map_err(str::to_owned)?;
                let command_id = self.start_displayed_search()?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetSearchPaused { paused } => {
                if self.viewing_past_search() {
                    return Err("Cannot pause or resume while viewing search history".into());
                }
                let command_id = self.try_send(Action::SetSearchPaused { paused })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::CancelSearch {} => {
                if self.viewing_past_search() {
                    return Err("Cannot cancel while viewing search history".into());
                }
                let command_id = self.try_send(Action::Cancel {})?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetViewedSearch { search_id } => {
                self.set_viewed_search(search_id)?;
            }
            UiAction::StepHistory { direction } => {
                self.step_history(direction)?;
            }
            UiAction::NavigateResult {
                search_id,
                result_index,
            } => {
                let result = self.result(search_id, result_index)?;
                let command_id = self.try_send(Action::Navigate {
                    search_id,
                    target: result.target(),
                })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::CopyResult {
                search_id,
                result_index,
                value,
            } => {
                let result = self.result(search_id, result_index)?;
                let text = match value {
                    ResultValue::PortalCode => result.target().portal_code(),
                    ResultValue::GalacticCoordinates => result.address.galactic_coordinates(),
                };
                ctx.copy_text(text.clone());
                return Ok(Some(UiOutput::CopiedText { value, text }));
            }
            UiAction::Export { source, format } => {
                self.notifications.clear_export();
                return self.export_output(source, format).map(Some);
            }
            UiAction::ShowToast { id, toast } => match toast {
                TestToast::ResultFound {
                    search_id,
                    result_index,
                } => {
                    let result = self.result(search_id, result_index)?;
                    self.notifications.push_controlled(
                        id,
                        Notification::ResultFound { result },
                        self.started.elapsed(),
                    )?;
                }
                TestToast::SearchCompleted {
                    systems,
                    system_limit,
                    planets,
                    fauna,
                    results,
                    result_limit,
                    elapsed_ms,
                } => {
                    self.notifications.push_controlled(
                        id,
                        Notification::SearchCompleted {
                            progress: SearchProgress {
                                systems,
                                system_limit,
                                planets,
                                fauna,
                                results,
                                result_limit,
                                elapsed_ms,
                            },
                        },
                        self.started.elapsed(),
                    )?;
                }
                TestToast::AutomaticPause { keep_game_running } => {
                    self.notifications.push_controlled(
                        id,
                        Notification::AutomaticallyPaused { keep_game_running },
                        self.started.elapsed(),
                    )?;
                }
                TestToast::AutomaticResume {
                    systems,
                    system_limit,
                    planets,
                    fauna,
                    results,
                    result_limit,
                    elapsed_ms,
                } => {
                    self.notifications.push_controlled(
                        id,
                        Notification::AutomaticallyResumed {
                            progress: SearchProgress {
                                systems,
                                system_limit,
                                planets,
                                fauna,
                                results,
                                result_limit,
                                elapsed_ms,
                            },
                        },
                        self.started.elapsed(),
                    )?;
                }
                TestToast::ExportCompleted { subject, path } => {
                    let subject = match subject.as_str() {
                        "Results" => "Results",
                        "Search" => "Search",
                        "Presets" => "Presets",
                        _ => {
                            return Err(
                                "Export toast subject must be Results, Search, or Presets".into()
                            );
                        }
                    };
                    self.notifications.exported_controlled(
                        id,
                        subject,
                        path,
                        self.started.elapsed(),
                    )?;
                }
            },
            UiAction::DismissToast { id } => {
                if !self
                    .notifications
                    .dismiss_controlled(&id, self.started.elapsed())
                {
                    return Err(format!("No controlled notification has ID {id:?}"));
                }
            }
            UiAction::ClearToasts {} => self.notifications.clear(self.started.elapsed()),
        }
        Ok(None)
    }

    fn ui_state(&self) -> UiState {
        UiState {
            overlay_visible: self.overlay_visible,
            settings_sidebar: self.settings_sidebar,
            hotkey_capture: self.binding_capture.as_ref().map(|capture| capture.device),
            selected_preset: self.selected_preset,
            preset_name: self.preset_name.clone(),
            preset_dialog_open: self.preset_dialog.is_some(),
            renaming_preset: match self.preset_dialog {
                Some(PresetNameDialog::Rename { index }) => Some(index),
                _ => None,
            },
            managed_preset: self.managed_preset,
            managed_preset_name: self.managed_preset_name.clone(),
            theme_editor: self.theme_editor.clone(),
            theme_editor_original_name: self.theme_editor_original_name.clone(),
            selected_theme_palette: Some(self.palette().to_model()),
            viewed_search: self.viewed_search,
            draft_settings: self.settings.clone(),
            toasts: self.notifications.snapshot(),
            last_command: self.ui_last_command,
            error: self.ui_error.clone(),
            output: self.ui_output.clone(),
        }
    }

    fn ui_state_matches(&self, published: &UiState) -> bool {
        published.overlay_visible == self.overlay_visible
            && published.settings_sidebar == self.settings_sidebar
            && published.hotkey_capture
                == self.binding_capture.as_ref().map(|capture| capture.device)
            && published.selected_preset == self.selected_preset
            && published.preset_name == self.preset_name
            && published.preset_dialog_open == self.preset_dialog.is_some()
            && published.renaming_preset
                == match self.preset_dialog {
                    Some(PresetNameDialog::Rename { index }) => Some(index),
                    _ => None,
                }
            && published.managed_preset == self.managed_preset
            && published.managed_preset_name == self.managed_preset_name
            && published.theme_editor == self.theme_editor
            && published.theme_editor_original_name == self.theme_editor_original_name
            && published.selected_theme_palette == Some(self.palette().to_model())
            && published.viewed_search == self.viewed_search
            && published.draft_settings == self.settings
            && published.last_command == self.ui_last_command
            && published.error == self.ui_error
            && published.output == self.ui_output
    }

    fn publish_ui_state(&mut self) {
        let notification_revision = self.notifications.revision();
        if self
            .published_ui
            .as_ref()
            .is_some_and(|(published, revision)| {
                *revision == notification_revision && self.ui_state_matches(published)
            })
        {
            return;
        }
        let ui = self.ui_state();
        self.controls.publish_ui(ui.clone());
        self.published_ui = Some((ui, notification_revision));
    }

    fn process_agent_commands(
        &mut self,
        ctx: &egui::Context,
        set_overlay_visible: &mut impl FnMut(bool) -> Result<(), String>,
    ) -> bool {
        let mut processed = false;
        for _ in 0..16 {
            let Some(UiCommand {
                generation,
                id,
                action,
            }) = self.controls.pop_ui()
            else {
                break;
            };
            processed = true;
            let result = if generation == self.state.generation {
                self.apply_agent_action(ctx, action, set_overlay_visible)
            } else {
                Err("UI command belongs to an expired game state".into())
            };
            self.ui_last_command = id;
            match result {
                Ok(output) => {
                    self.ui_error = None;
                    self.ui_output = output;
                }
                Err(error) => {
                    self.error = Some(error.clone());
                    self.ui_error = Some(error);
                    self.ui_output = None;
                }
            }
        }
        processed
    }

    /// Current draft settings, including edits that have not been applied to the game.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn take_export_request(&mut self) -> Option<ExportRequest> {
        let request = self.export_request.take()?;
        self.pending_export_subject = Some(request.subject());
        Some(request)
    }

    pub fn finish_export(&mut self, result: Result<Option<String>, String>) {
        let subject = self.pending_export_subject.take().unwrap_or("Results");
        match result {
            Ok(Some(path)) => {
                self.error = None;
                self.notifications.exported(
                    subject,
                    path,
                    self.started.elapsed(),
                    &self.settings.interface.notifications,
                );
            }
            Ok(None) => {}
            Err(error) => {
                self.notifications.clear_export();
                self.error = Some(format!("Cannot export {}: {error}", subject.to_lowercase()));
            }
        }
    }

    /// Supplies an already deduplicated lifecycle event. The runtime owns search identity and
    /// must not supply direct user pause/resume as automatic events.
    pub fn notify(&mut self, notification: Notification) {
        self.notifications.push(
            notification,
            self.started.elapsed(),
            &self.settings.interface.notifications,
        );
    }

    /// Both timestamps must use the producer's same monotonic epoch. Only their difference is
    /// mapped onto the UI clock, preserving expiry through delayed delivery or minimized windows.
    pub fn notify_at(
        &mut self,
        notification: Notification,
        occurred: Duration,
        producer_now: Duration,
    ) {
        self.notifications.push_aged(
            notification,
            producer_now.saturating_sub(occurred),
            self.started.elapsed(),
            &self.settings.interface.notifications,
        );
    }

    /// Call when the runtime begins a different search, before delivering its events.
    pub fn clear_search_notifications(&mut self) {
        self.notifications.clear_search();
    }

    /// Prepare both interactive and passive overlay frames. Call even while its window is hidden.
    pub fn prepare_frame(
        &mut self,
        ctx: &egui::Context,
        overlay_visible: bool,
        mut set_overlay_visible: impl FnMut(bool) -> Result<(), String>,
    ) {
        let visibility_changed = self.overlay_visible != overlay_visible;
        self.overlay_visible = overlay_visible;
        let refreshed_ui = self.refresh();
        let processed_commands = self.process_agent_commands(ctx, &mut set_overlay_visible);
        self.apply_theme(ctx);
        if overlay_visible
            || self.has_notifications()
            || visibility_changed
            || refreshed_ui
            || processed_commands
        {
            self.publish_ui_state();
        }
    }

    pub fn has_notifications(&self) -> bool {
        !self.notifications.is_empty()
    }

    pub fn window_title_colors(&self) -> (egui::Color32, egui::Color32) {
        let palette = self.palette();
        (palette.link, palette.foreground_on(palette.link))
    }

    pub fn show_notifications(&mut self, ctx: &egui::Context) {
        self.notifications.update(
            self.started.elapsed(),
            &self.settings.interface.notifications,
        );
        self.notifications.show(
            ctx,
            self.started.elapsed(),
            self.palette(),
            self.settings.interface.notifications.placement,
            self.settings.interface.result_cards,
            &self.portal_glyphs,
        );
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let selected_theme = self.settings.interface.theme.clone();
        let selected_palette = self.palette();
        let font_theme = self
            .theme_editor
            .is_none()
            .then(|| selected_theme.built_in())
            .flatten();
        if self.applied_theme.as_ref()
            != Some(&(selected_theme.clone(), selected_palette, font_theme))
        {
            theme::apply_palette(ctx, selected_palette, font_theme);
            self.applied_theme = Some((selected_theme, selected_palette, font_theme));
            ctx.request_repaint();
        }
    }

    fn consume_lifecycle_events(&mut self) {
        let events = self
            .controls
            .lifecycle_events_after(self.lifecycle_sequence);
        let producer_now = Duration::from_millis(self.controls.uptime_ms());
        for event in events {
            self.lifecycle_sequence = self.lifecycle_sequence.max(event.sequence);
            let occurred = Duration::from_millis(event.published_ms);
            let notification = match event.kind {
                LifecycleEventKind::Started => {
                    self.clear_search_notifications();
                    None
                }
                LifecycleEventKind::ResultsFound { count } => {
                    let Ok(record) = self.record(event.search_id) else {
                        continue;
                    };
                    let end = event.results.min(record.progress.results.len());
                    let start = end.saturating_sub(count);
                    for result in &record.progress.results[start..end] {
                        self.notify_at(
                            Notification::ResultFound {
                                result: result.clone(),
                            },
                            occurred,
                            producer_now,
                        );
                    }
                    continue;
                }
                LifecycleEventKind::AutomaticPause { cause } => {
                    if cause == SearchPause::User {
                        continue;
                    }
                    let Ok(record) = self.record(event.search_id) else {
                        continue;
                    };
                    Some(Notification::AutomaticallyPaused {
                        keep_game_running: record.execution.keep_game_running,
                    })
                }
                LifecycleEventKind::AutomaticResume { cause } => {
                    if cause == SearchPause::User {
                        continue;
                    }
                    let Ok(record) = self.record(event.search_id) else {
                        continue;
                    };
                    Some(Notification::AutomaticallyResumed {
                        progress: notification_progress(&event, &record.progress.options),
                    })
                }
                LifecycleEventKind::Finished { finish } => {
                    if finish == Finish::Cancelled {
                        None
                    } else {
                        let Ok(record) = self.record(event.search_id) else {
                            continue;
                        };
                        Some(Notification::SearchCompleted {
                            progress: notification_progress(&event, &record.progress.options),
                        })
                    }
                }
            };
            if let Some(notification) = notification {
                self.notify_at(notification, occurred, producer_now);
            }
        }
    }

    pub fn refresh(&mut self) -> bool {
        let Some((revision, state)) = self.controls.snapshot_if_changed(self.state_revision) else {
            return false;
        };
        let previous_generation = self.loaded_generation;
        let settings_changed = self.published_settings.as_ref() != Some(&state.settings);
        let previous_latest_search = self.latest_search_id;
        let previous_viewed_search = self.viewed_search;
        let previous_lifecycle = self.lifecycle_sequence;
        let previous_toasts = self.notifications.len();
        self.state_revision = revision;
        self.state = state;
        self.rolling_rates.observe(&self.state);
        let state = &self.state;
        if self.loaded_generation != Some(state.generation) {
            self.pending_search = None;
            self.loaded_generation = Some(state.generation);
            self.viewed_search = None;
            self.history_return_draft = None;
            self.latest_search_id = None;
        }
        if self.published_settings.as_ref() != Some(&state.settings) {
            let selected_name = self
                .selected_preset
                .and_then(|index| self.settings.presets.get(index))
                .map(|preset| preset.name.clone());
            let managed_name = self
                .managed_preset
                .and_then(|index| self.settings.presets.get(index))
                .map(|preset| preset.name.clone());
            if let Some(previous) = self.published_settings.as_ref() {
                reconcile_settings(&mut self.settings, previous, &state.settings);
            } else {
                self.settings = state.settings.clone();
            }
            self.published_settings = Some(state.settings.clone());
            self.selected_preset = selected_name.and_then(|name| {
                self.settings
                    .presets
                    .iter()
                    .position(|preset| preset.name == name)
            });
            self.managed_preset = managed_name.and_then(|name| {
                self.settings
                    .presets
                    .iter()
                    .position(|preset| preset.name == name)
            });
        }
        if self
            .pending_search
            .is_some_and(|id| state.last_command >= id)
        {
            self.pending_search = None;
        }
        let latest_search_id = state.search_history.last().map(|record| record.progress.id);
        if latest_search_id != self.latest_search_id {
            self.latest_search_id = latest_search_id;
            self.viewed_search = None;
            self.history_return_draft = None;
            self.settings.search = state.settings.search.clone();
            self.settings.workers = state.settings.workers;
            self.settings.frame_budget_ms = state.settings.frame_budget_ms;
            self.settings.keep_game_running = state.settings.keep_game_running;
        }
        if self.viewed_search.is_some_and(|id| {
            !state
                .search_history
                .iter()
                .any(|record| record.progress.id == id)
        }) {
            self.viewed_search = None;
        }
        self.consume_lifecycle_events();
        self.notifications.update(
            self.started.elapsed(),
            &self.settings.interface.notifications,
        );
        previous_generation != self.loaded_generation
            || settings_changed
            || previous_latest_search != self.latest_search_id
            || previous_viewed_search != self.viewed_search
            || previous_lifecycle != self.lifecycle_sequence
            || previous_toasts != self.notifications.len()
    }
    fn content_ui(&mut self, ui: &mut Ui) -> Option<f32> {
        self.apply_theme(ui.ctx());
        let palette = self.palette();
        self.capture_keyboard(ui.ctx());
        let content_rect = ui.available_rect_before_wrap();
        let compact_sidebar = ui.available_width() < 700.0;
        if compact_sidebar && self.settings_sidebar {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, palette.surface);
        }
        if compact_sidebar && self.settings_sidebar {
            egui::Frame::new()
                .fill(palette.surface)
                .inner_margin(12.0)
                .show(ui, |ui| self.settings_contents(ui));
            return None;
        }
        if self.settings_sidebar {
            ui.set_max_width((content_rect.width() - SETTINGS_SIDEBAR_WIDTH).max(1.0));
        }
        egui::Frame::new()
            .inner_margin(16.0)
            .fill(palette.background)
            .show(ui, |ui| {
                if ui.available_width() >= 700.0 {
                    let (_, header) = ui.allocate_space(egui::vec2(ui.available_width(), 44.0));
                    let mut left_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("header_title")
                            .max_rect(header)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    left_ui.heading(RichText::new("Search Probes").size(28.0));
                    self.history_navigation(ui, header);
                    let mut right_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("header_settings")
                            .max_rect(header)
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    self.settings_toggle(&mut right_ui);
                } else {
                    ui.horizontal(|ui| {
                        ui.heading(RichText::new("Search Probes").size(28.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.settings_toggle(ui)
                        });
                    });
                    let (_, history) = ui.allocate_space(egui::vec2(ui.available_width(), 44.0));
                    self.history_navigation(ui, history);
                }
                let modal_handles_ui_error =
                    self.preset_dialog.is_some() || self.theme_editor.is_some();
                let error = self.state.error.as_ref().or_else(|| {
                    (!modal_handles_ui_error)
                        .then_some(self.error.as_ref())
                        .flatten()
                });
                if let Some(error) = error {
                    error_callout(ui, palette, error);
                }

                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add_enabled_ui(!self.viewing_past_search(), |ui| self.search_controls(ui));
                });
                ui.add_space(10.0);

                if ui.available_width() >= MAIN_SPLIT_MIN_WIDTH {
                    let results_frame = section_frame(ui.style());
                    ui.columns(2, |columns| {
                        egui::ScrollArea::vertical()
                            .id_salt("criteria")
                            .show(&mut columns[0], |ui| self.criteria_panel(ui));
                        results_frame.show(&mut columns[1], |ui| {
                            ui.set_width(ui.available_width());
                            self.results(ui);
                        });
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("main_content")
                        .show(ui, |ui| {
                            self.criteria_panel(ui);
                            ui.separator();
                            section_frame(ui.style()).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                self.results(ui);
                            });
                        });
                }
            });
        self.settings_sidebar.then_some(content_rect.top())
    }

    fn settings_sidebar_rect(window_rect: egui::Rect, content_top: f32) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(window_rect.right() - SETTINGS_SIDEBAR_WIDTH, content_top),
            window_rect.max,
        )
    }

    fn show_settings_sidebar(
        &mut self,
        ctx: &egui::Context,
        parent_layer: egui::LayerId,
        window_rect: egui::Rect,
        content_top: f32,
    ) -> egui::Rect {
        let palette = self.palette();
        let sidebar_rect = Self::settings_sidebar_rect(window_rect, content_top);
        let sidebar_id = egui::Id::new(SETTINGS_SIDEBAR_LAYER);
        let sidebar_layer = egui::LayerId::new(parent_layer.order, sidebar_id);
        // Resizing or focusing an egui Window raises its Area within Order::Middle. Keep the
        // separately rendered sidebar attached to that Window so its reserved blank region can
        // never paint over the sidebar contents after the Window moves to the top.
        ctx.memory_mut(|memory| {
            memory.areas_mut().set_sublayer(parent_layer, sidebar_layer);
        });
        egui::Area::new(sidebar_id)
            .order(parent_layer.order)
            .fixed_pos(sidebar_rect.min)
            .movable(false)
            .show(ctx, |ui| {
                ui.set_min_size(sidebar_rect.size());
                ui.set_max_size(sidebar_rect.size());
                ui.set_clip_rect(sidebar_rect);
                ui.painter().rect_filled(sidebar_rect, 0.0, palette.surface);
                ui.painter().rect_stroke(
                    sidebar_rect,
                    0.0,
                    egui::Stroke::new(1.0, palette.outline),
                    egui::StrokeKind::Inside,
                );
                let inner = sidebar_rect.shrink(12.0);
                let mut sidebar_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("search_probes_settings")
                        .max_rect(inner)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                sidebar_ui.set_clip_rect(inner);
                self.settings_contents(&mut sidebar_ui);
            });
        sidebar_rect
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.preset_dialog(ctx);
        self.theme_editor_dialog(ctx);
    }

    /// Render into a standalone root UI, as used by the desktop preview.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let parent_layer = ui.layer_id();
        let window_rect = ui.max_rect();
        if let Some(content_top) = self.content_ui(ui) {
            self.show_settings_sidebar(&ctx, parent_layer, window_rect, content_top);
        }
        self.dialogs(&ctx);
    }

    /// Render the window-owned content and defer the wide settings sidebar until the caller knows
    /// the final outer window rectangle.
    pub fn window_ui(&mut self, ui: &mut Ui) -> Option<f32> {
        self.content_ui(ui)
    }

    /// Finish a window render using its final rectangle, so the sidebar can never inherit the
    /// viewport-sized pre-layout rectangle offered while the window contents are being measured.
    pub fn finish_window_ui(
        &mut self,
        ctx: &egui::Context,
        parent_layer: egui::LayerId,
        window_rect: egui::Rect,
        content_top: Option<f32>,
    ) -> Option<egui::Rect> {
        let sidebar_rect = content_top.map(|content_top| {
            self.show_settings_sidebar(ctx, parent_layer, window_rect, content_top)
        });
        self.dialogs(ctx);
        sidebar_rect
    }
}

fn reconcile_settings(draft: &mut Settings, previous: &Settings, published: &Settings) {
    if draft.search == previous.search {
        draft.search = published.search.clone();
    }
    if draft.presets == previous.presets {
        draft.presets = published.presets.clone();
    }
    if draft.interface.custom_themes == previous.interface.custom_themes {
        draft.interface.custom_themes = published.interface.custom_themes.clone();
    }
    if draft.interface.theme == previous.interface.theme {
        draft.interface.theme = published.interface.theme.clone();
    }
    if draft.interface.hotkey == previous.interface.hotkey {
        draft.interface.hotkey = published.interface.hotkey;
    }
    if draft.interface.notifications == previous.interface.notifications {
        draft.interface.notifications = published.interface.notifications.clone();
    }
    if draft.interface.result_cards == previous.interface.result_cards {
        draft.interface.result_cards = published.interface.result_cards;
    }
    if draft.workers == previous.workers {
        draft.workers = published.workers;
    }
    if draft.frame_budget_ms == previous.frame_budget_ms {
        draft.frame_budget_ms = published.frame_budget_ms;
    }
    if draft.keep_game_running == previous.keep_game_running {
        draft.keep_game_running = published.keep_game_running;
    }
}

const FILTER_CONTROL_WIDTH: f32 = 185.0;
const FILTER_CONTROL_MIN_WIDTH: f32 = 64.0;
const FILTER_LABEL_MIN_WIDTH: f32 = 72.0;
const FILTER_ROW_MIN_GAP: f32 = 12.0;
const CALLOUT_MAX_WIDTH: f32 = 720.0;

enum CalloutTone {
    Success,
    Warning,
    Error,
}

fn sidebar_separator(ui: &mut Ui) {
    ui.add(egui::Separator::default().grow(12.0));
}

fn binding_caps(ui: &mut Ui, binding: InputBinding, palette: Palette) {
    match binding {
        InputBinding::Keyboard { first, second } => {
            if let Some(second) = second {
                binding_cap_group(ui, &[first.label(), second.label()], palette);
            } else {
                binding_cap_group(ui, &[first.label()], palette);
            }
        }
        InputBinding::Controller { first, second } => {
            if let Some(second) = second {
                binding_cap_group(ui, &[first.label(), second.label()], palette);
            } else {
                binding_cap_group(ui, &[first.label()], palette);
            }
        }
    }
}

fn captured_caps(ui: &mut Ui, inputs: &[CapturedInput], palette: Palette) {
    let labels: Vec<_> = inputs.iter().map(|input| input.label()).collect();
    binding_cap_group(ui, &labels, palette);
}

fn binding_cap_group(ui: &mut Ui, labels: &[&str], palette: Palette) {
    let total_width = ui.available_width();
    let gap = ui.spacing().item_spacing.x;
    let cap_width = if labels.len() == 2 {
        (total_width - 14.0 - gap * 2.0) / 2.0
    } else {
        total_width
    };
    ui.horizontal(|ui| {
        for (index, label) in labels.iter().enumerate() {
            if index > 0 {
                input_separator(ui);
            }
            input_cap(ui, label, cap_width, palette);
        }
    });
}

fn input_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 30.0), egui::Sense::hover());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "+",
        egui::FontId::proportional(16.0),
        ui.visuals().text_color(),
    );
}

fn input_cap(ui: &mut Ui, label: &str, width: f32, palette: Palette) {
    egui::Frame::new()
        .fill(palette.raised_surface)
        .stroke(egui::Stroke::new(1.0, palette.outline))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_width((width - 16.0).max(1.0));
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(label).size(16.0).strong());
            });
        });
}

fn keyboard_key(key: egui::Key) -> Option<KeyboardKey> {
    use egui::Key;
    Some(match key {
        Key::ArrowDown => KeyboardKey::ArrowDown,
        Key::ArrowLeft => KeyboardKey::ArrowLeft,
        Key::ArrowRight => KeyboardKey::ArrowRight,
        Key::ArrowUp => KeyboardKey::ArrowUp,
        Key::Escape => KeyboardKey::Escape,
        Key::Tab => KeyboardKey::Tab,
        Key::Backspace => KeyboardKey::Backspace,
        Key::Enter => KeyboardKey::Enter,
        Key::Space => KeyboardKey::Space,
        Key::Insert => KeyboardKey::Insert,
        Key::Delete => KeyboardKey::Delete,
        Key::Home => KeyboardKey::Home,
        Key::End => KeyboardKey::End,
        Key::PageUp => KeyboardKey::PageUp,
        Key::PageDown => KeyboardKey::PageDown,
        Key::Colon => KeyboardKey::Semicolon,
        Key::Comma => KeyboardKey::Comma,
        Key::Backslash => KeyboardKey::Backslash,
        Key::Slash => KeyboardKey::Slash,
        Key::Pipe => KeyboardKey::Backslash,
        Key::Questionmark => KeyboardKey::Slash,
        Key::Exclamationmark => KeyboardKey::Num1,
        Key::OpenBracket => KeyboardKey::OpenBracket,
        Key::CloseBracket => KeyboardKey::CloseBracket,
        Key::OpenCurlyBracket => KeyboardKey::OpenBracket,
        Key::CloseCurlyBracket => KeyboardKey::CloseBracket,
        Key::Backtick => KeyboardKey::Backtick,
        Key::Minus => KeyboardKey::Minus,
        Key::Period => KeyboardKey::Period,
        Key::Plus => KeyboardKey::Equals,
        Key::Equals => KeyboardKey::Equals,
        Key::Semicolon => KeyboardKey::Semicolon,
        Key::Quote => KeyboardKey::Quote,
        Key::Num0 => KeyboardKey::Num0,
        Key::Num1 => KeyboardKey::Num1,
        Key::Num2 => KeyboardKey::Num2,
        Key::Num3 => KeyboardKey::Num3,
        Key::Num4 => KeyboardKey::Num4,
        Key::Num5 => KeyboardKey::Num5,
        Key::Num6 => KeyboardKey::Num6,
        Key::Num7 => KeyboardKey::Num7,
        Key::Num8 => KeyboardKey::Num8,
        Key::Num9 => KeyboardKey::Num9,
        Key::A => KeyboardKey::A,
        Key::B => KeyboardKey::B,
        Key::C => KeyboardKey::C,
        Key::D => KeyboardKey::D,
        Key::E => KeyboardKey::E,
        Key::F => KeyboardKey::F,
        Key::G => KeyboardKey::G,
        Key::H => KeyboardKey::H,
        Key::I => KeyboardKey::I,
        Key::J => KeyboardKey::J,
        Key::K => KeyboardKey::K,
        Key::L => KeyboardKey::L,
        Key::M => KeyboardKey::M,
        Key::N => KeyboardKey::N,
        Key::O => KeyboardKey::O,
        Key::P => KeyboardKey::P,
        Key::Q => KeyboardKey::Q,
        Key::R => KeyboardKey::R,
        Key::S => KeyboardKey::S,
        Key::T => KeyboardKey::T,
        Key::U => KeyboardKey::U,
        Key::V => KeyboardKey::V,
        Key::W => KeyboardKey::W,
        Key::X => KeyboardKey::X,
        Key::Y => KeyboardKey::Y,
        Key::Z => KeyboardKey::Z,
        Key::F1 => KeyboardKey::F1,
        Key::F2 => KeyboardKey::F2,
        Key::F3 => KeyboardKey::F3,
        Key::F4 => KeyboardKey::F4,
        Key::F5 => KeyboardKey::F5,
        Key::F6 => KeyboardKey::F6,
        Key::F7 => KeyboardKey::F7,
        Key::F8 => KeyboardKey::F8,
        Key::F9 => KeyboardKey::F9,
        Key::F10 => KeyboardKey::F10,
        Key::F11 => KeyboardKey::F11,
        Key::F12 => KeyboardKey::F12,
        Key::F13 => KeyboardKey::F13,
        Key::F14 => KeyboardKey::F14,
        Key::F15 => KeyboardKey::F15,
        Key::F16 => KeyboardKey::F16,
        Key::F17 => KeyboardKey::F17,
        Key::F18 => KeyboardKey::F18,
        Key::F19 => KeyboardKey::F19,
        Key::F20 => KeyboardKey::F20,
        Key::F21 => KeyboardKey::F21,
        Key::F22 => KeyboardKey::F22,
        Key::F23 => KeyboardKey::F23,
        Key::F24 => KeyboardKey::F24,
        Key::ShiftLeft => KeyboardKey::ShiftLeft,
        Key::ShiftRight => KeyboardKey::ShiftRight,
        Key::ControlLeft => KeyboardKey::ControlLeft,
        Key::ControlRight => KeyboardKey::ControlRight,
        Key::AltLeft => KeyboardKey::AltLeft,
        Key::AltRight => KeyboardKey::AltRight,
        Key::SuperLeft => KeyboardKey::SuperLeft,
        Key::SuperRight => KeyboardKey::SuperRight,
        Key::IntlBackslash => KeyboardKey::IntlBackslash,
        _ => return None,
    })
}

fn callout(ui: &mut Ui, palette: Palette, tone: CalloutTone, title: &str, message: &str) {
    let (fill, border, title_color) = match tone {
        CalloutTone::Success => (
            palette.success_container,
            palette.success_outline,
            palette.success,
        ),
        CalloutTone::Warning => (
            palette.warning_container,
            palette.warning_outline,
            palette.warning,
        ),
        CalloutTone::Error => {
            let (container, foreground) = palette.error_callout();
            (container, foreground, foreground)
        }
    };
    let width = ui.available_width().min(CALLOUT_MAX_WIDTH);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            egui::Frame::new()
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, border))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).strong().color(title_color));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            callout_icon(ui, tone, title_color);
                        });
                    });
                    if !message.is_empty() {
                        ui.label(message);
                    }
                });
        },
    );
}

fn error_callout(ui: &mut Ui, palette: Palette, message: &str) {
    callout(ui, palette, CalloutTone::Error, message, "");
}

fn callout_icon(ui: &mut Ui, tone: CalloutTone, color: egui::Color32) {
    let (uri, bytes): (&str, &'static [u8]) = match tone {
        CalloutTone::Success => (
            "bytes://search-probes/icons/info.svg",
            include_bytes!("../assets/icons/info.svg"),
        ),
        CalloutTone::Warning => (
            "bytes://search-probes/icons/warning.svg",
            include_bytes!("../assets/icons/warning.svg"),
        ),
        CalloutTone::Error => (
            "bytes://search-probes/icons/error.svg",
            include_bytes!("../assets/icons/error.svg"),
        ),
    };
    ui.add(
        egui::Image::from_bytes(uri, bytes)
            .fit_to_exact_size(egui::vec2(18.0, 18.0))
            .tint(color),
    );
}

fn animated_visibility(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    visible: bool,
    add_contents: impl FnOnce(&mut Ui),
) {
    let id = ui.make_persistent_id(id);
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, visible);
    state.set_open(visible);
    state.show_body_unindented(ui, add_contents);
}

fn filter_control_width(ui: &Ui) -> f32 {
    let nested_indents = 2.0 * ui.spacing().indent;
    (ui.available_width() - nested_indents - FILTER_ROW_MIN_GAP - FILTER_LABEL_MIN_WIDTH)
        .clamp(FILTER_CONTROL_MIN_WIDTH, FILTER_CONTROL_WIDTH)
}

fn filter_row(ui: &mut Ui, label: &str, control_width: f32, add_control: impl FnOnce(&mut Ui)) {
    let row_width = ui.available_width();
    let max_label_width = (row_width - control_width - FILTER_ROW_MIN_GAP).max(1.0);
    let font_id = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let color = ui.visuals().text_color();
    let label_width = max_label_width;
    let label_height = ui
        .painter()
        .layout(label.to_owned(), font_id, color, label_width)
        .size()
        .y
        .max(ui.spacing().interact_size.y);
    let row_height = label_height.max(ui.spacing().interact_size.y);

    let (rect, _) = ui.allocate_exact_size(egui::vec2(row_width, row_height), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let label_rect = egui::Rect::from_min_size(rect.min, egui::vec2(label_width, row_height));
    let control_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - control_width, rect.top()),
        rect.max,
    );
    let mut label_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(label_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let response = label_ui.add(egui::Label::new(label).wrap());
    let tooltip = if label == Field::Terrain.label() {
        Some("These features can occupy part of the planet; you may need to explore to find them.")
    } else if label == Field::Subtype.label() {
        Some(
            "Vegetation and formations associated with the selected biome. Explore the planet to find them.",
        )
    } else if label == Field::Grass.label() {
        Some(
            "Main grass colour in daylight; secondary patches do not qualify. Night, storms and nearby lights can change the colour you see.",
        )
    } else if label == Field::FloraColour.label() {
        Some(
            "Colours present on surface flora in daylight; different parts and species can have different colours.",
        )
    } else if label == Field::WaterColour.label() {
        Some(
            "Derived from the generated water's scattering and transmittance in clear daylight. Reflections, depth, foam and nearby lights can change its appearance.",
        )
    } else if label == Field::Sky.label() {
        Some(
            "Representative clear-day upper sky, evaluated midway through the game's upper-to-main sky gradient and away from the sun and horizon. The visible result still varies across the dome and with fog, weather and exposure.",
        )
    } else if label == Field::Horizon.label() {
        Some(
            "Generated atmospheric horizon tint. The visible lower sky also blends with the base sky, fog, sun direction and exposure.",
        )
    } else {
        None
    };
    if let Some(tooltip) = tooltip {
        response.on_hover_text(tooltip);
    }
    let mut control_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(control_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add_control(&mut control_ui);
}

fn choices(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    mask: &mut u64,
    options: impl Iterator<Item = (usize, &'static str)> + Clone,
    width: f32,
) {
    let available_mask = options
        .clone()
        .fold(0u64, |mask, (index, _)| mask | (1 << index));
    let text = if *mask & !available_mask != 0 {
        "Unavailable selection".to_owned()
    } else if *mask == 0 {
        "Any".to_owned()
    } else if mask.count_ones() == 1 {
        options
            .clone()
            .find_map(|(index, label)| (index == mask.trailing_zeros() as usize).then_some(label))
            .unwrap_or("Unknown")
            .to_string()
    } else {
        format!("{} selected", mask.count_ones())
    };
    let option_count = options.clone().count() + 1;
    select_menu(
        ui,
        id,
        text,
        width,
        option_count,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            if *mask & !available_mask != 0 {
                ui.label("Choose Any or a listed option to replace the unavailable selection.");
            }
            if ui.selectable_label(*mask == 0, "Any").clicked() {
                *mask = 0;
            }
            for (index, label) in options {
                let mut selected = *mask & (1 << index) != 0;
                if ui.checkbox(&mut selected, label).changed() {
                    *mask &= available_mask;
                    if selected {
                        *mask |= 1 << index;
                    } else {
                        *mask &= !(1 << index);
                    }
                }
            }
        },
    );
}

fn requirement(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<bool>,
    width: f32,
) {
    select_menu(
        ui,
        id,
        match value {
            None => "Any",
            Some(true) => "Require",
            Some(false) => "Exclude",
        },
        width,
        3,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.selectable_value(value, None, "Any");
            ui.selectable_value(value, Some(true), "Require");
            ui.selectable_value(value, Some(false), "Exclude");
        },
    );
}

fn choice_rows(ui: &mut Ui, criteria: &mut Criteria, fields: &[Field], input_width: f32) {
    for &field in fields {
        choice_row(ui, criteria, field, input_width);
    }
}

fn choice_row(ui: &mut Ui, criteria: &mut Criteria, field: Field, input_width: f32) {
    if field == Field::Subtype {
        subtype_row(ui, criteria, input_width);
        return;
    }
    if field == Field::Size
        && let Some(size) = criteria.derived_planet_size()
    {
        let mut mask = 1 << size;
        filter_row(ui, field.label(), input_width, |ui| {
            ui.add_enabled_ui(false, |ui| {
                choices(
                    ui,
                    ("choice", field),
                    &mut mask,
                    criteria.choice_options(field).into_iter(),
                    input_width,
                );
            });
        });
        return;
    }
    let original = if field == Field::Biome {
        criteria
            .biome_choices()
            .unwrap_or(1 << field.options().len())
    } else {
        criteria.choices.get(&field).copied().unwrap_or(0)
    };
    let mut mask = original;
    let options = criteria.choice_options(field);
    filter_row(ui, field.label(), input_width, |ui| {
        ui.add_enabled_ui(!options.is_empty(), |ui| {
            choices(
                ui,
                ("choice", field),
                &mut mask,
                options.into_iter(),
                input_width,
            );
        });
    });
    if mask != original {
        if field == Field::Biome {
            criteria.set_biome_choices(mask);
        } else if mask == 0 {
            criteria.choices.remove(&field);
        } else {
            criteria.choices.insert(field, mask);
        }
    }
}

fn subtype_row(ui: &mut Ui, criteria: &mut Criteria, input_width: f32) {
    let options = criteria.subtype_options();
    let original = criteria.choices.get(&Field::Subtype).copied().unwrap_or(0);
    let mut selected = original;
    subtype_choices(
        ui,
        "biome_subtype",
        Field::Subtype.label(),
        criteria.biome_choices().unwrap_or(0),
        &options,
        &mut selected,
        input_width,
    );
    if selected != original {
        if selected == 0 {
            criteria.choices.remove(&Field::Subtype);
        } else {
            criteria.choices.insert(Field::Subtype, selected);
        }
    }
}

fn subtype_choices(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    biomes: u64,
    options: &[(u64, &'static str)],
    selected: &mut u64,
    input_width: f32,
) {
    let available = options.iter().fold(0u64, |mask, (option, _)| mask | option);
    let text = if *selected & !available != 0 {
        "Unavailable selection".to_owned()
    } else if *selected == 0 {
        "Any".to_owned()
    } else {
        options
            .iter()
            .filter(|(mask, _)| *selected & mask != 0)
            .map(|(_, label)| *label)
            .collect::<Vec<_>>()
            .join(", ")
    };
    filter_row(ui, label, input_width, |ui| {
        select_menu(
            ui,
            id,
            text,
            input_width,
            options.len() + 1,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                if ui.selectable_label(*selected == 0, "Any").clicked() {
                    *selected = 0;
                }
                for &(mask, label) in options {
                    let mut checked = *selected & mask != 0;
                    if ui.checkbox(&mut checked, label).changed() {
                        if checked {
                            *selected |= mask;
                        } else {
                            *selected &= !mask;
                        }
                    }
                }
                if biomes == 0 {
                    ui.weak("Choose a biome to see its vegetation and formations.");
                }
            },
        );
    });
}

fn arena_criteria(ui: &mut Ui, criteria: &mut Criteria, width: f32) {
    criteria.arena.enforce_dependencies();
    ui.add_enabled_ui(criteria.arena.presence != Some(false), |ui| {
        for &field in &[
            ArenaField::Affinity,
            ArenaField::Agility,
            ArenaField::Combat,
            ArenaField::Health,
            ArenaField::OverallRank,
        ] {
            let mut mask = criteria.arena.choices.get(&field).copied().unwrap_or(0);
            filter_row(ui, field.label(), width, |ui| {
                choices(
                    ui,
                    ("arena", field),
                    &mut mask,
                    criteria.arena.choice_options(field).into_iter(),
                    width,
                )
            });
            if mask == 0 {
                criteria.arena.choices.remove(&field);
            } else {
                criteria.arena.choices.insert(field, mask);
            }
        }
    });
    criteria.arena.enforce_dependencies();
}

fn tameable_fauna_row(ui: &mut Ui, criteria: &mut Criteria, width: f32) {
    criteria.arena.enforce_dependencies();
    let details_active = criteria.arena.has_detail_filters();
    filter_row(ui, "Has tameable fauna", width, |ui| {
        ui.add_enabled_ui(!details_active, |ui| {
            requirement(ui, "arena_presence", &mut criteria.arena.presence, width);
        });
    });
    criteria.arena.enforce_dependencies();
}

fn fauna_domain_section(ui: &mut Ui, criteria: &mut Criteria, domain: Domain, width: f32) {
    egui::CollapsingHeader::new(RichText::new(domain.label()).size(16.0).strong())
        .id_salt(("fauna_criteria_domain", domain))
        .default_open(true)
        .show(ui, |ui| {
            let enabled = criteria.fauna_domain_available(domain);
            ui.add_enabled_ui(enabled, |ui| fauna_domain(ui, criteria, domain, width));
        });
}

fn fauna_domain(ui: &mut Ui, criteria: &mut Criteria, domain: Domain, width: f32) {
    let mut selected = criteria.fauna.get(&domain).cloned().unwrap_or_default();
    selected.enforce_domain_dependencies(domain);
    let details_active = selected.has_detail_filters();
    filter_row(ui, domain.presence_label(), width, |ui| {
        ui.add_enabled_ui(!details_active, |ui| {
            requirement(
                ui,
                ("fauna_presence", domain),
                &mut selected.presence,
                width,
            );
        });
    });
    selected.enforce_domain_dependencies(domain);
    ui.add_enabled_ui(selected.presence != Some(false), |ui| {
        for &field in &[
            FaunaField::Category,
            FaunaField::Rarity,
            FaunaField::Activity,
            FaunaField::Habitat,
            FaunaField::Hemisphere,
            FaunaField::Family,
        ] {
            fauna_field_row(ui, domain, field, &mut selected, width);
        }
        height_bound_row(
            ui,
            (domain, "maximum"),
            "Maximum height (m)",
            &mut selected.height.maximum_cm,
            width,
            100_000,
        );
        height_bound_row(
            ui,
            (domain, "minimum"),
            "Minimum height (m)",
            &mut selected.height.minimum_cm,
            width,
            0,
        );
    });
    selected.enforce_domain_dependencies(domain);
    if selected.presence.is_none() && !selected.has_detail_filters() {
        criteria.fauna.remove(&domain);
    } else {
        criteria.fauna.insert(domain, selected);
    }
}

fn fauna_field_row(
    ui: &mut Ui,
    domain: Domain,
    field: FaunaField,
    selected: &mut FaunaCriteria,
    width: f32,
) {
    let options = selected.choice_options(domain, field);
    if options.is_empty() {
        return;
    }
    let mut mask = selected.choices.get(&field).copied().unwrap_or(0);
    filter_row(ui, field.label(), width, |ui| {
        choices(
            ui,
            ("fauna", domain, field),
            &mut mask,
            options.into_iter(),
            width,
        )
    });
    if mask == 0 {
        selected.choices.remove(&field);
    } else {
        selected.choices.insert(field, mask);
    }
}
fn height_bound_row(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    bound: &mut Option<u32>,
    width: f32,
    initial: u32,
) {
    filter_row(ui, label, width, |ui| {
        ui.push_id(id, |ui| {
            ui.horizontal(|ui| {
                let mut any = bound.is_none();
                if ui.checkbox(&mut any, "Any").changed() {
                    *bound = if any { None } else { Some(initial) };
                }
                let mut metres = bound.unwrap_or(initial) as f64 / 100.;
                if ui
                    .add_enabled(
                        !any,
                        egui::DragValue::new(&mut metres)
                            .range(0.0..=1000.0)
                            .speed(0.1)
                            .max_decimals(2),
                    )
                    .changed()
                {
                    *bound = Some((metres * 100.).round() as u32);
                }
            })
        });
    });
}

fn water_row(
    ui: &mut Ui,
    id: &str,
    water: &mut WaterFilter,
    input_width: f32,
    available: impl Fn(WaterFilter) -> bool,
) {
    filter_row(ui, "Has water", input_width, |ui| {
        select_menu(
            ui,
            id,
            water.label(),
            input_width,
            WaterFilter::ALL.len(),
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                for &value in WaterFilter::ALL {
                    ui.add_enabled_ui(available(value), |ui| {
                        ui.selectable_value(water, value, value.label());
                    });
                }
            },
        );
    });
}

const COUNT_LABELS: [&str; 6] = ["1", "2", "3", "4", "5", "6"];

fn count_choices_row(
    ui: &mut Ui,
    label: &str,
    id: &str,
    value: &mut u8,
    maximum: u8,
    input_width: f32,
) {
    filter_row(ui, label, input_width, |ui| {
        let mut mask = u64::from(*value);
        choices(
            ui,
            id,
            &mut mask,
            COUNT_LABELS[..usize::from(maximum)]
                .iter()
                .copied()
                .enumerate(),
            input_width,
        );
        *value = mask as u8;
    });
}

fn composition_row(
    ui: &mut Ui,
    criteria: &mut Criteria,
    field: CompositionField,
    side: SelectionSide,
    label: &str,
    input_width: f32,
) {
    let original = criteria.composition.selections(field).get(side);
    let mut selected = original;
    let id = ("composition", field, side);
    if field == CompositionField::Subtypes {
        let biomes = criteria.composition.biomes.get(side);
        let options = subtype_options_for(biomes);
        subtype_choices(ui, id, label, biomes, &options, &mut selected, input_width);
    } else {
        let options = if field == CompositionField::Biomes {
            Field::Biome
        } else {
            Field::Size
        };
        filter_row(ui, label, input_width, |ui| {
            choices(
                ui,
                id,
                &mut selected,
                options.available_options(),
                input_width,
            )
        });
    }
    if selected != original {
        criteria.composition.set(field, side, selected);
    }
}

fn planet_flag_rows(ui: &mut Ui, criteria: &mut Criteria, flags: &[PlanetFlag], input_width: f32) {
    for &flag in flags {
        let mut value = criteria.planets.get(&flag).copied();
        filter_row(ui, flag.label(), input_width, |ui| {
            ui.add_enabled_ui(criteria.planet_requirement_available(flag), |ui| {
                requirement(ui, ("planet", flag), &mut value, input_width);
            });
        });
        if let Some(value) = value {
            criteria.planets.insert(flag, value);
        } else {
            criteria.planets.remove(&flag);
        }
    }
}

fn system_flag_rows(ui: &mut Ui, criteria: &mut Criteria, flags: &[SystemFlag], input_width: f32) {
    for &flag in flags {
        let mut value = criteria.systems.get(&flag).copied();
        filter_row(ui, flag.label(), input_width, |ui| {
            requirement(ui, ("system", flag), &mut value, input_width);
        });
        if let Some(value) = value {
            criteria.systems.insert(flag, value);
        } else {
            criteria.systems.remove(&flag);
        }
    }
}

fn sentinel_row(ui: &mut Ui, criteria: &mut Criteria, input_width: f32) {
    let original = criteria.sentinel_choices().unwrap_or(1 << 4);
    let mut selected = original;
    filter_row(ui, "Sentinels", input_width, |ui| {
        choices(
            ui,
            "sentinels",
            &mut selected,
            criteria.choice_options(Field::Sentinels).into_iter(),
            input_width,
        );
    });
    if selected != original {
        criteria.set_sentinel_choices(selected);
    }
}

fn notification_progress(event: &LifecycleEvent, search: &SearchOptions) -> SearchProgress {
    let plan = search.criteria.compile().ok();
    let includes_planets = plan
        .as_ref()
        .is_some_and(|plan| !plan.planet.is_empty() || plan.requirements.planet_query);
    let includes_fauna = plan.as_ref().is_some_and(|plan| plan.requirements.fauna);
    SearchProgress {
        systems: event.systems,
        system_limit: search.systems,
        planets: includes_planets.then_some(event.planets),
        fauna: includes_fauna.then_some(event.fauna),
        results: event.results,
        result_limit: search.results,
        elapsed_ms: event.elapsed_ms,
    }
}

fn number(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

fn elapsed(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    let hours = seconds / 3_600;
    let minutes = seconds / 60 % 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_probes_model::settings::ResultCardSettings;

    #[test]
    fn elapsed_time_is_stable_beyond_one_hour() {
        assert_eq!(elapsed(0), "00:00:00");
        assert_eq!(elapsed(3_661_999), "01:01:01");
        assert_eq!(elapsed(359_940_000), "99:59:00");
    }

    #[test]
    fn notification_progress_includes_only_scopes_used_by_the_search() {
        let event = LifecycleEvent {
            sequence: 1,
            published_ms: 0,
            search_id: 1,
            elapsed_ms: 83_000,
            systems: 2_400,
            planets: 13_842,
            fauna: 6_125,
            results: 4,
            kind: LifecycleEventKind::Started,
        };

        let system_search = SearchOptions::default();
        let progress = notification_progress(&event, &system_search);
        assert_eq!(progress.planets, None);
        assert_eq!(progress.fauna, None);

        let mut planet_search = SearchOptions::default();
        planet_search.criteria.choices.insert(Field::Biome, 1);
        let progress = notification_progress(&event, &planet_search);
        assert_eq!(progress.planets, Some(13_842));
        assert_eq!(progress.fauna, None);

        let mut fauna_search = SearchOptions::default();
        fauna_search.criteria.fauna.insert(
            Domain::Land,
            FaunaCriteria {
                presence: Some(true),
                ..FaunaCriteria::default()
            },
        );
        let progress = notification_progress(&event, &fauna_search);
        assert_eq!(progress.planets, Some(13_842));
        assert_eq!(progress.fauna, Some(6_125));
    }

    fn rendered_texts(app: &mut App, context: &egui::Context, size: egui::Vec2) -> Vec<String> {
        fn collect(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => texts.push(text.galley.job.text.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, texts);
                    }
                }
                _ => {}
            }
        }
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut output = context.run_ui(raw, |ui| app.ui(ui));
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut texts);
        }
        texts
    }

    fn rendered_window_geometry(
        app: &mut App,
        context: &egui::Context,
        size: egui::Vec2,
    ) -> (egui::Rect, Option<egui::Rect>) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut window_rect = egui::Rect::NOTHING;
        let mut sidebar_rect = None;
        let mut output = context.run_ui(raw, |root| {
            let response = egui::Window::new("Search Probes")
                .id(egui::Id::new("search_probes_overlay_test"))
                .default_size([1100.0, 840.0])
                .min_size([380.0, 240.0])
                .resizable(true)
                .collapsible(false)
                .frame(egui::Frame::window(root.style()).inner_margin(0.0))
                .show(root.ctx(), |ui| app.window_ui(ui))
                .expect("overlay window");
            window_rect = response.response.rect;
            sidebar_rect = app.finish_window_ui(
                root.ctx(),
                response.response.layer_id,
                window_rect,
                response.inner.flatten(),
            );
        });
        output.textures_delta.clear();
        (window_rect, sidebar_rect)
    }

    #[test]
    fn settings_sidebar_does_not_resize_the_overlay_window() {
        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let mut app = App::new(&context, controls);
        let viewport = egui::vec2(2560.0, 1440.0);
        let mut closed = egui::Rect::NOTHING;
        for _ in 0..3 {
            (closed, _) = rendered_window_geometry(&mut app, &context, viewport);
        }
        app.settings_sidebar = true;
        let mut open = egui::Rect::NOTHING;
        let mut sidebar = None;
        for _ in 0..3 {
            (open, sidebar) = rendered_window_geometry(&mut app, &context, viewport);
        }
        assert!(closed.height() <= 842.0, "closed window grew to {closed:?}");
        assert_eq!(open, closed);
        assert_eq!(sidebar.expect("open sidebar").bottom(), open.bottom());

        app.settings_sidebar = false;
        for _ in 0..2 {
            rendered_window_geometry(&mut app, &context, viewport);
        }
        app.settings_sidebar = true;
        for _ in 0..2 {
            (open, sidebar) = rendered_window_geometry(&mut app, &context, viewport);
        }
        let sidebar = sidebar.expect("reopened sidebar");
        assert_eq!(sidebar.bottom(), open.bottom());
        assert!(sidebar.bottom() < viewport.y);
    }

    #[test]
    fn settings_sidebar_stays_above_its_window_when_the_window_is_raised() {
        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let mut app = App::new(&context, controls);
        app.settings_sidebar = true;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2560.0, 1440.0),
            )),
            ..Default::default()
        };
        let mut output = context.run_ui(raw, |root| {
            let response = egui::Window::new("Search Probes")
                .id(egui::Id::new("raised_search_probes_overlay_test"))
                .default_size([1100.0, 840.0])
                .resizable(true)
                .show(root.ctx(), |ui| app.window_ui(ui))
                .expect("overlay window");
            let window_layer = response.response.layer_id;
            app.finish_window_ui(
                root.ctx(),
                window_layer,
                response.response.rect,
                response.inner.flatten(),
            );
            let sidebar_layer =
                egui::LayerId::new(window_layer.order, egui::Id::new(SETTINGS_SIDEBAR_LAYER));
            assert_eq!(
                root.ctx()
                    .memory(|memory| memory.areas().parent_layer(sidebar_layer)),
                Some(window_layer)
            );
            // Window resizing calls this same operation internally. The declared sublayer keeps
            // the sidebar directly above the window when the new order is applied.
            root.ctx().move_to_top(window_layer);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn settings_sidebar_and_search_history_navigation_render_at_wide_and_narrow_sizes() {
        for width in [380.0, 1100.0] {
            let context = egui::Context::default();
            let controls = Arc::new(Controls::new(State::new(Settings::default())));
            let mut app = App::new(&context, controls);
            app.settings_sidebar = true;
            let texts = rendered_texts(&mut app, &context, egui::vec2(width, 1600.0));
            for title in [
                "Settings",
                "Theme",
                "Preset Manager",
                "UI Hotkey",
                "Notifications",
                "Duration:",
                "seconds",
                "Screen placement:",
                "Result cards",
            ] {
                assert!(
                    texts.iter().any(|text| text == title),
                    "{width}px omitted {title}: {texts:?}"
                );
            }
            app.settings_sidebar = false;
            let texts = rendered_texts(&mut app, &context, egui::vec2(width, 840.0));
            assert!(texts.iter().any(|text| text == "No searches yet"));
            assert!(texts.iter().any(|text| text == "Search Scope"));
            assert!(!texts.iter().any(|text| text == "Presets"));
        }
    }

    #[test]
    fn settings_sidebar_buttons_stay_inside_the_scroll_clip() {
        fn collect(
            shape: &egui::Shape,
            clip: egui::Rect,
            text: &mut Option<(egui::Rect, egui::Rect)>,
            rectangles: &mut Vec<(egui::epaint::RectShape, egui::Rect)>,
        ) {
            match shape {
                egui::Shape::Text(shape) if shape.galley.job.text == "Reset to default" => {
                    *text = Some((shape.visual_bounding_rect(), clip));
                }
                egui::Shape::Rect(shape) => rectangles.push((shape.clone(), clip)),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, text, rectangles);
                    }
                }
                _ => {}
            }
        }

        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let mut app = App::new(&context, controls);
        app.settings_sidebar = true;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 840.0),
            )),
            ..Default::default()
        };
        let mut output = context.run_ui(raw, |ui| app.ui(ui));
        output.textures_delta.clear();
        let mut text = None;
        let mut rectangles = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, shape.clip_rect, &mut text, &mut rectangles);
        }
        let (text, clip) = text.expect("Reset to default text");
        let button = rectangles
            .iter()
            .filter(|(shape, shape_clip)| {
                *shape_clip == clip
                    && shape.stroke.width > 0.0
                    && shape.rect.height() >= 30.0
                    && shape.rect.height() <= 36.0
                    && shape.rect.contains_rect(text)
            })
            .min_by(|(left, _), (right, _)| {
                left.rect.area().partial_cmp(&right.rect.area()).unwrap()
            })
            .map(|(shape, _)| shape)
            .expect("Reset to default button");
        assert!(
            button.rect.right() < clip.right(),
            "button edge {:?} reaches scroll clip {:?}",
            button.rect,
            clip
        );
    }

    #[test]
    fn search_shell_and_save_dialog_render_the_reorganized_preset_controls() {
        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let mut app = App::new(&context, controls);
        let texts = rendered_texts(&mut app, &context, egui::vec2(1100.0, 840.0));
        for label in ["Preset", "Save as preset", "Clear criteria"] {
            assert!(
                texts.iter().any(|text| text == label),
                "missing {label}: {texts:?}"
            );
        }
        assert!(!texts.iter().any(|text| text == "Presets"));

        app.preset_dialog = Some(PresetNameDialog::SaveCurrentSearch);
        let _ = rendered_texts(&mut app, &context, egui::vec2(1100.0, 840.0));
        let texts = rendered_texts(&mut app, &context, egui::vec2(1100.0, 840.0));
        assert!(texts.iter().any(|text| text == "Save as preset"));
        assert!(texts.iter().any(|text| text == "Name this search preset"));
        assert!(texts.iter().any(|text| text == "Save"));
        assert!(texts.iter().any(|text| text == "Cancel"));
    }

    #[test]
    fn preset_manager_actions_create_load_rename_and_remove_searches() {
        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let mut app = App::new(&context, controls);
        let mut overlay = |_| Ok(());

        app.apply_agent_action(
            &context,
            UiAction::SetPresetDialog { open: true },
            &mut overlay,
        )
        .unwrap();
        assert_eq!(
            app.apply_agent_action(
                &context,
                UiAction::SetPresetName {
                    name: "p".repeat(81),
                },
                &mut overlay,
            ),
            Err("Preset names can contain at most 80 characters".into())
        );
        app.apply_agent_action(
            &context,
            UiAction::SetPresetName {
                name: "  Fixture preset  ".into(),
            },
            &mut overlay,
        )
        .unwrap();
        app.apply_agent_action(&context, UiAction::SavePreset {}, &mut overlay)
            .unwrap();
        assert!(app.preset_dialog.is_none());
        assert_eq!(app.settings.presets[0].name, "Fixture preset");

        let saved_search = app.settings.presets[0].search.clone();
        app.settings.search.results = 9;
        app.apply_agent_action(
            &context,
            UiAction::SetRenamePresetDialog { index: Some(0) },
            &mut overlay,
        )
        .unwrap();
        assert_eq!(app.ui_state().renaming_preset, Some(0));
        app.apply_agent_action(
            &context,
            UiAction::SetRenamePresetDialog { index: None },
            &mut overlay,
        )
        .unwrap();
        app.apply_agent_action(
            &context,
            UiAction::RenamePreset {
                index: 0,
                name: "  Renamed fixture  ".into(),
            },
            &mut overlay,
        )
        .unwrap();
        assert_eq!(app.settings.presets[0].name, "Renamed fixture");
        assert_eq!(app.settings.presets[0].search, saved_search);

        app.apply_agent_action(
            &context,
            UiAction::SelectManagedPreset { index: Some(0) },
            &mut overlay,
        )
        .unwrap();
        assert_eq!(app.managed_preset, Some(0));
        assert_eq!(app.managed_preset_name, "Renamed fixture");
        assert_eq!(app.selected_preset, Some(0));
        assert_eq!(app.settings.search, saved_search);

        app.apply_agent_action(&context, UiAction::RemovePreset { index: 0 }, &mut overlay)
            .unwrap();
        assert!(app.settings.presets.is_empty());
        assert_eq!(app.managed_preset, None);
    }

    #[test]
    fn custom_theme_actions_create_select_edit_and_remove_persisted_palettes() {
        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let mut app = App::new(&context, controls);
        let mut overlay = |_| Ok(());
        let palette = Palette::for_theme(UiTheme::GitHubLight).to_model();

        assert_eq!(
            app.apply_agent_action(
                &context,
                UiAction::SetThemeEditor {
                    draft: Some(CustomTheme {
                        name: "t".repeat(81),
                        palette: palette.clone(),
                    }),
                    original_name: None,
                },
                &mut overlay,
            ),
            Err("Theme names can contain at most 80 characters".into())
        );
        app.apply_agent_action(
            &context,
            UiAction::SetThemeEditor {
                draft: Some(CustomTheme {
                    name: "   ".into(),
                    palette: palette.clone(),
                }),
                original_name: None,
            },
            &mut overlay,
        )
        .unwrap();
        assert_eq!(
            app.apply_agent_action(&context, UiAction::SaveCustomTheme {}, &mut overlay),
            Err("Enter a custom theme name".into())
        );
        assert!(app.settings.interface.custom_themes.is_empty());

        app.apply_agent_action(
            &context,
            UiAction::SetThemeEditor {
                draft: Some(CustomTheme {
                    name: "  Daybreak  ".into(),
                    palette: palette.clone(),
                }),
                original_name: None,
            },
            &mut overlay,
        )
        .unwrap();
        app.apply_theme(&context);
        assert_eq!(
            context.style_of(egui::Theme::Light).visuals.panel_fill,
            Palette::from_model(&palette).background
        );
        app.apply_agent_action(&context, UiAction::SaveCustomTheme {}, &mut overlay)
            .unwrap();
        assert_eq!(app.settings.interface.custom_themes.len(), 1);
        assert_eq!(app.settings.interface.custom_themes[0].palette, palette);
        assert_eq!(
            app.settings.interface.theme,
            ThemeSelection::Custom {
                name: "Daybreak".into()
            }
        );
        assert_eq!(app.ui_state().selected_theme_palette, Some(palette));

        let mut renamed = app.settings.interface.custom_themes[0].clone();
        renamed.name = "Daybreak Revised".into();
        app.apply_agent_action(
            &context,
            UiAction::SetThemeEditor {
                draft: Some(renamed),
                original_name: Some("Daybreak".into()),
            },
            &mut overlay,
        )
        .unwrap();
        app.apply_agent_action(&context, UiAction::SaveCustomTheme {}, &mut overlay)
            .unwrap();
        assert_eq!(
            app.settings.interface.custom_themes[0].name,
            "Daybreak Revised"
        );

        app.apply_agent_action(
            &context,
            UiAction::RemoveCustomTheme {
                name: "Daybreak Revised".into(),
            },
            &mut overlay,
        )
        .unwrap();
        assert!(app.settings.interface.custom_themes.is_empty());
        assert_eq!(app.settings.interface.theme, ThemeSelection::default());
    }

    #[test]
    fn semantic_commands_drive_navigation_settings_and_notifications() {
        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let generation = controls.snapshot().generation;
        let mut app = App::new(&context, Arc::clone(&controls));

        let sidebar = controls
            .submit_ui(generation, UiAction::SetSettingsSidebar { open: true })
            .unwrap();
        app.prepare_frame(&context, false, |_| Ok(()));
        let state = controls.snapshot();
        assert_eq!(state.ui.last_command, sidebar);
        assert!(state.ui.settings_sidebar);

        controls
            .submit_ui(
                generation,
                UiAction::SetTheme {
                    theme: ThemeSelection::BuiltIn {
                        theme: UiTheme::NoMansSky,
                    },
                },
            )
            .unwrap();
        controls
            .submit_ui(
                generation,
                UiAction::SetResultCards {
                    result_cards: ResultCardSettings {
                        location_subtitle: false,
                        portal_glyphs: false,
                        set_destination: false,
                        copy_portal_code: true,
                        copy_galactic_coordinates: false,
                    },
                },
            )
            .unwrap();
        controls
            .submit_ui(
                generation,
                UiAction::ShowToast {
                    id: "review-result-found".into(),
                    toast: TestToast::SearchCompleted {
                        systems: 2_400,
                        system_limit: 10_000,
                        planets: Some(13_842),
                        fauna: Some(6_125),
                        results: 4,
                        result_limit: 5,
                        elapsed_ms: 83_000,
                    },
                },
            )
            .unwrap();
        controls
            .submit_ui(
                generation,
                UiAction::SetChoiceFilter {
                    field: Field::Star,
                    selected: vec!["Yellow".into(), "Blue".into()],
                },
            )
            .unwrap();
        controls
            .submit_ui(
                generation,
                UiAction::SetPlanetRequirement {
                    flag: PlanetFlag::Rings,
                    value: Some(true),
                },
            )
            .unwrap();
        app.prepare_frame(&context, false, |_| Ok(()));

        let state = controls.snapshot();
        assert!(state.ui.settings_sidebar);
        assert_eq!(
            state.ui.draft_settings.interface.theme,
            ThemeSelection::BuiltIn {
                theme: UiTheme::NoMansSky
            }
        );
        assert_eq!(
            state.ui.draft_settings.interface.result_cards,
            ResultCardSettings {
                location_subtitle: false,
                portal_glyphs: false,
                set_destination: false,
                copy_portal_code: true,
                copy_galactic_coordinates: false,
            }
        );
        assert_eq!(state.ui.toasts.len(), 1);
        assert_eq!(
            state.ui.toasts[0].id.as_deref(),
            Some("review-result-found")
        );
        assert_eq!(state.ui.toasts[0].expires_ms, None);
        assert_eq!(
            state.ui.toasts[0].message,
            "• Systems: 2,400 / 10,000\n\
             • Planets: 13,842\n\
             • Fauna: 6,125\n\
             • Results: 4 / 5\n\
             • Elapsed time: 00:01:23"
        );
        assert_eq!(
            state
                .ui
                .draft_settings
                .search
                .criteria
                .choices
                .get(&Field::Star),
            Some(&0b101)
        );
        assert_eq!(
            state
                .ui
                .draft_settings
                .search
                .criteria
                .planets
                .get(&PlanetFlag::Rings),
            Some(&true)
        );
    }

    #[test]
    fn semantic_filter_actions_enforce_visible_dependency_rules() {
        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let mut app = App::new(&context, controls);
        let mut overlay = |_| Ok(());

        app.apply_agent_action(
            &context,
            UiAction::SetChoiceFilter {
                field: Field::StormType,
                selected: vec!["Blizzard".into()],
            },
            &mut overlay,
        )
        .unwrap();
        app.apply_agent_action(
            &context,
            UiAction::SetPlanetRequirement {
                flag: PlanetFlag::ExtremeStorms,
                value: Some(true),
            },
            &mut overlay,
        )
        .unwrap();
        app.apply_agent_action(
            &context,
            UiAction::SetChoiceFilter {
                field: Field::StormFrequency,
                selected: vec!["None".into()],
            },
            &mut overlay,
        )
        .unwrap();
        assert!(
            !app.settings
                .search
                .criteria
                .choices
                .contains_key(&Field::StormType)
        );
        assert!(
            !app.settings
                .search
                .criteria
                .planets
                .contains_key(&PlanetFlag::ExtremeStorms)
        );
        assert!(
            app.apply_agent_action(
                &context,
                UiAction::SetChoiceFilter {
                    field: Field::StormType,
                    selected: vec!["Blizzard".into()],
                },
                &mut overlay,
            )
            .is_err()
        );
        assert!(
            app.apply_agent_action(
                &context,
                UiAction::SetPlanetRequirement {
                    flag: PlanetFlag::ExtremeSentinels,
                    value: Some(true),
                },
                &mut overlay,
            )
            .is_err()
        );

        app.apply_agent_action(
            &context,
            UiAction::SetChoiceFilter {
                field: Field::Race,
                selected: vec!["Gek".into()],
            },
            &mut overlay,
        )
        .unwrap();
        app.apply_agent_action(
            &context,
            UiAction::SetSystemRequirement {
                flag: SystemFlag::Pirate,
                value: Some(false),
            },
            &mut overlay,
        )
        .unwrap();
        for field in [Field::Wealth, Field::Economy, Field::ConflictLevel] {
            app.apply_agent_action(
                &context,
                UiAction::SetChoiceFilter {
                    field,
                    selected: vec![field.available_options().next().unwrap().1.into()],
                },
                &mut overlay,
            )
            .unwrap();
        }
        app.apply_agent_action(
            &context,
            UiAction::SetChoiceFilter {
                field: Field::Population,
                selected: vec!["Empty".into()],
            },
            &mut overlay,
        )
        .unwrap();
        assert!(
            !app.settings
                .search
                .criteria
                .choices
                .contains_key(&Field::Race)
        );
        assert!(
            !app.settings
                .search
                .criteria
                .systems
                .contains_key(&SystemFlag::Pirate)
        );
        for field in [Field::Wealth, Field::Economy, Field::ConflictLevel] {
            assert!(!app.settings.search.criteria.choices.contains_key(&field));
            assert!(
                app.apply_agent_action(
                    &context,
                    UiAction::SetChoiceFilter {
                        field,
                        selected: vec![field.available_options().next().unwrap().1.into()],
                    },
                    &mut overlay,
                )
                .is_err()
            );
        }
        assert!(
            app.apply_agent_action(
                &context,
                UiAction::SetSystemRequirement {
                    flag: SystemFlag::Pirate,
                    value: Some(true),
                },
                &mut overlay,
            )
            .is_err()
        );

        app.apply_agent_action(
            &context,
            UiAction::SetFaunaChoices {
                domain: Domain::Land,
                field: FaunaField::Rarity,
                selected: vec!["Rare".into()],
            },
            &mut overlay,
        )
        .unwrap();
        assert_eq!(
            app.settings.search.criteria.fauna[&Domain::Land].presence,
            Some(true)
        );
        assert!(
            app.apply_agent_action(
                &context,
                UiAction::SetFaunaPresence {
                    domain: Domain::Land,
                    value: None,
                },
                &mut overlay,
            )
            .is_err()
        );
        app.apply_agent_action(
            &context,
            UiAction::SetFaunaChoices {
                domain: Domain::Land,
                field: FaunaField::Rarity,
                selected: Vec::new(),
            },
            &mut overlay,
        )
        .unwrap();
        app.apply_agent_action(
            &context,
            UiAction::SetFaunaPresence {
                domain: Domain::Land,
                value: Some(false),
            },
            &mut overlay,
        )
        .unwrap();
        assert!(
            app.apply_agent_action(
                &context,
                UiAction::SetFaunaHeight {
                    domain: Domain::Land,
                    minimum_cm: Some(100),
                    maximum_cm: None,
                },
                &mut overlay,
            )
            .is_err()
        );
    }

    #[test]
    fn history_navigation_swaps_the_existing_ui_data_and_restores_the_current_draft() {
        let context = egui::Context::default();
        let origin = search_probes_model::address::Address::from_raw(0).unwrap();
        let mut first = Settings::default();
        first
            .search
            .criteria
            .planets
            .insert(PlanetFlag::Rings, true);
        let mut session = search_probes_model::session::Session::default();
        session
            .start(first.search.clone(), origin, first.execution())
            .unwrap();
        session.cancel();

        let mut current = Settings::default();
        current
            .search
            .criteria
            .systems
            .insert(SystemFlag::Pirate, false);
        session
            .start(current.search.clone(), origin, current.execution())
            .unwrap();
        let (progress, history) = session.publication();
        assert_eq!(history.len(), 2);

        let mut state = State::new(current.clone());
        state.progress = progress;
        state.search_history = history;
        let controls = Arc::new(Controls::new(state));
        let mut app = App::new(&context, Arc::clone(&controls));
        app.refresh();
        assert!(app.viewed_search.is_none());
        assert_eq!(app.settings.search, current.search);

        app.step_history(HistoryDirection::Older).unwrap();
        assert!(app.viewed_search.is_some());
        assert_eq!(app.settings.search, first.search);
        let texts = rendered_texts(&mut app, &context, egui::vec2(1100.0, 840.0));
        assert!(texts.iter().any(|text| text == "Search 1 of 2"));
        assert!(texts.iter().any(|text| text == "Search Scope"));
        assert!(texts.iter().any(|text| text == "Results"));

        let mut overlay = |_| Ok(());
        assert_eq!(
            app.apply_agent_action(
                &context,
                UiAction::SetSearchPaused { paused: true },
                &mut overlay,
            ),
            Err("Cannot pause or resume while viewing search history".into())
        );
        assert_eq!(
            app.apply_agent_action(&context, UiAction::CancelSearch {}, &mut overlay),
            Err("Cannot cancel while viewing search history".into())
        );
        let retained_history = app.state.search_history.clone();
        app.apply_agent_action(&context, UiAction::StartSearch {}, &mut overlay)
            .unwrap();
        assert!(app.viewed_search.is_none());
        assert_eq!(app.state.search_history.len(), retained_history.len());
        assert!(
            app.state
                .search_history
                .iter()
                .zip(&retained_history)
                .all(|(current, retained)| Arc::ptr_eq(current, retained))
        );

        let directory = std::env::temp_dir().join(format!(
            "search-probes-history-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        controls.prepare(&directory);
        assert!(matches!(controls.pop().unwrap().action, Action::Cancel {}));
        controls.prepare(&directory);
        assert!(matches!(
            controls.pop().unwrap().action,
            Action::Start { settings } if settings.search == first.search
        ));
        let _ = std::fs::remove_dir_all(directory);

        // Starting again returns to the current-search view without mutating the retained entry.
        assert_eq!(app.state.search_history[0].progress.options, first.search);
    }

    #[test]
    fn interface_reconciliation_preserves_pending_theme_while_accepting_published_hotkey() {
        let previous = Settings::default();
        let mut draft = previous.clone();
        draft.interface.theme = ThemeSelection::BuiltIn {
            theme: UiTheme::NoMansSky,
        };
        let mut published = previous.clone();
        published.interface.hotkey = InputBinding::Keyboard {
            first: KeyboardKey::F8,
            second: None,
        };
        published.interface.notifications.automatic_resume = false;
        published.interface.result_cards.location_subtitle = false;
        reconcile_settings(&mut draft, &previous, &published);
        assert_eq!(
            draft.interface.theme,
            ThemeSelection::BuiltIn {
                theme: UiTheme::NoMansSky
            }
        );
        assert_eq!(draft.interface.hotkey, published.interface.hotkey);
        assert_eq!(
            draft.interface.notifications,
            published.interface.notifications
        );
        assert_eq!(
            draft.interface.result_cards,
            published.interface.result_cards
        );
    }

    #[test]
    fn export_cancel_does_not_claim_success_and_error_clears_confirmation() {
        let context = egui::Context::default();
        let controls = Arc::new(Controls::new(State::new(Settings::default())));
        let mut app = App::new(&context, controls);
        app.finish_export(Ok(None));
        assert!(!app.has_notifications());
        app.finish_export(Ok(Some("results.csv".into())));
        assert!(app.has_notifications());
        app.export_request = Some(ExportRequest::presets_json(&Settings::default()).unwrap());
        assert_eq!(app.take_export_request().unwrap().subject(), "Presets");
        app.finish_export(Err("disk full".into()));
        assert!(!app.has_notifications());
        assert_eq!(
            app.error.as_deref(),
            Some("Cannot export presets: disk full")
        );
    }

    fn result(kind: TargetKind) -> CachedMatch {
        let address =
            search_probes_model::address::Address::new(search_probes_model::address::Coordinates {
                x: 0,
                y: 0,
                z: 0,
                galaxy: 0,
                system: 1,
                planet: if kind == TargetKind::System { 0 } else { 1 },
            })
            .unwrap();
        CachedMatch {
            address,
            kind,
            system_name: Some("Iousongola".into()),
            planet_name: (kind != TargetKind::System).then(|| "New Lennon".into()),
            fauna: (kind == TargetKind::Fauna).then(|| search_probes_model::cache::FaunaMatch {
                role: 0,
                name_identity: search_probes_model::fauna::NameIdentity {
                    species_seed: 1,
                    genus_seed: 2,
                },
                name: Some("Diplo".into()),
                domain: search_probes_model::fauna::Domain::Land,
                cave: false,
            }),
        }
    }

    #[test]
    fn result_subtitles_show_the_destination_hierarchy() {
        assert_eq!(
            result_subtitle(&result(TargetKind::System)),
            "System · Galaxy 1"
        );
        assert_eq!(
            result_subtitle(&result(TargetKind::Planet)),
            "Planet · System Iousongola · Galaxy 1"
        );
        assert_eq!(
            result_subtitle(&result(TargetKind::Fauna)),
            "Animal · Planet New Lennon · System Iousongola · Galaxy 1"
        );
    }

    #[test]
    fn one_input_binding_commits_on_release() {
        let mut capture = BindingCapture::new(BindingDevice::Keyboard);

        assert_eq!(
            capture.input(CapturedInput::Keyboard(KeyboardKey::F8), true),
            None
        );
        assert_eq!(
            capture.input(CapturedInput::Keyboard(KeyboardKey::F8), false),
            Some(InputBinding::Keyboard {
                first: KeyboardKey::F8,
                second: None,
            })
        );
    }

    #[test]
    fn two_input_binding_waits_until_both_inputs_are_released() {
        let mut capture = BindingCapture::new(BindingDevice::Keyboard);

        assert_eq!(
            capture.input(CapturedInput::Keyboard(KeyboardKey::ShiftLeft), true),
            None
        );
        assert_eq!(
            capture.input(CapturedInput::Keyboard(KeyboardKey::Z), true),
            None
        );
        assert_eq!(
            capture.input(CapturedInput::Keyboard(KeyboardKey::ShiftLeft), false),
            None
        );
        assert_eq!(
            capture.input(CapturedInput::Keyboard(KeyboardKey::Z), false),
            Some(InputBinding::Keyboard {
                first: KeyboardKey::ShiftLeft,
                second: Some(KeyboardKey::Z),
            })
        );
    }

    #[test]
    fn more_than_two_inputs_rejects_only_the_current_attempt() {
        let mut capture = BindingCapture::new(BindingDevice::Controller);
        capture.controller = Some(2);
        for button in [
            ControllerButton::LeftBumper,
            ControllerButton::RightBumper,
            ControllerButton::FaceBottom,
        ] {
            assert_eq!(capture.input(CapturedInput::Controller(button), true), None);
        }
        for button in [
            ControllerButton::LeftBumper,
            ControllerButton::RightBumper,
            ControllerButton::FaceBottom,
        ] {
            assert_eq!(
                capture.input(CapturedInput::Controller(button), false),
                None
            );
        }
        assert_eq!(capture.controller, None);

        assert_eq!(
            capture.input(CapturedInput::Controller(ControllerButton::Menu), true),
            None
        );
        assert_eq!(
            capture.input(CapturedInput::Controller(ControllerButton::Menu), false),
            Some(InputBinding::Controller {
                first: ControllerButton::Menu,
                second: None,
            })
        );
    }

    #[test]
    fn changing_device_clears_the_in_progress_attempt() {
        let mut capture = BindingCapture::new(BindingDevice::Controller);
        capture.controller = Some(1);
        assert_eq!(
            capture.input(
                CapturedInput::Controller(ControllerButton::LeftBumper),
                true,
            ),
            None
        );

        capture.set_device(BindingDevice::Keyboard);

        assert_eq!(capture.device, BindingDevice::Keyboard);
        assert_eq!(capture.controller, None);
        assert!(capture.held.is_empty());
        assert!(capture.candidate.is_empty());
    }

    #[test]
    fn published_intensity_changes_do_not_replace_draft_search_edits() {
        let previous = Settings::default();
        let mut draft = previous.clone();
        draft.search.results = 23;
        draft.frame_budget_ms = 16;
        let mut published = previous.clone();
        published.frame_budget_ms = 16;

        reconcile_settings(&mut draft, &previous, &published);

        assert_eq!(draft.search.results, 23);
        assert_eq!(draft.frame_budget_ms, 16);
    }
}

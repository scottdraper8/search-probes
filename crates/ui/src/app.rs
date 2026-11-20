use crate::{
    export::{ExportFormat, ExportRequest},
    notifications::{Notification, Notifications},
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
        CompositionField, Criteria, Field, PlanetFlag, Section, SelectionSide, SystemFlag,
        WaterFilter, subtype_options_for,
    },
    fauna::{Domain, FaunaCriteria, FaunaField},
    runtime::{Action, Availability, LifecycleEventKind, SearchPause, State},
    session::{Finish, Phase},
    settings::{
        BUDGET_LABELS, BUDGETS, MAX_PRESETS, MAX_RESULTS, Preset, Settings, TimeLimit,
        ToastPlacement, available_workers, recommended_workers, validate_frame_budget,
        validate_name,
    },
    target::TargetKind,
    theme::UiTheme,
    ui_control::{
        HistoryDirection, ResultValue, TestToast, UiAction, UiCommand, UiExportFormat,
        UiExportSource, UiOutput, UiPage, UiState, WaterScope,
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
const PRIMARY_SEARCH_ACTION_WIDTH: f32 = 110.0;
const SECONDARY_SEARCH_ACTION_WIDTH: f32 = 80.0;

fn search_action_button(button: egui::Button<'_>, width: f32) -> egui::Button<'_> {
    button.min_size(egui::vec2(width, SEARCH_ACTION_HEIGHT))
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
    if option_count <= NON_SCROLLING_SELECT_MAX_OPTIONS {
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
                    contents(ui);
                });
        });
    } else {
        egui::ComboBox::from_id_salt(id)
            .selected_text(selected_text)
            .width(width)
            .height(COMBO_POPUP_MAX_HEIGHT)
            .close_behavior(close_behavior)
            .truncate()
            .show_ui(ui, contents);
    }
}

pub type Page = UiPage;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CapturedInput {
    Keyboard(KeyboardKey),
    Controller(ControllerButton),
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
    error: Option<String>,
    published_settings: Option<Settings>,
    pending_search: Option<u64>,
    binding_window: bool,
    binding_capture: Option<BindingCapture>,
    export_request: Option<ExportRequest>,
    pending_export_subject: Option<&'static str>,
    notifications: Notifications,
    started: Instant,
    settings_sidebar: bool,
    page: Page,
    viewed_search: Option<u64>,
    lifecycle_sequence: u64,
    applied_theme: Option<UiTheme>,
    portal_glyphs: PortalGlyphs,
    overlay_visible: bool,
    ui_last_command: u64,
    ui_error: Option<String>,
    ui_output: Option<UiOutput>,
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
        Self {
            state_revision,
            state,
            controls,
            settings: Settings::default(),
            loaded_generation: None,
            preset_name: String::new(),
            selected_preset: None,
            error: None,
            published_settings: None,
            pending_search: None,
            binding_window: false,
            binding_capture: None,
            export_request: None,
            pending_export_subject: None,
            notifications: Notifications::default(),
            started: Instant::now(),
            settings_sidebar: false,
            page: Page::Search,
            viewed_search: None,
            lifecycle_sequence: 0,
            applied_theme: None,
            portal_glyphs: PortalGlyphs::new(ctx),
            overlay_visible: false,
            ui_last_command: 0,
            ui_error: None,
            ui_output: None,
        }
    }

    fn palette(&self) -> Palette {
        Palette::for_theme(self.settings.interface.theme)
    }

    fn theme_control(&mut self, ui: &mut Ui) {
        let current = self.settings.interface.theme;
        let mut selected = None;
        ui.menu_button(format!("Theme  ·  {}", current.label()), |ui| {
            egui::ScrollArea::vertical()
                .max_height(480.0)
                .show(ui, |ui| {
                    ui.set_min_width(190.0);
                    for &candidate in UiTheme::ALL {
                        if ui
                            .selectable_label(candidate == current, candidate.label())
                            .clicked()
                        {
                            selected = Some(candidate);
                            ui.close();
                        }
                    }
                });
        });
        if let Some(theme) = selected.filter(|theme| *theme != current) {
            self.settings.interface.theme = theme;
            self.applied_theme = None;
            self.send(Action::Configure {
                settings: self.settings.clone(),
            });
        }
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
        let palette = self.palette();
        ui.horizontal(|ui| {
            ui.heading("Settings");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.settings_sidebar = false;
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            for (page, label) in [(Page::Search, "Search"), (Page::History, "History")] {
                if ui.selectable_label(self.page == page, label).clicked() {
                    self.page = page;
                    self.settings_sidebar = false;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("settings_sidebar_scroll")
            .show(ui, |ui| {
                ui.heading("Theme");
                self.theme_control(ui);
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.heading("UI Hotkey");
                if ui
                    .button(binding_text(self.settings.interface.hotkey))
                    .clicked()
                {
                    self.binding_window = true;
                }
                ui.label(
                    RichText::new("Recovery: Ctrl + Shift + Backspace")
                        .small()
                        .color(palette.on_surface_variant),
                );
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.heading("Notifications");
                let previous = self.settings.interface.notifications.clone();
                let notifications = &mut self.settings.interface.notifications;
                ui.checkbox(&mut notifications.result_found, "Result found");
                ui.checkbox(&mut notifications.search_completed, "Search completed");
                ui.checkbox(&mut notifications.automatic_pause, "Automatic pause");
                ui.checkbox(&mut notifications.automatic_resume, "Automatic resume");
                ui.add_space(8.0);
                ui.label("Screen placement");
                select_menu(
                    ui,
                    "notification_placement",
                    notifications.placement.label(),
                    ui.spacing().combo_width,
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
                if *notifications != previous {
                    self.notifications
                        .update(self.started.elapsed(), notifications);
                    self.send(Action::Configure {
                        settings: self.settings.clone(),
                    });
                }
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.heading("Result cards");
                self.result_card_settings(ui);
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
    fn preset_controls(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            select_menu(
                ui,
                "preset",
                self.selected_preset
                    .and_then(|index| self.settings.presets.get(index))
                    .map_or_else(
                        || "Choose a preset".to_owned(),
                        |preset| preset.name.clone(),
                    ),
                240.0,
                self.settings.presets.len(),
                egui::PopupCloseBehavior::CloseOnClick,
                |ui| {
                    for (index, preset) in self.settings.presets.iter().enumerate() {
                        if ui
                            .selectable_label(
                                self.selected_preset == Some(index),
                                format!("{}. {}", index + 1, preset.name),
                            )
                            .clicked()
                        {
                            self.selected_preset = Some(index);
                            self.preset_name = preset.name.clone();
                            self.settings.search = preset.search.clone();
                        }
                    }
                },
            );
            if ui.button("Clear criteria").clicked() {
                self.settings.search.criteria = Criteria::default();
                self.selected_preset = None;
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.preset_name)
                    .hint_text("Preset name")
                    .desired_width(220.0)
                    .char_limit(80),
            );
            if ui.button("Save preset").clicked()
                && let Err(error) = self.save_preset()
            {
                self.error = Some(error);
            }
            if ui
                .add_enabled(self.selected_preset.is_some(), egui::Button::new("Remove"))
                .clicked()
                && let Err(error) = self.remove_preset()
            {
                self.error = Some(error);
            }
            if ui
                .add_enabled(
                    !self.state.settings.presets.is_empty(),
                    egui::Button::new("Export presets (.json)"),
                )
                .clicked()
            {
                match ExportRequest::presets_json(&self.state.settings) {
                    Ok(request) => {
                        self.export_request = Some(request);
                        self.notifications.clear_export();
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        });
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
        ui.separator();
        let state = &self.state;
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
        let current = state.progress.as_ref().and_then(|progress| {
            state
                .search_history
                .iter()
                .find(|record| record.progress.id == progress.id)
                .cloned()
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready && !active && valid.is_ok(),
                    search_action_button(
                        egui::Button::new(RichText::new("Search").strong()),
                        PRIMARY_SEARCH_ACTION_WIDTH,
                    ),
                )
                .clicked()
            {
                self.send(Action::Start {
                    settings: self.settings.clone(),
                });
            }
            if ui
                .add_enabled(
                    ready && running && !automatically_paused,
                    search_action_button(
                        egui::Button::new(if user_paused { "Resume" } else { "Pause" }),
                        SECONDARY_SEARCH_ACTION_WIDTH,
                    ),
                )
                .clicked()
            {
                self.send(Action::SetSearchPaused {
                    paused: !user_paused,
                });
            }
            if ui
                .add_enabled(
                    active,
                    search_action_button(
                        egui::Button::new("Cancel"),
                        SECONDARY_SEARCH_ACTION_WIDTH,
                    ),
                )
                .clicked()
            {
                self.send(Action::Cancel {});
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        current.is_some(),
                        search_action_button(
                            egui::Button::new("Export search"),
                            PRIMARY_SEARCH_ACTION_WIDTH,
                        ),
                    )
                    .on_hover_text("Export the current search settings and results as JSON")
                    .clicked()
                    && let Some(record) = &current
                {
                    match ExportRequest::search_json(record) {
                        Ok(request) => self.export_request = Some(request),
                        Err(error) => self.error = Some(error),
                    }
                    self.notifications.clear_export();
                }
            });
        });
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
                    ui.colored_label(self.palette().error, error);
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

    fn binding_dialog(&mut self, ctx: &egui::Context) {
        if !self.binding_window {
            return;
        }
        let binding = self.settings.interface.hotkey;
        let palette = self.palette();
        let mut window_open = true;
        let mut start_capture = false;
        let mut cancel_capture = false;
        let mut reset = false;
        egui::Window::new("UI Hotkey")
            .id(egui::Id::new("ui_hotkey_dialog"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(540.0)
            .min_width(360.0)
            .max_width(620.0)
            .open(&mut window_open)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading(
                        RichText::new("Toggle the Search Probes overlay")
                            .size(24.0)
                            .strong(),
                    );
                    ui.add_space(14.0);
                    if let Some(capture) = self.binding_capture.as_mut() {
                        ui.label(RichText::new("Input type").size(17.0).strong());
                        let mut selected_device = capture.device;
                        responsive_row(ui, 38.0, |ui| {
                            let button_width =
                                (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                            if ui
                                .add_sized(
                                    [button_width, 34.0],
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
                                    [button_width, 34.0],
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
                        ui.add_space(12.0);
                        egui::Frame::new()
                            .fill(palette.surface)
                            .stroke(egui::Stroke::new(1.0, palette.primary))
                            .corner_radius(5.0)
                            .inner_margin(egui::Margin::symmetric(18, 12))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(match capture.device {
                                            BindingDevice::Keyboard => {
                                                "Press one or two keys together"
                                            }
                                            BindingDevice::Controller => {
                                                "Press one or two controller inputs together"
                                            }
                                        })
                                        .size(17.0)
                                        .strong(),
                                    );
                                    ui.add_space(8.0);
                                    if capture.candidate.is_empty() {
                                        ui.label(
                                            RichText::new(match capture.device {
                                                BindingDevice::Keyboard => {
                                                    "Waiting for keyboard input…"
                                                }
                                                BindingDevice::Controller => {
                                                    "Waiting for any controller…"
                                                }
                                            })
                                            .size(16.0)
                                            .color(palette.on_surface_variant),
                                        );
                                    } else {
                                        responsive_row(ui, 46.0, |ui| {
                                            captured_caps(ui, &capture.candidate, palette)
                                        });
                                    }
                                    if capture.overflowed {
                                        ui.colored_label(
                                            palette.error,
                                            RichText::new(
                                                "Release the inputs and try again with no more than two.",
                                            )
                                            .size(15.0),
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new("Release all inputs to save.")
                                                .size(15.0),
                                        );
                                    }
                                });
                            });
                        ui.add_space(10.0);
                        responsive_row(ui, 38.0, |ui| {
                            let width = ui.available_width();
                            ui.add_space(width * 0.25);
                            if ui
                                .add_sized([width * 0.5, 34.0], egui::Button::new("Cancel"))
                                .clicked()
                            {
                                cancel_capture = true;
                            }
                        });
                    } else {
                        ui.label(RichText::new("Current hotkey").size(17.0).strong());
                        ui.add_space(4.0);
                        responsive_row(ui, 50.0, |ui| binding_caps(ui, binding, palette));
                        ui.add_space(16.0);
                        responsive_row(ui, 40.0, |ui| {
                            let button_width =
                                (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                            if ui
                                .add_sized([button_width, 36.0], egui::Button::new("Change"))
                                .clicked()
                            {
                                start_capture = true;
                            }
                            if ui
                                .add_enabled(
                                    binding != InputBinding::default(),
                                    egui::Button::new("Reset to Ctrl + ,")
                                        .min_size(egui::vec2(button_width, 36.0)),
                                )
                                .clicked()
                            {
                                reset = true;
                            }
                        });
                        ui.add_space(10.0);
                        ui.separator();
                        let (footer, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 48.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().text(
                            egui::pos2(footer.center().x, footer.top() + 12.0),
                            egui::Align2::CENTER_CENTER,
                            "Use up to two keyboard keys or controller inputs",
                            egui::FontId::proportional(16.0),
                            ui.visuals().text_color(),
                        );
                        ui.painter().text(
                            egui::pos2(footer.center().x, footer.bottom() - 10.0),
                            egui::Align2::CENTER_CENTER,
                            "Recovery hotkey: Ctrl + Shift + Backspace",
                            egui::FontId::proportional(14.0),
                            palette.on_surface_variant,
                        );
                    }
                });
            });
        self.binding_window = window_open;
        if !window_open || cancel_capture {
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
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let input_width = filter_control_width(ui);
            self.criteria(ui, input_width);
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
                        minimum_count_row(
                            ui,
                            "Minimum stars",
                            "minimum_stars",
                            &mut criteria.minimum_stars,
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
                    minimum_count_row(
                        ui,
                        "Minimum planets",
                        "minimum_planets",
                        &mut criteria.minimum_planets,
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
    fn history(&mut self, ui: &mut Ui) {
        let records = self.state.search_history.clone();
        if records.is_empty() {
            ui.label("Completed and current searches will appear here.");
            return;
        }
        let selected_id = self
            .viewed_search
            .filter(|id| records.iter().any(|record| record.progress.id == *id))
            .unwrap_or_else(|| records.last().unwrap().progress.id);
        self.viewed_search = Some(selected_id);
        let selected_index = records
            .iter()
            .position(|record| record.progress.id == selected_id)
            .unwrap();

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(selected_index > 0, egui::Button::new("← Older"))
                .clicked()
            {
                self.viewed_search = Some(records[selected_index - 1].progress.id);
            }
            if ui
                .add_enabled(
                    selected_index + 1 < records.len(),
                    egui::Button::new("Newer →"),
                )
                .clicked()
            {
                self.viewed_search = Some(records[selected_index + 1].progress.id);
            }
            ui.label(format!(
                "{} retained search{}",
                records.len(),
                if records.len() == 1 { "" } else { "es" }
            ));
        });
        ui.separator();

        let viewed_id = self.viewed_search.unwrap_or(selected_id);
        let record = records
            .iter()
            .find(|record| record.progress.id == viewed_id)
            .cloned()
            .unwrap_or_else(|| records[selected_index].clone());
        let progress = &record.progress;
        ui.horizontal_wrapped(|ui| {
            ui.heading(format!("Search #{}", progress.id));
            ui.label(search_phase(progress.phase, progress.finish));
            if ui.button("Export search (.json)").clicked() {
                match ExportRequest::search_json(&record) {
                    Ok(request) => {
                        self.export_request = Some(request);
                        self.notifications.clear_export();
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        });
        ui.label(format!(
            "{} elapsed · {} systems checked · {} result{}",
            elapsed(progress.elapsed_ms),
            number(progress.systems),
            progress.results.len(),
            if progress.results.len() == 1 { "" } else { "s" }
        ));
        let time_limit = progress.options.time_limit.map_or_else(
            || "No time limit".to_owned(),
            |limit| format!("Time limit {:02}:{:02}", limit.hours, limit.minutes),
        );
        ui.small(format!(
            "System limit {} · Result limit {} · {time_limit} · {} workers · {} ms frame budget · Keep game running: {}",
            number(progress.options.systems),
            progress.options.results,
            record
                .execution
                .workers
                .map_or_else(|| "Automatic".to_owned(), |workers| workers.to_string()),
            record.execution.frame_budget_ms,
            if record.execution.keep_game_running {
                "On"
            } else {
                "Off"
            },
        ));
        ui.add_space(8.0);
        ui.label(RichText::new("Criteria").strong());
        match history_criteria(&progress.options.criteria) {
            Ok(groups) if groups.is_empty() => {
                ui.label("No filters — every generated system is eligible.");
            }
            Ok(groups) => {
                ui.small(
                    "All filter groups must match; any selected value within one filter may match.",
                );
                for (section, descriptions) in groups {
                    ui.add_space(4.0);
                    ui.label(RichText::new(section_label(section)).strong());
                    for description in descriptions {
                        ui.label(format!("• {description}"));
                    }
                }
            }
            Err(error) => {
                ui.colored_label(
                    self.palette().error,
                    format!("Criteria unavailable: {error}"),
                );
            }
        }
        if let Some(error) = &progress.error {
            ui.colored_label(self.palette().error, error);
        }
        ui.separator();
        if progress.results.is_empty() {
            ui.label("This search has no results.");
            return;
        }
        for (index, result) in progress.results.iter().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!(
                        "{}. {}",
                        index + 1,
                        result.title().unwrap_or("Name unavailable")
                    ))
                    .strong(),
                );
                ui.small(result.address.galactic_coordinates());
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            self.state.availability == Availability::Ready
                                && result.destination_names_complete()
                                && self.state.player.is_some_and(|player| {
                                    player.coordinates().galaxy
                                        == result.address.coordinates().galaxy
                                }),
                            egui::Button::new("Set destination"),
                        )
                        .clicked()
                    {
                        self.send(Action::Navigate {
                            search_id: progress.id,
                            target: result.target(),
                        });
                    }
                    if ui.button("Copy portal code").clicked() {
                        ui.ctx().copy_text(result.target().portal_code());
                    }
                });
            });
            ui.add_space(6.0);
        }
    }

    fn results(&mut self, ui: &mut Ui) {
        let state = self.state.clone();
        let result_cards = self.settings.interface.result_cards;
        let palette = self.palette();
        let results = state
            .progress
            .as_ref()
            .map_or(&[][..], |progress| progress.results.as_slice());
        ui.horizontal(|ui| {
            ui.heading("Results");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(!results.is_empty(), |ui| {
                    let (response, _) =
                        egui::containers::menu::MenuButton::from_button(search_action_button(
                            egui::Button::new("Export results"),
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
                    response.on_hover_text("Export the current results as text or CSV");
                })
                .response
                .on_disabled_hover_text("There are no results to export");
            });
        });
        if state.availability != Availability::Ready {
            ui.label(match state.availability {
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
        let has_progress = state.progress.is_some();
        animated_visibility(ui, "search_progress", has_progress, |ui| {
            let Some(progress) = &state.progress else {
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
                Phase::Running => match state.search_pause {
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
            if state.metrics.elapsed_ms > 0
                && progress.phase == Phase::Running
                && state.search_pause.is_none()
            {
                let per_second = |count| count as f64 * 1000.0 / state.metrics.elapsed_ms as f64;
                let mut rates = vec![format!(
                    "{:.0} generated systems/s",
                    per_second(state.metrics.generated_systems)
                )];
                if let Ok(plan) = progress.options.criteria.compile() {
                    if !plan.planet.is_empty() {
                        rates.push(format!(
                            "{:.0} generated planets/s",
                            per_second(state.metrics.generated_planets)
                        ));
                    }
                    if plan.targets_animals() {
                        rates.push(format!(
                            "{:.0} generated fauna/s",
                            per_second(state.metrics.generated_fauna)
                        ));
                    }
                }
                ui.small(format!("Average {}", rates.join(" · ")));
            }
            if progress.unresolved_arena_planets > 0 {
                ui.small(format!(
                    "{} planets could not be checked against the selected Arena ranks.",
                    number(progress.unresolved_arena_planets)
                ));
            }
            if let Some(error) = &progress.error {
                ui.colored_label(self.palette().error, error);
            }
        });
        animated_visibility(ui, "search_instructions", !has_progress, |ui| {
            ui.label("Choose your criteria and start a search.");
        });
        let has_results = !results.is_empty();
        animated_visibility(ui, "search_result_cards", has_results, |ui| {
            if !has_results {
                return;
            }
            ui.separator();
            let results: Vec<_> = results.iter().collect();
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
                                            RichText::new(result_subtitle(
                                                result,
                                                u16::from(coordinates.galaxy) + 1,
                                            ))
                                            .small()
                                            .color(palette.on_surface_variant),
                                        );
                                    }
                                    if result_cards.portal_glyphs {
                                        self.portal_glyphs
                                            .show(ui, &portal_code, palette.on_surface_variant)
                                            .on_hover_text(format!("Portal code: {portal_code}"));
                                    }
                                    ui.horizontal_wrapped(|ui| {
                                        if result_cards.set_destination
                                            && ui
                                            .add_enabled(
                                                state.availability == Availability::Ready
                                                    && result.destination_names_complete()
                                                    && state.player.is_some_and(|player| player.coordinates().galaxy == coordinates.galaxy),
                                                egui::Button::new("Set destination"),
                                            )
                                            .clicked()
                                        {
                                            self.send(Action::Navigate {
                                                search_id: state.progress.as_ref().map_or(0, |progress| progress.id),
                                                target: result.target(),
                                            });
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
                                                "{}\nIdentifies this system.",
                                                result.address.galactic_coordinates()
                                            ))
                                            .clicked()
                                        {
                                            ui.ctx().copy_text(result.address.galactic_coordinates());
                                        }
                                        if state.destination == Some(result.target()) {
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
        } else if self.settings.presets.len() < MAX_PRESETS {
            self.selected_preset = Some(self.settings.presets.len());
            self.settings.presets.push(preset);
        } else {
            return Err("The Guide can display nine presets. Replace or remove one first.".into());
        }
        self.try_send(Action::Configure {
            settings: self.settings.clone(),
        })
    }

    fn remove_preset(&mut self) -> Result<u64, String> {
        let index = self
            .selected_preset
            .take()
            .ok_or_else(|| "No preset is selected".to_owned())?;
        if index >= self.settings.presets.len() {
            return Err("Selected preset is unavailable".into());
        }
        self.settings.presets.remove(index);
        self.preset_name.clear();
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
                let results = self
                    .state
                    .progress
                    .as_ref()
                    .map_or(&[][..], |progress| progress.results.as_slice());
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
            UiAction::SetPage { page } => {
                self.page = page;
                self.settings_sidebar = false;
            }
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
            UiAction::SetMinimumPlanets { minimum } => {
                if minimum.is_some_and(|minimum| !(1..=6).contains(&minimum)) {
                    return Err("Minimum planets must be Any or between 1 and 6".into());
                }
                self.settings.search.criteria.minimum_planets = minimum;
                self.selected_preset = None;
            }
            UiAction::SetMinimumStars { minimum } => {
                if minimum.is_some_and(|minimum| !(1..=3).contains(&minimum)) {
                    return Err("Minimum stars must be Any or between 1 and 3".into());
                }
                self.settings.search.criteria.minimum_stars = minimum;
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
                self.settings.interface.theme = theme;
                self.applied_theme = None;
                let command_id = self.try_send(Action::Configure {
                    settings: self.settings.clone(),
                })?;
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
            UiAction::SetHotkeyDialog { open } => {
                self.binding_window = open;
                if !open {
                    self.binding_capture = None;
                }
            }
            UiAction::SetHotkeyCapture { device } => {
                self.binding_window = device.is_some() || self.binding_window;
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
                self.preset_name = preset.name;
                self.settings.search = preset.search;
            }
            UiAction::ClearPresetSelection {} => {
                self.selected_preset = None;
                self.preset_name.clear();
            }
            UiAction::SetPresetName { name } => {
                if name.chars().count() > 80 || name.chars().any(char::is_control) {
                    return Err("Preset name exceeds the UI input bounds".into());
                }
                self.preset_name = name;
            }
            UiAction::SavePreset {} => {
                let command_id = self.save_preset()?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::RemovePreset {} => {
                let command_id = self.remove_preset()?;
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
                let command_id = self.try_send(Action::Start {
                    settings: self.settings.clone(),
                })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetSearchPaused { paused } => {
                let command_id = self.try_send(Action::SetSearchPaused { paused })?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::CancelSearch {} => {
                let command_id = self.try_send(Action::Cancel {})?;
                return Ok(Some(UiOutput::RuntimeCommand { command_id }));
            }
            UiAction::SetViewedSearch { search_id } => {
                if let Some(search_id) = search_id {
                    self.record(search_id)?;
                }
                self.viewed_search = search_id;
            }
            UiAction::StepHistory { direction } => {
                let records = &self.state.search_history;
                if records.is_empty() {
                    return Err("Search history is empty".into());
                }
                let selected_id = self
                    .viewed_search
                    .unwrap_or_else(|| records.last().unwrap().progress.id);
                let selected = records
                    .iter()
                    .position(|record| record.progress.id == selected_id)
                    .ok_or_else(|| "Selected history search is unavailable".to_owned())?;
                let next = match direction {
                    HistoryDirection::Older => selected.checked_sub(1),
                    HistoryDirection::Newer => {
                        (selected + 1 < records.len()).then_some(selected + 1)
                    }
                }
                .ok_or_else(|| "There is no search in that history direction".to_owned())?;
                self.viewed_search = Some(records[next].progress.id);
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
            UiAction::ShowToast { toast } => match toast {
                TestToast::ResultFound { count } => {
                    self.notify(Notification::ResultsFound { count })
                }
                TestToast::SearchCompleted { message } => {
                    self.notify(Notification::SearchCompleted { message })
                }
                TestToast::AutomaticPause { reason } => {
                    self.notify(Notification::AutomaticallyPaused { reason })
                }
                TestToast::AutomaticResume {} => self.notify(Notification::AutomaticallyResumed),
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
                    self.notifications
                        .exported(subject, path, self.started.elapsed());
                }
            },
            UiAction::ClearToasts {} => self.notifications.clear(),
        }
        Ok(None)
    }

    fn ui_state(&self) -> UiState {
        UiState {
            overlay_visible: self.overlay_visible,
            page: self.page,
            settings_sidebar: self.settings_sidebar,
            hotkey_dialog: self.binding_window,
            hotkey_capture: self.binding_capture.as_ref().map(|capture| capture.device),
            selected_preset: self.selected_preset,
            preset_name: self.preset_name.clone(),
            viewed_search: self.viewed_search,
            draft_settings: self.settings.clone(),
            toasts: self.notifications.snapshot(),
            last_command: self.ui_last_command,
            error: self.ui_error.clone(),
            output: self.ui_output.clone(),
        }
    }

    fn publish_ui_state(&self) {
        self.controls.publish_ui(self.ui_state());
    }

    fn process_agent_commands(
        &mut self,
        ctx: &egui::Context,
        set_overlay_visible: &mut impl FnMut(bool) -> Result<(), String>,
    ) {
        for _ in 0..16 {
            let Some(UiCommand {
                generation,
                id,
                action,
            }) = self.controls.pop_ui()
            else {
                break;
            };
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
                self.notifications
                    .exported(subject, path, self.started.elapsed());
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

    pub fn page(&self) -> Page {
        self.page
    }

    /// Prepare both interactive and passive overlay frames. Call even while its window is hidden.
    pub fn prepare_frame(
        &mut self,
        ctx: &egui::Context,
        overlay_visible: bool,
        mut set_overlay_visible: impl FnMut(bool) -> Result<(), String>,
    ) {
        self.overlay_visible = overlay_visible;
        self.refresh();
        self.process_agent_commands(ctx, &mut set_overlay_visible);
        self.apply_theme(ctx);
        self.publish_ui_state();
    }

    pub fn has_notifications(&self) -> bool {
        !self.notifications.is_empty()
    }

    pub fn show_notifications(&mut self, ctx: &egui::Context) {
        self.notifications.update(
            self.started.elapsed(),
            &self.settings.interface.notifications,
        );
        self.notifications.show(
            ctx,
            self.palette(),
            self.settings.interface.notifications.placement,
        );
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let selected_theme = self.settings.interface.theme;
        if self.applied_theme != Some(selected_theme) {
            theme::apply(ctx, selected_theme);
            self.applied_theme = Some(selected_theme);
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
                    Some(Notification::ResultsFound { count })
                }
                LifecycleEventKind::AutomaticPause { cause } => {
                    let reason = match cause {
                        SearchPause::Loading => "Search paused while the game loads.",
                        SearchPause::OtherGalaxy => {
                            "Search paused until you return to its starting galaxy."
                        }
                        SearchPause::User => continue,
                    };
                    Some(Notification::AutomaticallyPaused {
                        reason: reason.into(),
                    })
                }
                LifecycleEventKind::AutomaticResume { cause } => {
                    if cause == SearchPause::User {
                        continue;
                    }
                    Some(Notification::AutomaticallyResumed)
                }
                LifecycleEventKind::Finished { finish } => match finish {
                    Finish::Cancelled => None,
                    Finish::Results => Some(Notification::SearchCompleted {
                        message: format!(
                            "Search completed after finding {} result{}.",
                            event.results,
                            if event.results == 1 { "" } else { "s" }
                        ),
                    }),
                    Finish::Budget => Some(Notification::SearchCompleted {
                        message: format!(
                            "Search reached its system limit with {} result{}.",
                            event.results,
                            if event.results == 1 { "" } else { "s" }
                        ),
                    }),
                    Finish::Exhausted => Some(Notification::SearchCompleted {
                        message: format!(
                            "Search exhausted the available systems with {} result{}.",
                            event.results,
                            if event.results == 1 { "" } else { "s" }
                        ),
                    }),
                    Finish::TimeLimit => Some(Notification::SearchCompleted {
                        message: format!(
                            "Search reached its time limit with {} result{}.",
                            event.results,
                            if event.results == 1 { "" } else { "s" }
                        ),
                    }),
                    Finish::Failed => Some(Notification::SearchCompleted {
                        message: "Search stopped after an error.".into(),
                    }),
                },
            };
            if let Some(notification) = notification {
                self.notify_at(notification, occurred, producer_now);
            }
        }
    }

    pub fn refresh(&mut self) {
        let Some((revision, state)) = self.controls.snapshot_if_changed(self.state_revision) else {
            return;
        };
        self.state_revision = revision;
        self.state = state;
        let state = &self.state;
        if self.loaded_generation != Some(state.generation) {
            self.pending_search = None;
            self.loaded_generation = Some(state.generation);
        }
        if self.published_settings.as_ref() != Some(&state.settings) {
            if let Some(previous) = self.published_settings.as_ref() {
                reconcile_settings(&mut self.settings, previous, &state.settings);
            } else {
                self.settings = state.settings.clone();
            }
            self.published_settings = Some(state.settings.clone());
            self.selected_preset = None;
        }
        if self
            .pending_search
            .is_some_and(|id| state.last_command >= id)
        {
            self.pending_search = None;
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
    }
    pub fn ui(&mut self, ui: &mut Ui) {
        self.apply_theme(ui.ctx());
        let palette = self.palette();
        self.capture_keyboard(ui.ctx());
        let compact_sidebar = ui.available_width() < 700.0;
        let sidebar_width = if compact_sidebar {
            ui.available_width()
        } else {
            300.0
        };
        let sidebar_before = self.settings_sidebar;
        let mut sidebar_open = sidebar_before;
        let sidebar = egui::Panel::right("search_probes_settings")
            .exact_size(sidebar_width)
            .resizable(false)
            .frame(egui::Frame::new().fill(palette.surface).inner_margin(12.0))
            .show_collapsible(ui, &mut sidebar_open, |ui| self.settings_contents(ui));
        if self.settings_sidebar != sidebar_before {
            sidebar_open = self.settings_sidebar;
        }
        self.settings_sidebar = sidebar_open;
        if compact_sidebar && sidebar.is_some() && self.settings_sidebar {
            self.binding_dialog(ui.ctx());
            return;
        }
        egui::Frame::new()
            .inner_margin(16.0)
            .fill(palette.background)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("Search Probes").size(28.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let settings = ui
                            .add(
                                egui::Button::new(RichText::new("⚙").size(28.0))
                                    .frame(false)
                                    .min_size(egui::vec2(42.0, 36.0)),
                            )
                            .on_hover_text("Settings");
                        settings.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Settings")
                        });
                        if settings.clicked() {
                            self.settings_sidebar = !self.settings_sidebar;
                        }
                    });
                });
                if let Some(error) = self.error.as_ref().or(self.state.error.as_ref()) {
                    ui.colored_label(palette.error, error);
                }

                if self.page == Page::History {
                    ui.horizontal(|ui| {
                        ui.heading("History");
                        if ui.button("Back to Search").clicked() {
                            self.page = Page::Search;
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("search_history")
                        .show(ui, |ui| self.history(ui));
                    return;
                }

                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.heading(RichText::new("Presets").size(17.0).strong());
                    self.preset_controls(ui);
                });
                ui.add_space(8.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.search_controls(ui);
                });
                ui.add_space(10.0);

                if ui.available_width() >= MAIN_SPLIT_MIN_WIDTH {
                    let results_frame = egui::Frame::group(ui.style());
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
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                self.results(ui);
                            });
                        });
                }
            });
        self.binding_dialog(ui.ctx());
    }
}

fn reconcile_settings(draft: &mut Settings, previous: &Settings, published: &Settings) {
    if draft.search == previous.search {
        draft.search = published.search.clone();
    }
    if draft.presets == previous.presets {
        draft.presets = published.presets.clone();
    }
    if draft.interface.theme == previous.interface.theme {
        draft.interface.theme = published.interface.theme;
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

fn result_subtitle(result: &CachedMatch, galaxy: u16) -> String {
    let system = result.system_name.as_deref().unwrap_or("name unavailable");
    let planet = result.planet_name.as_deref().unwrap_or("name unavailable");
    match result.kind {
        TargetKind::System => format!("System · Galaxy {galaxy}"),
        TargetKind::Planet => format!("Planet · System {system} · Galaxy {galaxy}"),
        TargetKind::Fauna => {
            format!("Animal · Planet {planet} · System {system} · Galaxy {galaxy}")
        }
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

fn binding_text(binding: InputBinding) -> String {
    match binding {
        InputBinding::Keyboard { first, second } => second.map_or_else(
            || first.label().to_owned(),
            |second| format!("{} + {}", first.label(), second.label()),
        ),
        InputBinding::Controller { first, second } => second.map_or_else(
            || first.label().to_owned(),
            |second| format!("{} + {}", first.label(), second.label()),
        ),
    }
}

fn captured_caps(ui: &mut Ui, inputs: &[CapturedInput], palette: Palette) {
    let labels: Vec<_> = inputs.iter().map(|input| input.label()).collect();
    binding_cap_group(ui, &labels, palette);
}

fn responsive_row(ui: &mut Ui, height: f32, add_contents: impl FnOnce(&mut Ui)) {
    let available_width = ui.available_width();
    let content_width = available_width * 0.68;
    let (row, _) =
        ui.allocate_exact_size(egui::vec2(available_width, height), egui::Sense::hover());
    let content = egui::Rect::from_center_size(row.center(), egui::vec2(content_width, height));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add_contents(&mut child);
}

fn binding_cap_group(ui: &mut Ui, labels: &[&str], palette: Palette) {
    let total_width = ui.available_width();
    let gap = ui.spacing().item_spacing.x;
    let cap_width = if labels.len() == 2 {
        (total_width - 14.0 - gap * 2.0) / 2.0
    } else {
        total_width
    };
    for (index, label) in labels.iter().enumerate() {
        if index > 0 {
            input_separator(ui);
        }
        input_cap(ui, label, cap_width, palette);
    }
}

fn input_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 38.0), egui::Sense::hover());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "+",
        egui::FontId::proportional(18.0),
        ui.visuals().text_color(),
    );
}

fn input_cap(ui: &mut Ui, label: &str, width: f32, palette: Palette) {
    egui::Frame::new()
        .fill(palette.surface_container_high)
        .stroke(egui::Stroke::new(1.0, palette.outline))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(12, 7))
        .show(ui, |ui| {
            ui.set_width((width - 24.0).max(1.0));
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(label).size(18.0).strong());
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
                    ui.label(message);
                });
        },
    );
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

fn minimum_count_row(
    ui: &mut Ui,
    label: &str,
    id: &str,
    value: &mut Option<u8>,
    maximum: u8,
    input_width: f32,
) {
    filter_row(ui, label, input_width, |ui| {
        let mut minimum = value.unwrap_or(0);
        select_menu(
            ui,
            id,
            if minimum == 0 {
                "Any".into()
            } else {
                minimum.to_string()
            },
            input_width,
            usize::from(maximum) + 1,
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                ui.selectable_value(&mut minimum, 0, "Any");
                for count in 1..=maximum {
                    ui.selectable_value(&mut minimum, count, count.to_string());
                }
            },
        );
        *value = (minimum > 0).then_some(minimum);
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

fn history_criteria(criteria: &Criteria) -> Result<Vec<(Section, Vec<String>)>, &'static str> {
    let plan = criteria.compile()?;
    let predicates: Vec<_> = plan.system.iter().chain(&plan.planet).copied().collect();
    Ok(Section::ALL
        .iter()
        .copied()
        .filter_map(|section| {
            let descriptions: Vec<_> = predicates
                .iter()
                .copied()
                .filter(|predicate| predicate.section() == section)
                .map(|predicate| predicate.description())
                .collect();
            (!descriptions.is_empty()).then_some((section, descriptions))
        })
        .collect())
}

fn section_label(section: Section) -> &'static str {
    match section {
        Section::SystemFeatures => "System features",
        Section::SystemDemographics => "System demographics",
        Section::SystemComposition => "Planetary composition",
        Section::PlanetType => "Planet type",
        Section::Flora => "Flora",
        Section::SkyAtmospherics => "Sky & atmospherics",
        Section::PlanetFeatures => "Planet features",
        Section::GeneralFauna => "General fauna",
        Section::AquaticFauna => "Aquatic fauna",
        Section::LandFauna => "Land fauna",
        Section::AirborneFauna => "Airborne fauna",
        Section::XenoArena => "Xeno Arena",
        Section::Resources => "Resources",
        Section::Weather => "Weather",
    }
}

fn search_phase(phase: Phase, finish: Option<Finish>) -> &'static str {
    match (phase, finish) {
        (Phase::Running, _) => "Running",
        (Phase::Cancelled, _) | (_, Some(Finish::Cancelled)) => "Cancelled",
        (Phase::Failed, _) | (_, Some(Finish::Failed)) => "Failed",
        (_, Some(Finish::Results)) => "Result limit reached",
        (_, Some(Finish::Budget)) => "System limit reached",
        (_, Some(Finish::Exhausted)) => "Systems exhausted",
        (_, Some(Finish::TimeLimit)) => "Time limit reached",
        (Phase::Completed, None) => "Completed",
    }
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
    fn history_criteria_groups_actual_predicates_and_keeps_no_filter_state_explicit() {
        assert!(history_criteria(&Criteria::default()).unwrap().is_empty());
        let mut criteria = Criteria::default();
        criteria.planets.insert(PlanetFlag::Rings, true);
        criteria.systems.insert(SystemFlag::Pirate, false);
        criteria.choices.insert(Field::Star, (1 << 0) | (1 << 2));
        let groups = history_criteria(&criteria).unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, Section::SystemFeatures);
        assert!(
            groups[0]
                .1
                .iter()
                .any(|value| value.contains("Star colour:") && value.contains(" | "))
        );
        assert_eq!(groups[1].0, Section::SystemDemographics);
        assert!(groups[1].1.iter().any(|value| value == "Not Outlaw system"));
        assert_eq!(
            groups[2],
            (Section::PlanetFeatures, vec!["Has rings".into()])
        );
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

    #[test]
    fn settings_sidebar_and_history_shell_render_at_wide_and_narrow_sizes() {
        for width in [380.0, 1100.0] {
            let context = egui::Context::default();
            let controls = Arc::new(Controls::new(State::new(Settings::default())));
            let mut app = App::new(&context, controls);
            app.settings_sidebar = true;
            let texts = rendered_texts(&mut app, &context, egui::vec2(width, 840.0));
            for title in [
                "Settings",
                "History",
                "Theme",
                "UI Hotkey",
                "Notifications",
                "Screen placement",
                "Result cards",
            ] {
                assert!(
                    texts.iter().any(|text| text == title),
                    "{width}px omitted {title}: {texts:?}"
                );
            }
            app.settings_sidebar = false;
            app.page = Page::History;
            let texts = rendered_texts(&mut app, &context, egui::vec2(width, 840.0));
            assert!(texts.iter().any(|text| text == "History"));
            assert!(texts.iter().any(|text| text == "Back to Search"));
            assert!(!texts.iter().any(|text| text == "Presets"));
        }
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
                UiAction::SetPage {
                    page: UiPage::History,
                },
            )
            .unwrap();
        controls
            .submit_ui(
                generation,
                UiAction::SetTheme {
                    theme: UiTheme::NoMansSky,
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
                    toast: TestToast::ResultFound { count: 2 },
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
        assert_eq!(state.ui.page, UiPage::History);
        assert!(!state.ui.settings_sidebar);
        assert_eq!(state.ui.draft_settings.interface.theme, UiTheme::NoMansSky);
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
        assert_eq!(state.ui.toasts[0].message, "Found 2 matching results.");
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
    fn history_page_renders_retained_filter_details_and_no_filter_state() {
        fn render(criteria: Criteria) -> Vec<String> {
            let context = egui::Context::default();
            let mut settings = Settings::default();
            settings.search.criteria = criteria;
            let mut session = search_probes_model::session::Session::default();
            let origin = search_probes_model::address::Address::from_raw(0).unwrap();
            session
                .start(settings.search.clone(), origin, settings.execution())
                .unwrap();
            let mut state = State::new(settings);
            state.search_history = session.history();
            let controls = Arc::new(Controls::new(state));
            let mut app = App::new(&context, controls);
            app.page = Page::History;
            rendered_texts(&mut app, &context, egui::vec2(1100.0, 840.0))
        }

        let unfiltered = render(Criteria::default());
        assert!(
            unfiltered
                .iter()
                .any(|text| text == "No filters — every generated system is eligible.")
        );

        let mut filtered = Criteria::default();
        filtered.planets.insert(PlanetFlag::Rings, true);
        let filtered = render(filtered);
        assert!(filtered.iter().any(|text| text == "Planet features"));
        assert!(filtered.iter().any(|text| text == "• Has rings"));
    }

    #[test]
    fn interface_reconciliation_preserves_pending_theme_while_accepting_published_hotkey() {
        let previous = Settings::default();
        let mut draft = previous.clone();
        draft.interface.theme = UiTheme::NoMansSky;
        let mut published = previous.clone();
        published.interface.hotkey = InputBinding::Keyboard {
            first: KeyboardKey::F8,
            second: None,
        };
        published.interface.notifications.automatic_resume = false;
        published.interface.result_cards.location_subtitle = false;
        reconcile_settings(&mut draft, &previous, &published);
        assert_eq!(draft.interface.theme, UiTheme::NoMansSky);
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
            result_subtitle(&result(TargetKind::System), 1),
            "System · Galaxy 1"
        );
        assert_eq!(
            result_subtitle(&result(TargetKind::Planet), 2),
            "Planet · System Iousongola · Galaxy 2"
        );
        assert_eq!(
            result_subtitle(&result(TargetKind::Fauna), 3),
            "Animal · Planet New Lennon · System Iousongola · Galaxy 3"
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

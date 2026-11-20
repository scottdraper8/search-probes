use crate::{
    export::{ExportFormat, ExportRequest},
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
        CompositionField, Criteria, Field, PlanetFlag, ResourceMode, Section, SelectionSide,
        SystemFlag, WaterFilter, subtype_options_for,
    },
    fauna::{Domain, FaunaCriteria, FaunaField},
    runtime::{Action, Availability, SearchPause, State},
    session::Phase,
    settings::{
        BUDGET_LABELS, BUDGETS, MAX_PRESETS, MAX_RESULTS, Preset, Settings, TimeLimit,
        available_workers, recommended_workers, validate_name,
    },
    target::TargetKind,
    theme::UiTheme,
    vocabulary::RESOURCE_LABELS,
};
use std::sync::Arc;

const CRITERIA_SPLIT_MIN_WIDTH: f32 = 820.0;
const CRITERIA_THREE_COLUMN_MIN_WIDTH: f32 = 1230.0;
const MAIN_SPLIT_MIN_WIDTH: f32 = 2460.0;
const SEARCH_PANEL_SPLIT_MIN_WIDTH: f32 = 800.0;
const RESULT_CARD_MIN_WIDTH: f32 = 310.0;

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
    export_status: Option<String>,
    applied_theme: Option<UiTheme>,
    portal_glyphs: PortalGlyphs,
}

impl App {
    pub fn new(ctx: &egui::Context, controls: Arc<Controls>) -> Self {
        for egui_theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.style_mut_of(egui_theme, |style| {
                style.spacing.item_spacing = egui::vec2(10.0, 8.0);
                for (kind, size) in [
                    (egui::TextStyle::Body, 15.0),
                    (egui::TextStyle::Button, 15.0),
                    (egui::TextStyle::Small, 12.0),
                ] {
                    style
                        .text_styles
                        .insert(kind, egui::FontId::proportional(size));
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
            export_status: None,
            applied_theme: None,
            portal_glyphs: PortalGlyphs::new(ctx),
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
    fn send(&mut self, action: Action) {
        let pending = matches!(&action, Action::Start { .. });
        match self.controls.submit(self.state.generation, action) {
            Ok(id) => {
                self.error = None;
                if pending {
                    self.pending_search = Some(id);
                }
            }
            Err(error) => self.error = Some(error.into()),
        }
    }
    fn preset_controls(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("preset")
                .width(240.0)
                .selected_text(
                    self.selected_preset
                        .and_then(|index| self.settings.presets.get(index))
                        .map_or("Choose a preset", |preset| preset.name.as_str()),
                )
                .truncate()
                .show_ui(ui, |ui| {
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
                });
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
            if ui.button("Save preset").clicked() {
                let name = self.preset_name.trim().to_owned();
                match validate_name(&name).and_then(|_| self.settings.search.validate()) {
                    Err(error) => self.error = Some(error.into()),
                    Ok(()) => {
                        let existing =
                            self.settings.presets.iter().position(|preset| {
                                preset.name.to_lowercase() == name.to_lowercase()
                            });
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
                            self.error = Some(
                                "The Guide can display nine presets. Replace or remove one first."
                                    .into(),
                            );
                            return;
                        }
                        self.send(Action::Configure {
                            settings: self.settings.clone(),
                        });
                    }
                }
            }
            if ui
                .add_enabled(self.selected_preset.is_some(), egui::Button::new("Remove"))
                .clicked()
                && let Some(index) = self.selected_preset.take()
            {
                self.settings.presets.remove(index);
                self.preset_name.clear();
                self.send(Action::Configure {
                    settings: self.settings.clone(),
                });
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
                        self.export_status = None;
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
        ui.horizontal_wrapped(|ui| {
            let mut enabled = self.settings.search.time_limit.is_some();
            if ui.checkbox(&mut enabled, "Stop after").changed() {
                self.settings.search.time_limit = enabled.then_some(TimeLimit {
                    hours: 1,
                    minutes: 0,
                });
            }
            if let Some(limit) = &mut self.settings.search.time_limit {
                ui.add(
                    egui::DragValue::new(&mut limit.hours)
                        .range(0..=99)
                        .suffix(" h"),
                );
                ui.add(
                    egui::DragValue::new(&mut limit.minutes)
                        .range(0..=99)
                        .suffix(" min"),
                );
                ui.label("maximum 99:99");
            }
        });
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
        let valid = self.settings.validate();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    ready && !active && valid.is_ok(),
                    egui::Button::new(RichText::new("Search").strong())
                        .min_size(egui::vec2(110.0, 32.0)),
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
                    egui::Button::new(if user_paused { "Resume" } else { "Pause" })
                        .min_size(egui::vec2(80.0, 32.0)),
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
                    egui::Button::new("Cancel").min_size(egui::vec2(80.0, 32.0)),
                )
                .clicked()
            {
                self.send(Action::Cancel {});
            }
        });
        if self.pending_search.is_some() {
            ui.label("Starting search…");
        }
        if let Err(error) = valid {
            ui.colored_label(self.palette().error, error);
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
        ui.heading(RichText::new("Search Scope").size(16.0).strong());
        let selected = BUDGETS
            .iter()
            .position(|&budget| budget == self.settings.search.systems)
            .unwrap_or(1);
        egui::Grid::new("search_scope")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Systems to search");
                egui::ComboBox::from_id_salt("systems")
                    .width(180.0)
                    .selected_text(BUDGET_LABELS[selected])
                    .truncate()
                    .show_ui(ui, |ui| {
                        for (index, &budget) in BUDGETS.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.settings.search.systems,
                                budget,
                                BUDGET_LABELS[index],
                            );
                        }
                    });
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
            "Search completion",
            "The search stops after processing all systems, or finding the number of requested results",
        );
    }
    fn search_performance(&mut self, ui: &mut Ui) {
        let previous_intensity = self.settings.frame_budget_ms;
        ui.heading(RichText::new("Performance").size(16.0).strong());
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
                egui::ComboBox::from_id_salt("effort")
                    .width(input_width)
                    .selected_text(
                        intensity_levels
                            .iter()
                            .find_map(|&(value, label)| {
                                (value == self.settings.frame_budget_ms).then_some(label)
                            })
                            .unwrap_or("Unknown"),
                    )
                    .truncate()
                    .show_ui(ui, |ui| {
                        for (value, label) in intensity_levels {
                            ui.selectable_value(&mut self.settings.frame_budget_ms, value, label);
                        }
                    });
                ui.end_row();
                ui.label("Workers");
                egui::ComboBox::from_id_salt("workers")
                    .width(input_width)
                    .selected_text(match self.settings.workers {
                        None => format!("Auto ({recommended_workers})"),
                        Some(count) if count <= available_workers => count.to_string(),
                        Some(count) => format!("{count} (limited to {available_workers})"),
                    })
                    .truncate()
                    .show_ui(ui, |ui| {
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
                    });
                ui.end_row();
            });
        if self.settings.frame_budget_ms != previous_intensity {
            self.send(Action::ConfigureIntensity {
                frame_budget_ms: self.settings.frame_budget_ms,
            });
        }
        callout(
            ui,
            self.palette(),
            CalloutTone::Warning,
            "Performance impact",
            "Higher search intensity and more workers can find results faster, but may make the game less smooth",
        );
    }
    fn criteria(&mut self, ui: &mut Ui, input_width: f32) {
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
                egui::CollapsingHeader::new(
                    RichText::new("Planetary Composition").size(16.0).strong(),
                )
                .default_open(true)
                .show(ui, |ui| {
                    match_mode_row(ui, criteria, Section::Composition);
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
                    filter_row(ui, "Minimum planets", input_width, |ui| {
                        let mut minimum = criteria.minimum_planets.unwrap_or(0);
                        egui::ComboBox::from_id_salt("minimum")
                            .width(input_width)
                            .selected_text(if minimum == 0 {
                                "Any".into()
                            } else {
                                minimum.to_string()
                            })
                            .truncate()
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut minimum, 0, "Any");
                                for count in 1..=6 {
                                    ui.selectable_value(&mut minimum, count, count.to_string());
                                }
                            });
                        criteria.minimum_planets = (minimum > 0).then_some(minimum);
                    });
                });

                egui::CollapsingHeader::new(RichText::new("System Profile").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Profile);
                        choice_rows(ui, criteria, &[Field::Conflict], input_width);
                        system_flag_rows(ui, criteria, &[SystemFlag::Dissonance], input_width);
                        choice_rows(ui, criteria, &[Field::Race], input_width);
                        choice_rows(ui, criteria, &[Field::Wealth, Field::Economy], input_width);
                        water_row(ui, "system_water", &mut criteria.system_water, input_width);
                        system_flag_rows(ui, criteria, &[SystemFlag::Pirate], input_width);
                        choice_rows(
                            ui,
                            criteria,
                            &[Field::Population, Field::Anomaly, Field::Star],
                            input_width,
                        );
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
                        match_mode_row(ui, criteria, Section::GeneralFauna);
                        choice_rows(ui, criteria, &[Field::Fauna], input_width);
                        tameable_fauna_row(ui, criteria, input_width);
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
                    .show(ui, |ui| arena_criteria(ui, criteria, input_width));
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
                        match_mode_row(ui, criteria, Section::Flora);
                        choice_rows(ui, criteria, &[Field::Flora], input_width);
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

                egui::CollapsingHeader::new(RichText::new("Planet Features").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::PlanetFeatures);
                        planet_flag_rows(
                            ui,
                            criteria,
                            &[PlanetFlag::Moons, PlanetFlag::Rings],
                            input_width,
                        );
                        water_row(ui, "planet_water", &mut criteria.planet_water, input_width);
                        sentinel_row(ui, criteria, input_width);
                    });

                egui::CollapsingHeader::new(RichText::new("Planet Type").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::PlanetType);
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
                        match_mode_row(ui, criteria, Section::Resources);
                        choice_rows(ui, criteria, &[Field::ResourceRichness], input_width);
                        resource_match_row(
                            ui,
                            "Resource match",
                            "resource_mode",
                            &mut criteria.resources.mode,
                            input_width,
                        );
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
                        resource_match_row(
                            ui,
                            "Unique resource match",
                            "unique_resource_mode",
                            &mut criteria.unique_resource_mode,
                            input_width,
                        );
                        choice_rows(ui, criteria, &[Field::UniqueResources], input_width);
                    });

                egui::CollapsingHeader::new(
                    RichText::new("Sky & Atmospherics").size(16.0).strong(),
                )
                .default_open(true)
                .show(ui, |ui| {
                    match_mode_row(ui, criteria, Section::SkyAtmospherics);
                    choice_rows(
                        ui,
                        criteria,
                        &[
                            Field::Sky,
                            Field::Horizon,
                            Field::WaterColour,
                            Field::WorldColourFilter,
                        ],
                        input_width,
                    );
                });

                egui::CollapsingHeader::new(RichText::new("Weather").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Weather);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::ExtremeStorms], input_width);
                        choice_rows(
                            ui,
                            criteria,
                            &[Field::StormFrequency, Field::StormType],
                            input_width,
                        );
                    });
            });
    }
    fn results(&mut self, ui: &mut Ui) {
        let state = self.state.clone();
        ui.horizontal(|ui| {
            ui.heading("Results");
            if state.progress.is_some() || !state.results.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("Export", |ui| {
                        for format in [ExportFormat::Text, ExportFormat::Csv] {
                            if ui
                                .add_enabled(
                                    !state.results.is_empty(),
                                    egui::Button::new(format.label()),
                                )
                                .clicked()
                            {
                                match ExportRequest::new(format, &state.results) {
                                    Ok(request) => self.export_request = Some(request),
                                    Err(error) => self.error = Some(error),
                                }
                                self.export_status = None;
                                ui.close();
                            }
                        }
                        let current = state.progress.as_ref().and_then(|progress| {
                            state
                                .search_history
                                .iter()
                                .find(|record| record.progress.id == progress.id)
                        });
                        if ui
                            .add_enabled(
                                current.is_some(),
                                egui::Button::new(ExportFormat::Json.label()),
                            )
                            .clicked()
                        {
                            if let Some(record) = current {
                                match ExportRequest::search_json(record) {
                                    Ok(request) => self.export_request = Some(request),
                                    Err(error) => self.error = Some(error),
                                }
                            }
                            self.export_status = None;
                            ui.close();
                        }
                    });
                });
            }
        });
        if let Some(status) = &self.export_status {
            ui.colored_label(self.palette().success, status);
        }
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
        if let Some(progress) = &state.progress {
            ui.label(format!(
                "{} systems checked · {} results · {} elapsed",
                number(progress.systems),
                progress.results.len(),
                elapsed(progress.elapsed_ms),
            ));
            ui.add(
                egui::ProgressBar::new(
                    (progress.systems as f64 / progress.options.systems as f64) as f32,
                )
                .text(format!(
                    "{} / {} systems",
                    number(progress.systems),
                    number(progress.options.systems)
                )),
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
        } else if state.results.is_empty() {
            ui.label("Choose your criteria and start a search.");
        }
        if !state.results.is_empty() {
            ui.separator();
            let results: Vec<_> = state.results.iter().collect();
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
                                    ui.small(result_subtitle(
                                        result,
                                        u16::from(coordinates.galaxy) + 1,
                                    ));
                                    self.portal_glyphs
                                    .show(
                                        ui,
                                        &portal_code,
                                        self.palette().on_surface_variant,
                                    )
                                    .on_hover_text(format!("Portal code: {portal_code}"));
                                    ui.horizontal_wrapped(|ui| {
                                        if ui
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
                                        if ui
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
                                        if ui
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
        }
    }
}

impl App {
    /// Current draft settings, including edits that have not been applied to the game.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn take_export_request(&mut self) -> Option<ExportRequest> {
        self.export_request.take()
    }

    pub fn finish_export(&mut self, result: Result<Option<String>, String>) {
        match result {
            Ok(Some(path)) => {
                self.error = None;
                self.export_status = Some(format!("Exported results to {path}"));
            }
            Ok(None) => {}
            Err(error) => {
                self.export_status = None;
                self.error = Some(format!("Cannot export results: {error}"));
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
    }
    pub fn ui(&mut self, ui: &mut Ui) {
        let selected_theme = self.settings.interface.theme;
        if self.applied_theme != Some(selected_theme) {
            theme::apply(ui.ctx(), selected_theme);
            self.applied_theme = Some(selected_theme);
            ui.ctx().request_repaint();
        }
        let palette = self.palette();
        self.capture_keyboard(ui.ctx());
        egui::Frame::new()
            .inner_margin(16.0)
            .fill(palette.background)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("Search Probes").size(28.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(format!(
                                "UI Hotkey  ·  {}",
                                binding_text(self.settings.interface.hotkey)
                            ))
                            .clicked()
                        {
                            self.binding_window = true;
                        }
                        self.theme_control(ui);
                    });
                });
                if let Some(error) = self.error.as_ref().or(self.state.error.as_ref()) {
                    ui.colored_label(palette.error, error);
                }

                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.heading(RichText::new("Presets").size(17.0).strong());
                    self.preset_controls(ui);
                });
                ui.add_space(8.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.heading(RichText::new("Search").size(17.0).strong());
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
    if draft.interface == previous.interface {
        draft.interface = published.interface.clone();
    }
    if draft.workers == previous.workers {
        draft.workers = published.workers;
    }
    if draft.frame_budget_ms == previous.frame_budget_ms {
        draft.frame_budget_ms = published.frame_budget_ms;
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
                    ui.label(RichText::new(title).strong().color(title_color));
                    ui.label(message);
                });
        },
    );
}

fn filter_control_width(ui: &Ui) -> f32 {
    let nested_indents = 2.0 * ui.spacing().indent;
    (ui.available_width() - nested_indents - FILTER_ROW_MIN_GAP - FILTER_LABEL_MIN_WIDTH)
        .clamp(FILTER_CONTROL_MIN_WIDTH, FILTER_CONTROL_WIDTH)
}

fn resource_match_row(
    ui: &mut Ui,
    label: &str,
    id: &str,
    mode: &mut ResourceMode,
    control_width: f32,
) {
    filter_row(ui, label, control_width, |ui| {
        egui::ComboBox::from_id_salt(id)
            .width(control_width)
            .selected_text(match mode {
                ResourceMode::All => "All selected",
                ResourceMode::Any => "Any selected",
            })
            .truncate()
            .show_ui(ui, |ui| {
                ui.selectable_value(mode, ResourceMode::All, "All selected");
                ui.selectable_value(mode, ResourceMode::Any, "Any selected");
            });
    });
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
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(width)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .truncate()
        .show_ui(ui, |ui| {
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
        });
}

fn requirement(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<bool>,
    width: f32,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(match value {
            None => "Any",
            Some(true) => "Require",
            Some(false) => "Exclude",
        })
        .width(width)
        .truncate()
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "Any");
            ui.selectable_value(value, Some(true), "Require");
            ui.selectable_value(value, Some(false), "Exclude");
        });
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
    let original = if field == Field::Biome {
        criteria
            .biome_choices()
            .unwrap_or(1 << field.options().len())
    } else {
        criteria.choices.get(&field).copied().unwrap_or(0)
    };
    let mut mask = original;
    filter_row(ui, field.label(), input_width, |ui| {
        choices(
            ui,
            ("choice", field),
            &mut mask,
            field.available_options(),
            input_width,
        );
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
        egui::ComboBox::from_id_salt(id)
            .selected_text(text)
            .width(input_width)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .truncate()
            .show_ui(ui, |ui| {
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
            });
    });
}

fn match_mode_row(ui: &mut Ui, criteria: &mut Criteria, section: Section) {
    let mut mode = criteria.match_mode(section);
    ui.push_id(("match_mode", section), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut mode, ResourceMode::All, "Match all selected criteria");
            ui.radio_value(&mut mode, ResourceMode::Any, "Match any selected criteria");
        });
    });
    if mode == ResourceMode::All {
        criteria.match_modes.remove(&section);
    } else {
        criteria.match_modes.insert(section, mode);
    }
}

fn arena_criteria(ui: &mut Ui, criteria: &mut Criteria, width: f32) {
    match_mode_row(ui, criteria, Section::XenoArena);
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
                field.options().into_iter(),
                width,
            )
        });
        if mask == 0 {
            criteria.arena.choices.remove(&field);
        } else {
            criteria.arena.choices.insert(field, mask);
        }
    }
}

fn tameable_fauna_row(ui: &mut Ui, criteria: &mut Criteria, width: f32) {
    filter_row(ui, "Has tameable fauna", width, |ui| {
        requirement(ui, "arena_presence", &mut criteria.arena.presence, width);
    });
}

fn fauna_domain_section(ui: &mut Ui, criteria: &mut Criteria, domain: Domain, width: f32) {
    egui::CollapsingHeader::new(RichText::new(domain.label()).size(16.0).strong())
        .id_salt(("fauna_criteria_domain", domain))
        .default_open(true)
        .show(ui, |ui| fauna_domain(ui, criteria, domain, width));
}

fn fauna_domain(ui: &mut Ui, criteria: &mut Criteria, domain: Domain, width: f32) {
    match_mode_row(ui, criteria, domain.section());
    let mut selected = criteria.fauna.get(&domain).cloned().unwrap_or_default();
    for &field in &[
        FaunaField::Activity,
        FaunaField::Family,
        FaunaField::Category,
        FaunaField::Habitat,
    ] {
        fauna_field_row(ui, domain, field, &mut selected, width);
    }
    filter_row(ui, domain.presence_label(), width, |ui| {
        requirement(
            ui,
            ("fauna_presence", domain),
            &mut selected.presence,
            width,
        );
    });
    fauna_field_row(ui, domain, FaunaField::Hemisphere, &mut selected, width);
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
    fauna_field_row(ui, domain, FaunaField::Rarity, &mut selected, width);
    if selected.presence.is_none() && selected.choices.is_empty() && !selected.height.active() {
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
    let options = field.options(domain);
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

fn water_row(ui: &mut Ui, id: &str, water: &mut WaterFilter, input_width: f32) {
    filter_row(ui, "Has water", input_width, |ui| {
        egui::ComboBox::from_id_salt(id)
            .width(input_width)
            .selected_text(water.label())
            .truncate()
            .show_ui(ui, |ui| {
                for &value in WaterFilter::ALL {
                    ui.selectable_value(water, value, value.label());
                }
            });
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
            requirement(ui, ("planet", flag), &mut value, input_width);
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
            Field::Sentinels.available_options(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time_is_stable_beyond_one_hour() {
        assert_eq!(elapsed(0), "00:00:00");
        assert_eq!(elapsed(3_661_999), "01:01:01");
        assert_eq!(elapsed(359_940_000), "99:59:00");
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

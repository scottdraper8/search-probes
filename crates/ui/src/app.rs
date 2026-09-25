use crate::control::Control;
use eframe::egui::{self, Color32, RichText, Ui};
use search_probes_model::{
    criteria::{Criteria, Field, PlanetFlag, ResourceMode, SystemFlag},
    protocol::{Action, Availability},
    session::Phase,
    settings::{BUDGET_LABELS, BUDGETS, MAX_PRESETS, MAX_RESULTS, Preset, Settings, validate_name},
    storage,
    vocabulary::RESOURCE_LABELS,
};
use std::{path::PathBuf, time::Duration};

pub struct App {
    control: Control,
    settings: Settings,
    loaded_session: Option<String>,
    loaded_generation: Option<u64>,
    preset_name: String,
    selected_preset: Option<usize>,
    restart: bool,
    use_cache: bool,
    error: Option<String>,
    last_toggle: Option<u64>,
    last_show: Option<u64>,
    visible: bool,
    exit: bool,
    owner: Option<crate::process::Owner>,
    editing: bool,
    pending_search: Option<u64>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.style_mut_of(egui::Theme::Dark, |style| {
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
            style.visuals.panel_fill = Color32::from_rgb(18, 24, 32);
            style.visuals.selection.bg_fill = Color32::from_rgb(32, 108, 126);
        });
        let arguments: Vec<_> = std::env::args_os().collect();
        let directory = arguments
            .windows(2)
            .find(|pair| pair[0] == "--directory")
            .map(|pair| PathBuf::from(&pair[1]))
            .or_else(|| storage::user_directory().ok());
        let error = directory
            .is_none()
            .then(|| "The user application-data directory is unavailable.".to_owned());
        Self {
            control: Control::new(directory.unwrap_or_default()),
            settings: Settings::default(),
            loaded_session: None,
            loaded_generation: None,
            preset_name: String::new(),
            selected_preset: None,
            restart: false,
            use_cache: true,
            error,
            last_toggle: None,
            last_show: None,
            visible: true,
            exit: false,
            owner: None,
            editing: true,
            pending_search: None,
        }
    }
    fn send(&mut self, action: Action) {
        let pending = matches!(&action, Action::Start { .. } | Action::Continue { .. });
        let resume = matches!(
            &action,
            Action::Start { .. }
                | Action::Continue { .. }
                | Action::Cancel {}
                | Action::Navigate { .. }
                | Action::ClearHistory {}
        );
        match self.control.send(action) {
            Ok(id) => {
                self.error = None;
                if pending {
                    self.pending_search = Some(id);
                }
                if resume {
                    self.resume_game(true);
                }
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn resume_game(&mut self, visible: bool) {
        if let Some(state) = &self.control.state {
            match crate::process::passive(state.pid, visible) {
                Ok(visible) => {
                    self.visible = visible;
                    self.editing = false;
                }
                Err(error) => {
                    self.error = Some(format!(
                        "Return to No Man's Sky to resume the search: {error}"
                    ))
                }
            }
        }
    }
    fn preset_controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("preset")
                .selected_text(
                    self.selected_preset
                        .and_then(|index| self.settings.presets.get(index))
                        .map_or("Choose a preset", |preset| preset.name.as_str()),
                )
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
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.preset_name)
                    .hint_text("Preset name")
                    .desired_width(190.0)
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
            {
                if let Some(index) = self.selected_preset.take() {
                    self.settings.presets.remove(index);
                    self.preset_name.clear();
                    self.send(Action::Configure {
                        settings: self.settings.clone(),
                    });
                }
            }
        });
    }
    fn search_controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Systems to search");
            let selected = BUDGETS
                .iter()
                .position(|&budget| budget == self.settings.search.systems)
                .unwrap_or(1);
            egui::ComboBox::from_id_salt("systems")
                .selected_text(BUDGET_LABELS[selected])
                .show_ui(ui, |ui| {
                    for (index, &budget) in BUDGETS.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.settings.search.systems,
                            budget,
                            BUDGET_LABELS[index],
                        );
                    }
                });
            ui.label("Results");
            ui.add(egui::DragValue::new(&mut self.settings.search.results).range(1..=MAX_RESULTS));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.use_cache, "Show cached matches first");
            ui.checkbox(&mut self.restart, "Restart from current location");
        });
        let state = self.control.state.as_ref();
        let ready = state.is_some_and(|state| state.availability == Availability::Ready);
        let active = self.pending_search.is_some()
            || state
                .and_then(|state| state.progress.as_ref())
                .is_some_and(|progress| matches!(progress.phase, Phase::Running | Phase::Cached));
        let valid = self.settings.validate();
        ui.horizontal(|ui| {
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
                    restart: self.restart,
                    use_cache: self.use_cache,
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
            if ui.button("Hide · F7").clicked() {
                self.resume_game(false);
                self.visible = false;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        });
        if self.pending_search.is_some() {
            ui.label("Starting search…");
        }
        if let Err(error) = valid {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    }
    fn criteria(&mut self, ui: &mut Ui) {
        ui.label("Any skips a filter. Multiple choices within a field are alternatives; different fields must all match.");
        let criteria = &mut self.settings.search.criteria;
        for (title, fields) in [
            (
                "System",
                &[
                    Field::Star,
                    Field::Wealth,
                    Field::Economy,
                    Field::Conflict,
                    Field::Population,
                    Field::Race,
                    Field::SystemClass,
                    Field::Anomaly,
                ][..],
            ),
            (
                "Planet",
                &[
                    Field::Size,
                    Field::Biome,
                    Field::Subtype,
                    Field::Terrain,
                    Field::Flora,
                    Field::Fauna,
                    Field::Buildings,
                    Field::ResourceRichness,
                ][..],
            ),
            (
                "Weather",
                &[
                    Field::WeatherConditions,
                    Field::WeatherType,
                    Field::StormFrequency,
                    Field::WeatherIntensity,
                ][..],
            ),
            (
                "Colours",
                &[
                    Field::Grass,
                    Field::Plant,
                    Field::Leaf,
                    Field::Cloud,
                    Field::SkyFog,
                    Field::SkyHeightFog,
                    Field::Horizon,
                    Field::Sky,
                    Field::Sunset,
                    Field::NightSky,
                    Field::WaterColour,
                ][..],
            ),
        ] {
            egui::CollapsingHeader::new(title)
                .default_open(matches!(title, "System" | "Planet"))
                .show(ui, |ui| {
                    egui::Grid::new(title)
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            for &field in fields {
                                ui.label(field.label());
                                let mut mask = criteria.choices.get(&field).copied().unwrap_or(0);
                                choices(ui, ("choice", field), &mut mask, field.options());
                                if mask == 0 {
                                    criteria.choices.remove(&field);
                                } else {
                                    criteria.choices.insert(field, mask);
                                }
                                ui.end_row();
                            }
                            if title == "System" {
                                ui.label("Minimum planets");
                                let mut minimum = criteria.minimum_planets.unwrap_or(0);
                                egui::ComboBox::from_id_salt("minimum")
                                    .selected_text(if minimum == 0 {
                                        "Any".into()
                                    } else {
                                        minimum.to_string()
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut minimum, 0, "Any");
                                        for count in 1..=8 {
                                            ui.selectable_value(
                                                &mut minimum,
                                                count,
                                                count.to_string(),
                                            );
                                        }
                                    });
                                criteria.minimum_planets = (minimum > 0).then_some(minimum);
                                ui.end_row();
                            }
                        });
                });
        }
        egui::CollapsingHeader::new("Planet characteristics").show(ui, |ui| {
            egui::Grid::new("planet_flags")
                .num_columns(2)
                .show(ui, |ui| {
                    for &flag in PlanetFlag::ALL {
                        ui.label(flag.label());
                        let mut value = criteria.planets.get(&flag).copied();
                        requirement(ui, ("planet", flag), &mut value);
                        if let Some(value) = value {
                            criteria.planets.insert(flag, value);
                        } else {
                            criteria.planets.remove(&flag);
                        }
                        ui.end_row();
                    }
                });
        });
        egui::CollapsingHeader::new("System characteristics").show(ui, |ui| {
            egui::Grid::new("system_flags")
                .num_columns(2)
                .show(ui, |ui| {
                    for &flag in SystemFlag::ALL {
                        ui.label(flag.label());
                        let mut value = criteria.systems.get(&flag).copied();
                        requirement(ui, ("system", flag), &mut value);
                        if let Some(value) = value {
                            criteria.systems.insert(flag, value);
                        } else {
                            criteria.systems.remove(&flag);
                        }
                        ui.end_row();
                    }
                });
        });
        egui::CollapsingHeader::new("Listed resources").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut criteria.resources.mode,
                    ResourceMode::All,
                    "All selected",
                );
                ui.selectable_value(
                    &mut criteria.resources.mode,
                    ResourceMode::Any,
                    "Any selected",
                );
            });
            let mut selected = u64::from(criteria.resources.selected);
            choices(ui, "resources", &mut selected, RESOURCE_LABELS);
            criteria.resources.selected = selected as u32;
            ui.small("A planet lists up to three resources. An empty selection skips this filter.");
        });
        ui.add_space(8.0);
        ui.small("Colours describe generated palettes. Lighting and atmosphere can change their appearance.");
    }
    fn results(&mut self, ui: &mut Ui) {
        ui.heading("Results");
        let Some(state) = self.control.state.clone() else {
            ui.label("Waiting for the game.");
            return;
        };
        if state.availability != Availability::Ready {
            ui.label(match state.availability {
                Availability::MainMenu => "Load a save to search.",
                Availability::Unsupported => {
                    "This game build is not supported by the installed mod."
                }
                Availability::Stopped => "The game has closed.",
                Availability::Failed => "The runtime could not start.",
                _ => "Starting the game runtime…",
            });
        }
        if let Some(progress) = state.progress {
            ui.label(format!(
                "{} systems checked · {} results",
                number(progress.systems),
                progress.results.len()
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
            if progress.phase == Phase::Cached {
                ui.label(format!(
                    "{} matches found in systems already searched.",
                    progress.cached_results
                ));
                let full = progress.results.len() >= MAX_RESULTS;
                let label = if full {
                    "Continue live search · replace displayed results"
                } else {
                    "Continue live search · up to 100 results"
                };
                if ui
                    .add_enabled(
                        self.pending_search.is_none() && state.availability == Availability::Ready,
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    self.send(Action::Continue {
                        results: MAX_RESULTS,
                    });
                }
            } else {
                ui.label(match progress.phase {
                    Phase::Running => "Searching…",
                    Phase::Completed => "Search complete",
                    Phase::Cancelled => "Search cancelled",
                    Phase::Failed => "Search stopped after an error",
                    Phase::Cached => "",
                });
            }
            if progress.previous_systems > 0 {
                ui.small(format!(
                    "Continuing beyond {} previously checked systems.",
                    number(progress.previous_systems)
                ));
            }
            if state.metrics.elapsed_ms > 0 && progress.phase == Phase::Running {
                ui.small(format!(
                    "{:.0} generated systems/s · {} served from cache",
                    state.metrics.generated_systems as f64 * 1000.0
                        / state.metrics.elapsed_ms as f64,
                    number(progress.cached_systems)
                ));
            }
            if progress.history_evicted {
                ui.small("Continuation for the oldest filter was removed from session history.");
            }
            if let Some(error) = progress.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("results")
                .show(ui, |ui| {
                    for result in progress.results {
                        ui.push_id(result.address.raw(), |ui| {
                            let coordinates = result.address.coordinates();
                            ui.label(
                                RichText::new(
                                    result
                                        .name
                                        .as_deref()
                                        .filter(|name| !name.is_empty())
                                        .map(str::to_owned)
                                        .unwrap_or_else(|| {
                                            format!("Planet {}", coordinates.planet)
                                        }),
                                )
                                .strong(),
                            );
                            ui.small(format!(
                                "Galaxy {} · {}, {}, {} · System {}",
                                u16::from(coordinates.galaxy) + 1,
                                coordinates.x,
                                coordinates.y,
                                coordinates.z,
                                coordinates.system
                            ));
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        state.availability == Availability::Ready,
                                        egui::Button::new("Set destination"),
                                    )
                                    .clicked()
                                {
                                    self.send(Action::Navigate {
                                        address: result.address,
                                    });
                                }
                                if ui.button("Copy address").clicked() {
                                    ui.ctx().copy_text(format!("{:016X}", result.address.raw()));
                                }
                                if state.destination == Some(result.address) {
                                    ui.label("Active destination");
                                }
                            });
                            ui.separator();
                        });
                    }
                });
        } else {
            ui.label("Choose your criteria and start a search.");
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.control.poll();
        if let Some(state) = &self.control.state {
            if self.loaded_session.as_deref() != Some(&state.session) {
                self.settings = state.settings.clone();
                self.loaded_session = Some(state.session.clone());
                self.last_toggle = None;
                self.last_show = None;
                self.selected_preset = None;
                self.pending_search = None;
                self.loaded_generation = None;
                match crate::process::Owner::open(state.pid) {
                    Ok(owner) => self.owner = Some(owner),
                    Err(_) => self.exit = true,
                }
            }
            if self.loaded_generation != Some(state.generation) {
                self.pending_search = None;
                self.loaded_generation = Some(state.generation);
            }
            let show = self.last_show.is_some_and(|last| last != state.show);
            if show {
                self.settings = state.settings.clone();
                self.selected_preset = self
                    .settings
                    .presets
                    .iter()
                    .position(|preset| preset.search == self.settings.search);
                self.preset_name = self
                    .selected_preset
                    .map(|index| self.settings.presets[index].name.clone())
                    .unwrap_or_default();
            }
            if show || self.last_toggle.is_some_and(|last| last != state.toggle) {
                self.visible = show || !self.visible || !self.editing;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
                if self.visible {
                    self.editing = true;
                    if let Err(error) = crate::process::interactive() {
                        self.error = Some(error.to_string());
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else {
                    let _ = crate::process::passive(state.pid, false);
                }
            }
            if self
                .pending_search
                .is_some_and(|id| state.last_command >= id)
            {
                self.pending_search = None;
            }
            self.last_toggle = Some(state.toggle);
            self.last_show = Some(state.show);
            if state.availability == Availability::Stopped {
                self.exit = true;
            }
        }
        if self.owner.as_ref().is_some_and(|owner| !owner.alive()) {
            self.exit = true;
        }
        if self.exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if !self.exit && ctx.input(|input| input.viewport().close_requested()) {
            self.resume_game(false);
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.visible = false;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::Frame::new().inner_margin(16.0).fill(Color32::from_rgb(18,24,32)).show(ui,|ui| {
            ui.horizontal(|ui| {ui.heading(RichText::new("Search Probes").size(28.0));ui.label("Find your next world");});
            if let Some(error)=self.error.as_ref().or(self.control.error.as_ref()).or_else(||self.control.state.as_ref().and_then(|state|state.error.as_ref())) {ui.colored_label(Color32::LIGHT_RED,error);}
            self.preset_controls(ui);ui.separator();self.search_controls(ui);ui.separator();
            ui.columns(2,|columns| {
                egui::ScrollArea::vertical().id_salt("criteria").show(&mut columns[0],|ui| {
                    self.criteria(ui);
                    egui::CollapsingHeader::new("Search performance").show(ui,|ui| {
                        ui.horizontal(|ui| {ui.label("Workers");egui::ComboBox::from_id_salt("workers").selected_text(self.settings.workers.to_string()).show_ui(ui,|ui| {for count in [1,2,4,8,16] {ui.selectable_value(&mut self.settings.workers,count,count.to_string());}});});
                        ui.horizontal(|ui| {ui.label("Game time per batch");egui::ComboBox::from_id_salt("effort").selected_text(format!("{} ms",self.settings.frame_budget_ms)).show_ui(ui,|ui| {for (value,label) in [(2,"2 ms · Lowest impact"),(4,"4 ms · Background"),(8,"8 ms · Balanced"),(16,"16 ms · Search priority")] {ui.selectable_value(&mut self.settings.frame_budget_ms,value,label);}});});
                        ui.small("More workers and longer batches may reduce game frame rate. Individual native calls can exceed the batch target.");
                        if ui.button("Apply settings").clicked() {self.send(Action::Configure {settings:self.settings.clone()});}
                        if ui.button("Clear session cache and continuation").clicked() {self.send(Action::ClearHistory {});}
                    });
                });
                self.results(&mut columns[1]);
            });
        });
    }
}

fn choices(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    mask: &mut u64,
    options: &[&str],
) {
    let text = if *mask == 0 {
        "Any".to_owned()
    } else if mask.count_ones() == 1 {
        options
            .get(mask.trailing_zeros() as usize)
            .unwrap_or(&"Unknown")
            .to_string()
    } else {
        format!("{} selected", mask.count_ones())
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(185.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            if ui.selectable_label(*mask == 0, "Any").clicked() {
                *mask = 0;
            }
            for (index, &label) in options.iter().enumerate() {
                let mut selected = *mask & (1 << index) != 0;
                if ui.checkbox(&mut selected, label).changed() {
                    if selected {
                        *mask |= 1 << index;
                    } else {
                        *mask &= !(1 << index);
                    }
                }
            }
        });
}

fn requirement(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut Option<bool>) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(match value {
            None => "Any",
            Some(true) => "Require",
            Some(false) => "Exclude",
        })
        .width(185.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "Any");
            ui.selectable_value(value, Some(true), "Require");
            ui.selectable_value(value, Some(false), "Exclude");
        });
}

fn number(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

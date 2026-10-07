use crate::control::Control;
use eframe::egui::{self, Color32, RichText, Ui};
use search_probes_model::{
    criteria::{Criteria, Field, GiantFilter, PlanetFlag, ResourceMode, SystemFlag},
    protocol::{Action, Availability},
    session::Phase,
    settings::{BUDGET_LABELS, BUDGETS, MAX_PRESETS, MAX_RESULTS, Preset, Settings, validate_name},
    storage,
    vocabulary::RESOURCE_LABELS,
};
use std::{path::PathBuf, time::Duration};

const CRITERIA_SPLIT_MIN_WIDTH: f32 = 820.0;
const MAIN_SPLIT_MIN_WIDTH: f32 = 960.0;
const SEARCH_PANEL_SPLIT_MIN_WIDTH: f32 = 800.0;
const RESULT_CARD_MIN_WIDTH: f32 = 280.0;

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
    last_open: Option<u64>,
    visible: bool,
    exit: bool,
    owner: Option<crate::process::Owner>,
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
            last_open: None,
            visible: true,
            exit: false,
            owner: None,
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
                    self.resume_game();
                }
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn resume_game(&mut self) {
        if let Some(state) = &self.control.state {
            match crate::process::focus_game(state.pid) {
                Ok(()) => {}
                Err(error) => {
                    self.error = Some(format!(
                        "Return to No Man's Sky to resume the search: {error}"
                    ))
                }
            }
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
        let state = self.control.state.as_ref();
        let ready = state.is_some_and(|state| state.availability == Availability::Ready);
        let active = self.pending_search.is_some()
            || state
                .and_then(|state| state.progress.as_ref())
                .is_some_and(|progress| matches!(progress.phase, Phase::Running | Phase::Cached));
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
        });
        if self.pending_search.is_some() {
            ui.label("Starting search…");
        }
        if let Err(error) = valid {
            ui.colored_label(Color32::LIGHT_RED, error);
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
        ui.add_space(4.0);
        ui.checkbox(&mut self.use_cache, "Show cached matches first");
        ui.checkbox(&mut self.restart, "Restart from current location");
    }
    fn search_performance(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Performance").size(16.0).strong());
        let input_width = filter_control_width(ui);
        egui::Grid::new("performance_settings")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Workers");
                egui::ComboBox::from_id_salt("workers")
                    .width(input_width)
                    .selected_text(self.settings.workers.to_string())
                    .show_ui(ui, |ui| {
                        for count in [1, 2, 4, 8, 16] {
                            ui.selectable_value(
                                &mut self.settings.workers,
                                count,
                                count.to_string(),
                            );
                        }
                    });
                ui.end_row();
                ui.label("Game time per batch");
                egui::ComboBox::from_id_salt("effort")
                    .width(input_width)
                    .selected_text(format!("{} ms", self.settings.frame_budget_ms))
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            (2, "2 ms · Lowest impact"),
                            (4, "4 ms · Background"),
                            (8, "8 ms · Balanced"),
                            (16, "16 ms · Search priority"),
                        ] {
                            ui.selectable_value(&mut self.settings.frame_budget_ms, value, label);
                        }
                    });
                ui.end_row();
            });
        ui.small("More workers and longer batches may reduce game frame rate. Individual native calls can exceed the batch target.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Apply settings").clicked() {
                self.send(Action::Configure {
                    settings: self.settings.clone(),
                });
            }
            if ui.button("Clear session cache and continuation").clicked() {
                self.send(Action::ClearHistory {});
            }
        });
    }
    fn criteria(&mut self, ui: &mut Ui, input_width: f32) {
        ui.label("Any skips a filter. Multiple choices within a field are alternatives; different fields must all match.");
        if ui.available_width() >= CRITERIA_SPLIT_MIN_WIDTH {
            ui.columns(2, |columns| {
                self.system_criteria(&mut columns[0], input_width);
                self.planet_criteria(&mut columns[1], input_width);
            });
        } else {
            self.system_criteria(ui, input_width);
            ui.add_space(8.0);
            self.planet_criteria(ui, input_width);
        }
    }
    fn criteria_panel(&mut self, ui: &mut Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.heading(RichText::new("Criteria").size(17.0).strong());
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
                    giant_filter_row(ui, criteria, input_width);
                    system_flag_rows(
                        ui,
                        criteria,
                        &[
                            SystemFlag::Rgb,
                            SystemFlag::Dead,
                            SystemFlag::Relic,
                            SystemFlag::Waterworld,
                            SystemFlag::Weird,
                            SystemFlag::Infested,
                            SystemFlag::CorruptSentinels,
                            SystemFlag::DeepWater,
                            SystemFlag::ExtremeStorms,
                            SystemFlag::Water,
                        ],
                        input_width,
                    );
                    filter_row(ui, "Minimum planets", input_width, |ui| {
                        let mut minimum = criteria.minimum_planets.unwrap_or(0);
                        egui::ComboBox::from_id_salt("minimum")
                            .width(input_width)
                            .selected_text(if minimum == 0 {
                                "Any".into()
                            } else {
                                minimum.to_string()
                            })
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
                        choice_rows(ui, criteria, &[Field::Conflict, Field::Race], input_width);
                        choice_rows(ui, criteria, &[Field::Wealth, Field::Economy], input_width);
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
    fn planet_criteria(&mut self, ui: &mut Ui, input_width: f32) {
        let criteria = &mut self.settings.search.criteria;
        egui::CollapsingHeader::new(RichText::new("Planet Criteria").size(19.0).strong())
            .default_open(true)
            .show(ui, |ui| {
                egui::CollapsingHeader::new(
                    RichText::new("Biome & Environment").size(16.0).strong(),
                )
                .default_open(true)
                .show(ui, |ui| {
                    choice_rows(ui, criteria, &[Field::Biome, Field::Subtype], input_width);
                    planet_flag_rows(ui, criteria, &[PlanetFlag::Rgb], input_width);
                    choice_rows(ui, criteria, &[Field::Flora], input_width);
                });

                egui::CollapsingHeader::new(RichText::new("Colours").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(
                            ui,
                            criteria,
                            &[
                                Field::Cloud,
                                Field::Sky,
                                Field::Grass,
                                Field::SkyHeightFog,
                                Field::Horizon,
                                Field::Leaf,
                                Field::NightSky,
                                Field::Plant,
                                Field::SkyFog,
                                Field::Sunset,
                                Field::WaterColour,
                            ],
                            input_width,
                        );
                    });

                egui::CollapsingHeader::new(RichText::new("Conditions").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        sentinel_row(ui, criteria, input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::DeepWater], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Water], input_width);
                    });

                egui::CollapsingHeader::new(RichText::new("Creatures").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(ui, criteria, &[Field::Fauna], input_width);
                        planet_flag_rows(
                            ui,
                            criteria,
                            &[
                                PlanetFlag::CreatureDiscovery,
                                PlanetFlag::CreatureTaming,
                                PlanetFlag::RobotCreatureDiscovery,
                                PlanetFlag::WeirdCreatureDiscovery,
                            ],
                            input_width,
                        );
                    });

                egui::CollapsingHeader::new(RichText::new("Features").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(ui, criteria, &[Field::Buildings], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::FloatingIslands], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Moons], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Rings], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Scrap], input_width);
                        choice_rows(ui, criteria, &[Field::Terrain], input_width);
                    });

                egui::CollapsingHeader::new(RichText::new("Resources").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        choice_rows(ui, criteria, &[Field::ResourceRichness], input_width);
                        filter_row(ui, "Resource match", input_width, |ui| {
                            egui::ComboBox::from_id_salt("resource_mode")
                                .width(input_width)
                                .selected_text(match criteria.resources.mode {
                                    ResourceMode::All => "All selected",
                                    ResourceMode::Any => "Any selected",
                                })
                                .show_ui(ui, |ui| {
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
                        });
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
                    });

                egui::CollapsingHeader::new(RichText::new("Type").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        if let Some(required) =
                            criteria.planets.get(&PlanetFlag::NonGasGiant).copied()
                        {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(if required {
                                    "Saved rule: require a non-gas giant."
                                } else {
                                    "Saved rule: exclude non-gas giants."
                                });
                                if ui.button("Clear saved rule").clicked() {
                                    criteria.planets.remove(&PlanetFlag::NonGasGiant);
                                }
                            });
                        }
                        planet_flag_rows(
                            ui,
                            criteria,
                            &[PlanetFlag::Dead, PlanetFlag::Weird, PlanetFlag::Infested],
                            input_width,
                        );
                        choice_rows(ui, criteria, &[Field::Size], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Relic], input_width);
                    });

                egui::CollapsingHeader::new(RichText::new("Weather").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        planet_flag_rows(ui, criteria, &[PlanetFlag::ExtremeWeather], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Paradise], input_width);
                        choice_rows(ui, criteria, &[Field::StormFrequency], input_width);
                        choice_rows(
                            ui,
                            criteria,
                            &[Field::WeatherIntensity, Field::WeatherType],
                            input_width,
                        );
                    });
            });
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
        if let Some(progress) = &state.progress {
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
            if let Some(error) = &progress.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        } else if state.results.is_empty() {
            ui.label("Choose your criteria and start a search.");
        }
        if !state.results.is_empty() {
            ui.separator();
            let current = state.player.map(|address| address.system());
            let results: Vec<_> = state
                .results
                .iter()
                .filter(|result| current != Some(result.address.system()))
                .collect();
            if results.is_empty() {
                ui.label("The current system is omitted from the result list.");
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("results")
                .show(ui, |ui| {
                    let gap = ui.spacing().item_spacing.x;
                    let available = ui.available_width();
                    let columns = ((available + gap) / (RESULT_CARD_MIN_WIDTH + gap))
                        .floor()
                        .max(1.0) as usize;
                    for (row, results) in results.chunks(columns).enumerate() {
                        ui.columns(columns, |cards| {
                            for (column, result) in results.iter().enumerate() {
                                let index = row * columns + column;
                                cards[column].push_id(result.address.raw(), |ui| {
                                    egui::Frame::group(ui.style()).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        let coordinates = result.address.coordinates();
                                        let name = result
                                            .name
                                            .as_deref()
                                            .filter(|name| !name.is_empty())
                                            .map(str::to_owned)
                                            .unwrap_or_else(|| {
                                                format!("Planet {}", coordinates.planet)
                                            });
                                        ui.label(
                                            RichText::new(format!("{}. {name}", index + 1))
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
                                        ui.horizontal_wrapped(|ui| {
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
                                                ui.ctx().copy_text(format!(
                                                    "{:016X}",
                                                    result.address.raw()
                                                ));
                                            }
                                            if state.destination == Some(result.address) {
                                                ui.label("Active destination");
                                            }
                                        });
                                    });
                                });
                            }
                        });
                    }
                });
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
                self.last_open = None;
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
            let open = self.last_open.is_some_and(|last| last != state.open);
            if open || self.last_toggle.is_some_and(|last| last != state.toggle) {
                self.visible = open || !self.visible;
                if open {
                    self.settings = state.settings.clone();
                    self.selected_preset = None;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
                if self.visible {
                    if let Err(error) = crate::process::interactive() {
                        self.error = Some(error.to_string());
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else {
                    let _ = crate::process::hide(state.pid);
                }
            }
            if self
                .pending_search
                .is_some_and(|id| state.last_command >= id)
            {
                self.pending_search = None;
            }
            self.last_toggle = Some(state.toggle);
            self.last_open = Some(state.open);
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
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::Frame::new()
            .inner_margin(16.0)
            .fill(Color32::from_rgb(18, 24, 32))
            .show(ui, |ui| {
                ui.heading(RichText::new("Search Probes").size(28.0));
                if let Some(error) =
                    self.error
                        .as_ref()
                        .or(self.control.error.as_ref())
                        .or_else(|| {
                            self.control
                                .state
                                .as_ref()
                                .and_then(|state| state.error.as_ref())
                        })
                {
                    ui.colored_label(Color32::LIGHT_RED, error);
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
    }
}

const FILTER_CONTROL_WIDTH: f32 = 185.0;
const FILTER_CONTROL_MIN_WIDTH: f32 = 64.0;
const FILTER_LABEL_MIN_WIDTH: f32 = 72.0;
const FILTER_ROW_MIN_GAP: f32 = 12.0;

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
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id.clone(), color)
        .size()
        .x;
    let label_width = text_width.min(max_label_width);
    let label_height = ui
        .painter()
        .layout(label.to_owned(), font_id, color, label_width)
        .size()
        .y
        .max(ui.spacing().interact_size.y);
    let row_height = label_height.max(ui.spacing().interact_size.y);

    ui.allocate_ui_with_layout(
        egui::vec2(row_width, row_height),
        egui::Layout::left_to_right(egui::Align::Center),
        |row| {
            row.spacing_mut().item_spacing.x = 0.0;
            row.allocate_ui_with_layout(
                egui::vec2(label_width, row_height),
                egui::Layout::left_to_right(egui::Align::Center),
                |label_ui| {
                    label_ui.add(egui::Label::new(label).wrap());
                },
            );
            row.add_space((row_width - label_width - control_width).max(0.0));
            add_control(row);
        },
    );
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
    let mut mask = criteria.choices.get(&field).copied().unwrap_or(0);
    filter_row(ui, field.label(), input_width, |ui| {
        choices(
            ui,
            ("choice", field),
            &mut mask,
            field.available_options(),
            input_width,
        );
    });
    if mask == 0 {
        criteria.choices.remove(&field);
    } else {
        criteria.choices.insert(field, mask);
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

fn giant_filter_row(ui: &mut Ui, criteria: &mut Criteria, input_width: f32) {
    let original = criteria.giant_filter();
    let mut selected = original;
    filter_row(ui, "Contains a giant planet", input_width, |ui| {
        egui::ComboBox::from_id_salt("system_giant")
            .width(input_width)
            .selected_text(selected.map_or("Custom saved selection", GiantFilter::label))
            .show_ui(ui, |ui| {
                if original.is_none() {
                    ui.label("Choose an option to replace the saved combination of giant filters.");
                }
                for &filter in GiantFilter::ALL {
                    ui.selectable_value(&mut selected, Some(filter), filter.label());
                }
            });
    });
    if selected != original {
        if let Some(filter) = selected {
            criteria.set_giant_filter(filter);
        }
    }
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

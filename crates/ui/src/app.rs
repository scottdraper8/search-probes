use egui::{self, Color32, RichText, Ui};
use search_probes_model::controls::Controls;
use search_probes_model::{
    criteria::{
        CompositionField, Criteria, Field, PlanetFlag, ResourceMode, Section, SelectionSide,
        SystemFlag, WaterFilter, subtype_options_for,
    },
    fauna::{Domain, FaunaField},
    runtime::{Action, Availability, SearchPause, State},
    session::Phase,
    settings::{BUDGET_LABELS, BUDGETS, MAX_PRESETS, MAX_RESULTS, Preset, Settings, validate_name},
    vocabulary::RESOURCE_LABELS,
};
use std::sync::Arc;

const CRITERIA_SPLIT_MIN_WIDTH: f32 = 820.0;
const MAIN_SPLIT_MIN_WIDTH: f32 = 960.0;
const SEARCH_PANEL_SPLIT_MIN_WIDTH: f32 = 800.0;
const RESULT_CARD_MIN_WIDTH: f32 = 280.0;

pub struct App {
    controls: Arc<Controls>,
    state: State,
    settings: Settings,
    loaded_generation: Option<u64>,
    preset_name: String,
    selected_preset: Option<usize>,
    restart: bool,
    use_cache: bool,
    error: Option<String>,
    published_settings: Option<Settings>,
    pending_search: Option<u64>,
}

impl App {
    pub fn new(ctx: &egui::Context, controls: Arc<Controls>) -> Self {
        ctx.set_theme(egui::Theme::Dark);
        ctx.style_mut_of(egui::Theme::Dark, |style| {
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
        Self {
            state: controls.snapshot(),
            controls,
            settings: Settings::default(),
            loaded_generation: None,
            preset_name: String::new(),
            selected_preset: None,
            restart: false,
            use_cache: true,
            error: None,
            published_settings: None,
            pending_search: None,
        }
    }
    fn send(&mut self, action: Action) {
        let pending = matches!(&action, Action::Start { .. } | Action::Continue { .. });
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
        let state = &self.state;
        let ready = state.availability == Availability::Ready;
        let active = self.pending_search.is_some()
            || state
                .progress
                .as_ref()
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
                    .truncate()
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
                    .truncate()
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
                            SelectionSide::Exclude,
                            "Exclude system by planet size",
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

                egui::CollapsingHeader::new(RichText::new("System Profile").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Profile);
                        water_row(ui, "system_water", &mut criteria.system_water, input_width);
                        system_flag_rows(ui, criteria, &[SystemFlag::Dissonance], input_width);
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
                    match_mode_row(ui, criteria, Section::Environment);
                    choice_rows(ui, criteria, &[Field::Biome, Field::Subtype], input_width);
                    choice_rows(ui, criteria, &[Field::Flora], input_width);
                    planet_flag_rows(
                        ui,
                        criteria,
                        &[PlanetFlag::GlowingGrass, PlanetFlag::GlowingFlora],
                        input_width,
                    );
                });

                egui::CollapsingHeader::new(RichText::new("Colours").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Colours);
                        choice_rows(
                            ui,
                            criteria,
                            &[
                                Field::WorldColourFilter,
                                Field::Cloud,
                                Field::Sky,
                                Field::FloraColour,
                                Field::Grass,
                                Field::SkyHeightFog,
                                Field::Horizon,
                                Field::NightSky,
                                Field::SkyFog,
                                Field::WaterColour,
                            ],
                            input_width,
                        );
                    });

                egui::CollapsingHeader::new(RichText::new("Conditions").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Conditions);
                        sentinel_row(ui, criteria, input_width);
                        water_row(ui, "planet_water", &mut criteria.planet_water, input_width);
                    });

                egui::CollapsingHeader::new(RichText::new("Fauna").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Fauna);
                        choice_rows(ui, criteria, &[Field::Fauna], input_width);
                        planet_flag_rows(
                            ui,
                            criteria,
                            &[
                                PlanetFlag::PlayerAttackingFauna,
                                PlanetFlag::VileBrood,
                                PlanetFlag::TitanWorms,
                                PlanetFlag::Sandworms,
                            ],
                            input_width,
                        );
                        for &domain in Domain::ALL {
                            egui::CollapsingHeader::new(domain.label())
                                .id_salt(("fauna_domain", domain))
                                .default_open(false)
                                .show(ui, |ui| fauna_domain(ui, criteria, domain, input_width));
                        }
                    });

                egui::CollapsingHeader::new(RichText::new("Features").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Features);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Moons], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::Rings], input_width);
                        choice_rows(ui, criteria, &[Field::Terrain], input_width);
                    });

                egui::CollapsingHeader::new(RichText::new("Resources").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Resources);
                        choice_rows(ui, criteria, &[Field::ResourceRichness], input_width);
                        resource_match_row(
                            ui,
                            "Unique resource match",
                            "unique_resource_mode",
                            &mut criteria.unique_resource_mode,
                            input_width,
                        );
                        choice_rows(ui, criteria, &[Field::UniqueResources], input_width);
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
                    });

                egui::CollapsingHeader::new(RichText::new("Type").size(16.0).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        match_mode_row(ui, criteria, Section::Type);
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
                        match_mode_row(ui, criteria, Section::Weather);
                        choice_rows(ui, criteria, &[Field::StormFrequency], input_width);
                        planet_flag_rows(ui, criteria, &[PlanetFlag::ExtremeStorms], input_width);
                        choice_rows(ui, criteria, &[Field::StormType], input_width);
                    });
            });
    }
    fn results(&mut self, ui: &mut Ui) {
        ui.heading("Results");
        let state = self.state.clone();
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
                    Phase::Running => match state.search_pause {
                        Some(SearchPause::Loading) => "Search paused while the game loads.",
                        Some(SearchPause::OtherGalaxy) => {
                            "Search paused: return to its starting galaxy to resume."
                        }
                        None => "Searching…",
                    },
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
            if state.metrics.elapsed_ms > 0
                && progress.phase == Phase::Running
                && state.search_pause.is_none()
            {
                ui.small(format!(
                    "Average {:.0} generated systems/s · {} served from cache",
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
            let results: Vec<_> = state.results.iter().collect();
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
                                                match result.kind {
                                                    search_probes_model::target::TargetKind::Planet => format!("Planet {}", coordinates.planet),
                                                    kind => kind.label().to_owned(),
                                                }
                                            });
                                        ui.label(
                                            RichText::new(format!("{}. {name}", index + 1))
                                                .strong(),
                                        );
                                        ui.small(format!(
                                            "{} · Galaxy {} · {}, {}, {} · System {}",
                                            result.kind.label(),
                                            u16::from(coordinates.galaxy) + 1,
                                            coordinates.x,
                                            coordinates.y,
                                            coordinates.z,
                                            coordinates.system
                                        ));
                                        ui.horizontal_wrapped(|ui| {
                                            if ui
                                                .add_enabled(
                                                    state.availability == Availability::Ready
                                                        && state.player.is_some_and(|player| player.coordinates().galaxy == coordinates.galaxy),
                                                    egui::Button::new("Set destination"),
                                                )
                                                .clicked()
                                            {
                                                self.send(Action::Navigate {
                                                    target: result.target(),
                                                });
                                            }
                                            if ui
                                                .button("Copy portal code")
                                                .on_hover_text(format!(
                                                    "{}\nUse a portal in Galaxy {}. {}",
                                                    result.target().portal_code(),
                                                    u16::from(coordinates.galaxy) + 1,
                                                    if result.kind == search_probes_model::target::TargetKind::Planet { "Arrives on the target planet." } else { "Arrives through the first planet’s portal in this system." }
                                                ))
                                                .clicked()
                                            {
                                                ui.ctx().copy_text(result.target().portal_code());
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
                                });
                            }
                        });
                    }
                });
        }
    }
}

impl App {
    /// Current draft settings, including edits that have not been applied to the game.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn refresh(&mut self) {
        self.state = self.controls.snapshot();
        let state = &self.state;
        if self.loaded_generation != Some(state.generation) {
            self.pending_search = None;
            self.loaded_generation = Some(state.generation);
        }
        if self.published_settings.as_ref() != Some(&state.settings) {
            self.settings = state.settings.clone();
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
        egui::Frame::new()
            .inner_margin(16.0)
            .fill(Color32::from_rgb(18, 24, 32))
            .show(ui, |ui| {
                ui.heading(RichText::new("Search Probes").size(28.0));
                if let Some(error) = self.error.as_ref().or(self.state.error.as_ref()) {
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
            "The water's own colour in clear daylight. Reflections, foam and nearby lights can change its appearance.",
        )
    } else if label == Field::NightSky.label() {
        Some(
            "Clear nighttime sky colour. Stars, nearby planets, storms and lights can change what you see.",
        )
    } else if [
        Field::Cloud,
        Field::Sky,
        Field::Horizon,
        Field::SkyFog,
        Field::SkyHeightFog,
    ]
    .iter()
    .any(|field| label == field.label())
    {
        Some(
            "Typical colour in clear daylight. Looking toward the sun, sunset, storms and nearby lights can change what you see.",
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

fn fauna_domain(ui: &mut Ui, criteria: &mut Criteria, domain: Domain, width: f32) {
    match_mode_row(ui, criteria, domain.section());
    let mut selected = criteria.fauna.get(&domain).cloned().unwrap_or_default();
    filter_row(ui, domain.presence_label(), width, |ui| {
        requirement(
            ui,
            ("fauna_presence", domain),
            &mut selected.presence,
            width,
        );
    });
    for &field in FaunaField::ALL {
        let options = field.options(domain);
        if options.is_empty() {
            continue;
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
    height_bound_row(
        ui,
        (domain, "minimum"),
        "Minimum height (m)",
        &mut selected.height.minimum_cm,
        width,
        0,
    );
    height_bound_row(
        ui,
        (domain, "maximum"),
        "Maximum height (m)",
        &mut selected.height.maximum_cm,
        width,
        100_000,
    );
    if selected.presence.is_none() && selected.choices.is_empty() && !selected.height.active() {
        criteria.fauna.remove(&domain);
    } else {
        criteria.fauna.insert(domain, selected);
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
                if let Some(cm) = bound {
                    let mut metres = *cm as f64 / 100.;
                    if ui
                        .add(
                            egui::DragValue::new(&mut metres)
                                .range(0.0..=1000.0)
                                .speed(0.1)
                                .max_decimals(2),
                        )
                        .changed()
                    {
                        *cm = (metres * 100.).round() as u32;
                    }
                }
            })
        });
    });
}

fn water_row(ui: &mut Ui, id: &str, water: &mut WaterFilter, input_width: f32) {
    filter_row(ui, "Water", input_width, |ui| {
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
        if index > 0 && (digits.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_labels_and_long_choices_preserve_the_control_column() {
        for row_width in [240.0, 440.0] {
            let ctx = egui::Context::default();
            let mut controls = Vec::new();
            let mut heights = Vec::new();
            let mut right = 0.0;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 1000.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    controls.clear();
                    heights.clear();
                    ui.set_width(row_width);
                    right = ui.available_rect_before_wrap().right();
                    for (index, label) in [
                        "Water",
                        "Exclude system by biome subtype",
                        "Include system by planet size",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let before = ui.cursor().top();
                        filter_row(ui, label, 120.0, |ui| {
                            let response = egui::ComboBox::from_id_salt(index)
                                .width(120.0)
                                .truncate()
                                .selected_text(if index == 2 {
                                    "A very long selected value that should stay inside the column"
                                } else {
                                    "Any"
                                })
                                .show_ui(ui, |_| {})
                                .response;
                            controls.push(response.rect);
                        });
                        heights.push(ui.cursor().top() - before);
                    }
                },
            );
            output.drop_without_applying_deltas();
            assert_eq!(controls.len(), 3);
            for rect in &controls {
                assert!((rect.left() - controls[0].left()).abs() < 0.1);
                assert!((rect.right() - controls[0].right()).abs() < 0.1);
                assert!(rect.right() <= right + 0.1);
            }
            if row_width == 240.0 {
                assert!(heights[1] > heights[0]);
            }
        }
    }
}

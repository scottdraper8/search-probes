use crate::{
    address::Address,
    criteria::{Criteria, Plan, Predicate, Section, Stage},
    facts::{FactSnapshot, Facts, Verdict},
    session::{Finish, Progress, SearchRecord},
    settings::{Settings, SettingsRecovery, validate_frame_budget},
    target::Target,
    ui_control::UiState,
};
use serde::{Deserialize, Serialize};

/// Includes twenty bounded searches with up to one hundred fully named results each.
pub const MAX_STATE_BYTES: usize = 8 * 1_048_576;
pub const MAX_PENDING_COMMANDS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub generation: u64,
    pub id: u64,
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Start { settings: Settings },
    Cancel {},
    SetSearchPaused { paused: bool },
    Configure { settings: Settings },
    ConfigureIntensity { frame_budget_ms: u8 },
    InspectCurrent {},
    Navigate { search_id: u64, target: Target },
}

impl Command {
    pub fn validate(&self, generation: u64) -> Result<(), &'static str> {
        if self.generation != generation || self.id == 0 {
            return Err("command belongs to an expired game state");
        }
        match &self.action {
            Action::Start { settings, .. } => settings.validate(),
            Action::Configure { settings } => settings.validate(),
            Action::ConfigureIntensity { frame_budget_ms } => {
                validate_frame_budget(*frame_budget_ms)
            }
            Action::Navigate { search_id, target } => {
                if *search_id == 0 {
                    return Err("navigation has no search identity");
                }
                target.validate()
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Starting,
    MainMenu,
    Loading,
    Ready,
    Unsupported,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchPause {
    Loading,
    OtherGalaxy,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LifecycleEventKind {
    Started,
    ResultsFound { count: usize },
    AutomaticPause { cause: SearchPause },
    AutomaticResume { cause: SearchPause },
    Finished { finish: Finish },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleEvent {
    pub sequence: u64,
    pub published_ms: u64,
    pub search_id: u64,
    pub elapsed_ms: u64,
    pub systems: u64,
    pub planets: u64,
    pub fauna: u64,
    pub results: usize,
    pub kind: LifecycleEventKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    pub generated_systems: u64,
    pub generated_planets: u64,
    pub generated_fauna: u64,
    pub metadata_captures: u64,
    pub colour_captures: u64,
    pub object_scans: u64,
    pub object_table_loads: u64,
    pub object_table_hits: u64,
    pub cached_object_tables: usize,
    pub name_captures: u64,
    pub regions: u64,
    pub batches: u64,
    pub active_ns: u64,
    pub longest_batch_ns: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionScope {
    System,
    Planet,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionEvaluation {
    pub scope: CriterionScope,
    pub section: Section,
    pub stage: Stage,
    pub description: String,
    pub predicate: Predicate,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SectionEvaluation {
    pub scope: CriterionScope,
    pub section: Section,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionScope {
    CriteriaAndCore,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentWorld {
    pub command_id: u64,
    pub address: Address,
    pub acquisition: AcquisitionScope,
    pub criteria: Criteria,
    pub system: FactSnapshot,
    pub planet: Option<FactSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planet_observations: Option<crate::observations::PlanetObservations>,
    pub system_verdict: Verdict,
    pub planet_verdict: Option<Verdict>,
    pub target_verdict: Verdict,
    pub criteria_evaluations: Vec<CriterionEvaluation>,
    pub section_evaluations: Vec<SectionEvaluation>,
}

impl CurrentWorld {
    pub fn capture(
        command_id: u64,
        address: Address,
        criteria: Criteria,
        plan: &Plan,
        system: &Facts,
        planets: &[Facts],
        planet_observations: Option<crate::observations::PlanetObservations>,
    ) -> Result<Self, &'static str> {
        if command_id == 0 {
            return Err("current-world inspection has no command identity");
        }
        if let Some(observations) = &planet_observations {
            observations.validate_address(address)?;
        }
        let planet_index = match address.coordinates().planet {
            0 => None,
            value => Some(usize::from(value - 1)),
        };
        if planet_index.is_some_and(|index| index >= planets.len()) {
            return Err("current planet is outside the generated system");
        }
        let planet_facts = planet_index.map(|index| &planets[index]);
        let system_verdict = plan.system_match(system, planets);
        let planet_verdict = planet_facts.map(|facts| plan.planet_match(facts));
        let target_verdict = if plan.planet.is_empty() {
            system_verdict
        } else {
            match (system_verdict, planet_verdict) {
                (Verdict::NoMatch, _) | (_, Some(Verdict::NoMatch)) => Verdict::NoMatch,
                (Verdict::Unknown, _) | (_, None | Some(Verdict::Unknown)) => Verdict::Unknown,
                (Verdict::Match, Some(Verdict::Match)) => Verdict::Match,
            }
        };
        let mut criteria_evaluations = Vec::with_capacity(plan.system.len() + plan.planet.len());
        criteria_evaluations.extend(plan.system.iter().copied().map(|predicate| {
            CriterionEvaluation {
                scope: CriterionScope::System,
                section: predicate.section(),
                stage: predicate.stage(),
                description: predicate.description(),
                predicate,
                verdict: plan.system_predicate_verdict(predicate, system, planets),
            }
        }));
        criteria_evaluations.extend(plan.planet.iter().copied().map(|predicate| {
            CriterionEvaluation {
                scope: CriterionScope::Planet,
                section: predicate.section(),
                stage: predicate.stage(),
                description: predicate.description(),
                predicate,
                verdict: planet_facts.map_or(Verdict::Unknown, |facts| {
                    plan.planet_predicate_verdict(predicate, facts)
                }),
            }
        }));
        let mut section_evaluations = Vec::new();
        let system_sections = plan.system_section_verdicts(system, planets);
        let planet_sections = planet_facts.map(|facts| plan.planet_section_verdicts(facts));
        for &section in Section::ALL {
            if plan
                .system
                .iter()
                .any(|predicate| predicate.section() == section)
            {
                section_evaluations.push(SectionEvaluation {
                    scope: CriterionScope::System,
                    section,
                    verdict: system_sections[&section],
                });
            }
            if plan
                .planet
                .iter()
                .any(|predicate| predicate.section() == section)
            {
                section_evaluations.push(SectionEvaluation {
                    scope: CriterionScope::Planet,
                    section,
                    verdict: planet_sections
                        .as_ref()
                        .map_or(Verdict::Unknown, |sections| sections[&section]),
                });
            }
        }
        Ok(Self {
            command_id,
            address,
            acquisition: AcquisitionScope::CriteriaAndCore,
            criteria,
            system: FactSnapshot::from(system),
            planet: planet_facts.map(FactSnapshot::from),
            planet_observations,
            system_verdict,
            planet_verdict,
            target_verdict,
            criteria_evaluations,
            section_evaluations,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub generation: u64,
    pub availability: Availability,
    pub player: Option<Address>,
    pub settings: Settings,
    /// One-shot startup report for files repaired while loading persisted settings.
    pub settings_recovery: Option<SettingsRecovery>,
    pub progress: Option<std::sync::Arc<Progress>>,
    pub search_history: Vec<std::sync::Arc<SearchRecord>>,
    pub search_pause: Option<SearchPause>,
    pub metrics: Metrics,
    pub cached_systems: usize,
    pub last_command: u64,
    /// Command that started the currently published search.
    pub search_command: Option<u64>,
    pub error: Option<String>,
    pub destination: Option<Target>,
    pub ui: UiState,
    pub current_world: Option<CurrentWorld>,
}

impl State {
    pub fn new(settings: Settings) -> Self {
        let ui = UiState {
            draft_settings: settings.clone(),
            ..UiState::default()
        };
        Self {
            generation: 1,
            availability: Availability::Starting,
            player: None,
            settings,
            settings_recovery: None,
            progress: None,
            search_history: Vec::new(),
            search_pause: None,
            metrics: Metrics::default(),
            cached_systems: 0,
            last_command: 0,
            search_command: None,
            error: None,
            destination: None,
            ui,
            current_world: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        address::Coordinates, session::Phase, settings::SearchOptions, target::TargetKind,
    };

    fn address() -> Address {
        Address::new(Coordinates {
            x: 1,
            y: 2,
            z: 3,
            galaxy: 0,
            system: 4,
            planet: 0,
        })
        .unwrap()
    }

    #[test]
    fn fully_populated_bounded_history_fits_the_published_state_limit() {
        use crate::{
            cache::{CachedMatch, FaunaMatch, MAX_NAME_BYTES},
            fauna::{Domain, NameIdentity},
            session::{MAX_SEARCH_HISTORY, Session},
            settings::MAX_RESULTS,
        };
        let settings = Settings::default();
        let mut session = Session::default();
        session
            .start(settings.search.clone(), address(), settings.execution())
            .unwrap();
        let mut record = (*session.history()[0]).clone();
        let name: Box<str> = "\u{0001}".repeat(MAX_NAME_BYTES).into();
        let result = CachedMatch {
            address: address().planet(0).unwrap(),
            kind: TargetKind::Fauna,
            system_name: Some(name.clone()),
            planet_name: Some(name.clone()),
            fauna: Some(FaunaMatch {
                role: 0,
                name_identity: NameIdentity {
                    species_seed: u64::MAX,
                    genus_seed: u64::MAX,
                },
                name: Some(name),
                domain: Domain::Land,
                cave: false,
            }),
        };
        std::sync::Arc::make_mut(&mut record.progress).results = vec![result; MAX_RESULTS];
        let mut state = State::new(settings);
        state.progress = Some(record.progress.clone());
        state.search_history = (1..=MAX_SEARCH_HISTORY)
            .map(|id| {
                let mut record = record.clone();
                std::sync::Arc::make_mut(&mut record.progress).id = id as u64;
                std::sync::Arc::new(record)
            })
            .collect();
        let bytes = serde_json::to_vec(&state).unwrap();
        assert!(bytes.len() > 1_048_576);
        assert!(bytes.len() < MAX_STATE_BYTES);
    }

    #[test]
    fn state_has_one_canonical_result_collection() {
        let mut state = State::new(Settings::default());
        state.progress = Some(std::sync::Arc::new(Progress {
            id: 1,
            elapsed_ms: 0,
            user_paused: false,
            phase: Phase::Running,
            finish: None,
            options: SearchOptions::default(),
            origin: address(),
            systems: 0,
            cached_systems: 0,
            unresolved_arena_planets: 0,
            unresolved_arena_examples: Vec::new(),
            results: Vec::new(),
            error: None,
        }));

        let value = serde_json::to_value(state).unwrap();
        assert!(value.get("results").is_none());
        assert!(value["progress"].get("results").is_some());
    }

    #[test]
    fn live_intensity_commands_enforce_the_frame_budget_bounds() {
        let valid = Command {
            generation: 7,
            id: 1,
            action: Action::ConfigureIntensity {
                frame_budget_ms: 16,
            },
        };
        let invalid = Command {
            generation: 7,
            id: 2,
            action: Action::ConfigureIntensity { frame_budget_ms: 0 },
        };

        assert_eq!(valid.validate(7), Ok(()));
        assert_eq!(
            invalid.validate(7),
            Err("frame budget must be between 1 and 16 milliseconds")
        );
    }
}

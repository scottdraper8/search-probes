use crate::{
    address::Address,
    cache::CachedMatch,
    criteria::{Criteria, Plan, Predicate, ResourceMode, Section, Stage},
    facts::{FactSnapshot, Facts, Verdict},
    session::Progress,
    settings::{Settings, validate_frame_budget},
    target::Target,
};
use serde::{Deserialize, Serialize};

pub const MAX_STATE_BYTES: usize = 1_048_576;
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
    Navigate { target: Target },
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
            Action::Navigate { target } => target.validate(),
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
    pub mode: ResourceMode,
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
    pub schema: u32,
    pub command_id: u64,
    pub address: Address,
    pub acquisition: AcquisitionScope,
    pub criteria: Criteria,
    pub system: FactSnapshot,
    pub planet: Option<FactSnapshot>,
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
    ) -> Result<Self, &'static str> {
        if command_id == 0 {
            return Err("current-world inspection has no command identity");
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
                    mode: plan.match_modes[section as usize],
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
                    mode: plan.match_modes[section as usize],
                    verdict: planet_sections
                        .as_ref()
                        .map_or(Verdict::Unknown, |sections| sections[&section]),
                });
            }
        }
        Ok(Self {
            schema: 1,
            command_id,
            address,
            acquisition: AcquisitionScope::CriteriaAndCore,
            criteria,
            system: FactSnapshot::from(system),
            planet: planet_facts.map(FactSnapshot::from),
            system_verdict,
            planet_verdict,
            target_verdict,
            criteria_evaluations,
            section_evaluations,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub generation: u64,
    pub availability: Availability,
    pub player: Option<Address>,
    pub settings: Settings,
    pub progress: Option<Progress>,
    pub search_pause: Option<SearchPause>,
    pub results: Vec<CachedMatch>,
    pub metrics: Metrics,
    pub cached_systems: usize,
    pub last_command: u64,
    /// Command that started the currently published search.
    #[serde(default)]
    pub search_command: Option<u64>,
    pub error: Option<String>,
    pub destination: Option<Target>,
    #[serde(default, skip_deserializing)]
    pub current_world: Option<CurrentWorld>,
}

impl State {
    pub fn publish_progress(&mut self, progress: Option<Progress>) {
        if let Some(progress) = progress.as_ref() {
            self.results = progress.results.clone();
        }
        self.progress = progress;
    }

    pub fn new(settings: Settings) -> Self {
        Self {
            generation: 1,
            availability: Availability::Starting,
            player: None,
            settings,
            progress: None,
            search_pause: None,
            results: Vec::new(),
            metrics: Metrics::default(),
            cached_systems: 0,
            last_command: 0,
            search_command: None,
            error: None,
            destination: None,
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
    fn publishing_a_new_empty_search_clears_previous_results() {
        let origin = address();
        let mut state = State::new(Settings::default());
        state.results.push(CachedMatch {
            address: origin,
            kind: TargetKind::System,
            system_name: Some("Old result".into()),
            planet_name: None,
            fauna: None,
        });

        state.publish_progress(Some(Progress {
            phase: Phase::Running,
            finish: None,
            options: SearchOptions::default(),
            origin,
            systems: 0,
            cached_systems: 0,
            unresolved_arena_planets: 0,
            unresolved_arena_examples: Vec::new(),
            results: Vec::new(),
            error: None,
        }));

        assert!(state.results.is_empty());
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

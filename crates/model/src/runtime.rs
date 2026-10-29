use crate::{
    address::Address,
    cache::CachedMatch,
    session::Progress,
    settings::{MAX_RESULTS, Settings},
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
    Start {
        settings: Settings,
        restart: bool,
        use_cache: bool,
    },
    Continue {
        results: usize,
    },
    Cancel {},
    Configure {
        settings: Settings,
    },
    Navigate {
        target: Target,
    },
    ClearHistory {},
}

impl Command {
    pub fn validate(&self, generation: u64) -> Result<(), &'static str> {
        if self.generation != generation || self.id == 0 {
            return Err("command belongs to an expired game state");
        }
        match &self.action {
            Action::Start { settings, .. } => settings.validate(),
            Action::Continue { results } if !(1..=MAX_RESULTS).contains(results) => {
                Err("invalid continuation result limit")
            }
            Action::Configure { settings } => settings.validate(),
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    pub generated_systems: u64,
    pub generated_planets: u64,
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
    /// Command that started or continued the currently published search.
    #[serde(default)]
    pub search_command: Option<u64>,
    pub error: Option<String>,
    pub destination: Option<Target>,
}

impl State {
    pub fn publish_progress(&mut self, progress: Option<Progress>) {
        if let Some(progress) = progress
            .as_ref()
            .filter(|progress| !progress.results.is_empty())
        {
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
        }
    }
}

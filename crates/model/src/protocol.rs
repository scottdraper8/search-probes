use crate::{
    address::Address,
    cache::CachedMatch,
    session::Progress,
    settings::{MAX_RESULTS, Settings},
};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: u32 = 4;
pub const MAX_COMMAND_BYTES: usize = 65_536;
pub const MAX_STATE_BYTES: usize = 1_048_576;
pub const MAX_PENDING_COMMANDS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub protocol: u32,
    pub session: String,
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
        address: Address,
    },
    ClearHistory {},
}

impl Command {
    pub fn validate(&self, session: &str, generation: u64) -> Result<(), &'static str> {
        if self.protocol != PROTOCOL
            || self.session != session
            || self.generation != generation
            || self.id == 0
        {
            return Err("command does not belong to this runtime session");
        }
        match &self.action {
            Action::Start { settings, .. } => settings.validate(),
            Action::Continue { results } if !(1..=MAX_RESULTS).contains(results) => {
                Err("invalid continuation result limit")
            }
            Action::Configure { settings } => settings.validate(),
            Action::Navigate { address }
                if address.coordinates().system == 0
                    || !(1..=8).contains(&address.coordinates().planet) =>
            {
                Err("destination must identify a generated planet")
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
    Ready,
    Unsupported,
    Failed,
    Stopped,
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
    pub protocol: u32,
    pub session: String,
    pub generation: u64,
    pub pid: u32,
    pub availability: Availability,
    pub player: Option<Address>,
    pub settings: Settings,
    pub progress: Option<Progress>,
    pub results: Vec<CachedMatch>,
    pub metrics: Metrics,
    pub cached_systems: usize,
    pub last_command: u64,
    pub error: Option<String>,
    pub destination: Option<Address>,
    pub toggle: u64,
    pub open: u64,
}

impl State {
    pub fn new(session: String, pid: u32, settings: Settings) -> Self {
        Self {
            protocol: PROTOCOL,
            session,
            generation: 1,
            pid,
            availability: Availability::Starting,
            player: None,
            settings,
            progress: None,
            results: Vec::new(),
            metrics: Metrics::default(),
            cached_systems: 0,
            last_command: 0,
            error: None,
            destination: None,
            toggle: 0,
            open: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_sessions_invalid_addresses_and_unknown_actions_fail_closed() {
        let command = Command {
            protocol: PROTOCOL,
            session: "current".into(),
            generation: 1,
            id: 1,
            action: Action::Cancel {},
        };
        assert!(command.validate("old", 1).is_err());
        assert!(command.validate("current", 2).is_err());
        command.validate("current", 1).unwrap();
        let invalid = r#"{"protocol":1,"session":"current","id":1,"action":{"type":"navigate","address":18446744073709551615}}"#;
        assert!(serde_json::from_str::<Command>(invalid).is_err());
        assert!(serde_json::from_str::<Action>(r#"{"type":"run_script","script":""}"#).is_err());
        assert!(serde_json::from_str::<Action>(r#"{"type":"cancel","ignored":true}"#).is_err());
    }
}

use crate::{
    agent_control::{AgentCommand, CommandChannel, Endpoint, PROTOCOL_ID, Request, Response},
    runtime::MAX_STATE_BYTES,
    storage,
    ui_control::{UiAction, UiState},
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_ENDPOINT_BYTES: usize = 16 * 1024;
const MAX_REQUEST_BYTES: usize = 1_100_000;
const LIVE_AGE: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
static REQUEST_ID: AtomicU64 = AtomicU64::new(0);

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn path_error<T>(action: &str, path: &Path, result: io::Result<T>) -> io::Result<T> {
    result.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot {action} {}: {error}", path.display()),
        )
    })
}

fn request_id() -> io::Result<String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let sequence = u128::from(REQUEST_ID.fetch_add(1, Ordering::Relaxed));
    Ok(format!(
        "{:032x}",
        stamp ^ sequence ^ u128::from(std::process::id())
    ))
}

fn live(path: &Path) -> io::Result<()> {
    let age = SystemTime::now()
        .duration_since(fs::metadata(path)?.modified()?)
        .unwrap_or_default();
    if age > LIVE_AGE {
        return Err(invalid(format!(
            "runtime publication is stale: {}",
            path.display()
        )));
    }
    Ok(())
}

fn validate_endpoint(endpoint: &Endpoint) -> io::Result<()> {
    if endpoint.protocol != PROTOCOL_ID {
        return Err(invalid("unsupported Search Probes control protocol"));
    }
    for required in ["search.control", "ui.control"] {
        if !endpoint.capabilities.iter().any(|value| value == required) {
            return Err(invalid(format!("runtime does not provide {required}")));
        }
    }
    if endpoint.ui_actions.is_empty() {
        return Err(invalid("runtime did not advertise semantic UI actions"));
    }
    Ok(())
}

struct StatePublication {
    raw: Value,
    generation: u64,
    last_command: u64,
    error: Option<String>,
    ui: UiState,
}

fn state(path: &Path) -> io::Result<StatePublication> {
    let raw: Value = storage::read_json(path, MAX_STATE_BYTES)?;
    let object = raw
        .as_object()
        .ok_or_else(|| invalid("runtime state is not an object"))?;
    let expected: BTreeSet<_> = [
        "generation",
        "availability",
        "player",
        "settings",
        "progress",
        "search_history",
        "search_pause",
        "metrics",
        "cached_systems",
        "last_command",
        "search_command",
        "error",
        "destination",
        "ui",
        "current_world",
    ]
    .into_iter()
    .collect();
    if object.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
        return Err(invalid(
            "runtime state fields do not match the current contract",
        ));
    }
    let generation = object["generation"]
        .as_u64()
        .ok_or_else(|| invalid("runtime state has no game generation"))?;
    let last_command = object["last_command"]
        .as_u64()
        .ok_or_else(|| invalid("runtime state has no command cursor"))?;
    let error = match &object["error"] {
        Value::Null => None,
        Value::String(error) => Some(error.clone()),
        _ => return Err(invalid("runtime state error is malformed")),
    };
    let ui = serde_json::from_value(object["ui"].clone()).map_err(invalid)?;
    Ok(StatePublication {
        raw,
        generation,
        last_command,
        error,
        ui,
    })
}

fn live_status(directory: &Path) -> io::Result<(Endpoint, StatePublication)> {
    let endpoint_path = directory.join(crate::agent_control::ENDPOINT);
    let state_path = directory.join("current.json");
    let endpoint: Endpoint = path_error(
        "read the control endpoint",
        &endpoint_path,
        storage::read_json(&endpoint_path, MAX_ENDPOINT_BYTES),
    )?;
    validate_endpoint(&endpoint)?;
    live(&endpoint_path)?;
    live(&state_path)?;
    let state = path_error("read the runtime state", &state_path, state(&state_path))?;
    let confirmed: Endpoint = path_error(
        "confirm the control endpoint",
        &endpoint_path,
        storage::read_json(&endpoint_path, MAX_ENDPOINT_BYTES),
    )?;
    if confirmed.instance != endpoint.instance {
        return Err(invalid("Search Probes restarted while its state was read"));
    }
    Ok((endpoint, state))
}

pub fn status(directory: &Path) -> io::Result<(Endpoint, Value)> {
    live_status(directory).map(|(endpoint, state)| (endpoint, state.raw))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandPhase {
    Queued,
    Applied,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandResult {
    pub phase: CommandPhase,
    pub acknowledgement: Response,
    pub state: Value,
}

fn workspace(directory: &Path, endpoint: &Endpoint) -> io::Result<PathBuf> {
    let directory = path_error(
        "resolve the settings directory",
        directory,
        directory.canonicalize(),
    )?;
    let workspace = directory
        .join("agent-control")
        .join(&endpoint.instance)
        .canonicalize()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot resolve the live control workspace: {error}"),
            )
        })?;
    if !workspace.starts_with(&directory) {
        return Err(invalid(
            "runtime control workspace escapes its settings directory",
        ));
    }
    Ok(workspace)
}

pub fn command(
    directory: &Path,
    action: UiAction,
    wait: bool,
    timeout: Duration,
) -> io::Result<CommandResult> {
    if !(Duration::from_millis(100)..=Duration::from_secs(600)).contains(&timeout) {
        return Err(invalid("timeout must be between 0.1 and 600 seconds"));
    }
    let (endpoint, initial) = live_status(directory)?;
    let kind = serde_json::to_value(&action)
        .map_err(invalid)?
        .get("type")
        .and_then(|value| value.as_str())
        .ok_or_else(|| invalid("UI action has no semantic type"))?
        .to_owned();
    if !endpoint
        .ui_actions
        .iter()
        .any(|descriptor| descriptor.kind == kind)
    {
        return Err(invalid(format!(
            "runtime did not advertise UI action {kind}"
        )));
    }
    let workspace = workspace(directory, &endpoint)?;
    let id = request_id()?;
    let request_path = workspace.join("requests").join(format!("{id}.json"));
    let response_path = workspace.join("responses").join(format!("{id}.json"));
    let expires_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis()
        .saturating_add(timeout.as_millis().min(120_000))
        .min(u128::from(u64::MAX)) as u64;
    let request = Request {
        instance: endpoint.instance.clone(),
        request_id: id.clone(),
        generation: initial.generation,
        expires_unix_ms,
        command: AgentCommand::Ui(action),
    };
    path_error(
        "write the control request",
        &request_path,
        storage::write_json(&request_path, &request, MAX_REQUEST_BYTES),
    )?;
    let result = (|| {
        let deadline = Instant::now() + timeout;
        let mut acknowledgement = None;
        while Instant::now() < deadline {
            let (current_endpoint, state) = live_status(directory)?;
            if current_endpoint.instance != endpoint.instance
                || state.generation != initial.generation
            {
                return Err(invalid(
                    "Search Probes or the loaded game changed during the command",
                ));
            }
            if acknowledgement.is_none() && response_path.exists() {
                let response: Response = path_error(
                    "read the control response",
                    &response_path,
                    storage::read_json(&response_path, MAX_REQUEST_BYTES),
                )?;
                if response.instance != endpoint.instance
                    || response.request_id != id
                    || response.channel != Some(CommandChannel::Ui)
                {
                    return Err(invalid(
                        "runtime acknowledgement does not match the request",
                    ));
                }
                if let Some(error) = &response.error {
                    return Err(invalid(format!("runtime rejected the command: {error}")));
                }
                if response.command_id.is_none_or(|command_id| command_id == 0) {
                    return Err(invalid("runtime acknowledgement has no command identity"));
                }
                if !wait {
                    return Ok(CommandResult {
                        phase: CommandPhase::Queued,
                        acknowledgement: response,
                        state: state.raw,
                    });
                }
                acknowledgement = Some(response);
            }
            if let Some(response) = &acknowledgement {
                let command_id = response.command_id.expect("validated command identity");
                if state.ui.last_command >= command_id {
                    if state.ui.last_command == command_id
                        && let Some(error) = &state.ui.error
                    {
                        return Err(invalid(format!("UI command failed: {error}")));
                    }
                    if let Some(crate::ui_control::UiOutput::RuntimeCommand { command_id }) =
                        state.ui.output
                    {
                        if state.last_command < command_id {
                            thread::sleep(POLL_INTERVAL);
                            continue;
                        }
                        if state.last_command == command_id
                            && let Some(error) = &state.error
                        {
                            return Err(invalid(format!("runtime command failed: {error}")));
                        }
                    }
                    return Ok(CommandResult {
                        phase: CommandPhase::Applied,
                        acknowledgement: acknowledgement.expect("present acknowledgement"),
                        state: state.raw,
                    });
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "timed out waiting for Search Probes",
        ))
    })();
    let _ = fs::remove_file(request_path);
    let _ = fs::remove_file(response_path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent_control::{AgentControl, CAPABILITIES},
        controls::Controls,
        runtime::State,
        settings::Settings,
        ui_control::{UiAction, UiPage},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    static DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "search-probes-client-{}-{}",
            std::process::id(),
            DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn semantic_ui_command_round_trips_through_the_live_runtime_workspace() {
        let directory = directory();
        let ready = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_directory = directory.clone();
        let worker_ready = Arc::clone(&ready);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let control = AgentControl::open(&worker_directory, CAPABILITIES).unwrap();
            let controls = Controls::new(State::new(Settings::default()));
            storage::write_json(
                &worker_directory.join("current.json"),
                &controls.snapshot(),
                MAX_STATE_BYTES,
            )
            .unwrap();
            worker_ready.store(true, Ordering::Release);
            while !worker_stop.load(Ordering::Acquire) {
                control.poll(&controls).unwrap();
                if let Some(command) = controls.pop_ui() {
                    let mut ui = controls.snapshot().ui;
                    match command.action {
                        UiAction::SetPage { page } => ui.page = page,
                        _ => panic!("unexpected test action"),
                    }
                    ui.last_command = command.id;
                    ui.error = None;
                    ui.output = None;
                    controls.publish_ui(ui);
                }
                storage::write_json(
                    &worker_directory.join("current.json"),
                    &controls.snapshot(),
                    MAX_STATE_BYTES,
                )
                .unwrap();
                control.publish().unwrap();
                thread::sleep(Duration::from_millis(2));
            }
            control.cleanup().unwrap();
        });
        while !ready.load(Ordering::Acquire) {
            thread::yield_now();
        }

        let result = command(
            &directory,
            UiAction::SetPage {
                page: UiPage::History,
            },
            true,
            Duration::from_secs(2),
        )
        .unwrap();

        assert!(matches!(result.phase, CommandPhase::Applied));
        assert_eq!(result.state["ui"]["page"], "history");
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }
}

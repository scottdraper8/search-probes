use search_probes_model::{
    agent_client,
    arena::ArenaField,
    binding::{ControllerButton, KeyboardKey},
    criteria::{Field, PlanetFlag, SystemFlag, WaterFilter},
    fauna::{Domain, FaunaField},
    settings::{BUDGETS, MAX_RESULTS, MAX_WORKERS, ToastPlacement},
    storage,
    theme::UiTheme,
    ui_control::{UiAction, UiState},
    vocabulary::RESOURCE_LABELS,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    error::Error,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MAX_ACTION_BYTES: usize = 1_100_000;

enum Operation {
    Status,
    Actions,
    Command(PathBuf),
}

struct Arguments {
    directory: PathBuf,
    timeout: Duration,
    wait: bool,
    operation: Operation,
}

fn usage() -> &'static str {
    "usage: SearchProbesControl [--settings-dir DIR] [--timeout SECONDS] [--no-wait] status|actions|command ACTION.json"
}

fn parse_seconds(value: &OsString) -> Result<Duration> {
    let value = value.to_str().ok_or("timeout is not UTF-8")?;
    let seconds: f64 = value.parse()?;
    if !seconds.is_finite() || !(0.1..=600.0).contains(&seconds) {
        return Err("timeout must be between 0.1 and 600 seconds".into());
    }
    Ok(Duration::from_secs_f64(seconds))
}

fn take_value(arguments: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<OsString> {
    arguments
        .next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn parse() -> Result<Arguments> {
    let mut arguments = std::env::args_os().skip(1);
    let mut directory = None;
    let mut timeout = Duration::from_secs(120);
    let mut wait = true;
    let mut positional = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--settings-dir") => {
                directory = Some(PathBuf::from(take_value(&mut arguments, "--settings-dir")?));
            }
            Some("--timeout") => {
                timeout = parse_seconds(&take_value(&mut arguments, "--timeout")?)?;
            }
            Some("--no-wait") => wait = false,
            Some(value) if value.starts_with('-') => {
                return Err(format!("unknown option: {value}").into());
            }
            _ => positional.push(argument),
        }
    }
    let operation = match positional.as_slice() {
        [operation] if operation == "status" => Operation::Status,
        [operation] if operation == "actions" => Operation::Actions,
        [operation, action] if operation == "command" => Operation::Command(action.into()),
        _ => return Err(usage().into()),
    };
    Ok(Arguments {
        directory: directory.unwrap_or(storage::user_directory()?),
        timeout,
        wait,
        operation,
    })
}

fn read_action(path: &Path) -> Result<UiAction> {
    if !path.symlink_metadata()?.is_file() {
        return Err(format!("action is not a regular file: {}", path.display()).into());
    }
    storage::read_json(path, MAX_ACTION_BYTES).map_err(Into::into)
}

fn print(value: &impl serde::Serialize) -> Result<()> {
    serde_json::to_writer_pretty(io::stdout().lock(), value)?;
    println!();
    Ok(())
}

fn value<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).expect("catalog values are serializable")
}

fn option<T: Serialize>(value: T, label: &str) -> Value {
    json!({"value": value, "label": label})
}

fn catalog(actions: Value, ui: UiState) -> Value {
    let fields: Vec<_> = Field::ALL
        .iter()
        .map(|&field| {
            let options: Vec<_> = if field == Field::Subtype {
                ui.draft_settings
                    .search
                    .criteria
                    .subtype_options()
                    .into_iter()
                    .map(|(_, label)| label)
                    .collect()
            } else {
                field.available_options().map(|(_, label)| label).collect()
            };
            json!({"value": value(field), "label": field.label(), "options": options})
        })
        .collect();
    let fauna: Vec<_> = Domain::ALL
        .iter()
        .map(|&domain| {
            let fields: Vec<_> = FaunaField::ALL
                .iter()
                .filter_map(|&field| {
                    let options: Vec<_> = field
                        .options(domain)
                        .into_iter()
                        .map(|(_, label)| label)
                        .collect();
                    (!options.is_empty()).then(|| {
                        json!({
                            "value": value(field),
                            "label": field.label(),
                            "options": options,
                        })
                    })
                })
                .collect();
            json!({"value": value(domain), "label": domain.label(), "fields": fields})
        })
        .collect();
    let arena: Vec<_> = ArenaField::ALL
        .iter()
        .map(|&field| {
            json!({
                "value": value(field),
                "label": field.label(),
                "options": field.options().into_iter().map(|(_, label)| label).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "actions": actions,
        "current_ui": ui,
        "values": {
            "pages": ["search", "history"],
            "themes": UiTheme::ALL.iter().map(|&theme| option(theme, theme.label())).collect::<Vec<_>>(),
            "toast_placements": ToastPlacement::ALL.iter().map(|&placement| option(placement, placement.label())).collect::<Vec<_>>(),
            "keyboard_keys": KeyboardKey::ALL.iter().map(|&key| option(key, key.label())).collect::<Vec<_>>(),
            "controller_buttons": ControllerButton::ALL.iter().map(|&button| option(button, button.label())).collect::<Vec<_>>(),
            "history_directions": ["older", "newer"],
            "result_values": ["portal_code", "galactic_coordinates"],
            "export_sources": ["results", "current_search", "viewed_search", "presets"],
            "export_formats": ["text", "csv", "json"],
            "test_toasts": ["result_found", "search_completed", "automatic_pause", "automatic_resume", "export_completed"],
            "water_filters": WaterFilter::ALL.iter().map(|&filter| option(filter, filter.label())).collect::<Vec<_>>(),
            "choice_filters": fields,
            "planet_requirements": PlanetFlag::ALL.iter().map(|&flag| option(flag, flag.label())).collect::<Vec<_>>(),
            "system_requirements": SystemFlag::ALL.iter().map(|&flag| option(flag, flag.label())).collect::<Vec<_>>(),
            "resources": RESOURCE_LABELS,
            "fauna": fauna,
            "arena": arena,
            "system_budgets": BUDGETS,
            "result_limit": {"minimum": 1, "maximum": MAX_RESULTS},
            "worker_limit": {"minimum": 1, "maximum": MAX_WORKERS},
            "frame_budget_ms": {"minimum": 1, "maximum": 16},
            "time_limit": {"hours_maximum": 99, "minutes_maximum": 99},
        },
    })
}

fn run() -> Result<()> {
    let arguments = parse()?;
    match arguments.operation {
        Operation::Status => {
            let (endpoint, state) = agent_client::status(&arguments.directory)?;
            print(&json!({"endpoint": endpoint, "state": state}))
        }
        Operation::Actions => {
            let (endpoint, state) = agent_client::status(&arguments.directory)?;
            let ui: UiState = serde_json::from_value(state["ui"].clone())?;
            print(&catalog(value(endpoint.ui_actions), ui))
        }
        Operation::Command(path) => print(&agent_client::command(
            &arguments.directory,
            read_action(&path)?,
            arguments.wait,
            arguments.timeout,
        )?),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

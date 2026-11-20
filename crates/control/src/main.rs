use search_probes_model::{
    agent_client,
    arena::ArenaField,
    binding::{ControllerButton, KeyboardKey},
    criteria::{CompositionField, Field, PlanetFlag, SelectionSide, SystemFlag, WaterFilter},
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
    let criteria = &ui.draft_settings.search.criteria;
    let fields: Vec<_> = Field::ALL
        .iter()
        .map(|&field| {
            let derived = (field == Field::Size)
                .then(|| criteria.derived_planet_size())
                .flatten()
                .and_then(|value| {
                    Field::Size
                        .available_options()
                        .find_map(|(index, label)| (index == value).then_some(label))
                });
            let options: Vec<_> = if field == Field::Subtype {
                criteria
                    .subtype_options()
                    .into_iter()
                    .map(|(_, label)| label)
                    .collect()
            } else {
                criteria
                    .choice_options(field)
                    .into_iter()
                    .map(|(_, label)| label)
                    .collect()
            };
            json!({
                "value": value(field),
                "label": field.label(),
                "available": derived.is_none() && !options.is_empty(),
                "derived_value": derived,
                "options": options,
            })
        })
        .collect();
    let fauna: Vec<_> = Domain::ALL
        .iter()
        .map(|&domain| {
            let selected = criteria.fauna.get(&domain).cloned().unwrap_or_default();
            let fields: Vec<_> = FaunaField::ALL
                .iter()
                .filter_map(|&field| {
                    let options: Vec<_> = selected
                        .choice_options(domain, field)
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
            json!({
                "value": value(domain),
                "label": domain.label(),
                "available": criteria.fauna_domain_available(domain),
                "presence_locked": selected.has_detail_filters(),
                "details_enabled": selected.presence != Some(false),
                "fields": fields,
            })
        })
        .collect();
    let arena: Vec<_> = ArenaField::ALL
        .iter()
        .map(|&field| {
            json!({
                "value": value(field),
                "label": field.label(),
                "options": criteria.arena.choice_options(field).into_iter().map(|(_, label)| label).collect::<Vec<_>>(),
            })
        })
        .collect();
    let composition = [
        CompositionField::Biomes,
        CompositionField::Subtypes,
        CompositionField::Sizes,
    ]
    .into_iter()
    .map(|field| {
        let sides = [SelectionSide::Include, SelectionSide::Exclude]
            .into_iter()
            .map(|side| {
                let options: Vec<_> = match field {
                    CompositionField::Biomes => Field::Biome
                        .available_options()
                        .map(|(_, label)| label)
                        .collect(),
                    CompositionField::Sizes => Field::Size
                        .available_options()
                        .map(|(_, label)| label)
                        .collect(),
                    CompositionField::Subtypes => {
                        let biomes = criteria
                            .composition
                            .selections(CompositionField::Biomes)
                            .get(side);
                        search_probes_model::criteria::subtype_options_for(biomes)
                            .into_iter()
                            .map(|(_, label)| label)
                            .collect()
                    }
                };
                json!({"side": value(side), "options": options})
            })
            .collect::<Vec<_>>();
        json!({"value": value(field), "sides": sides})
    })
    .collect::<Vec<_>>();
    let water = |scope| {
        WaterFilter::ALL
            .iter()
            .map(|&filter| {
                let available = match scope {
                    search_probes_model::ui_control::WaterScope::System => {
                        criteria.system_water_option_available(filter)
                    }
                    search_probes_model::ui_control::WaterScope::Planet => {
                        criteria.planet_water_option_available(filter)
                    }
                };
                json!({"value": value(filter), "label": filter.label(), "available": available})
            })
            .collect::<Vec<_>>()
    };
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
            "water_filters": {
                "system": water(search_probes_model::ui_control::WaterScope::System),
                "planet": water(search_probes_model::ui_control::WaterScope::Planet),
            },
            "choice_filters": fields,
            "planet_requirements": PlanetFlag::ALL.iter().copied()
                .filter(|flag| !flag.is_sentinel_level_projection())
                .map(|flag| json!({
                "value": value(flag),
                "label": flag.label(),
                "available": criteria.planet_requirement_available(flag),
            })).collect::<Vec<_>>(),
            "system_requirements": SystemFlag::ALL.iter().map(|&flag| json!({
                "value": value(flag),
                "label": flag.label(),
                "available": flag != SystemFlag::Pirate || criteria.outlaw_filter_available(),
            })).collect::<Vec<_>>(),
            "composition_filters": composition,
            "resources": RESOURCE_LABELS,
            "fauna": fauna,
            "arena": {
                "available": criteria.arena_available(),
                "presence_locked": criteria.arena.has_detail_filters(),
                "details_enabled": criteria.arena.presence != Some(false),
                "fields": arena,
            },
            "system_budgets": BUDGETS,
            "minimum_planets": {"minimum": 1, "maximum": 6},
            "minimum_stars": {"minimum": 1, "maximum": 3},
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

#[cfg(test)]
mod tests {
    use super::*;
    use search_probes_model::fauna::FaunaCriteria;

    fn choice<'a>(catalog: &'a Value, field: &str) -> &'a Value {
        catalog["values"]["choice_filters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["value"] == field)
            .unwrap()
    }

    fn entry<'a>(catalog: &'a Value, group: &str, value: &str) -> &'a Value {
        catalog["values"][group]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["value"] == value)
            .unwrap()
    }

    #[test]
    fn catalog_reports_the_same_dynamic_availability_as_the_ui() {
        let mut ui = UiState::default();
        let criteria = &mut ui.draft_settings.search.criteria;
        criteria.choices.insert(Field::Population, 1 << 1);
        criteria.choices.insert(Field::StormFrequency, 1);
        criteria.enforce_dependencies();

        let catalog = catalog(json!([]), ui);
        for field in ["race", "wealth", "economy", "conflict_level", "storm_type"] {
            assert_eq!(choice(&catalog, field)["available"], false, "{field}");
            assert_eq!(choice(&catalog, field)["options"], json!([]), "{field}");
        }
        assert_eq!(
            entry(&catalog, "system_requirements", "pirate")["available"],
            false
        );
    }

    #[test]
    fn catalog_reports_derived_size_and_system_exclusion_constraints() {
        let mut ui = UiState::default();
        let criteria = &mut ui.draft_settings.search.criteria;
        criteria.set_biome_choices(1 << 15);
        criteria.systems.insert(SystemFlag::Dissonance, false);
        criteria.enforce_dependencies();

        let catalog = catalog(json!([]), ui);
        assert_eq!(choice(&catalog, "size")["available"], false);
        assert_eq!(choice(&catalog, "size")["derived_value"], "Giant");
        assert!(
            !choice(&catalog, "sentinels")["options"]
                .as_array()
                .unwrap()
                .contains(&json!("Corrupted"))
        );
        assert_eq!(
            entry(&catalog, "planet_requirements", "dissonant")["available"],
            false
        );
        for hidden in ["sentinels", "extreme_sentinels", "corrupt_sentinels"] {
            assert!(
                catalog["values"]["planet_requirements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|entry| entry["value"] != hidden)
            );
        }
    }

    #[test]
    fn catalog_reports_water_fauna_and_arena_locks() {
        let mut ui = UiState::default();
        let criteria = &mut ui.draft_settings.search.criteria;
        criteria.system_water = WaterFilter::ExcludeAll;
        criteria.fauna.insert(
            Domain::Land,
            FaunaCriteria {
                choices: [(FaunaField::Rarity, 1 << 2)].into(),
                ..FaunaCriteria::default()
            },
        );
        for field in [ArenaField::Combat, ArenaField::Agility, ArenaField::Health] {
            criteria.arena.choices.insert(field, 1 << 3);
        }
        criteria.enforce_dependencies();

        let catalog = catalog(json!([]), ui);
        assert_eq!(choice(&catalog, "water_colour")["available"], false);
        assert_eq!(entry(&catalog, "fauna", "aquatic")["available"], false);
        let land = entry(&catalog, "fauna", "land");
        assert_eq!(land["presence_locked"], true);
        assert_eq!(land["details_enabled"], true);
        assert_eq!(catalog["values"]["arena"]["presence_locked"], true);
        let overall = catalog["values"]["arena"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["value"] == "overall_rank")
            .unwrap();
        assert_eq!(overall["options"], json!(["S"]));
        for value in ["require_any", "require_deep"] {
            assert_eq!(
                catalog["values"]["water_filters"]["planet"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|entry| entry["value"] == value)
                    .unwrap()["available"],
                false
            );
        }
    }
}

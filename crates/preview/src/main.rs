//! Standalone host for the production Search Probes UI and agent-control endpoint.
use search_probes_model::{
    address::{Address, Coordinates},
    agent_control::AgentControl,
    cache::{CachedMatch, FaunaMatch},
    controls::Controls,
    fauna::{Domain, NameIdentity},
    runtime::{Action, Availability, Command, MAX_STATE_BYTES, Metrics, State},
    session::{Finish, MAX_SEARCH_HISTORY, Phase, Progress, SearchRecord},
    settings::{MAX_RESULTS, Settings},
    storage,
    target::TargetKind,
};
use search_probes_ui::{App, ExportFormat, ExportRequest, egui};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    process::Command as ProcessCommand,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_FIXTURES: usize = MAX_RESULTS;

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn address(index: usize, planet: u8) -> Address {
    let system = u16::try_from(index % 4_095 + 1).expect("bounded preview fixture system");
    let x = i16::try_from(index / 4_095).expect("bounded preview fixture region") - 12;
    Address::new(Coordinates {
        x,
        y: 7,
        z: 24,
        galaxy: 0,
        system,
        planet,
    })
    .expect("preview fixture address")
}

fn destination(
    index: usize,
    planet: u8,
    kind: TargetKind,
    name: &str,
    fauna: Option<(Domain, bool)>,
) -> CachedMatch {
    let ordinal = index + 1;
    CachedMatch {
        address: address(index, planet),
        kind,
        system_name: Some(format!("Fixture System {ordinal}").into()),
        planet_name: (kind != TargetKind::System)
            .then(|| format!("Fixture Planet {ordinal}").into()),
        fauna: fauna.map(|(domain, cave)| FaunaMatch {
            role: (index % 30) as u8,
            name_identity: NameIdentity {
                species_seed: ordinal as u64,
                genus_seed: ordinal as u64 + 100,
            },
            name: Some(format!("{name} {ordinal}").into()),
            domain,
            cave,
        }),
    }
}

fn sample_results(count: usize) -> Vec<CachedMatch> {
    (0..count)
        .map(|index| match index % 10 {
            0 => destination(index, 0, TargetKind::System, "", None),
            1 => destination(index, 1, TargetKind::Planet, "", None),
            2 => destination(
                index,
                1,
                TargetKind::Fauna,
                "Surface Grazer",
                Some((Domain::Land, false)),
            ),
            3 => destination(
                index,
                2,
                TargetKind::Fauna,
                "Cave Burrower",
                Some((Domain::Land, true)),
            ),
            4 => destination(
                index,
                1,
                TargetKind::Fauna,
                "Highland Ray",
                Some((Domain::Airborne, false)),
            ),
            5 => destination(
                index,
                3,
                TargetKind::Fauna,
                "Reef Drifter",
                Some((Domain::Aquatic, false)),
            ),
            6 => destination(index, 1, TargetKind::Planet, "", None),
            7 => destination(index, 0, TargetKind::System, "", None),
            8 => destination(index, 4, TargetKind::Planet, "", None),
            _ => destination(index, 2, TargetKind::Planet, "", None),
        })
        .collect()
}

fn progress(
    id: u64,
    settings: &Settings,
    results: Vec<CachedMatch>,
    phase: Phase,
) -> Arc<Progress> {
    Arc::new(Progress {
        id,
        elapsed_ms: 83_000,
        user_paused: false,
        phase,
        finish: (phase != Phase::Running).then_some(Finish::Results),
        options: settings.search.clone(),
        origin: address(0, 0),
        systems: 2_400,
        cached_systems: 160,
        unresolved_arena_planets: 0,
        unresolved_arena_examples: Vec::new(),
        results,
        error: None,
    })
}

fn record(settings: &Settings, progress: Arc<Progress>) -> Arc<SearchRecord> {
    let now = unix_ms();
    Arc::new(SearchRecord {
        execution: settings.execution(),
        started_unix_ms: now.saturating_sub(progress.elapsed_ms),
        finished_unix_ms: (progress.phase != Phase::Running).then_some(now),
        progress,
    })
}

fn preview_state(
    settings: Settings,
    recovery: Option<search_probes_model::settings::SettingsRecovery>,
    fixtures: usize,
) -> State {
    let mut state = State::new(settings);
    state.settings_recovery = recovery;
    state.availability = Availability::Ready;
    state.player = Some(address(0, 0));
    if fixtures > 0 {
        let current = progress(
            1,
            &state.settings,
            sample_results(fixtures),
            Phase::Completed,
        );
        state.progress = Some(current.clone());
        state.search_history = vec![record(&state.settings, current)];
        state.metrics = Metrics {
            generated_systems: 2_400,
            generated_planets: 13_842,
            generated_fauna: 6_125,
            elapsed_ms: 83_000,
            ..Metrics::default()
        };
    }
    state
}

#[derive(Debug)]
struct Options {
    settings_directory: PathBuf,
    remove_directory: bool,
    fixtures: usize,
}

fn generated_directory() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "search-probes-preview-{}-{stamp}",
        std::process::id()
    ))
}

fn arguments(values: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut values = values.into_iter();
    let mut settings_directory = None;
    let mut fixtures = None;
    while let Some(flag) = values.next() {
        match flag.to_str() {
            Some("--settings-dir") if settings_directory.is_none() => {
                settings_directory = Some(PathBuf::from(
                    values.next().ok_or("--settings-dir needs a path")?,
                ));
            }
            Some("--fixtures") if fixtures.is_none() => {
                let value = values
                    .next()
                    .and_then(|value| value.into_string().ok())
                    .ok_or("--fixtures needs a non-negative integer")?;
                let count = value
                    .parse()
                    .map_err(|_| "--fixtures needs a non-negative integer")?;
                if count > MAX_FIXTURES {
                    return Err(format!("--fixtures cannot exceed {MAX_FIXTURES}"));
                }
                fixtures = Some(count);
            }
            _ => {
                return Err(format!(
                    "unexpected preview argument: {}",
                    flag.to_string_lossy()
                ));
            }
        }
    }
    let remove_directory = settings_directory.is_none();
    Ok(Options {
        settings_directory: settings_directory.unwrap_or_else(generated_directory),
        remove_directory,
        fixtures: fixtures.unwrap_or(0),
    })
}

fn export_filter(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Text => "Text files (*.txt) | *.txt",
        ExportFormat::Csv => "CSV files (*.csv) | *.csv",
        ExportFormat::Json => "JSON files (*.json) | *.json",
    }
}

fn save_export(request: &ExportRequest) -> Result<Option<String>, String> {
    let suggested = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(request.suggested_name());
    let filter = export_filter(request.format);
    let output = ProcessCommand::new("zenity")
        .arg("--file-selection")
        .arg("--save")
        .arg("--confirm-overwrite")
        .arg(format!("--title={}", request.dialog_title()))
        .arg(format!("--filename={}", suggested.display()))
        .arg(format!("--file-filter={filter}"))
        .output()
        .map_err(|error| format!("cannot open the preview save dialog: {error}"))?;
    if !output.status.success() {
        return if output.status.code() == Some(1) {
            Ok(None)
        } else {
            Err(format!(
                "preview save dialog failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        };
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    if path.as_os_str().is_empty() {
        return Err("preview save dialog returned an empty path".into());
    }
    fs::write(&path, request.contents.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

fn replace_record(state: &mut State, current: Arc<Progress>, finished_unix_ms: Option<u64>) {
    state.progress = Some(current.clone());
    let current_record = Arc::new(SearchRecord {
        execution: state.settings.execution(),
        started_unix_ms: unix_ms().saturating_sub(current.elapsed_ms),
        finished_unix_ms,
        progress: current.clone(),
    });
    if let Some(existing) = state
        .search_history
        .iter_mut()
        .find(|candidate| candidate.progress.id == current.id)
    {
        *existing = current_record;
    } else {
        state.search_history.push(current_record);
        if state.search_history.len() > MAX_SEARCH_HISTORY {
            state.search_history.remove(0);
        }
    }
}

fn apply_runtime_command(controls: &Controls, command: Command) {
    controls.update(|state| {
        state.error = None;
        match command.action {
            Action::Start { settings } => {
                state.settings = settings;
                let id = state
                    .search_history
                    .iter()
                    .map(|record| record.progress.id)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                let current = progress(id, &state.settings, Vec::new(), Phase::Running);
                replace_record(state, current, None);
                state.search_command = Some(command.id);
            }
            Action::Cancel {} => {
                if let Some(current) = state.progress.as_ref() {
                    let mut cancelled = (**current).clone();
                    cancelled.phase = Phase::Cancelled;
                    cancelled.finish = Some(Finish::Cancelled);
                    replace_record(state, Arc::new(cancelled), Some(unix_ms()));
                } else {
                    state.error = Some("There is no preview search to cancel".into());
                }
            }
            Action::SetSearchPaused { paused } => {
                if let Some(current) = state
                    .progress
                    .as_ref()
                    .filter(|progress| progress.phase == Phase::Running)
                {
                    let mut changed = (**current).clone();
                    changed.user_paused = paused;
                    replace_record(state, Arc::new(changed), None);
                } else {
                    state.error = Some("There is no preview search to pause".into());
                }
            }
            Action::Configure { settings } => state.settings = settings,
            Action::ConfigureIntensity { frame_budget_ms } => {
                state.settings.frame_budget_ms = frame_budget_ms;
            }
            Action::InspectCurrent {} => {
                state.error = Some("Current-world inspection requires a running game".into());
            }
            Action::Navigate { target, .. } => state.destination = Some(target),
        }
        state.last_command = command.id;
    });
}

struct ControlWorker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ControlWorker {
    fn start(controls: Arc<Controls>, directory: PathBuf) -> Result<Self, String> {
        let agent = AgentControl::open(&directory, &["search.control", "ui.control"])
            .map_err(|error| format!("cannot open preview agent control: {error}"))?;
        storage::write_json(
            &directory.join("current.json"),
            &controls.snapshot(),
            MAX_STATE_BYTES,
        )
        .map_err(|error| format!("cannot publish preview state: {error}"))?;
        agent
            .publish()
            .map_err(|error| format!("cannot publish preview endpoint: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("Search Probes preview control".into())
            .spawn(move || {
                let mut published = Instant::now();
                while !worker_stop.load(Ordering::Acquire) {
                    if let Err(error) = agent.poll(&controls) {
                        controls.update(|state| {
                            state.error = Some(format!("Agent command interface: {error}"))
                        });
                    }
                    controls.prepare(&directory);
                    if published.elapsed() >= Duration::from_millis(100) {
                        if let Err(error) = storage::write_json(
                            &directory.join("current.json"),
                            &controls.snapshot(),
                            MAX_STATE_BYTES,
                        ) {
                            controls.update(|state| {
                                state.error = Some(format!("Cannot publish preview state: {error}"))
                            });
                        }
                        let _ = agent.publish();
                        published = Instant::now();
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                let _ = agent.cleanup();
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ControlWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Preview {
    app: App,
    controls: Arc<Controls>,
    worker: ControlWorker,
    settings_directory: PathBuf,
    remove_directory: bool,
    overlay_visible: bool,
    announced: bool,
}

impl Preview {
    fn new(cc: &eframe::CreationContext<'_>, options: Options) -> Result<Self, String> {
        let loaded = storage::load_settings_with_recovery(&options.settings_directory)
            .map_err(|error| format!("cannot load preview settings: {error}"))?;
        let controls = Arc::new(Controls::new(preview_state(
            loaded.settings,
            loaded.recovery,
            options.fixtures,
        )));
        let worker =
            ControlWorker::start(Arc::clone(&controls), options.settings_directory.clone())?;
        Ok(Self {
            app: App::new(&cc.egui_ctx, Arc::clone(&controls)),
            controls,
            worker,
            settings_directory: options.settings_directory,
            remove_directory: options.remove_directory,
            overlay_visible: true,
            announced: false,
        })
    }
}

impl eframe::App for Preview {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let before = self.overlay_visible;
        let mut visible = self.overlay_visible;
        self.app.prepare_frame(ui.ctx(), visible, |requested| {
            visible = requested;
            Ok(())
        });
        self.overlay_visible = visible;
        if before != visible {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(visible));
        }
        if visible {
            self.app.ui(ui);
            self.app.show_notifications(ui.ctx());
        }
        if let Some(request) = self.app.take_export_request() {
            self.app.finish_export(save_export(&request));
            ui.ctx().request_repaint();
        }
        while let Some(command) = self.controls.pop() {
            apply_runtime_command(&self.controls, command);
            ui.ctx().request_repaint();
        }
        if !self.announced {
            println!(
                "Search Probes preview ready.\nControl it with:\n  cargo run -p search-probes-control -- --settings-dir '{}' actions",
                self.settings_directory.display()
            );
            self.announced = true;
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.controls
            .update(|state| state.availability = Availability::Stopped);
        self.worker.stop();
        if self.remove_directory {
            let _ = fs::remove_dir_all(&self.settings_directory);
        }
    }
}

fn main() -> eframe::Result {
    let options = arguments(std::env::args_os().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}\nusage: SearchProbesPreview [--settings-dir DIR] [--fixtures COUNT]");
        std::process::exit(2);
    });
    println!(
        "Search Probes preview control directory: {}",
        options.settings_directory.display()
    );
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("search-probes-ui-preview")
            .with_inner_size([1800.0, 1100.0])
            .with_min_inner_size([420.0, 300.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Search Probes — UI Preview",
        native,
        Box::new(move |cc| Ok(Box::new(Preview::new(cc, options)?))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_save_dialog_supports_every_export_format() {
        assert_eq!(
            export_filter(ExportFormat::Text),
            "Text files (*.txt) | *.txt"
        );
        assert_eq!(
            export_filter(ExportFormat::Csv),
            "CSV files (*.csv) | *.csv"
        );
        assert_eq!(
            export_filter(ExportFormat::Json),
            "JSON files (*.json) | *.json"
        );
    }

    #[test]
    fn arguments_accept_an_explicit_control_directory_and_fixture_count() {
        let options = arguments([
            OsString::from("--settings-dir"),
            OsString::from("control"),
            OsString::from("--fixtures"),
            OsString::from("23"),
        ])
        .unwrap();
        assert_eq!(options.settings_directory, std::path::Path::new("control"));
        assert!(!options.remove_directory);
        assert_eq!(options.fixtures, 23);
        assert!(arguments([OsString::from("--fixtures")]).is_err());
    }

    #[test]
    fn requested_fixtures_populate_current_progress_and_history() {
        let state = preview_state(Settings::default(), None, 23);
        assert_eq!(state.progress.as_ref().unwrap().results.len(), 23);
        assert_eq!(state.search_history.len(), 1);
        assert!(
            state
                .progress
                .as_ref()
                .unwrap()
                .results
                .iter()
                .filter_map(|result| result.fauna.as_ref())
                .any(|fauna| fauna.domain == Domain::Land && fauna.cave)
        );
    }

    #[test]
    fn runtime_commands_update_the_state_published_to_the_ui() {
        let controls = Controls::new(preview_state(Settings::default(), None, 2));
        apply_runtime_command(
            &controls,
            Command {
                generation: 1,
                id: 7,
                action: Action::Start {
                    settings: Settings::default(),
                },
            },
        );
        let state = controls.snapshot();
        assert_eq!(state.last_command, 7);
        assert_eq!(state.progress.as_ref().unwrap().phase, Phase::Running);
        assert_eq!(state.search_history.len(), 2);
    }
}

use crate::{
    bridge::Bridge,
    game_state::{Epoch, Snapshot},
    gameplay_hook, guide_hook,
    identity::Image,
    lifecycle_hook,
    navigation::Navigation,
    navigation_hook, platform, query_hooks,
    scheduler::Scheduler,
    workers::Workers,
};
use search_probes_model::{
    protocol::{Action, Availability, Command, Metrics, State},
    session::Phase,
    settings::Settings,
    storage,
};
use std::{
    sync::{Arc, Mutex, OnceLock, atomic::Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
static BRIDGE: OnceLock<Arc<Bridge>> = OnceLock::new();

pub fn report_startup_failure(error: &str) {
    let message: String = error.chars().take(4096).collect();
    if let Ok(directory) = storage::user_directory() {
        let _ = storage::write_bytes(
            &directory.join("startup-error.txt"),
            message.as_bytes(),
            16384,
        );
    }
    if let Some(bridge) = BRIDGE.get() {
        bridge.cancel.store(true, Ordering::Release);
        bridge.update(|state| {
            if state.availability != Availability::Unsupported {
                state.availability = Availability::Failed;
            }
            state.error = Some(message);
        });
    }
}

struct Runtime {
    image: Image,
    bridge: Arc<Bridge>,
    pool: Workers,
    scheduler: Scheduler,
    settings: Settings,
    epoch: Option<Epoch>,
    navigation: Navigation,
    navigation_polled: Option<Instant>,
    started: Option<Instant>,
    elapsed_ms: u64,
}

pub fn initialize() -> Result<(), String> {
    let executable = platform::executable_path().map_err(|error| error.to_string())?;
    if executable
        .file_name()
        .is_none_or(|name| !name.to_string_lossy().eq_ignore_ascii_case("NMS.exe"))
    {
        return Ok(());
    }
    let directory = storage::user_directory().map_err(|error| error.to_string())?;
    let ui = executable
        .parent()
        .ok_or("NMS has no installation directory")?
        .join("SearchProbes")
        .join("SearchProbes.exe");
    initialize_at(directory, ui)
}

pub(crate) fn initialize_at(
    directory: std::path::PathBuf,
    ui: std::path::PathBuf,
) -> Result<(), String> {
    let loaded = storage::load_settings(&directory);
    let settings = loaded.as_ref().cloned().unwrap_or_default();
    let session = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let mut state = State::new(session, std::process::id(), settings.clone());
    state.error = loaded
        .err()
        .map(|error| format!("Cannot load settings: {error}. The original file was preserved."));
    let bridge = Arc::new(Bridge::new(state));
    BRIDGE
        .set(bridge.clone())
        .map_err(|_| "Search Probes is already initialized")?;
    bridge.clone().start(directory, ui)?;
    let image = match Image::verify() {
        Ok(image) => image,
        Err(error) => {
            bridge.update(|state| {
                state.availability = Availability::Unsupported;
                state.error = Some(error.clone());
            });
            return Err(error);
        }
    };
    let result = attach(image, bridge.clone(), settings);
    if let Err(error) = &result {
        bridge.update(|state| {
            state.availability = Availability::Failed;
            state.error = Some(error.clone());
        });
    }
    result
}

#[cfg(feature = "investigation")]
pub(crate) fn stop() -> Result<(), String> {
    if let Some(bridge) = BRIDGE.get() {
        bridge.cancel.store(true, Ordering::Release);
        bridge.stop();
    }
    if let Some(runtime) = RUNTIME.get() {
        let mut runtime = runtime.lock().map_err(|_| "runtime poisoned")?;
        runtime.scheduler.session.cancel();
        runtime
            .bridge
            .update(|state| state.availability = Availability::Stopped);
    }
    let mut errors = Vec::new();
    for result in unsafe {
        [
            gameplay_hook::disable(),
            guide_hook::disable(),
            navigation_hook::disable(),
            query_hooks::disable(),
            lifecycle_hook::disable(),
        ]
    } {
        if let Err(error) = result {
            errors.push(format!("{error:?}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn attach(image: Image, bridge: Arc<Bridge>, settings: Settings) -> Result<(), String> {
    let base = image.base();
    let runtime = Runtime {
        image,
        bridge: bridge.clone(),
        pool: Workers::new(16)
            .map_err(|error| format!("Cannot start search workers: {error:?}"))?,
        scheduler: Scheduler::default(),
        settings,
        epoch: None,
        navigation: Navigation::default(),
        navigation_polled: None,
        started: None,
        elapsed_ms: 0,
    };
    let hooks: Result<(), String> = (|| {
        unsafe { lifecycle_hook::enable(base) }
            .map_err(|error| format!("Cannot observe game sessions: {error:?}"))?;
        unsafe { query_hooks::enable(base) }
            .map_err(|error| format!("Cannot capture generated planets: {error:?}"))?;
        unsafe { guide_hook::enable(base) }
            .map_err(|error| format!("Cannot launch Guide presets: {error:?}"))?;
        unsafe { navigation_hook::enable(base) }
            .map_err(|error| format!("Cannot preserve destination planets: {error:?}"))?;
        RUNTIME
            .set(Mutex::new(runtime))
            .map_err(|_| "Search Probes is already attached")?;
        bridge.update(|state| state.availability = Availability::MainMenu);
        unsafe { gameplay_hook::enable(base, frame) }
            .map_err(|error| format!("Cannot schedule gameplay searches: {error:?}"))?;
        Ok(())
    })();
    if let Err(mut error) = hooks {
        bridge.cancel.store(true, Ordering::Release);
        for rollback in unsafe {
            [
                gameplay_hook::disable(),
                guide_hook::disable(),
                navigation_hook::disable(),
                query_hooks::disable(),
                lifecycle_hook::disable(),
            ]
        } {
            if let Err(failure) = rollback {
                error.push_str(&format!("; hook restoration failed: {failure:?}"));
            }
        }
        return Err(error);
    }
    Ok(())
}

fn frame() {
    let Some(runtime) = RUNTIME.get() else {
        return;
    };
    match runtime.lock() {
        Ok(mut runtime) => runtime.tick(),
        Err(_) => {
            if let Some(bridge) = BRIDGE.get() {
                bridge.cancel.store(true, Ordering::Release);
                bridge.update(|state| {
                    state.availability = Availability::Failed;
                    state.error = Some(
                        "Search Probes stopped after an internal failure. Restart the game.".into(),
                    );
                });
            }
        }
    }
}

impl Runtime {
    fn reset(&mut self) {
        self.scheduler.reset();
        self.navigation = Navigation::default();
        navigation_hook::reset();
        self.navigation_polled = None;
        self.started = None;
        self.elapsed_ms = 0;
    }
    fn tick(&mut self) {
        let available = self
            .bridge
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .availability;
        if matches!(
            available,
            Availability::Failed | Availability::Unsupported | Availability::Stopped
        ) {
            return;
        }
        let transition = lifecycle_hook::epoch();
        self.bridge.observe_transition(transition);
        let snapshot = match Snapshot::read(&self.image, transition) {
            Ok(snapshot) => snapshot,
            Err(_) => {
                if self.epoch.take().is_some() {
                    self.reset();
                    self.bridge
                        .update(|state| state.generation = state.generation.wrapping_add(1));
                }
                self.bridge.update(|state| {
                    state.availability = Availability::MainMenu;
                    state.player = None;
                    state.progress = None;
                    state.cached_systems = 0;
                    state.destination = None;
                });
                return;
            }
        };
        if self.epoch.as_ref() != Some(&snapshot.epoch) {
            self.reset();
            self.epoch = Some(snapshot.epoch.clone());
            self.bridge.update(|state| {
                state.generation = state.generation.wrapping_add(1);
                state.progress = None;
                state.cached_systems = 0;
                state.destination = None;
            });
        }
        while let Some(command) = self.bridge.pop() {
            let id = command.id;
            let result = self.command(command, &snapshot);
            self.bridge.update(|state| {
                state.last_command = state.last_command.max(id);
                state.error = result.err().map(str::to_owned);
            });
        }
        let (request, overflow) = guide_hook::take();
        if overflow {
            self.bridge.update(|state| {
                state.error =
                    Some("A Guide request is already pending. Launch one preset at a time.".into())
            });
        }
        if let Some(request) = request.filter(|request| request.epoch == transition) {
            if let Err(error) = self.start_guide(request.slot, &snapshot) {
                self.bridge.update(|state| state.error = Some(error.into()));
            }
        }
        if self
            .navigation_polled
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(250))
        {
            self.navigation_polled = Some(Instant::now());
            if let Err(error) = unsafe { self.navigation.update(&self.image, &snapshot) } {
                self.bridge.update(|state| state.error = Some(error.into()));
            }
        }
        if let Err(error) = unsafe {
            self.scheduler.tick(
                &self.pool,
                self.image.base(),
                snapshot.planet as *const u8,
                snapshot.difficulty,
                self.settings.workers,
                Duration::from_millis(u64::from(self.settings.frame_budget_ms)),
                &self.bridge.cancel,
            )
        } {
            self.scheduler.session.fail(error);
            self.bridge.update(|state| state.error = Some(error.into()));
        }
        if let Some(started) = self.started {
            self.elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            if self
                .scheduler
                .session
                .progress()
                .is_none_or(|progress| progress.phase != Phase::Running)
            {
                self.started = None;
            }
        }
        let statistics = self.scheduler.statistics();
        let work = statistics.work;
        let object_tables = self.scheduler.object_statistics();
        let progress = self.scheduler.session.progress().cloned();
        let completed_results = progress
            .as_ref()
            .filter(|progress| {
                matches!(progress.phase, Phase::Cached | Phase::Completed)
                    && !progress.results.is_empty()
            })
            .map(|progress| progress.results.clone());
        self.bridge.update(|state| {
            state.availability = Availability::Ready;
            state.player = Some(snapshot.address);
            state.progress = progress;
            if let Some(results) = completed_results {
                state.results = results;
            }
            state.cached_systems = self.scheduler.session.cache_len();
            state.destination = self.navigation.destination();
            state.metrics = Metrics {
                generated_systems: work.systems,
                generated_planets: work.planets,
                metadata_captures: work.metadata_captures,
                colour_captures: work.colour_captures,
                object_scans: work.object_scans,
                object_table_loads: object_tables.loads,
                object_table_hits: object_tables.hits,
                cached_object_tables: object_tables.retained,
                name_captures: work.names,
                regions: statistics.regions,
                batches: statistics.batches,
                active_ns: statistics.active_ns,
                longest_batch_ns: statistics.longest_batch_ns,
                elapsed_ms: self.elapsed_ms,
            };
        });
    }
    fn start_guide(&mut self, slot: usize, snapshot: &Snapshot) -> Result<(), &'static str> {
        let preset = self
            .settings
            .presets
            .get(slot)
            .ok_or("This Guide slot is empty. Save a preset in Search Probes first.")?;
        self.scheduler
            .session
            .start(preset.search.clone(), snapshot.address, false, true)?;
        self.settings.search = preset.search.clone();
        self.bridge.cancel.store(false, Ordering::Release);
        self.scheduler.reset_statistics();
        self.started = Some(Instant::now());
        self.elapsed_ms = 0;
        self.bridge.update(|state| {
            state.settings = self.settings.clone();
            state.error = None;
        });
        Ok(())
    }
    fn command(&mut self, command: Command, snapshot: &Snapshot) -> Result<(), &'static str> {
        {
            let state = self
                .bridge
                .state
                .lock()
                .map_err(|_| "runtime state poisoned")?;
            command.validate(&state.session, state.generation)?;
        }
        match command.action {
            Action::Start {
                settings,
                restart,
                use_cache,
            } => {
                self.scheduler.session.start(
                    settings.search.clone(),
                    snapshot.address,
                    restart,
                    use_cache,
                )?;
                self.bridge.cancel.store(false, Ordering::Release);
                self.scheduler.reset_statistics();
                self.started = Some(Instant::now());
                self.elapsed_ms = 0;
                self.settings = settings;
                self.bridge
                    .update(|state| state.settings = self.settings.clone());
            }
            Action::Continue { results } => {
                self.scheduler.session.continue_live(results)?;
                self.bridge.cancel.store(false, Ordering::Release);
                self.started = Some(Instant::now());
            }
            Action::Cancel {} => {
                self.bridge.cancel.store(true, Ordering::Release);
                self.scheduler.session.cancel();
            }
            Action::Configure { settings } => {
                self.settings = settings;
                self.bridge
                    .update(|state| state.settings = self.settings.clone());
            }
            Action::ClearHistory {} => {
                self.scheduler.reset();
                self.started = None;
                self.elapsed_ms = 0;
            }
            Action::Navigate { address } => {
                let displayed = self
                    .bridge
                    .state
                    .lock()
                    .map_err(|_| "runtime state poisoned")?
                    .results
                    .iter()
                    .any(|result| result.address == address);
                if !displayed || address.system() == snapshot.address.system() {
                    return Err("destination is not a current search result");
                }
                unsafe { self.navigation.start(&self.image, snapshot, address) }?;
                self.navigation_polled = None;
            }
        }
        Ok(())
    }
}

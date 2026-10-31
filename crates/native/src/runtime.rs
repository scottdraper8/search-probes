use crate::{
    control_worker,
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
    controls::Controls,
    runtime::{Action, Availability, Command, Metrics, SearchPause, State},
    session::Phase,
    settings::{Settings, available_workers},
    storage,
};
use std::{
    sync::{Arc, OnceLock, atomic::Ordering},
    time::{Duration, Instant},
};

static RUNTIME: OnceLock<crate::dispatch_gate::Gate<Runtime>> = OnceLock::new();
static CONTROLS: OnceLock<Arc<Controls>> = OnceLock::new();

pub fn report_startup_failure(error: &str) {
    let message: String = error.chars().take(4096).collect();
    if let Ok(directory) = storage::user_directory() {
        let _ = storage::write_bytes(
            &directory.join("startup-error.txt"),
            message.as_bytes(),
            16384,
        );
    }
    if let Some(controls) = CONTROLS.get() {
        controls.cancel.store(true, Ordering::Release);
        controls.update(|state| {
            if state.availability != Availability::Unsupported {
                state.availability = Availability::Failed;
            }
            state.error = Some(message);
        });
    }
}

struct Runtime {
    image: Image,
    controls: Arc<Controls>,
    pool: Workers,
    scheduler: Scheduler,
    settings: Settings,
    epoch: Option<Epoch>,
    session: u64,
    navigation: Navigation,
    navigation_polled: Option<Instant>,
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
    initialize_at(directory)
}

pub(crate) fn initialize_at(directory: std::path::PathBuf) -> Result<(), String> {
    let loaded = storage::load_settings(&directory);
    let settings = loaded.as_ref().cloned().unwrap_or_default();
    let mut state = State::new(settings.clone());
    state.error = loaded
        .err()
        .map(|error| format!("Cannot load settings: {error}. The original file was preserved."));
    let controls = Arc::new(Controls::new(state));
    CONTROLS
        .set(controls.clone())
        .map_err(|_| "Search Probes is already initialized")?;
    control_worker::start(controls.clone(), directory)?;
    let image = match Image::verify() {
        Ok(image) => image,
        Err(error) => {
            controls.update(|state| {
                state.availability = Availability::Unsupported;
                state.error = Some(error.clone());
            });
            return Err(error);
        }
    };
    crate::overlay::attach(controls.clone())?;
    let result = attach(image, controls.clone(), settings);
    if let Err(error) = &result {
        controls.update(|state| {
            state.availability = Availability::Failed;
            state.error = Some(error.clone());
        });
    }
    result
}

fn attach(image: Image, controls: Arc<Controls>, settings: Settings) -> Result<(), String> {
    let base = image.base();
    let runtime = Runtime {
        image,
        controls: controls.clone(),
        pool: Workers::new(available_workers())
            .map_err(|error| format!("Cannot start search workers: {error:?}"))?,
        scheduler: Scheduler::default(),
        settings,
        epoch: None,
        session: lifecycle_hook::session(),
        navigation: Navigation::default(),
        navigation_polled: None,
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
            .set(crate::dispatch_gate::Gate::new(runtime))
            .map_err(|_| "Search Probes is already attached")?;
        controls.update(|state| state.availability = Availability::MainMenu);
        unsafe { crate::source_shutdown_hook::enable(base) }
            .map_err(|error| format!("Cannot observe source-manager shutdown: {error:?}"))?;
        unsafe { gameplay_hook::enable(base, frame) }
            .map_err(|error| format!("Cannot schedule gameplay searches: {error:?}"))?;
        Ok(())
    })();
    if let Err(mut error) = hooks {
        controls.cancel.store(true, Ordering::Release);
        for rollback in unsafe {
            [
                gameplay_hook::disable(),
                crate::source_shutdown_hook::disable(),
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
    let Some(admission) = runtime.admission() else {
        return;
    };
    match admission.lock() {
        Ok(Some(mut runtime)) => runtime.tick(),
        Ok(None) => {}
        Err(_) => {
            if let Some(controls) = CONTROLS.get() {
                controls.cancel.store(true, Ordering::Release);
                controls.update(|state| {
                    state.availability = Availability::Failed;
                    state.error = Some(
                        "Search Probes stopped after an internal failure. Restart the game.".into(),
                    );
                });
            }
        }
    }
}

/// Dispatch is closed across native teardown, without holding Runtime's mutex through native
/// callbacks. The original shutdown owns destruction of all retired source references.
pub(crate) struct SourceShutdown {
    dispatch: Option<crate::dispatch_gate::Closed<'static, Runtime>>,
}

/// # Safety
/// Called only at entry to irrevocable global resource shutdown. It cannot run inside a gameplay
/// dispatch or a source call on this thread.
pub(crate) unsafe fn begin_source_shutdown() -> Result<SourceShutdown, &'static str> {
    if let Some(controls) = CONTROLS.get() {
        controls.cancel.store(true, Ordering::Release);
    }
    let dispatch = RUNTIME.get().map(|runtime| runtime.close()).transpose()?;
    // Closing dispatch joined the previous frame and all of its borrowed search workers.
    let shutdown = SourceShutdown { dispatch };
    // Retain the token even if CPU cleanup panics: the detour must still run original teardown.
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(runtime) = RUNTIME.get() {
            let mut runtime = runtime
                .lock()
                .expect("dispatch drained before runtime cleanup");
            runtime.reset();
            runtime.epoch = None;
            runtime.controls.update(|state| {
                state.generation = state.generation.wrapping_add(1);
                state.progress = None;
                state.results.clear();
                state.cached_systems = 0;
                state.player = None;
                state.destination = None;
                state.metrics = Metrics::default();
                state.search_pause = Some(SearchPause::Loading);
                if !matches!(
                    state.availability,
                    Availability::Failed | Availability::Unsupported | Availability::Stopped
                ) {
                    state.availability = Availability::Loading;
                }
            });
        }
    }))
    .is_err()
    {
        report_startup_failure(
            "Search cleanup failed during source-manager shutdown. Restart the game.",
        );
    }
    Ok(shutdown)
}

/// # Safety
/// The matching original global shutdown has returned. Its source manager, source registry and
/// scene simulation must all be gone before a new source generation can be admitted.
pub(crate) unsafe fn finish_source_shutdown(
    shutdown: SourceShutdown,
    base: usize,
) -> Result<(), &'static str> {
    for offset in [0x6ef1e50, 0x59e5d70, 0x6ef1e58] {
        if crate::platform::read::<8>(base + offset)? != [0; 8] {
            return Err("native source shutdown retained a manager, registry or simulation");
        }
    }
    if let Some(dispatch) = shutdown.dispatch {
        dispatch.complete()?;
    }
    Ok(())
}

impl Runtime {
    fn reset(&mut self) {
        self.scheduler.reset();
        self.navigation = Navigation::default();
        navigation_hook::reset();
        self.navigation_polled = None;
        self.controls.reset_search_clock();
    }
    fn tick(&mut self) {
        let available = self
            .controls
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
        let session = lifecycle_hook::session();
        self.controls.observe_transition(transition, session);
        if session != self.session {
            self.reset();
            self.epoch = None;
            self.session = session;
        }
        let snapshot = match Snapshot::read(&self.image, transition) {
            Ok(snapshot) => snapshot,
            Err(_) => {
                self.controls.update(|state| {
                    state.availability = if lifecycle_hook::menu() {
                        Availability::MainMenu
                    } else {
                        Availability::Loading
                    };
                    state.player = None;
                    state.destination = None;
                    state.search_pause = Some(SearchPause::Loading);
                });
                return;
            }
        };
        if self.epoch.as_ref() != Some(&snapshot.epoch) {
            if self
                .epoch
                .as_ref()
                .is_some_and(|epoch| epoch.evaluation_rules_changed(&snapshot.epoch))
            {
                self.reset();
                self.controls.update(|state| {
                    state.generation = state.generation.wrapping_add(1);
                    state.progress = None;
                    state.search_command = None;
                    state.results.clear();
                    state.search_pause = None;
                });
            }
            if self
                .epoch
                .as_ref()
                .is_none_or(|epoch| epoch.resources_changed(&snapshot.epoch))
            {
                self.scheduler.refresh_resources();
            }
            self.epoch = Some(snapshot.epoch.clone());
        }
        while let Some(command) = self.controls.pop() {
            let id = command.id;
            let result = self.command(command, &snapshot);
            self.controls.update(|state| {
                state.last_command = state.last_command.max(id);
                state.error = result.err().map(str::to_owned);
            });
        }
        let (request, overflow) = guide_hook::take();
        if overflow {
            self.controls.update(|state| {
                state.error =
                    Some("A Guide request is already pending. Launch one preset at a time.".into())
            });
        }
        if let Some(request) = request.filter(|request| request.epoch == transition) {
            if let Err(error) = self.start_guide(request.slot, &snapshot) {
                self.controls
                    .update(|state| state.error = Some(error.into()));
            }
        }
        if self
            .navigation_polled
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(250))
        {
            self.navigation_polled = Some(Instant::now());
            if let Err(error) = unsafe { self.navigation.update(&self.image, &snapshot) } {
                self.controls
                    .update(|state| state.error = Some(error.into()));
            }
        }
        let in_search_galaxy = self.scheduler.session.is_in_search_galaxy(snapshot.address);
        if in_search_galaxy {
            if let Err(error) = unsafe {
                self.scheduler.tick(
                    &self.pool,
                    self.image.base(),
                    snapshot.planet as *const u8,
                    snapshot.difficulty,
                    self.settings.workers,
                    Duration::from_millis(u64::from(self.settings.frame_budget_ms)),
                    &self.controls.cancel,
                )
            } {
                self.scheduler.session.fail(error);
                self.controls
                    .update(|state| state.error = Some(error.into()));
            }
        }
        if let Err(error) = unsafe { self.scheduler.name_system_results(self.image.base()) } {
            self.scheduler.session.fail(error);
            self.controls
                .update(|state| state.error = Some(error.into()));
        }
        let statistics = self.scheduler.statistics();
        let work = statistics.work;
        let object_tables = self.scheduler.object_statistics();
        let progress = self.scheduler.session.progress().cloned();
        self.controls.update(|state| {
            state.availability = Availability::Ready;
            state.player = Some(snapshot.address);
            state.publish_progress(progress);
            state.search_pause = (!in_search_galaxy
                && state
                    .progress
                    .as_ref()
                    .is_some_and(|progress| progress.phase == Phase::Running))
            .then_some(SearchPause::OtherGalaxy);
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
                elapsed_ms: 0,
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
            .start(preset.search.clone(), snapshot.address)?;
        self.settings.search = preset.search.clone();
        self.controls.cancel.store(false, Ordering::Release);
        self.scheduler.reset_statistics();
        self.controls.reset_search_clock();
        self.controls.update(|state| {
            state.settings = self.settings.clone();
            state.error = None;
            state.availability = Availability::Ready;
            state.search_pause = None;
            state.metrics = Metrics::default();
            state.publish_progress(self.scheduler.session.progress().cloned());
        });
        Ok(())
    }
    fn command(&mut self, command: Command, snapshot: &Snapshot) -> Result<(), &'static str> {
        {
            let state = self
                .controls
                .state
                .lock()
                .map_err(|_| "runtime state poisoned")?;
            command.validate(state.generation)?;
        }
        match command.action {
            Action::Start { settings } => {
                self.scheduler
                    .session
                    .start(settings.search.clone(), snapshot.address)?;
                self.controls.cancel.store(false, Ordering::Release);
                self.scheduler.reset_statistics();
                self.controls.reset_search_clock();
                self.settings = settings;
                self.controls.update(|state| {
                    state.search_command = Some(command.id);
                    state.settings = self.settings.clone();
                    state.availability = Availability::Ready;
                    state.search_pause = None;
                    state.metrics = Metrics::default();
                    state.publish_progress(self.scheduler.session.progress().cloned());
                });
            }
            Action::Cancel {} => {
                self.controls.cancel.store(true, Ordering::Release);
                self.scheduler.session.cancel();
            }
            Action::Configure { settings } => {
                self.settings = settings;
                self.controls
                    .update(|state| state.settings = self.settings.clone());
            }
            Action::Navigate { target } => {
                let displayed = self
                    .controls
                    .state
                    .lock()
                    .map_err(|_| "runtime state poisoned")?
                    .results
                    .iter()
                    .any(|result| result.target() == target);
                if !displayed {
                    return Err("destination is not a current search result");
                }
                unsafe { self.navigation.start(&self.image, snapshot, target) }?;
                self.navigation_polled = None;
            }
        }
        Ok(())
    }
}

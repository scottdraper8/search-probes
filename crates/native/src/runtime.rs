use crate::{
    control_worker, discovery_hook,
    game_state::{Epoch, Snapshot},
    gameplay_hook, guide_hook,
    identity::Image,
    lifecycle_hook, mission_text_hook,
    navigation::Navigation,
    navigation_hook, platform, query_hooks,
    scheduler::{Scheduler, TickRequest},
    workers::Workers,
};
use search_probes_model::{
    controls::Controls,
    runtime::{Action, Availability, Command, CurrentWorld, Metrics, SearchPause, State},
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
static APPLICATION_TRANSITION: std::sync::Mutex<Option<LifecycleTransition>> =
    std::sync::Mutex::new(None);

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
    guide_topics: Option<guide_hook::Topics>,
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
    mission_text_hook::set_presets(&settings.presets)
        .map_err(|error| format!("Cannot name Guide presets: {error}"))?;
    let runtime = Runtime {
        image,
        controls: controls.clone(),
        pool: Workers::new(available_workers())
            .map_err(|error| format!("Cannot start search workers: {error:?}"))?,
        scheduler: Scheduler::default(),
        settings,
        epoch: None,
        session: lifecycle_hook::session(),
        guide_topics: None,
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
        unsafe { discovery_hook::enable(base) }
            .map_err(|error| format!("Cannot observe creature discoveries: {error:?}"))?;
        unsafe { mission_text_hook::enable(base) }
            .map_err(|error| format!("Cannot format Search Probes mission text: {error:?}"))?;
        RUNTIME
            .set(crate::dispatch_gate::Gate::new(runtime))
            .map_err(|_| "Search Probes is already attached")?;
        controls.update(|state| state.availability = Availability::MainMenu);
        unsafe { gameplay_hook::enable(base, frame) }
            .map_err(|error| format!("Cannot schedule gameplay searches: {error:?}"))?;
        Ok(())
    })();
    if let Err(mut error) = hooks {
        controls.cancel.store(true, Ordering::Release);
        for rollback in unsafe {
            [
                gameplay_hook::disable(),
                mission_text_hook::disable(),
                discovery_hook::disable(),
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

/// Dispatch closure retained across application-state transitions until gameplay resumes.
pub(crate) struct LifecycleTransition {
    dispatch: Option<crate::dispatch_gate::Closed<'static, Runtime>>,
}

/// Close new admissions before the application changes state and drain the active callback.
/// Nested state changes share this token; it remains closed while the game is outside gameplay.
pub(crate) fn begin_application_transition() -> Result<(), &'static str> {
    let mut transition = APPLICATION_TRANSITION
        .lock()
        .map_err(|_| "application transition state poisoned")?;
    if transition.is_none() {
        let dispatch = RUNTIME.get().map(|runtime| runtime.close()).transpose()?;
        if let Some(runtime) = RUNTIME.get() {
            runtime.lock()?.restore_application_sources()?;
        }
        *transition = Some(LifecycleTransition { dispatch });
    }
    Ok(())
}

/// Reopen only after the application FSM has entered a verified gameplay state.
pub(crate) fn finish_application_transition() -> Result<(), &'static str> {
    let transition = APPLICATION_TRANSITION
        .lock()
        .map_err(|_| "application transition state poisoned")?
        .take();
    if let Some(transition) = transition
        && let Some(dispatch) = transition.dispatch
    {
        dispatch.complete()?;
    }
    Ok(())
}

/// Retire application-owned sources after the native FSM has left gameplay. Search state contains
/// only copied model data and survives ordinary loading; a main-menu transition starts a new game
/// session and resets it. Dispatch is already closed at this point.
pub(crate) fn retire_application_sources(menu: bool) -> Result<(), &'static str> {
    if let Some(runtime) = RUNTIME.get() {
        let mut runtime = runtime.lock()?;
        if menu {
            runtime.reset_session();
            runtime.controls.cancel.store(true, Ordering::Release);
        } else {
            runtime.retire_application_sources();
        }
        runtime.epoch = None;
        runtime.controls.update(|state| {
            state.generation = state.generation.wrapping_add(1);
            state.player = None;
            state.destination = None;
            state.current_world = None;
            if menu {
                state.progress = None;
                state.search_command = None;
                state.results.clear();
                state.cached_systems = 0;
                state.metrics = Metrics::default();
                state.search_pause = None;
            } else {
                state.search_pause = Some(SearchPause::Loading);
            }
            if !matches!(
                state.availability,
                Availability::Failed | Availability::Unsupported | Availability::Stopped
            ) {
                state.availability = if menu {
                    Availability::MainMenu
                } else {
                    Availability::Loading
                };
            }
        });
    }
    Ok(())
}

impl Runtime {
    fn reset_search(&mut self) {
        self.scheduler.reset();
        self.controls.reset_search_clock();
    }

    fn retire_application_sources(&mut self) {
        // Scheduler state is copied model data. Do not reset or cancel it here: this is the pause
        // boundary that preserves an active search through same-session warps and teleports.
        self.navigation.retire_application_sources();
        navigation_hook::reset();
        discovery_hook::reset();
        self.guide_topics = None;
        self.navigation_polled = None;
    }

    fn restore_application_sources(&mut self) -> Result<(), &'static str> {
        if let Some(topics) = self.guide_topics.take() {
            unsafe { topics.restore()? };
        }
        Ok(())
    }

    unsafe fn update_guide(&mut self, application: usize) -> Result<(), &'static str> {
        if self.guide_topics.is_none() {
            self.guide_topics = Some(unsafe { guide_hook::Topics::locate(application)? });
        }
        unsafe {
            self.guide_topics
                .as_ref()
                .expect("located Guide topics")
                .sync(self.settings.presets.len())
        }
    }

    fn reset_session(&mut self) {
        self.reset_search();
        self.retire_application_sources();
        self.navigation = Navigation::default();
        mission_text_hook::clear();
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
            self.reset_session();
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
                    state.current_world = None;
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
                self.reset_search();
                self.controls.update(|state| {
                    state.generation = state.generation.wrapping_add(1);
                    state.progress = None;
                    state.search_command = None;
                    state.results.clear();
                    state.search_pause = None;
                    state.current_world = None;
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
        if let Err(error) = unsafe { self.update_guide(snapshot.application) } {
            self.controls
                .update(|state| state.error = Some(error.into()));
        }
        let (request, overflow) = guide_hook::take();
        if overflow {
            self.controls.update(|state| {
                state.error =
                    Some("A Guide request is already pending. Launch one preset at a time.".into())
            });
        }
        if let Some(request) = request.filter(|request| request.epoch == transition)
            && let Err(error) = self.start_guide(request.slot, &snapshot)
        {
            self.controls
                .update(|state| state.error = Some(error.into()));
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
        if in_search_galaxy
            && let Err(error) = unsafe {
                self.scheduler.tick(TickRequest {
                    pool: &self.pool,
                    base: self.image.base(),
                    live: snapshot.planet as *const u8,
                    difficulty: snapshot.difficulty,
                    workers: self.settings.workers,
                    budget: Duration::from_millis(u64::from(self.settings.frame_budget_ms)),
                    cancel: &self.controls.cancel,
                })
            }
        {
            self.scheduler.session.fail(error);
            self.controls
                .update(|state| state.error = Some(error.into()));
        }
        if let Err(error) = unsafe { self.scheduler.name_results(self.image.base()) } {
            self.scheduler.session.fail(error);
            self.controls
                .update(|state| state.error = Some(error.into()));
        }
        let statistics = self.scheduler.statistics();
        let work = statistics.work;
        let object_tables = self.scheduler.object_statistics();
        let progress = self.scheduler.session.progress().cloned();
        self.controls.update(|state| {
            if state.player != Some(snapshot.address) {
                state.current_world = None;
            }
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
                generated_fauna: work.fauna,
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
            state.current_world = None;
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
                if self.settings.presets != settings.presets {
                    mission_text_hook::set_presets(&settings.presets)?;
                }
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
                    state.current_world = None;
                    state.publish_progress(self.scheduler.session.progress().cloned());
                });
            }
            Action::Cancel {} => {
                self.controls.cancel.store(true, Ordering::Release);
                self.scheduler.session.cancel();
            }
            Action::Configure { settings } => {
                if self.settings.presets != settings.presets {
                    mission_text_hook::set_presets(&settings.presets)?;
                }
                self.settings = settings;
                self.controls.update(|state| {
                    state.settings = self.settings.clone();
                    state.current_world = None;
                });
            }
            Action::InspectCurrent {} => {
                let criteria = self.settings.search.criteria.clone();
                let plan = criteria.compile()?;
                let outcome = unsafe {
                    self.scheduler.inspect_current(
                        self.image.base(),
                        snapshot.planet as *const u8,
                        snapshot.difficulty,
                        snapshot.address,
                        &plan,
                    )
                }?;
                if outcome.cancelled {
                    return Err("current-world inspection was cancelled");
                }
                let world = CurrentWorld::capture(
                    command.id,
                    snapshot.address,
                    criteria,
                    &plan,
                    &outcome.snapshot.system,
                    &outcome.snapshot.planets[..outcome.snapshot.count],
                )?;
                self.controls
                    .update(|state| state.current_world = Some(world));
            }
            Action::Navigate { target } => {
                let displayed = self
                    .controls
                    .state
                    .lock()
                    .map_err(|_| "runtime state poisoned")?
                    .results
                    .iter()
                    .find(|result| result.target() == target)
                    .cloned()
                    .ok_or("destination is not a current search result")?;
                unsafe { self.navigation.start(&self.image, snapshot, &displayed) }?;
                self.navigation_polled = None;
            }
        }
        Ok(())
    }
}

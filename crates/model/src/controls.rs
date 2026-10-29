use crate::{
    runtime::{Action, Availability, Command, MAX_PENDING_COMMANDS, SearchPause, State},
    session::Phase,
    storage,
};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct SearchClock {
    elapsed: Duration,
    running_since: Option<Instant>,
}

impl SearchClock {
    fn elapsed(&self, now: Instant) -> Duration {
        self.elapsed
            + self
                .running_since
                .map_or(Duration::ZERO, |started| now.duration_since(started))
    }

    fn observe(&mut self, now: Instant, running: bool) -> u64 {
        self.elapsed = self.elapsed(now);
        self.running_since = running.then_some(now);
        self.elapsed.as_millis().min(u64::MAX as u128) as u64
    }
}

#[derive(Default)]
struct Commands {
    next: u64,
    preparing: bool,
    incoming: VecDeque<Command>,
    ready: VecDeque<Command>,
}

pub struct Controls {
    pub state: Mutex<State>,
    pub cancel: AtomicBool,
    commands: Mutex<Commands>,
    transition: AtomicU64,
    session: AtomicU64,
    search_clock: Mutex<SearchClock>,
}

impl Controls {
    pub fn new(state: State) -> Self {
        Self {
            state: Mutex::new(state),
            cancel: AtomicBool::new(false),
            commands: Mutex::new(Commands::default()),
            transition: AtomicU64::new(0),
            session: AtomicU64::new(0),
            search_clock: Mutex::new(SearchClock::default()),
        }
    }
    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn update(&self, operation: impl FnOnce(&mut State)) {
        self.update_at(None, operation);
    }
    fn update_at(&self, now: Option<Instant>, operation: impl FnOnce(&mut State)) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        operation(&mut state);
        let running = state.availability == Availability::Ready
            && state.search_pause.is_none()
            && state
                .progress
                .as_ref()
                .is_some_and(|progress| progress.phase == Phase::Running);
        state.metrics.elapsed_ms = self
            .search_clock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe(now.unwrap_or_else(Instant::now), running);
    }
    pub fn reset_search_clock(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *self.search_clock.lock().unwrap_or_else(|e| e.into_inner()) = SearchClock::default();
        state.metrics.elapsed_ms = 0;
    }
    pub fn submit(&self, generation: u64, action: Action) -> Result<u64, &'static str> {
        let state = self.state.lock().map_err(|_| "runtime state poisoned")?;
        let mut queue = self.commands.lock().map_err(|_| "command queue poisoned")?;
        if queue.incoming.len() + queue.ready.len() + usize::from(queue.preparing)
            >= MAX_PENDING_COMMANDS
        {
            return Err("The game is not accepting commands. Wait for it to resume.");
        }
        let id = queue
            .next
            .checked_add(1)
            .ok_or("command identity exhausted")?;
        let command = Command {
            generation,
            id,
            action,
        };
        command.validate(state.generation)?;
        if matches!(command.action, Action::Cancel {}) {
            self.cancel.store(true, Ordering::Release);
        }
        queue.next = id;
        queue.incoming.push_back(command);
        Ok(id)
    }
    pub fn prepare(&self, directory: &Path) {
        let mut queue = self.commands.lock().unwrap_or_else(|e| e.into_inner());
        let Some(command) = queue.incoming.pop_front() else {
            return;
        };
        queue.preparing = true;
        drop(queue);
        let validated = command.validate(self.snapshot().generation);
        let persisted =
            validated
                .map_err(std::io::Error::other)
                .and_then(|_| match &command.action {
                    Action::Start { settings, .. } | Action::Configure { settings } => {
                        storage::save_settings(directory, settings)
                    }
                    _ => Ok(()),
                });
        let mut queue = self.commands.lock().unwrap_or_else(|e| e.into_inner());
        queue.preparing = false;
        match persisted {
            Ok(()) => queue.ready.push_back(command),
            Err(error) => {
                drop(queue);
                self.update(|state| {
                    state.last_command = state.last_command.max(command.id);
                    state.error = Some(format!("Command could not be prepared: {error}"));
                });
            }
        }
    }
    pub fn pop(&self) -> Option<Command> {
        self.commands
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ready
            .pop_front()
    }
    pub fn observe_transition(&self, current: u64, session: u64) {
        let new_session = self.session.fetch_max(session, Ordering::AcqRel) < session;
        if self.transition.fetch_max(current, Ordering::AcqRel) >= current && !new_session {
            return;
        }
        if new_session {
            self.cancel.store(true, Ordering::Release);
        }
        self.update(|state| {
            if matches!(
                state.availability,
                Availability::Failed | Availability::Unsupported | Availability::Stopped
            ) {
                return;
            }
            state.generation = state.generation.wrapping_add(1);
            state.availability = Availability::Loading;
            state.player = None;
            state.destination = None;
            state.search_pause = Some(SearchPause::Loading);
            if new_session {
                state.progress = None;
                state.search_command = None;
                state.results.clear();
                state.cached_systems = 0;
                state.search_pause = None;
            }
        });
    }
}

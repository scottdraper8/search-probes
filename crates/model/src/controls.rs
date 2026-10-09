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
                state.results.clear();
                state.cached_systems = 0;
                state.search_pause = None;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    #[test]
    fn search_clock_excludes_travel_and_completion_and_accumulates_continuation() {
        use crate::{address::Address, session::Session};
        let controls = Controls::new(State::new(Settings::default()));
        let mut session = Session::default();
        session
            .start(
                Settings::default().search,
                Address::from_raw(0).unwrap(),
                false,
                false,
            )
            .unwrap();
        let start = Instant::now();
        controls.update_at(Some(start), |state| {
            state.availability = Availability::Ready;
            state.publish_progress(session.progress().cloned());
        });
        controls.update_at(Some(start + Duration::from_secs(2)), |state| {
            state.availability = Availability::Loading;
            state.search_pause = Some(SearchPause::Loading);
        });
        controls.update_at(Some(start + Duration::from_secs(62)), |state| {
            state.availability = Availability::Ready;
            state.search_pause = Some(SearchPause::OtherGalaxy);
        });
        assert_eq!(controls.snapshot().metrics.elapsed_ms, 2000);
        controls.update_at(Some(start + Duration::from_secs(122)), |state| {
            state.search_pause = None;
        });
        controls.update_at(Some(start + Duration::from_secs(125)), |state| {
            state.progress.as_mut().unwrap().phase = Phase::Completed;
        });
        assert_eq!(controls.snapshot().metrics.elapsed_ms, 5000);
        controls.update_at(Some(start + Duration::from_secs(185)), |state| {
            state.progress.as_mut().unwrap().phase = Phase::Running;
        });
        controls.update_at(Some(start + Duration::from_secs(189)), |state| {
            state.progress.as_mut().unwrap().phase = Phase::Cancelled;
        });
        assert_eq!(controls.snapshot().metrics.elapsed_ms, 9000);
        controls.reset_search_clock();
        controls.update_at(Some(start + Duration::from_secs(200)), |state| {
            state.progress.as_mut().unwrap().phase = Phase::Running;
        });
        controls.update_at(Some(start + Duration::from_secs(201)), |_| {});
        assert_eq!(controls.snapshot().metrics.elapsed_ms, 1000);
    }
    #[test]
    fn travel_suspends_without_cancelling_and_new_sessions_clear_search_state() {
        use crate::{address::Address, cache::CachedMatch, session::Session};
        let controls = Controls::new(State::new(Settings::default()));
        controls.observe_transition(1, 1);
        controls.cancel.store(false, Ordering::Release);
        let mut session = Session::default();
        session
            .start(
                Settings::default().search,
                Address::from_raw(0).unwrap(),
                false,
                false,
            )
            .unwrap();
        let result = CachedMatch {
            address: Address::from_raw(1 << 40).unwrap().planet(0).unwrap(),
            kind: crate::target::TargetKind::Planet,
            name: None,
        };
        controls.update(|state| {
            state.publish_progress(session.progress().cloned());
            state.results.push(result.clone());
            state.cached_systems = 5;
        });
        controls.observe_transition(2, 1);
        let state = controls.snapshot();
        assert!(!controls.cancel.load(Ordering::Acquire));
        assert!(state.progress.is_some());
        assert_eq!(state.results[0].address, result.address);
        assert_eq!(state.cached_systems, 5);
        assert_eq!(state.search_pause, Some(SearchPause::Loading));
        controls.observe_transition(1, 1);
        assert_eq!(controls.snapshot().generation, state.generation);
        controls.observe_transition(3, 2);
        let state = controls.snapshot();
        assert!(controls.cancel.load(Ordering::Acquire));
        assert!(state.progress.is_none());
        assert!(state.results.is_empty());
        assert_eq!(state.cached_systems, 0);
    }
    #[test]
    fn bounded_commands_reject_stale_generations_and_cancel_immediately() {
        let controls = Controls::new(State::new(Settings::default()));
        assert!(controls.submit(0, Action::Cancel {}).is_err());
        assert!(!controls.cancel.load(Ordering::Acquire));
        for _ in 0..MAX_PENDING_COMMANDS {
            controls.submit(1, Action::Cancel {}).unwrap();
        }
        assert!(controls.cancel.load(Ordering::Acquire));
        assert!(controls.submit(1, Action::Cancel {}).is_err());
        controls.observe_transition(7, 1);
        let state = controls.snapshot();
        assert_eq!(state.generation, 2);
        assert!(controls.submit(1, Action::Cancel {}).is_err());
        controls.observe_transition(7, 1);
        assert_eq!(controls.snapshot().generation, 2);
    }
    #[test]
    fn commands_require_preparation_and_preserve_order() {
        let controls = Controls::new(State::new(Settings::default()));
        let a = controls.submit(1, Action::Cancel {}).unwrap();
        let b = controls.submit(1, Action::ClearHistory {}).unwrap();
        assert!(controls.pop().is_none());
        controls.prepare(Path::new("unused"));
        controls.prepare(Path::new("unused"));
        assert_eq!(controls.pop().unwrap().id, a);
        assert_eq!(controls.pop().unwrap().id, b);
        assert!(controls.pop().is_none());
    }
}

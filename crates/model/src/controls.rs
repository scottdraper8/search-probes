use crate::{
    runtime::{
        Action, Availability, Command, LifecycleEvent, LifecycleEventKind, MAX_PENDING_COMMANDS,
        SearchPause, State,
    },
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
    time::Instant,
};

#[derive(Default)]
struct EventJournal {
    next: u64,
    events: VecDeque<LifecycleEvent>,
}

impl EventJournal {
    fn push(
        &mut self,
        progress: &crate::session::Progress,
        kind: LifecycleEventKind,
        published_ms: u64,
    ) {
        let Some(next) = self.next.checked_add(1) else {
            return;
        };
        self.next = next;
        self.events.push_back(LifecycleEvent {
            sequence: next,
            published_ms,
            search_id: progress.id,
            elapsed_ms: progress.elapsed_ms,
            systems: progress.systems,
            results: progress.results.len(),
            kind,
        });
        while self.events.len() > 64 {
            self.events.pop_front();
        }
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
    events: Mutex<EventJournal>,
    started: Instant,
    revision: AtomicU64,
}

impl Controls {
    pub fn new(state: State) -> Self {
        Self {
            state: Mutex::new(state),
            cancel: AtomicBool::new(false),
            commands: Mutex::new(Commands::default()),
            transition: AtomicU64::new(0),
            session: AtomicU64::new(0),
            events: Mutex::new(EventJournal::default()),
            started: Instant::now(),
            revision: AtomicU64::new(1),
        }
    }
    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn availability(&self) -> Availability {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .availability
    }
    /// Presentation consumers can poll without copying unchanged results and retained records.
    pub fn snapshot_if_changed(&self, last_revision: u64) -> Option<(u64, State)> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let revision = self.revision.load(Ordering::Acquire);
        (revision != last_revision).then(|| (revision, state.clone()))
    }
    pub fn update(&self, operation: impl FnOnce(&mut State)) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = state
            .progress
            .as_ref()
            .map(|progress| (progress.id, progress.phase, progress.user_paused));
        let prior_pause = state.search_pause;
        operation(&mut state);
        self.revision.fetch_add(1, Ordering::Release);
        let Some(progress) = state.progress.as_ref() else {
            return;
        };
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        let new_search = before.is_none_or(|(id, _, _)| id != progress.id);
        if new_search {
            events.push(progress, LifecycleEventKind::Started, self.uptime_ms());
        }
        if progress.phase != Phase::Running {
            if (new_search || before.is_some_and(|(_, phase, _)| phase == Phase::Running))
                && let Some(finish) = progress.finish
            {
                events.push(
                    progress,
                    LifecycleEventKind::Finished { finish },
                    self.uptime_ms(),
                );
            }
        } else if !new_search
            && !progress.user_paused
            && before.is_some_and(|(_, _, paused)| !paused)
        {
            let automatic = |pause| match pause {
                Some(SearchPause::Loading | SearchPause::OtherGalaxy) => pause,
                _ => None,
            };
            match (automatic(prior_pause), automatic(state.search_pause)) {
                (None, Some(cause)) => {
                    events.push(
                        progress,
                        LifecycleEventKind::AutomaticPause { cause },
                        self.uptime_ms(),
                    );
                }
                (Some(cause), None) => {
                    events.push(
                        progress,
                        LifecycleEventKind::AutomaticResume { cause },
                        self.uptime_ms(),
                    );
                }
                (None, None) | (Some(_), Some(_)) => {}
            }
        }
    }
    /// Independent consumers retain their own sequence cursor. The bounded journal is not drained.
    pub fn lifecycle_events_after(&self, sequence: u64) -> Vec<LifecycleEvent> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .events
            .iter()
            .filter(|event| event.sequence > sequence)
            .cloned()
            .collect()
    }
    pub fn uptime_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
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
                    Action::ConfigureIntensity { frame_budget_ms } => {
                        let mut settings = storage::load_settings(directory)?;
                        settings.frame_budget_ms = *frame_budget_ms;
                        storage::save_settings(directory, &settings)
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
                state.search_history.clear();
                state.search_command = None;
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn unchanged_presentation_snapshots_do_not_clone_state_and_updates_advance_revision() {
        let controls = Controls::new(State::new(crate::settings::Settings::default()));
        let (revision, initial) = controls.snapshot_if_changed(0).unwrap();
        assert!(controls.snapshot_if_changed(revision).is_none());
        controls.update(|state| state.error = Some("new state".into()));
        let (next, updated) = controls.snapshot_if_changed(revision).unwrap();
        assert!(next > revision);
        assert!(initial.error.is_none());
        assert_eq!(updated.error.as_deref(), Some("new state"));
        assert!(controls.snapshot_if_changed(next).is_none());
    }

    #[test]
    fn lifecycle_journal_records_automatic_causes_and_terminal_outcomes_without_manual_pause_events()
     {
        use crate::{
            address::{Address, Coordinates},
            runtime::LifecycleEventKind,
            session::Session,
            settings::Settings,
        };
        let settings = Settings::default();
        let controls = Controls::new(State::new(settings.clone()));
        let origin = Address::new(Coordinates {
            x: 1,
            y: 2,
            z: 3,
            galaxy: 0,
            system: 1,
            planet: 0,
        })
        .unwrap();
        let mut session = Session::default();
        session
            .start(settings.search.clone(), origin, settings.execution())
            .unwrap();
        let publish = |session: &Session, pause| {
            controls.update(|state| {
                state.publish_progress(session.progress().cloned().map(std::sync::Arc::new));
                state.search_pause = pause;
            })
        };
        publish(&session, None);
        session.set_paused(true).unwrap();
        publish(&session, Some(SearchPause::User));
        session.set_available(false);
        publish(&session, Some(SearchPause::Loading));
        session.set_available(true);
        publish(&session, Some(SearchPause::User));
        session.set_paused(false).unwrap();
        publish(&session, None);
        assert_eq!(controls.lifecycle_events_after(0).len(), 1);
        session.set_available(false);
        publish(&session, Some(SearchPause::Loading));
        publish(&session, Some(SearchPause::Loading));
        publish(&session, Some(SearchPause::OtherGalaxy));
        session.set_available(true);
        publish(&session, None);
        session.cancel();
        publish(&session, None);
        publish(&session, None);
        let events = controls.lifecycle_events_after(0);
        assert_eq!(events.len(), 4);
        assert_eq!(
            events[1].kind,
            LifecycleEventKind::AutomaticPause {
                cause: SearchPause::Loading
            }
        );
        assert_eq!(
            events[2].kind,
            LifecycleEventKind::AutomaticResume {
                cause: SearchPause::OtherGalaxy
            }
        );
        assert_eq!(
            events[3].kind,
            LifecycleEventKind::Finished {
                finish: crate::session::Finish::Cancelled
            }
        );
        assert!(
            events
                .iter()
                .all(|event| event.published_ms <= controls.uptime_ms())
        );
        assert_eq!(
            controls.lifecycle_events_after(events[3].sequence),
            events[4..]
        );
        // Reading does not drain events needed by another consumer.
        assert_eq!(controls.lifecycle_events_after(0), events);
    }

    fn test_directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "search-probes-controls-{}-{}",
            std::process::id(),
            DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn preparing_live_intensity_preserves_every_other_saved_setting() {
        let directory = test_directory();
        let mut settings = crate::settings::Settings::default();
        settings.search.results = 17;
        settings.workers = Some(3);
        storage::save_settings(&directory, &settings).expect("save initial settings");
        let controls = Controls::new(State::new(settings.clone()));

        controls
            .submit(
                1,
                Action::ConfigureIntensity {
                    frame_budget_ms: 16,
                },
            )
            .expect("submit intensity change");
        controls.prepare(&directory);

        assert!(matches!(
            controls.pop().map(|command| command.action),
            Some(Action::ConfigureIntensity {
                frame_budget_ms: 16
            })
        ));
        let saved = storage::load_settings(&directory).expect("load updated settings");
        settings.frame_budget_ms = 16;
        assert_eq!(saved, settings);

        std::fs::remove_dir_all(directory).expect("remove test settings");
    }
}

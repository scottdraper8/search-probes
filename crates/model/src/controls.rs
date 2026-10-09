use crate::{
    runtime::{Action, Availability, Command, MAX_PENDING_COMMANDS, State},
    storage,
};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

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
}

impl Controls {
    pub fn new(state: State) -> Self {
        Self {
            state: Mutex::new(state),
            cancel: AtomicBool::new(false),
            commands: Mutex::new(Commands::default()),
            transition: AtomicU64::new(0),
        }
    }
    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn update(&self, operation: impl FnOnce(&mut State)) {
        operation(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()));
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
    pub fn observe_transition(&self, current: u64) {
        if self.transition.swap(current, Ordering::AcqRel) == current {
            return;
        }
        self.cancel.store(true, Ordering::Release);
        self.update(|state| {
            if matches!(
                state.availability,
                Availability::Failed | Availability::Unsupported | Availability::Stopped
            ) {
                return;
            }
            state.generation = state.generation.wrapping_add(1);
            state.availability = Availability::MainMenu;
            state.player = None;
            state.progress = None;
            state.destination = None;
            state.cached_systems = 0;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
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
        controls.observe_transition(7);
        let state = controls.snapshot();
        assert_eq!(state.generation, 2);
        assert!(controls.submit(1, Action::Cancel {}).is_err());
        controls.observe_transition(7);
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

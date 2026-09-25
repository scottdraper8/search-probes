use search_probes_model::{
    protocol::{Action, Command, MAX_COMMAND_BYTES, MAX_PENDING_COMMANDS, MAX_STATE_BYTES, State},
    storage,
};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Child,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F7},
    WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

pub struct Bridge {
    pub state: Mutex<State>,
    pub cancel: AtomicBool,
    commands: Mutex<VecDeque<Command>>,
    changed: AtomicBool,
    stop: AtomicBool,
    show: AtomicBool,
    transition: Mutex<u64>,
}

impl Bridge {
    pub fn new(state: State) -> Self {
        Self {
            state: Mutex::new(state),
            cancel: AtomicBool::new(false),
            commands: Mutex::new(VecDeque::new()),
            changed: AtomicBool::new(true),
            stop: AtomicBool::new(false),
            show: AtomicBool::new(false),
            transition: Mutex::new(crate::lifecycle_hook::epoch()),
        }
    }
    pub fn update(&self, operation: impl FnOnce(&mut State)) {
        operation(&mut self.state.lock().unwrap_or_else(|error| error.into_inner()));
        self.changed.store(true, Ordering::Release);
    }
    pub fn pop(&self) -> Option<Command> {
        self.commands
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
    }
    pub fn observe_transition(&self, current: u64) {
        let mut previous = self
            .transition
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *previous == current {
            return;
        }
        *previous = current;
        self.cancel.store(true, Ordering::Release);
        self.update(|state| {
            if matches!(
                state.availability,
                search_probes_model::protocol::Availability::Failed
                    | search_probes_model::protocol::Availability::Unsupported
                    | search_probes_model::protocol::Availability::Stopped
            ) {
                return;
            }
            state.generation = state.generation.wrapping_add(1);
            state.availability = search_probes_model::protocol::Availability::MainMenu;
            state.player = None;
            state.progress = None;
            state.destination = None;
            state.cached_systems = 0;
        });
    }
    pub fn request_window(&self) {
        self.show.store(true, Ordering::Release);
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.cancel.store(true, Ordering::Release);
    }
    pub fn start(self: Arc<Self>, directory: PathBuf, ui: PathBuf) -> Result<(), String> {
        thread::Builder::new()
            .name("Search Probes controls".into())
            .spawn(move || {
                if let Err(error) = self.serve(&directory, &ui) {
                    self.cancel.store(true, Ordering::Release);
                    self.update(|state| {
                        state.availability = search_probes_model::protocol::Availability::Failed;
                        state.error = Some(format!("Search Probes controls stopped: {error}"));
                    });
                    let state = self
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone();
                    let _ = storage::write_json(
                        &directory.join("current.json"),
                        &state,
                        MAX_STATE_BYTES,
                    );
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    fn serve(&self, directory: &Path, ui: &Path) -> Result<(), String> {
        let initial = self
            .state
            .lock()
            .map_err(|_| "runtime state poisoned")?
            .clone();
        let commands = directory
            .join("sessions")
            .join(&initial.session)
            .join("commands");
        fs::create_dir_all(&commands)
            .map_err(|error| format!("creating command directory: {error}"))?;
        storage::write_json(&directory.join("current.json"), &initial, MAX_STATE_BYTES)
            .map_err(|error| format!("publishing initial status: {error}"))?;
        let mut accepted = initial.last_command;
        let mut child: Option<Child> = None;
        let mut down = false;
        let mut published = Instant::now();
        while !self.stop.load(Ordering::Acquire) {
            self.observe_transition(crate::lifecycle_hook::epoch());
            if let Some(error) = crate::gameplay_hook::failure() {
                self.cancel.store(true, Ordering::Release);
                self.update(|state| {
                    state.availability = search_probes_model::protocol::Availability::Failed;
                    state.error = Some(error);
                });
            }
            if let Some(process) = child.as_mut() {
                match process.try_wait() {
                    Ok(Some(_)) => child = None,
                    Ok(None) => {}
                    Err(error) => {
                        child = None;
                        self.update(|state|state.error=Some(format!("The UI process could not be inspected. Press F7 to reopen it: {error}")));
                    }
                }
            }
            let pressed = unsafe { GetAsyncKeyState(i32::from(VK_F7)) } as u16 & 0x8000 != 0;
            let mut foreground = 0;
            unsafe {
                GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground);
            }
            let ours = foreground == initial.pid
                || child
                    .as_ref()
                    .is_some_and(|process| process.id() == foreground);
            let requested = self.show.swap(false, Ordering::AcqRel);
            if (pressed && !down && ours) || requested {
                self.update(|state| {
                    if requested {
                        state.show = state.show.wrapping_add(1);
                    } else {
                        state.toggle = state.toggle.wrapping_add(1);
                    }
                });
                if child.is_none() {
                    let state = self
                        .state
                        .lock()
                        .map_err(|_| "runtime state poisoned")?
                        .clone();
                    storage::write_json(&directory.join("current.json"), &state, MAX_STATE_BYTES)
                        .map_err(|error| format!("publishing status before opening the UI: {error}"))?;
                    match std::process::Command::new(ui)
                        .arg("--directory")
                        .arg(directory)
                        .spawn()
                    {
                        Ok(process) => child = Some(process),
                        Err(error) => self.update(|state| {
                            state.error =
                                Some(format!("Cannot open the Search Probes window: {error}"))
                        }),
                    }
                }
            }
            down = pressed;
            let mut paths: Vec<_> = fs::read_dir(&commands)
                .map_err(|error| format!("reading command directory: {error}"))?
                .take(MAX_PENDING_COMMANDS * 2)
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .collect();
            paths.sort();
            for path in paths {
                if self
                    .commands
                    .lock()
                    .map_err(|_| "command queue poisoned")?
                    .len()
                    >= MAX_PENDING_COMMANDS
                {
                    break;
                }
                let result = storage::read_json::<Command>(&path, MAX_COMMAND_BYTES)
                    .map_err(|error| error.to_string())
                    .and_then(|command| {
                        let generation = self
                            .state
                            .lock()
                            .map_err(|_| "runtime state poisoned")?
                            .generation;
                        command
                            .validate(&initial.session, generation)
                            .map_err(str::to_owned)?;
                        if path
                            .file_name()
                            .is_none_or(|name| name != format!("{:020}.json", command.id).as_str())
                        {
                            return Err("command filename does not match its identity".into());
                        }
                        Ok(command)
                    });
                match result {
                    Ok(command) if command.id > accepted => {
                        accepted = command.id;
                        if matches!(command.action, Action::Cancel {}) {
                            self.cancel.store(true, Ordering::Release);
                        }
                        let persisted = match &command.action {
                            Action::Start { settings, .. } | Action::Configure { settings } => {
                                storage::save_settings(directory, settings)
                            }
                            _ => Ok(()),
                        };
                        match persisted {
                            Ok(()) => self
                                .commands
                                .lock()
                                .map_err(|_| "command queue poisoned")?
                                .push_back(command),
                            Err(error) => self.update(|state| {
                                state.last_command = command.id;
                                state.error = Some(format!("Settings were not saved: {error}"));
                            }),
                        }
                    }
                    Ok(_) => {}
                    Err(error) => self.update(|state| {
                        state.error = Some(format!(
                            "Rejected command: {}",
                            error.chars().take(512).collect::<String>()
                        ))
                    }),
                }
                fs::remove_file(path)
                    .map_err(|error| format!("removing processed command: {error}"))?;
            }
            if published.elapsed() >= Duration::from_millis(250)
                && self.changed.swap(false, Ordering::AcqRel)
            {
                let state = self
                    .state
                    .lock()
                    .map_err(|_| "runtime state poisoned")?
                    .clone();
                storage::write_json(&directory.join("current.json"), &state, MAX_STATE_BYTES)
                    .map_err(|error| format!("publishing runtime status: {error}"))?;
                published = Instant::now();
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.update(|state| {
            state.availability = search_probes_model::protocol::Availability::Stopped
        });
        let state = self
            .state
            .lock()
            .map_err(|_| "runtime state poisoned")?
            .clone();
        storage::write_json(&directory.join("current.json"), &state, MAX_STATE_BYTES)
            .map_err(|error| format!("publishing stopped status: {error}"))?;
        Ok(())
    }
}

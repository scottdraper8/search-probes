use search_probes_model::{
    protocol::{
        Action, Command, MAX_COMMAND_BYTES, MAX_PENDING_COMMANDS, MAX_STATE_BYTES, PROTOCOL, State,
    },
    storage,
};
use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct Control {
    pub state: Option<State>,
    pub error: Option<String>,
    directory: PathBuf,
    next_id: u64,
    polled: Option<Instant>,
}

impl Control {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            state: None,
            error: None,
            directory,
            next_id: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128 - 1) as u64,
            polled: None,
        }
    }
    pub fn poll(&mut self) {
        if self
            .polled
            .is_some_and(|last| last.elapsed() < Duration::from_millis(200))
        {
            return;
        }
        self.polled = Some(Instant::now());
        match storage::read_json::<State>(&self.directory.join("current.json"), MAX_STATE_BYTES) {
            Ok(state) if state.protocol == PROTOCOL && valid_session(&state.session) => {
                self.next_id = self.next_id.max(state.last_command.saturating_add(1));
                self.state = Some(state);
                self.error = None;
            }
            Ok(_) => self.error = Some("The game and UI use different runtime protocols.".into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.error = Some(
                "Waiting for the Search Probes runtime. Start No Man's Sky with the mod installed."
                    .into(),
            ),
            Err(error) => self.error = Some(format!("Cannot read game status: {error}")),
        }
        if self.error.is_some() {
            self.state = None;
        }
    }
    pub fn send(&mut self, action: Action) -> Result<u64, String> {
        let state = self
            .state
            .as_ref()
            .ok_or("The game runtime is unavailable.")?;
        if !valid_session(&state.session) {
            return Err("Invalid runtime session.".into());
        }
        let directory = self
            .directory
            .join("sessions")
            .join(&state.session)
            .join("commands");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let count = fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .take(MAX_PENDING_COMMANDS)
            .count();
        if count >= MAX_PENDING_COMMANDS {
            return Err("The game is not accepting commands. Wait for it to resume.".into());
        }
        let id = self
            .next_id
            .checked_add(1)
            .ok_or("Command identity exhausted.")?;
        self.next_id = id;
        let command = Command {
            protocol: PROTOCOL,
            session: state.session.clone(),
            generation: state.generation,
            id,
            action,
        };
        command
            .validate(&state.session, state.generation)
            .map_err(str::to_owned)?;
        storage::write_json(
            &directory.join(format!("{id:020}.json")),
            &command,
            MAX_COMMAND_BYTES,
        )
        .map_err(|error| error.to_string())?;
        Ok(id)
    }
}

fn valid_session(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_probes_model::settings::Settings;

    #[test]
    fn commands_are_bounded_atomic_and_scoped_to_current_session() {
        let directory = std::env::temp_dir().join(format!(
            "search-probes-ui-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut control = Control::new(directory.clone());
        assert!(control.send(Action::Cancel {}).is_err());
        let state = State::new("test-session".into(), 0, Settings::default());
        storage::write_json(&directory.join("current.json"), &state, MAX_STATE_BYTES).unwrap();
        control.poll();
        for _ in 0..MAX_PENDING_COMMANDS {
            control.send(Action::Cancel {}).unwrap();
        }
        assert!(control.send(Action::Cancel {}).is_err());
        let commands = directory.join("sessions/test-session/commands");
        for path in fs::read_dir(&commands).unwrap() {
            let command: Command =
                storage::read_json(&path.unwrap().path(), MAX_COMMAND_BYTES).unwrap();
            command.validate("test-session", 1).unwrap();
        }
        fs::remove_file(directory.join("current.json")).unwrap();
        control.polled = None;
        control.poll();
        assert!(control.state.is_none());
        assert!(control.send(Action::Cancel {}).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn session_names_cannot_escape_the_command_directory() {
        assert!(valid_session("session-123-abc"));
        for value in ["", "../escape", "a/b", "a\\b", ".", "é"] {
            assert!(!valid_session(value));
        }
    }
}

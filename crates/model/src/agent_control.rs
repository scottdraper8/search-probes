//! Local, session-bound requests to the same command queue used by the UI.
use crate::{
    controls::Controls,
    runtime::{Action, Availability},
    session::Phase,
    storage,
    ui_control::{UI_ACTIONS, UiAction},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub const ENDPOINT: &str = "agent-control.json";
pub const PROTOCOL_ID: &str = "search-probes.agent-control";
pub const CAPABILITIES: &[&str] = &["search.control", "nms.current-world", "ui.control"];
const MAX_BYTES: usize = 1_100_000;
const MAX_LIFETIME_MS: u64 = 120_000;
const MAX_RECEIPTS: usize = 256;
const MAX_REQUEST_FILES: usize = 256;

fn now_ms() -> io::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .map_err(io::Error::other)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit())
}

fn atomic_write_candidate(name: &str) -> bool {
    name.strip_prefix(".search-probes-")
        .and_then(|name| name.strip_suffix(".tmp"))
        .is_some_and(|parts| {
            let parts: Vec<_> = parts.split('-').collect();
            parts.len() == 2
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub protocol: String,
    pub product: String,
    pub capabilities: Vec<String>,
    pub ui_actions: Vec<UiActionDescriptor>,
    pub instance: String,
    pub process_id: u32,
    pub executable_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionDescriptor {
    #[serde(rename = "type")]
    pub kind: String,
    pub fields: Vec<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub instance: String,
    pub request_id: String,
    pub generation: u64,
    pub expires_unix_ms: u64,
    pub command: AgentCommand,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandChannel {
    Runtime,
    Ui,
}
#[derive(Deserialize, Serialize)]
#[serde(
    tag = "channel",
    content = "action",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AgentCommand {
    Runtime(Action),
    Ui(UiAction),
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub instance: String,
    pub request_id: String,
    pub channel: Option<CommandChannel>,
    pub command_id: Option<u64>,
    pub error: Option<String>,
}

pub struct AgentControl {
    directory: PathBuf,
    endpoint: Endpoint,
    requests: PathBuf,
    responses: PathBuf,
    seen: Mutex<BTreeMap<String, (u64, Response)>>,
    _owner: File,
}
impl AgentControl {
    pub fn open(directory: &Path, capabilities: &[&str]) -> io::Result<Self> {
        if capabilities.is_empty()
            || capabilities
                .iter()
                .any(|capability| capability.is_empty() || !CAPABILITIES.contains(capability))
        {
            return Err(invalid("Runtime capabilities are invalid"));
        }
        fs::create_dir_all(directory)?;
        if unsafe_link(&fs::symlink_metadata(directory)?) {
            return Err(invalid("Runtime directory is a link or reparse point"));
        }
        if let Ok(metadata) = fs::symlink_metadata(directory.join("runtime.lock"))
            && (unsafe_link(&metadata) || !metadata.is_file())
        {
            return Err(invalid("Runtime ownership file is not a real file"));
        }
        let owner = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("runtime.lock"))?;
        owner.try_lock().map_err(io::Error::other)?;
        // Holding the exclusive process lock proves that every previous instance is stale.
        let instances = directory.join("agent-control");
        if instances.exists() {
            let metadata = fs::symlink_metadata(&instances)?;
            if unsafe_link(&metadata) || !metadata.is_dir() {
                return Err(invalid("Runtime workspace is not a real directory"));
            }
            for entry in fs::read_dir(&instances)? {
                let entry = entry?;
                let name = entry.file_name();
                if recognized_instance(&name.to_string_lossy()) {
                    remove_owned_tree(&entry.path())?;
                }
            }
        }
        for name in ["current.json", "overlay.json", ENDPOINT] {
            remove_owned_file(&directory.join(name))?;
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if atomic_write_candidate(&name) {
                remove_owned_file(&entry.path())?;
            }
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let endpoint = Endpoint {
            protocol: PROTOCOL_ID.into(),
            product: "search-probes".into(),
            capabilities: capabilities.iter().map(|value| (*value).into()).collect(),
            ui_actions: UI_ACTIONS
                .iter()
                .map(|(kind, fields)| UiActionDescriptor {
                    kind: (*kind).into(),
                    fields: fields.iter().map(|field| (*field).into()).collect(),
                })
                .collect(),
            instance: format!("{:08x}-{stamp:032x}", std::process::id()),
            process_id: std::process::id(),
            executable_sha256: crate::supported::EXECUTABLE_SHA256.into(),
        };
        let root = directory.join("agent-control").join(&endpoint.instance);
        let requests = root.join("requests");
        let responses = root.join("responses");
        fs::create_dir_all(&requests)?;
        fs::create_dir_all(&responses)?;
        let control = Self {
            directory: directory.into(),
            endpoint,
            requests,
            responses,
            seen: Mutex::new(BTreeMap::new()),
            _owner: owner,
        };
        control.publish()?;
        Ok(control)
    }
    /// Refresh alongside current.json; clients must check both files before sending a request.
    pub fn publish(&self) -> io::Result<()> {
        storage::write_json(&self.directory.join(ENDPOINT), &self.endpoint, 4096)
    }
    /// Only the exclusive artifact owner calls this, after publishing and polling have stopped.
    pub fn cleanup(&self) -> io::Result<()> {
        remove_owned_tree(
            self.requests
                .parent()
                .ok_or_else(|| invalid("Missing runtime workspace"))?,
        )?;
        for name in ["current.json", "overlay.json", ENDPOINT] {
            remove_owned_file(&self.directory.join(name))?;
        }
        Ok(())
    }
    fn enqueue(
        &self,
        id: &str,
        path: &Path,
        controls: &Controls,
    ) -> Result<(u64, CommandChannel), (Option<CommandChannel>, String)> {
        let request: Request =
            storage::read_json(path, MAX_BYTES).map_err(|error| (None, error.to_string()))?;
        let channel = match &request.command {
            AgentCommand::Runtime(_) => CommandChannel::Runtime,
            AgentCommand::Ui(_) => CommandChannel::Ui,
        };
        let fail = |message: &str| (Some(channel), message.to_owned());
        let now = now_ms().map_err(|error| (Some(channel), error.to_string()))?;
        if request.request_id != id || request.instance != self.endpoint.instance {
            return Err(fail("Request does not belong to this runtime instance"));
        }
        if request.expires_unix_ms < now
            || request.expires_unix_ms > now.saturating_add(MAX_LIFETIME_MS)
        {
            return Err(fail("Request expired or exceeds the maximum lifetime"));
        }
        match request.command {
            AgentCommand::Runtime(action) => {
                if matches!(action, Action::Navigate { .. }) {
                    return Err(fail(
                        "Direct navigation is unavailable; select a published UI result",
                    ));
                }
                if matches!(
                    action,
                    Action::Start { .. }
                        | Action::SetSearchPaused { .. }
                        | Action::InspectCurrent {}
                ) && controls.availability() != Availability::Ready
                {
                    return Err(fail("The loaded game is not ready for this request"));
                }
                if matches!(action, Action::Start { .. })
                    && controls
                        .snapshot()
                        .progress
                        .as_ref()
                        .is_some_and(|p| p.phase == Phase::Running)
                {
                    return Err(fail("A search is already active; cancel it first"));
                }
                controls
                    .submit(request.generation, action)
                    .map(|id| (id, CommandChannel::Runtime))
                    .map_err(&fail)
            }
            AgentCommand::Ui(action) => controls
                .submit_ui(request.generation, action)
                .map(|id| (id, CommandChannel::Ui))
                .map_err(fail),
        }
    }
    /// Claim complete request files before enqueueing. A duplicate ID cannot enqueue twice.
    /// Only the persistence worker calls this; no native work or disk IO runs on the game thread.
    pub fn poll(&self, controls: &Controls) -> io::Result<()> {
        self.prune_receipts()?;
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.requests)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if file_type.is_file()
                && path.extension().is_some_and(|s| s == "json")
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(valid_id)
            {
                files.push(path);
            } else if file_type.is_file() && atomic_write_candidate(&name) {
                let age = SystemTime::now()
                    .duration_since(entry.metadata()?.modified()?)
                    .unwrap_or_default();
                if age > std::time::Duration::from_millis(MAX_LIFETIME_MS) {
                    fs::remove_file(path)?;
                }
            } else if file_type.is_dir() {
                remove_owned_tree(&path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
        files.sort();
        for path in files.drain(MAX_REQUEST_FILES.min(files.len())..) {
            fs::remove_file(path)?;
        }
        for path in files.into_iter().take(8) {
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| invalid("Invalid request ID"))?;
            let response_path = self.responses.join(format!("{id}.json"));
            if response_path.exists() {
                fs::remove_file(&path)?;
                continue;
            }
            let claimed = path.with_extension("processing");
            if claimed.exists() {
                continue;
            }
            match fs::rename(&path, &claimed) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            }
            let now = now_ms()?;
            let mut seen = self
                .seen
                .lock()
                .map_err(|_| invalid("Agent receipts poisoned"))?;
            seen.retain(|_, (until, _)| *until >= now);
            let response = if let Some((_, prior)) = seen.get(id) {
                prior.clone()
            } else {
                let result = if seen.len() >= MAX_RECEIPTS {
                    Err((None, "Agent receipt capacity reached; retry later".into()))
                } else {
                    self.enqueue(id, &claimed, controls)
                };
                let (command_id, channel, error) = match result {
                    Ok((id, channel)) => (Some(id), Some(channel), None),
                    Err((channel, error)) => (None, channel, Some(error)),
                };
                let response = Response {
                    instance: self.endpoint.instance.clone(),
                    request_id: id.into(),
                    channel,
                    command_id,
                    error,
                };
                if command_id.is_some() {
                    seen.insert(
                        id.into(),
                        (now.saturating_add(MAX_LIFETIME_MS), response.clone()),
                    );
                }
                response
            };
            drop(seen);
            storage::write_json(&response_path, &response, 16384)?;
            fs::remove_file(&claimed)?;
        }
        Ok(())
    }
    fn prune_receipts(&self) -> io::Result<()> {
        let now = SystemTime::now();
        let lifetime = std::time::Duration::from_millis(MAX_LIFETIME_MS);
        let mut receipts = Vec::new();
        for entry in fs::read_dir(&self.responses)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json")
                || !path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(valid_id)
            {
                continue;
            }
            let modified = entry.metadata()?.modified()?;
            if now.duration_since(modified).unwrap_or_default() > lifetime {
                fs::remove_file(path)?;
            } else {
                receipts.push((modified, path));
            }
        }
        receipts.sort();
        let excess = receipts
            .len()
            .saturating_sub(MAX_RECEIPTS.saturating_sub(8));
        for (_, path) in receipts.into_iter().take(excess) {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn recognized_instance(name: &str) -> bool {
    name.len() == 41
        && name.as_bytes()[8] == b'-'
        && name
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 8 || byte.is_ascii_hexdigit())
}

fn unsafe_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn remove_owned_file(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if unsafe_link(&metadata) || !metadata.is_file() => {
            Err(invalid("Runtime artifact is not a real file"))
        }
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_owned_tree(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if unsafe_link(&metadata) || !metadata.is_dir() {
        return Err(invalid("Runtime workspace is not a real directory"));
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_owned_tree(&entry.path())?;
        } else {
            remove_owned_file(&entry.path())?;
        }
    }
    fs::remove_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "search-probes-artifacts-{}-{}",
            std::process::id(),
            DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn only_the_exact_runtime_instance_directory_format_is_owned() {
        assert!(recognized_instance(
            "00000001-00000000000000000000000000000001"
        ));
        for invalid in [
            "1-00000000000000000000000000000001",
            "00000001-0000000000000000000000000000000",
            "00000001-0000000000000000000000000000000g",
            "00000001_00000000000000000000000000000001",
        ] {
            assert!(!recognized_instance(invalid));
        }
    }

    #[test]
    fn polling_preserves_an_atomic_request_write_until_its_final_rename() {
        let directory = directory();
        let agent = AgentControl::open(&directory, CAPABILITIES).unwrap();
        let controls = Controls::new(crate::runtime::State::new(
            crate::settings::Settings::default(),
        ));
        let candidate = agent
            .requests
            .join(format!(".search-probes-{}-1.tmp", std::process::id()));
        fs::write(&candidate, b"partial").unwrap();

        agent.poll(&controls).unwrap();

        assert!(candidate.is_file());
        fs::remove_file(candidate).unwrap();
        agent.cleanup().unwrap();
        drop(agent);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exclusive_ownership_prevents_deleting_a_live_instances_artifacts() {
        let directory = directory();
        let agent = AgentControl::open(&directory, CAPABILITIES).unwrap();
        let endpoint = storage::read_bytes(&directory.join(ENDPOINT), 4096).unwrap();
        assert!(AgentControl::open(&directory, CAPABILITIES).is_err());
        assert_eq!(
            storage::read_bytes(&directory.join(ENDPOINT), 4096).unwrap(),
            endpoint
        );
        assert!(agent.requests.is_dir());
        agent.cleanup().unwrap();
        drop(agent);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn normal_and_stale_cleanup_remove_only_owned_artifacts_and_preserve_settings() {
        let directory = directory();
        let agent = AgentControl::open(&directory, CAPABILITIES).unwrap();
        let previous = agent.requests.parent().unwrap().to_owned();
        storage::write_bytes(&directory.join("settings.json"), b"keep settings", 100).unwrap();
        storage::write_bytes(&directory.join("settings.backup.json"), b"keep backup", 100).unwrap();
        storage::write_bytes(&directory.join("current.json"), b"stale", 100).unwrap();
        storage::write_bytes(
            &directory.join(".search-probes-123-456.tmp"),
            b"partial",
            100,
        )
        .unwrap();
        storage::write_bytes(
            &directory.join("agent-control/notes/keep.txt"),
            b"unrelated",
            100,
        )
        .unwrap();
        storage::write_bytes(
            &agent.requests.join("unfinished.processing"),
            b"partial",
            100,
        )
        .unwrap();
        drop(agent); // Simulates loss of process ownership without orderly cleanup.
        let replacement = AgentControl::open(&directory, CAPABILITIES).unwrap();
        assert!(!previous.exists());
        assert!(!directory.join("current.json").exists());
        assert!(!directory.join(".search-probes-123-456.tmp").exists());
        assert!(directory.join("settings.json").exists());
        assert!(directory.join("settings.backup.json").exists());
        assert!(directory.join("agent-control/notes/keep.txt").exists());
        storage::write_bytes(&directory.join("current.json"), b"current", 100).unwrap();
        storage::write_bytes(&directory.join("overlay.json"), b"overlay", 100).unwrap();
        replacement.cleanup().unwrap();
        replacement.cleanup().unwrap();
        assert!(!directory.join(ENDPOINT).exists());
        assert!(!directory.join("current.json").exists());
        assert!(!directory.join("overlay.json").exists());
        drop(replacement);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn receipt_files_have_a_count_bound() {
        let directory = directory();
        let agent = AgentControl::open(&directory, CAPABILITIES).unwrap();
        for index in 0..MAX_RECEIPTS + 30 {
            fs::write(agent.responses.join(format!("{index:032x}.json")), b"{}").unwrap();
        }
        agent.prune_receipts().unwrap();
        assert_eq!(
            fs::read_dir(&agent.responses).unwrap().count(),
            MAX_RECEIPTS - 8
        );
        agent.cleanup().unwrap();
        drop(agent);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_request_artifacts_cannot_hide_a_valid_request() {
        let directory = directory();
        let agent = AgentControl::open(&directory, CAPABILITIES).unwrap();
        for index in 0..MAX_REQUEST_FILES + 30 {
            fs::write(agent.requests.join(format!("junk-{index}")), b"junk").unwrap();
        }
        fs::create_dir(agent.requests.join("abandoned-directory")).unwrap();
        fs::write(agent.requests.join("abandoned.processing"), b"partial").unwrap();
        let id = "f".repeat(32);
        let controls = Controls::new(crate::runtime::State::new(
            crate::settings::Settings::default(),
        ));
        let request = Request {
            instance: agent.endpoint.instance.clone(),
            request_id: id.clone(),
            generation: controls.snapshot().generation,
            expires_unix_ms: now_ms().unwrap() + 1_000,
            command: AgentCommand::Runtime(Action::Cancel {}),
        };
        storage::write_json(
            &agent.requests.join(format!("{id}.json")),
            &request,
            MAX_BYTES,
        )
        .unwrap();

        agent.poll(&controls).unwrap();

        assert!(agent.responses.join(format!("{id}.json")).is_file());
        assert_eq!(fs::read_dir(&agent.requests).unwrap().count(), 0);
        agent.cleanup().unwrap();
        drop(agent);
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stale_cleanup_refuses_to_follow_links_outside_the_instance() {
        let directory = directory();
        let outside = directory.with_extension("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"keep").unwrap();
        let stale = directory.join("agent-control/00000001-00000000000000000000000000000001");
        fs::create_dir_all(&stale).unwrap();
        std::os::unix::fs::symlink(&outside, stale.join("escape")).unwrap();
        assert!(AgentControl::open(&directory, CAPABILITIES).is_err());
        assert!(outside.join("keep.txt").exists());
        fs::remove_file(stale.join("escape")).unwrap();
        fs::remove_dir_all(directory).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn root_directory_and_lock_links_are_refused_before_cleanup() {
        let directory = directory();
        let outside = directory.with_extension("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("current.json"), b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, &directory).unwrap();
        assert!(AgentControl::open(&directory, CAPABILITIES).is_err());
        assert!(outside.join("current.json").exists());
        fs::remove_file(&directory).unwrap();
        fs::create_dir_all(&directory).unwrap();
        std::os::unix::fs::symlink(outside.join("current.json"), directory.join("runtime.lock"))
            .unwrap();
        assert!(AgentControl::open(&directory, CAPABILITIES).is_err());
        assert_eq!(fs::read(outside.join("current.json")).unwrap(), b"keep");
        fs::remove_file(directory.join("runtime.lock")).unwrap();
        fs::remove_dir_all(directory).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}

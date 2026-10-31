//! Local, session-bound requests to the same command queue used by the UI.
use crate::{
    controls::Controls,
    runtime::{Action, Availability},
    session::Phase,
    storage,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub const ENDPOINT: &str = "agent-control.json";
const MAX_BYTES: usize = 1_100_000;
const MAX_LIFETIME_MS: u64 = 120_000;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub schema: u32,
    pub instance: String,
    pub process_id: u32,
    pub executable_sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: u32,
    pub instance: String,
    pub request_id: String,
    pub generation: u64,
    pub expires_unix_ms: u64,
    pub action: Action,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub schema: u32,
    pub instance: String,
    pub request_id: String,
    pub command_id: Option<u64>,
    pub error: Option<String>,
}

pub struct AgentControl {
    directory: PathBuf,
    endpoint: Endpoint,
    requests: PathBuf,
    responses: PathBuf,
    seen: Mutex<BTreeMap<String, (u64, Response)>>,
}
impl AgentControl {
    pub fn open(directory: &Path) -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let endpoint = Endpoint {
            schema: 1,
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
        };
        control.publish()?;
        Ok(control)
    }
    /// Refresh alongside current.json; clients must check both files before sending a request.
    pub fn publish(&self) -> io::Result<()> {
        storage::write_json(&self.directory.join(ENDPOINT), &self.endpoint, 4096)
    }
    fn enqueue(&self, id: &str, path: &Path, controls: &Controls) -> Result<u64, String> {
        let request: Request = storage::read_json(path, MAX_BYTES).map_err(|e| e.to_string())?;
        let now = now_ms().map_err(|e| e.to_string())?;
        if request.schema != 1
            || request.request_id != id
            || request.instance != self.endpoint.instance
        {
            return Err("Request does not belong to this runtime instance".into());
        }
        if request.expires_unix_ms < now
            || request.expires_unix_ms > now.saturating_add(MAX_LIFETIME_MS)
        {
            return Err("Request expired or exceeds the maximum lifetime".into());
        }
        if matches!(request.action, Action::Navigate { .. }) {
            return Err("Agent control accepts search commands, not navigation".into());
        }
        if matches!(request.action, Action::Start { .. })
            && controls.snapshot().availability != Availability::Ready
        {
            return Err("The loaded game is not ready to search".into());
        }
        if matches!(request.action, Action::Start { .. })
            && controls
                .snapshot()
                .progress
                .as_ref()
                .is_some_and(|p| p.phase == Phase::Running)
        {
            return Err("A search is already active; cancel it first".into());
        }
        controls
            .submit(request.generation, request.action)
            .map_err(str::to_owned)
    }
    /// Claim complete request files before enqueueing. A duplicate ID cannot enqueue twice.
    /// Only the persistence worker calls this; no native work or disk IO runs on the game thread.
    pub fn poll(&self, controls: &Controls) -> io::Result<()> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.requests)?.take(256) {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file()
                && path.extension().is_some_and(|s| s == "json")
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(valid_id)
            {
                files.push(path);
            }
        }
        files.sort();
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
                let result = if seen.len() >= 8192 {
                    Err("Agent receipt capacity reached; retry later".into())
                } else {
                    self.enqueue(id, &claimed, controls)
                };
                let (command_id, error) = match result {
                    Ok(id) => (Some(id), None),
                    Err(e) => (None, Some(e)),
                };
                let response = Response {
                    schema: 1,
                    instance: self.endpoint.instance.clone(),
                    request_id: id.into(),
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
}

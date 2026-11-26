use search_probes_model::{
    controls::Controls,
    runtime::{Availability, MAX_STATE_BYTES},
    storage,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::Ordering},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

static WORKER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

pub fn start(controls: Arc<Controls>, directory: PathBuf) -> Result<(), String> {
    // Establish exclusive artifact ownership synchronously before starting any publisher.
    let agent = search_probes_model::agent_control::AgentControl::open(
        &directory,
        search_probes_model::agent_control::CAPABILITIES,
    )
    .map_err(|error| format!("Cannot own runtime artifacts: {error}"))?;
    let worker = thread::Builder::new()
        .name("Search Probes persistence".into())
        .spawn(move || {
            let mut agent_polled = Instant::now();
            let mut published = Instant::now();
            let mut overlay_published = Instant::now();
            let mut published_revision = 0;
            loop {
                if controls.availability() == Availability::Stopped {
                    break;
                }
                controls.observe_transition(
                    crate::lifecycle_hook::epoch(),
                    crate::lifecycle_hook::session(),
                );
                if let Some(error) = crate::gameplay_hook::failure() {
                    controls.cancel.store(true, Ordering::Release);
                    controls.update(|state| {
                        state.availability = Availability::Failed;
                        state.error = Some(error);
                    });
                }
                if let Some(error) = crate::overlay::take_failure() {
                    crate::runtime::report_startup_failure(&error);
                }
                if agent_polled.elapsed() >= Duration::from_millis(50) {
                    let result = agent.poll(&controls);
                    if let Err(error) = result {
                        controls.update(|state| {
                            state.error = Some(format!("Agent command interface: {error}"))
                        });
                    }
                    agent_polled = Instant::now();
                }
                controls.prepare(&directory);
                if published.elapsed() >= Duration::from_millis(250) {
                    if let Some((revision, snapshot)) =
                        controls.snapshot_if_changed(published_revision)
                    {
                        match storage::write_json(
                            &directory.join("current.json"),
                            &snapshot,
                            MAX_STATE_BYTES,
                        ) {
                            Ok(()) => {
                                published_revision = revision;
                                let _ = agent.publish();
                            }
                            Err(error) => controls.update(|state| {
                                state.error = Some(format!("Cannot publish diagnostics: {error}"))
                            }),
                        }
                    }
                    published = Instant::now();
                }
                if overlay_published.elapsed() >= Duration::from_secs(1) {
                    if let Some(diagnostics) = crate::overlay::diagnostics() {
                        let _ = storage::write_json(
                            &directory.join("overlay.json"),
                            &diagnostics,
                            16384,
                        );
                    }
                    overlay_published = Instant::now();
                }
                if controls.availability() == Availability::Stopped {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            if let Err(error) = agent.cleanup() {
                controls.update(|state| {
                    state.error = Some(format!("Cannot clean runtime artifacts: {error}"))
                });
            }
        })
        .map_err(|e| e.to_string())?;
    *WORKER.lock().map_err(|_| "persistence worker poisoned")? = Some(worker);
    Ok(())
}

pub fn finish() {
    if let Some(worker) = WORKER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = worker.join();
    }
}

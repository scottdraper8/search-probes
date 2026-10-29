use search_probes_model::{
    controls::Controls,
    runtime::{Availability, MAX_STATE_BYTES},
    storage,
};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

pub fn start(controls: Arc<Controls>, directory: PathBuf) -> Result<(), String> {
    thread::Builder::new()
        .name("Search Probes persistence".into())
        .spawn(move || {
            let agent = search_probes_model::agent_control::AgentControl::open(&directory);
            let mut agent_polled = Instant::now();
            let mut published = Instant::now();
            loop {
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
                    let result = match &agent {
                        Ok(agent) => agent.poll(&controls),
                        Err(error) => Err(std::io::Error::other(error.to_string())),
                    };
                    if let Err(error) = result {
                        controls.update(|state| {
                            state.error = Some(format!("Agent command interface: {error}"))
                        });
                    }
                    agent_polled = Instant::now();
                }
                controls.prepare(&directory);
                if published.elapsed() >= Duration::from_millis(250) {
                    if let Err(error) = storage::write_json(
                        &directory.join("current.json"),
                        &controls.snapshot(),
                        MAX_STATE_BYTES,
                    ) {
                        controls.update(|state| {
                            state.error = Some(format!("Cannot publish diagnostics: {error}"))
                        });
                    }
                    if let Some(diagnostics) = crate::overlay::diagnostics() {
                        let _ = storage::write_json(
                            &directory.join("overlay.json"),
                            &diagnostics,
                            16384,
                        );
                    }
                    if let Ok(agent) = &agent {
                        let _ = agent.publish();
                    }
                    published = Instant::now();
                }
                if controls
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .availability
                    == Availability::Stopped
                {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

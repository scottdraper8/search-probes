use crate::{scheduler::Scheduler, workers::Workers};
use search_probes_model::address::Address;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct State {
    scheduler: Scheduler,
    pool: Workers,
    workers: usize,
}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
pub extern "system" fn sqn_session_cancel() {
    CANCEL.store(true, Ordering::Release);
}

fn run(
    base: usize,
    live: *const u8,
    command: serde_json::Value,
) -> Result<serde_json::Value, &'static str> {
    let mut state = STATE.lock().map_err(|_| "session state poisoned")?;
    if state.is_none() {
        *state = Some(State {
            scheduler: Scheduler::default(),
            pool: Workers::new(16).map_err(|_| "worker startup failed")?,
            workers: 2,
        });
    }
    let state = state.as_mut().ok_or("missing session")?;
    match command["action"].as_str().ok_or("missing action")? {
        "start" => {
            let options = serde_json::from_value(command["options"].clone())
                .map_err(|_| "invalid search options")?;
            let origin = Address::from_raw(command["origin"].as_u64().ok_or("missing origin")?)?;
            state.workers = command["workers"].as_u64().unwrap_or(2) as usize;
            if !(1..=16).contains(&state.workers) {
                return Err("invalid worker count");
            }
            state.scheduler.session.start(
                options,
                origin,
                command["restart"].as_bool().unwrap_or(false),
                command["use_cache"].as_bool().unwrap_or(true),
            )?;
            state.scheduler.reset_statistics();
            CANCEL.store(false, Ordering::Release);
        }
        "tick" => unsafe {
            state.scheduler.tick(
                &state.pool,
                base,
                live,
                command["difficulty"].as_u64().unwrap_or(0) as usize,
                state.workers,
                Duration::from_millis(4),
                &CANCEL,
            )
        }?,
        "cancel" => {
            CANCEL.store(true, Ordering::Release);
            state.scheduler.session.cancel();
        }
        "continue" => state
            .scheduler
            .session
            .continue_live(command["results"].as_u64().ok_or("missing result limit")? as usize)?,
        "reset" => {
            state.scheduler.reset();
            CANCEL.store(false, Ordering::Release);
        }
        "status" | "dump" => {}
        _ => return Err("unknown action"),
    }
    if command["action"].as_str() == Some("dump") {
        let offset = command["offset"].as_u64().unwrap_or(0) as usize;
        let rows: Vec<_> = state
            .scheduler
            .session
            .cached_records()
            .skip(offset)
            .take(64)
            .collect();
        return Ok(serde_json::json!({"records":rows}));
    }
    let statistics = state.scheduler.statistics();
    let work = statistics.work;
    Ok(
        serde_json::json!({"progress":state.scheduler.session.progress(),"cache_size":state.scheduler.session.cache_len(),"statistics":{"work":[work.systems,work.planets,work.metadata_captures,work.colour_captures,work.object_scans,work.names],"batches":statistics.batches,"active_ns":statistics.active_ns,"longest_batch_ns":statistics.longest_batch_ns,"regions":statistics.regions}}),
    )
}

/// # Safety
/// Commands run on the loaded gameplay callback with query hooks enabled. Buffers must be disjoint,
/// valid for their extents, and remain alive until return. Native state obeys Scheduler::tick.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_session(
    base: usize,
    live: *const u8,
    input: *const u8,
    length: usize,
    output: *mut u8,
    capacity: usize,
) -> i32 {
    if input.is_null() || output.is_null() || length > 65536 || capacity < 65536 {
        return -1;
    }
    let result = std::panic::catch_unwind(|| {
        let command = serde_json::from_slice(unsafe { std::slice::from_raw_parts(input, length) })
            .map_err(|_| "invalid command JSON")?;
        run(base, live, command)
    })
    .unwrap_or(Err("session callback panicked"));
    let value = match result {
        Ok(value) => value,
        Err(error) => serde_json::json!({"error":error}),
    };
    let Ok(serialized) = serde_json::to_vec(&value) else {
        return -2;
    };
    if serialized.len() >= capacity {
        return -3;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(serialized.as_ptr(), output, serialized.len());
        output.add(serialized.len()).write(0);
    }
    serialized.len() as i32
}

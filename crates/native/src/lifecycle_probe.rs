use crate::{
    game_state::{Epoch, Snapshot},
    identity::Image,
    lifecycle_hook,
};
use std::sync::{Mutex, OnceLock};

static IMAGE: OnceLock<Result<Image, String>> = OnceLock::new();
static PREVIOUS: Mutex<Option<Epoch>> = Mutex::new(None);

fn run(action: u32) -> Result<serde_json::Value, String> {
    let image = IMAGE
        .get_or_init(Image::verify)
        .as_ref()
        .map_err(Clone::clone)?;
    match action {
        1 => {
            unsafe { lifecycle_hook::enable(image.base()) }.map_err(|error| format!("{error:?}"))?
        }
        2 => unsafe { lifecycle_hook::disable() }.map_err(|error| format!("{error:?}"))?,
        0 => {}
        _ => return Err("unknown lifecycle probe action".into()),
    }
    let transition = lifecycle_hook::epoch();
    let snapshot = Snapshot::read(image, transition)?;
    let mut previous = PREVIOUS.lock().map_err(|_| "lifecycle probe poisoned")?;
    let changed = previous
        .as_ref()
        .map(|previous| previous.differences(&snapshot.epoch));
    *previous = Some(snapshot.epoch);
    Ok(
        serde_json::json!({"transition":transition,"address":snapshot.address.raw(),"application":snapshot.application,"planet":snapshot.planet,"difficulty":snapshot.difficulty,"changed":changed}),
    )
}

/// # Safety
/// Invoke on the gameplay callback, outside a native query. Output must own capacity writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_lifecycle_probe(
    action: u32,
    output: *mut u8,
    capacity: usize,
) -> i32 {
    if output.is_null() || capacity < 4096 {
        return -1;
    }
    let result = std::panic::catch_unwind(|| run(action))
        .unwrap_or_else(|_| Err("lifecycle probe panicked".into()));
    let value = match result {
        Ok(value) => value,
        Err(error) => serde_json::json!({"error":error}),
    };
    let Ok(bytes) = serde_json::to_vec(&value) else {
        return -2;
    };
    if bytes.len() >= capacity {
        return -3;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
        output.add(bytes.len()).write(0);
    }
    bytes.len() as i32
}

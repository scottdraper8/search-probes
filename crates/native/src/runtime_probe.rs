/// # Safety
/// Buffers are valid and disjoint. Start/stop runs outside native callbacks and all other probe hooks
/// are disabled. Start takes a UTF-8 JSON object containing isolated directory and UI paths.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_runtime_probe(
    action: u32,
    input: *const u8,
    length: usize,
    output: *mut u8,
    capacity: usize,
) -> i32 {
    if output.is_null() || capacity < 4096 || length > 65536 || input.is_null() {
        return -1;
    }
    let result = std::panic::catch_unwind(|| -> Result<serde_json::Value, String> {
        match action {
            0 => {
                let value: serde_json::Value =
                    serde_json::from_slice(unsafe { std::slice::from_raw_parts(input, length) })
                        .map_err(|error| error.to_string())?;
                let directory = value["directory"].as_str().ok_or("missing directory")?;
                let ui = value["ui"].as_str().ok_or("missing UI path")?;
                crate::runtime::initialize_at(directory.into(), ui.into()).map(|_|serde_json::json!({"ok":true}))
            }
            1 => crate::runtime::stop().map(|_|serde_json::json!({"ok":true})),
            2 => Ok(serde_json::json!({"thread":crate::gameplay_hook::thread_id(),"epoch":crate::lifecycle_hook::epoch(),"failure":crate::gameplay_hook::failure()})),
            _ => Err("unknown runtime probe action".into()),
        }
    })
    .unwrap_or_else(|_| Err("runtime probe panicked".into()));
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

#![cfg(windows)]

#[unsafe(no_mangle)]
pub extern "system" fn SearchProbesStart() {
    match std::panic::catch_unwind(search_probes_native::runtime::initialize) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => search_probes_native::runtime::report_startup_failure(&error),
        Err(_) => search_probes_native::runtime::report_startup_failure(
            "Search Probes initialization panicked. Restart the game.",
        ),
    }
}

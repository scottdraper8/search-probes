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

#[unsafe(no_mangle)]
/// Resolves the bootstrap's triggering Vulkan lookup through the installed overlay dispatcher.
///
/// # Safety
///
/// `module` and `name` must satisfy the native `GetProcAddress` contract.
pub unsafe extern "system" fn SearchProbesGetProcAddress(
    module: windows_sys::Win32::Foundation::HMODULE,
    name: windows_sys::core::PCSTR,
) -> windows_sys::Win32::Foundation::FARPROC {
    unsafe { search_probes_native::overlay::resolve_proc_address(module, name) }
}

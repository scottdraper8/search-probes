use crate::{planet_hook, registration_hook, resident_hook::HookError};

/// # Safety
/// Requires an initialized runtime outside DllMain and active queries.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    unsafe { registration_hook::enable(base) }?;
    if let Err(error) = unsafe { planet_hook::enable(base) } {
        unsafe { planet_hook::disable() }?;
        unsafe { registration_hook::disable() }?;
        return Err(error);
    }
    Ok(())
}

/// # Safety
/// Stop dispatching and complete every query before restoring the original entry points.
pub unsafe fn disable() -> Result<(), HookError> {
    let planet = unsafe { planet_hook::disable() };
    let registration = unsafe { registration_hook::disable() };
    planet.and(registration)
}

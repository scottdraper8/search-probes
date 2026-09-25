use crate::resident_hook::{HookError, ResidentHook};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const PREFIX: [u8; 17] = [
    0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x91, 0x20, 0x34, 0, 0,
];
static BASE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);

unsafe extern "system" fn detour(planet: *mut u8) {
    unsafe {
        let _ = crate::capture_session::observe(planet);
        let original = std::mem::transmute::<usize, unsafe extern "system" fn(*mut u8)>(
            ORIGINAL.load(Ordering::Acquire),
        );
        original(planet);
    }
}

/// # Safety
/// Requires the verified 7.04 image and initialized runtime, outside DllMain. Module and trampoline
/// remain resident until process exit so in-flight detours stay valid.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        let hook = unsafe {
            ResidentHook::prepare(base + 0x1cea50, detour as *const () as usize, &PREFIX)
        }?;
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
        BASE.store(base, Ordering::Release);
        *state = Some(hook);
    }
    if BASE.load(Ordering::Acquire) != base {
        return Err(HookError::PrefixChanged);
    }
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(true) }?;
    }
    Ok(())
}

/// # Safety
/// Stop and join query workers first. The module and trampoline remain resident until process exit.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(false) }?;
    }
    Ok(())
}

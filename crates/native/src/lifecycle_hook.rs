use crate::resident_hook::{HookError, ResidentHook};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

type Change = unsafe extern "system" fn(*mut u8, *const u8, *mut u8, bool);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static APPLICATION: AtomicUsize = AtomicUsize::new(0);
static EPOCH: AtomicU64 = AtomicU64::new(1);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
const PREFIX: [u8; 15] = [
    0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec, 0x20,
];

pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

unsafe extern "system" fn detour(this: *mut u8, key: *const u8, data: *mut u8, restart: bool) {
    if this as usize == APPLICATION.load(Ordering::Acquire) {
        let current = crate::platform::pointer(this as usize + 0x10)
            .and_then(|current| crate::platform::read::<16>(current + 8));
        let requested = crate::platform::read::<16>(key as usize);
        let unchanged =
            matches!((current,requested),(Ok(current),Ok(requested)) if current==requested);
        if restart || !unchanged {
            EPOCH.fetch_add(1, Ordering::AcqRel);
        }
    }
    let original =
        unsafe { std::mem::transmute::<usize, Change>(ORIGINAL.load(Ordering::Acquire)) };
    unsafe {
        original(this, key, data, restart);
    }
}

/// # Safety
/// Requires the verified image outside DllMain. The module/trampoline remain pinned until exit.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        let hook = unsafe {
            ResidentHook::prepare(base + 0x2bd9250, detour as *const () as usize, &PREFIX)
        }?;
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
        APPLICATION.store(base + 0x6e7aab0, Ordering::Release);
        *state = Some(hook);
    }
    if APPLICATION.load(Ordering::Acquire) != base + 0x6e7aab0 {
        return Err(HookError::PrefixChanged);
    }
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(true) }?;
    }
    Ok(())
}

/// # Safety
/// Stop all search dispatch before disabling lifecycle observation.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(false) }?;
    }
    Ok(())
}

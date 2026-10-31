use crate::resident_hook::{HookError, ResidentHook};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

static BASE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
const PREFIX: [u8; 18] = [
    0x48, 0x89, 0x5c, 0x24, 0x18, 0x48, 0x89, 0x74, 0x24, 0x20, 0x55, 0x57, 0x41, 0x54, 0x41, 0x56,
    0x41, 0x57,
];

#[unsafe(naked)]
unsafe extern "system" fn entry(_resource: *mut u8) {
    core::arch::naked_asm!("mov rdx, [rsp]", "jmp {}", sym detour);
}

unsafe extern "system" fn detour(resource: *mut u8, return_address: usize) {
    let base = BASE.load(Ordering::Acquire);
    if return_address == base + 0x16ce4a2 && crate::capture_session::is_active() {
        // This call site transfers one acquired table reference solely for global filename registration.
        unsafe {
            let release = std::mem::transmute::<usize, unsafe extern "system" fn(*mut u8, *mut u8)>(
                base + 0x4bb8a0,
            );
            release((base + 0x755f570) as *mut u8, resource);
        }
    } else {
        unsafe {
            let original = std::mem::transmute::<usize, unsafe extern "system" fn(*mut u8)>(
                ORIGINAL.load(Ordering::Acquire),
            );
            original(resource);
        }
    }
}

/// # Safety
/// Requires an initialized runtime outside DllMain.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        let hook = unsafe {
            ResidentHook::prepare(base + 0x16d6960, entry as *const () as usize, &PREFIX)
        }?;
        BASE.store(base, Ordering::Release);
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
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
/// Every native query must have completed before disabling; resident code remains valid until exit.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(false) }?;
    }
    Ok(())
}

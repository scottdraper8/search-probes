use crate::resident_hook::{HookError, ResidentHook};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

type Shutdown = unsafe extern "system" fn();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static BASE: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
const PREFIX: [u8; 15] = [
    0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xec, 0x20,
];

unsafe extern "system" fn detour() {
    let shutdown = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::runtime::begin_source_shutdown()
    }));
    let original =
        unsafe { std::mem::transmute::<usize, Shutdown>(ORIGINAL.load(Ordering::Acquire)) };
    unsafe {
        original();
    }
    let result = match shutdown {
        Ok(Ok(shutdown)) => catch_unwind(AssertUnwindSafe(|| unsafe {
            crate::runtime::finish_source_shutdown(shutdown, BASE.load(Ordering::Acquire))
        }))
        .unwrap_or(Err("source shutdown completion panicked")),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("source shutdown retirement panicked"),
    };
    if let Err(error) = result {
        crate::runtime::report_startup_failure(error);
    }
}

/// # Safety
/// Call outside DllMain before gameplay dispatch. No dispatched search work may enter the shutdown
/// entry recursively.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        let hook = unsafe {
            ResidentHook::prepare(base + 0x1874100, detour as *const () as usize, &PREFIX)
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
/// Stop all dispatched gameplay batches first. Hook and trampoline storage remains pinned even
/// if a native shutdown callback disables this hook while the detour is still executing.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(false) }?;
    }
    Ok(())
}

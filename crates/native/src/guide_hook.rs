use crate::{
    lifecycle_hook, platform,
    resident_hook::{HookError, ResidentHook},
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

type StartMission = unsafe extern "system" fn(usize, usize, usize, bool, bool) -> bool;
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
static PENDING: Mutex<Option<Request>> = Mutex::new(None);
static OVERFLOW: AtomicBool = AtomicBool::new(false);
const PREFIX: [u8; 26] = [
    0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x55, 0x57, 0x41, 0x56, 0x48, 0x8d,
    0x6c, 0x24, 0xc1, 0x48, 0x81, 0xec, 0x00, 0x01, 0x00, 0x00,
];

pub struct Request {
    pub slot: usize,
    pub epoch: u64,
}

pub fn take() -> (Option<Request>, bool) {
    (
        PENDING
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take(),
        OVERFLOW.swap(false, Ordering::AcqRel),
    )
}

unsafe extern "system" fn detour(
    manager: usize,
    mission: usize,
    seed: usize,
    restart: bool,
    selected: bool,
) -> bool {
    if let Ok(name) = platform::read::<16>(mission) {
        if let Some(slot) = search_probes_model::guide::mission_slot(&name) {
            let mut pending = PENDING.lock().unwrap_or_else(|error| error.into_inner());
            if pending.is_none() {
                *pending = Some(Request {
                    slot,
                    epoch: lifecycle_hook::epoch(),
                });
            } else {
                OVERFLOW.store(true, Ordering::Release);
            }
            return true;
        }
    }
    let original =
        unsafe { std::mem::transmute::<usize, StartMission>(ORIGINAL.load(Ordering::Acquire)) };
    unsafe { original(manager, mission, seed, restart, selected) }
}

/// # Safety
/// The exact-build mission starter is unpatched. Only exact preset mission IDs are consumed.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        let hook = unsafe {
            ResidentHook::prepare(base + 0x9b8e00, detour as *const () as usize, &PREFIX)
        }?;
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
        *state = Some(hook);
    }
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(true) }?;
    }
    Ok(())
}

/// # Safety
/// No request may be dispatched after the runtime has stopped.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(false) }?;
    }
    take();
    Ok(())
}

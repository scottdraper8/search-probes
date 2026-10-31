use crate::resident_hook::{HookError, ResidentHook};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering},
};

type Change = unsafe extern "system" fn(*mut u8, *const u8, *mut u8, bool);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static APPLICATION: AtomicUsize = AtomicUsize::new(0);
static EPOCH: AtomicU64 = AtomicU64::new(1);
static SESSION: AtomicU64 = AtomicU64::new(1);
static BASE: AtomicUsize = AtomicUsize::new(0);
static STATE: AtomicU8 = AtomicU8::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
const PREFIX: [u8; 15] = [
    0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec, 0x20,
];
const SIMULATION_VTABLE: usize = 0x4b12098;
const GALACTIC_MAP_VTABLE: usize = 0x4b11fe0;
const SAVE_SELECTION_VTABLE: usize = 0x4b121b8;
const TITLE_SCREEN_VTABLE: usize = 0x4b12240;
const SHUTDOWN_VTABLE: usize = 0x4b11f90;
const GLOBAL_LOAD_VTABLE: usize = 0x4b11e70;

#[repr(u8)]
enum ApplicationState {
    Loading,
    Gameplay,
    NewSession,
}

pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

pub fn session() -> u64 {
    SESSION.load(Ordering::Acquire)
}

pub fn ready() -> bool {
    STATE.load(Ordering::Acquire) == ApplicationState::Gameplay as u8
}

pub fn menu() -> bool {
    STATE.load(Ordering::Acquire) == ApplicationState::NewSession as u8
}

fn application_state(machine: usize, base: usize) -> ApplicationState {
    let vtable = crate::platform::pointer(machine + 0x10)
        .and_then(crate::platform::pointer)
        .ok();
    match vtable.and_then(|vtable| vtable.checked_sub(base)) {
        Some(SIMULATION_VTABLE | GALACTIC_MAP_VTABLE) => ApplicationState::Gameplay,
        Some(
            SAVE_SELECTION_VTABLE | TITLE_SCREEN_VTABLE | SHUTDOWN_VTABLE | GLOBAL_LOAD_VTABLE,
        ) => ApplicationState::NewSession,
        _ => ApplicationState::Loading,
    }
}

unsafe extern "system" fn detour(this: *mut u8, key: *const u8, data: *mut u8, restart: bool) {
    let mut changed = false;
    if this as usize == APPLICATION.load(Ordering::Acquire) {
        let current = crate::platform::pointer(this as usize + 0x10)
            .and_then(|current| crate::platform::read::<16>(current + 8));
        let requested = crate::platform::read::<16>(key as usize);
        let unchanged =
            matches!((current,requested),(Ok(current),Ok(requested)) if current==requested);
        if restart || !unchanged {
            changed = true;
            STATE.store(ApplicationState::Loading as u8, Ordering::Release);
            EPOCH.fetch_add(1, Ordering::AcqRel);
        }
    }
    let original =
        unsafe { std::mem::transmute::<usize, Change>(ORIGINAL.load(Ordering::Acquire)) };
    unsafe {
        original(this, key, data, restart);
    }
    if changed {
        let state = application_state(this as usize, BASE.load(Ordering::Acquire));
        if matches!(state, ApplicationState::NewSession) {
            SESSION.fetch_add(1, Ordering::AcqRel);
        }
        STATE.store(state as u8, Ordering::Release);
    }
}

/// # Safety
/// Call outside DllMain. The module and trampoline remain pinned until exit.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        BASE.store(base, Ordering::Release);
        let hook = unsafe {
            ResidentHook::prepare(base + 0x2c4ed70, detour as *const () as usize, &PREFIX)
        }?;
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
        APPLICATION.store(base + 0x6f5f830, Ordering::Release);
        STATE.store(
            application_state(base + 0x6f5f830, base) as u8,
            Ordering::Release,
        );
        *state = Some(hook);
    }
    if APPLICATION.load(Ordering::Acquire) != base + 0x6f5f830 {
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

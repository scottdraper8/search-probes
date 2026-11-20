use crate::resident_hook::{HookError, ResidentHook};
use std::cell::Cell;
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
const SIMULATION_VTABLE: usize = 0x4b1d768;
const GALACTIC_MAP_VTABLE: usize = 0x4b1d6b0;
const SAVE_SELECTION_VTABLE: usize = 0x4b1d888;
const TITLE_SCREEN_VTABLE: usize = 0x4b1d910;
const SHUTDOWN_VTABLE: usize = 0x4b1d660;

thread_local! {
    static TRANSITION_DEPTH: Cell<u32> = const { Cell::new(0) };
}

struct TransitionDepth(bool);

impl TransitionDepth {
    fn enter() -> Self {
        Self(TRANSITION_DEPTH.with(|depth| {
            let outermost = depth.get() == 0;
            depth.set(depth.get().saturating_add(1));
            outermost
        }))
    }
}

impl Drop for TransitionDepth {
    fn drop(&mut self) {
        TRANSITION_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum ApplicationState {
    Loading,
    Gameplay,
    NewSession,
    Shutdown,
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

/// Reconcile an application transition that completed after `StateChange` returned.
///
/// NMS may leave the application in an intermediate loading state when the state-change call
/// returns, then install the simulation state before the first gameplay update. The gameplay
/// callback is the authoritative point at which that deferred transition has completed.
pub(crate) fn reconcile_gameplay() {
    let application = APPLICATION.load(Ordering::Acquire);
    let base = BASE.load(Ordering::Acquire);
    if application == 0
        || base == 0
        || !matches!(
            application_state(application, base),
            ApplicationState::Gameplay
        )
    {
        return;
    }
    if STATE.swap(ApplicationState::Gameplay as u8, Ordering::AcqRel)
        != ApplicationState::Gameplay as u8
        && let Err(error) = crate::runtime::finish_application_transition()
    {
        crate::runtime::report_startup_failure(error);
    }
}

fn application_state(machine: usize, base: usize) -> ApplicationState {
    let vtable = crate::platform::pointer(machine + 0x10)
        .and_then(crate::platform::pointer)
        .ok();
    match vtable.and_then(|vtable| vtable.checked_sub(base)) {
        Some(SIMULATION_VTABLE | GALACTIC_MAP_VTABLE) => ApplicationState::Gameplay,
        Some(SHUTDOWN_VTABLE) => ApplicationState::Shutdown,
        Some(SAVE_SELECTION_VTABLE | TITLE_SCREEN_VTABLE) => ApplicationState::NewSession,
        _ => ApplicationState::Loading,
    }
}

unsafe extern "system" fn detour(this: *mut u8, key: *const u8, data: *mut u8, restart: bool) {
    let original =
        unsafe { std::mem::transmute::<usize, Change>(ORIGINAL.load(Ordering::Acquire)) };
    if this as usize != APPLICATION.load(Ordering::Acquire) {
        unsafe { original(this, key, data, restart) };
        return;
    }
    let current = crate::platform::pointer(this as usize + 0x10)
        .and_then(|current| crate::platform::read::<16>(current + 8));
    let requested = crate::platform::read::<16>(key as usize);
    let unchanged = matches!((current,requested),(Ok(current),Ok(requested)) if current==requested);
    if !restart && unchanged {
        unsafe { original(this, key, data, restart) };
        return;
    }

    let transition = TransitionDepth::enter();
    if transition.0
        && let Err(error) = crate::runtime::begin_application_transition()
    {
        crate::runtime::report_startup_failure(error);
    }
    STATE.store(ApplicationState::Loading as u8, Ordering::Release);
    EPOCH.fetch_add(1, Ordering::AcqRel);
    unsafe {
        original(this, key, data, restart);
    }
    if transition.0 {
        let state = application_state(this as usize, BASE.load(Ordering::Acquire));
        if matches!(state, ApplicationState::NewSession) {
            SESSION.fetch_add(1, Ordering::AcqRel);
        }
        STATE.store(state as u8, Ordering::Release);
        let result = match state {
            ApplicationState::Gameplay => crate::runtime::finish_application_transition(),
            ApplicationState::Shutdown => crate::runtime::shutdown_application(),
            ApplicationState::Loading | ApplicationState::NewSession => {
                crate::runtime::retire_application_sources(matches!(
                    state,
                    ApplicationState::NewSession
                ))
            }
        };
        if let Err(error) = result {
            crate::runtime::report_startup_failure(error);
        }
    }
}

/// # Safety
/// Call outside DllMain. The module and trampoline remain pinned until exit.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        BASE.store(base, Ordering::Release);
        let hook = unsafe {
            ResidentHook::prepare(base + 0x2c53c90, detour as *const () as usize, &PREFIX)
        }?;
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
        APPLICATION.store(base + 0x6f6c250, Ordering::Release);
        STATE.store(
            application_state(base + 0x6f6c250, base) as u8,
            Ordering::Release,
        );
        *state = Some(hook);
    }
    if APPLICATION.load(Ordering::Acquire) != base + 0x6f6c250 {
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

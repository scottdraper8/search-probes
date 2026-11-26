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
static STATE: AtomicU8 = AtomicU8::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
const PREFIX: [u8; 15] = [
    0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec, 0x20,
];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    if application == 0 || !matches!(application_state(application), ApplicationState::Gameplay) {
        return;
    }
    if STATE.swap(ApplicationState::Gameplay as u8, Ordering::AcqRel)
        != ApplicationState::Gameplay as u8
        && let Err(error) = crate::runtime::finish_application_transition()
    {
        crate::runtime::report_startup_failure(error);
    }
}

fn fixed_state(actual: &[u8; 16], expected: &[u8]) -> bool {
    actual.get(..expected.len()) == Some(expected)
        && actual[expected.len()..].iter().all(|byte| *byte == 0)
}

fn classify_state(identity: [u8; 16]) -> ApplicationState {
    if fixed_state(&identity, b"APPVIEW") || fixed_state(&identity, b"GALAXYMAP") {
        ApplicationState::Gameplay
    } else if fixed_state(&identity, b"APPSHUTDOWN") {
        ApplicationState::Shutdown
    } else if fixed_state(&identity, b"MODESELECTOR") || fixed_state(&identity, b"TITLESCREEN") {
        ApplicationState::NewSession
    } else {
        ApplicationState::Loading
    }
}

fn application_state(machine: usize) -> ApplicationState {
    crate::platform::pointer(machine + 0x10)
        .and_then(|current| crate::platform::read::<16>(current + 8))
        .map_or(ApplicationState::Loading, classify_state)
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
        let state = application_state(this as usize);
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
        let hook = unsafe {
            ResidentHook::prepare(base + 0x2c57670, detour as *const () as usize, &PREFIX)
        }?;
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
        APPLICATION.store(base + 0x6f71730, Ordering::Release);
        STATE.store(application_state(base + 0x6f71730) as u8, Ordering::Release);
        *state = Some(hook);
    }
    if APPLICATION.load(Ordering::Acquire) != base + 0x6f71730 {
        return Err(HookError::PrefixChanged);
    }
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(true) }?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ApplicationState, classify_state};

    fn identity(value: &[u8]) -> [u8; 16] {
        let mut result = [0; 16];
        result[..value.len()].copy_from_slice(value);
        result
    }

    #[test]
    fn classifies_stable_application_state_ids() {
        assert_eq!(
            classify_state(identity(b"APPVIEW")),
            ApplicationState::Gameplay
        );
        assert_eq!(
            classify_state(identity(b"GALAXYMAP")),
            ApplicationState::Gameplay
        );
        assert_eq!(
            classify_state(identity(b"APPSHUTDOWN")),
            ApplicationState::Shutdown
        );
        assert_eq!(
            classify_state(identity(b"MODESELECTOR")),
            ApplicationState::NewSession
        );
        assert_eq!(
            classify_state(identity(b"TITLESCREEN")),
            ApplicationState::NewSession
        );
        assert_eq!(
            classify_state(identity(b"APPLOCALLOAD")),
            ApplicationState::Loading
        );
    }

    #[test]
    fn rejects_non_fixed_state_ids() {
        assert_eq!(
            classify_state(*b"APPVIEW-NOTZERO!"),
            ApplicationState::Loading
        );
    }
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

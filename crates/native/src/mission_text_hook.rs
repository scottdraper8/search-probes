use crate::resident_hook::{HookError, ResidentHook};
use search_probes_model::fauna::Domain;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const LOCALISE_RVA: usize = 0x2c45990;
const PREFIX: [u8; 15] = [
    0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec, 0x40,
];
const OBJECTIVE_ID: &[u8] = b"SQN_SP_FAUNA_MISSION_OBJ";
const TIP_ID: &[u8] = b"SQN_SP_FAUNA_MISSION_OBJ_TIP";
const STATUS_ID: &[u8] = b"SQN_SP_FAUNA_SCAN";

type Localise = unsafe extern "system" fn(usize, *const u8, *const u8) -> *const u8;

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
static OBJECTIVE: AtomicUsize = AtomicUsize::new(0);
static TIP: AtomicUsize = AtomicUsize::new(0);

unsafe fn matches(value: *const u8, expected: &[u8]) -> bool {
    if value.is_null() {
        return false;
    }
    for (index, expected) in expected.iter().copied().enumerate() {
        if unsafe { value.add(index).read() } != expected {
            return false;
        }
    }
    unsafe { value.add(expected.len()).read() == 0 }
}

unsafe extern "system" fn detour(
    language: usize,
    identity: *const u8,
    fallback: *const u8,
) -> *const u8 {
    let replacement =
        if unsafe { matches(identity, OBJECTIVE_ID) } || unsafe { matches(identity, STATUS_ID) } {
            OBJECTIVE.load(Ordering::Acquire)
        } else if unsafe { matches(identity, TIP_ID) } {
            TIP.load(Ordering::Acquire)
        } else {
            0
        };
    if replacement != 0 {
        return replacement as *const u8;
    }
    let original =
        unsafe { std::mem::transmute::<usize, Localise>(ORIGINAL.load(Ordering::Acquire)) };
    unsafe { original(language, identity, fallback) }
}

fn publish(destination: &AtomicUsize, text: String) -> Result<(), &'static str> {
    if text.as_bytes().contains(&0) {
        return Err("generated mission text contains a null byte");
    }
    let mut bytes = text.into_bytes();
    bytes.push(0);
    // NMS retains localized string pointers while building HUD and Log widgets. Each fauna target
    // therefore receives immutable process-lifetime storage; publishing a later target only swaps
    // the pointer and never invalidates a reader of the previous one.
    let text = Box::leak(bytes.into_boxed_slice());
    destination.store(text.as_ptr() as usize, Ordering::Release);
    Ok(())
}

pub fn set_fauna(name: Option<&str>, domain: Domain, cave: bool) -> Result<(), &'static str> {
    let habitat = match (domain, cave) {
        (Domain::Land, true) => "The target is an underground-dwelling creature.",
        (Domain::Land, false) => "The target is a surface-dwelling creature.",
        (Domain::Aquatic, _) => "The target is an aquatic creature.",
        (Domain::Airborne, _) => "The target is an airborne creature.",
    };
    publish(
        &OBJECTIVE,
        name.map_or_else(
            || "Use the Analysis Visor to scan the target animal".to_owned(),
            |name| format!("Use the Analysis Visor to scan {name}"),
        ),
    )?;
    publish(&TIP, habitat.into())
}

pub fn clear() {
    OBJECTIVE.store(0, Ordering::Release);
    TIP.store(0, Ordering::Release);
}

/// # Safety
/// The verified NMS image and the localization manager remain loaded while this hook is enabled.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut hook = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if hook.is_none() {
        let prepared = unsafe {
            ResidentHook::prepare(base + LOCALISE_RVA, detour as *const () as usize, &PREFIX)?
        };
        ORIGINAL.store(prepared.trampoline(), Ordering::Release);
        *hook = Some(prepared);
    }
    unsafe {
        hook.as_mut()
            .expect("prepared mission localization hook")
            .set_enabled(true)
    }
}

/// # Safety
/// All localization calls must have left the patched entry point before it is restored.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut hook = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = hook.as_mut() {
        unsafe { hook.set_enabled(false)? };
    }
    clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objective_and_habitat_text_are_null_terminated() {
        set_fauna(Some("I. Festerluwelus"), Domain::Land, false).unwrap();
        let objective = OBJECTIVE.load(Ordering::Acquire) as *const u8;
        let tip = TIP.load(Ordering::Acquire) as *const u8;
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(objective.cast()) }
                .to_str()
                .unwrap(),
            "Use the Analysis Visor to scan I. Festerluwelus"
        );
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(tip.cast()) }
                .to_str()
                .unwrap(),
            "The target is a surface-dwelling creature."
        );
        clear();
    }
}

use crate::resident_hook::{HookError, ResidentHook};
use search_probes_model::{cache::CachedMatch, fauna::Domain, target::TargetKind};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const FIXED_LOCALISE_RVA: usize = 0x2c45da0;
const FIXED_PREFIX: [u8; 17] = [
    0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x05, 0x77, 0x9c, 0x98,
    0x00,
];
const STRING_LOCALISE_RVA: usize = 0x2c45f70;
const STRING_PREFIX: [u8; 15] = [
    0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec, 0x40,
];
const SYSTEM_OBJECTIVE_ID: &[u8] = b"SQN_SP_SYSTEM_MISSION_OBJ";
const PLANET_OBJECTIVE_ID: &[u8] = b"SQN_SP_NAV_MISSION_OBJ";
const FAUNA_OBJECTIVE_ID: &[u8] = b"SQN_SP_FAUNA_MISSION_OBJ";
const FAUNA_TIP_ID: &[u8] = b"SQN_SP_FAUNA_MISSION_OBJ_TIP";
const FAUNA_STATUS_ID: &[u8] = b"SQN_SP_FAUNA_SCAN";

type FixedLocalise = unsafe extern "system" fn(usize, *const u8) -> *const u8;
type StringLocalise = unsafe extern "system" fn(usize, *const u8, *const u8) -> *const u8;

static FIXED_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static STRING_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOKS: Mutex<Option<Hooks>> = Mutex::new(None);
static SYSTEM_OBJECTIVE: AtomicUsize = AtomicUsize::new(0);
static PLANET_OBJECTIVE: AtomicUsize = AtomicUsize::new(0);
static FAUNA_OBJECTIVE: AtomicUsize = AtomicUsize::new(0);
static FAUNA_TIP: AtomicUsize = AtomicUsize::new(0);

struct Hooks {
    fixed: ResidentHook,
    string: ResidentHook,
}

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

unsafe fn replacement(identity: *const u8) -> usize {
    if unsafe { matches(identity, SYSTEM_OBJECTIVE_ID) } {
        SYSTEM_OBJECTIVE.load(Ordering::Acquire)
    } else if unsafe { matches(identity, PLANET_OBJECTIVE_ID) } {
        PLANET_OBJECTIVE.load(Ordering::Acquire)
    } else if unsafe { matches(identity, FAUNA_OBJECTIVE_ID) }
        || unsafe { matches(identity, FAUNA_STATUS_ID) }
    {
        FAUNA_OBJECTIVE.load(Ordering::Acquire)
    } else if unsafe { matches(identity, FAUNA_TIP_ID) } {
        FAUNA_TIP.load(Ordering::Acquire)
    } else {
        0
    }
}

unsafe extern "system" fn fixed_detour(language: usize, identity: *const u8) -> *const u8 {
    let replacement = unsafe { replacement(identity) };
    if replacement != 0 {
        return replacement as *const u8;
    }
    let original = unsafe {
        std::mem::transmute::<usize, FixedLocalise>(FIXED_ORIGINAL.load(Ordering::Acquire))
    };
    unsafe { original(language, identity) }
}

unsafe extern "system" fn string_detour(
    language: usize,
    identity: *const u8,
    fallback: *const u8,
) -> *const u8 {
    let replacement = unsafe { replacement(identity) };
    if replacement != 0 {
        return replacement as *const u8;
    }
    let original = unsafe {
        std::mem::transmute::<usize, StringLocalise>(STRING_ORIGINAL.load(Ordering::Acquire))
    };
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

fn required<'a>(name: Option<&'a str>, kind: &'static str) -> Result<&'a str, &'static str> {
    name.filter(|name| !name.is_empty()).ok_or(kind)
}

pub fn set_system(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("destination has no system name");
    }
    publish(&SYSTEM_OBJECTIVE, format!("Enter {name}"))
}

pub fn set_planet(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("destination has no planet name");
    }
    publish(&PLANET_OBJECTIVE, format!("Land on {name}"))
}

pub fn set_fauna(name: &str, domain: Domain, cave: bool) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("destination has no fauna name");
    }
    let habitat = match (domain, cave) {
        (Domain::Land, true) => "The target is an underground-dwelling creature",
        (Domain::Land, false) => "The target is a surface-dwelling creature",
        (Domain::Aquatic, _) => "The target is an aquatic creature",
        (Domain::Airborne, _) => "The target is an airborne creature",
    };
    publish(
        &FAUNA_OBJECTIVE,
        format!("Use the Analysis Visor to scan {name}"),
    )?;
    publish(&FAUNA_TIP, habitat.into())
}

pub fn set_destination(result: &CachedMatch) -> Result<(), &'static str> {
    let system = required(
        result.system_name.as_deref(),
        "destination has no system name",
    )?;
    let planet = (result.kind != TargetKind::System)
        .then(|| {
            required(
                result.planet_name.as_deref(),
                "destination has no planet name",
            )
        })
        .transpose()?;
    let fauna = if result.kind == TargetKind::Fauna {
        let fauna = result.fauna.as_ref().ok_or("destination has no fauna")?;
        Some((
            required(fauna.name.as_deref(), "destination has no fauna name")?,
            fauna.domain,
            fauna.cave,
        ))
    } else {
        None
    };
    set_system(system)?;
    if let Some(planet) = planet {
        set_planet(planet)?;
    }
    if let Some((name, domain, cave)) = fauna {
        set_fauna(name, domain, cave)?;
    }
    Ok(())
}

pub fn clear() {
    SYSTEM_OBJECTIVE.store(0, Ordering::Release);
    PLANET_OBJECTIVE.store(0, Ordering::Release);
    FAUNA_OBJECTIVE.store(0, Ordering::Release);
    FAUNA_TIP.store(0, Ordering::Release);
}

/// # Safety
/// The verified NMS image and the localization manager remain loaded while this hook is enabled.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut hooks = HOOKS.lock().map_err(|_| HookError::StatePoisoned)?;
    if hooks.is_none() {
        let fixed = unsafe {
            ResidentHook::prepare(
                base + FIXED_LOCALISE_RVA,
                fixed_detour as *const () as usize,
                &FIXED_PREFIX,
            )?
        };
        let string = unsafe {
            ResidentHook::prepare(
                base + STRING_LOCALISE_RVA,
                string_detour as *const () as usize,
                &STRING_PREFIX,
            )?
        };
        FIXED_ORIGINAL.store(fixed.trampoline(), Ordering::Release);
        STRING_ORIGINAL.store(string.trampoline(), Ordering::Release);
        *hooks = Some(Hooks { fixed, string });
    }
    let hooks = hooks.as_mut().expect("prepared mission localization hooks");
    unsafe { hooks.fixed.set_enabled(true)? };
    if let Err(error) = unsafe { hooks.string.set_enabled(true) } {
        let _ = unsafe { hooks.fixed.set_enabled(false) };
        return Err(error);
    }
    Ok(())
}

/// # Safety
/// All localization calls must have left the patched entry point before it is restored.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut hooks = HOOKS.lock().map_err(|_| HookError::StatePoisoned)?;
    let mut failure = None;
    if let Some(hooks) = hooks.as_mut() {
        if let Err(error) = unsafe { hooks.string.set_enabled(false) } {
            failure = Some(error);
        }
        if let Err(error) = unsafe { hooks.fixed.set_enabled(false) }
            && failure.is_none()
        {
            failure = Some(error);
        }
    }
    clear();
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objective_and_habitat_text_are_null_terminated() {
        set_system("Yihytaka XII").unwrap();
        set_planet("Giumfi Major").unwrap();
        set_fauna("I. Festerluwelus", Domain::Land, false).unwrap();
        let system = SYSTEM_OBJECTIVE.load(Ordering::Acquire) as *const u8;
        let planet = PLANET_OBJECTIVE.load(Ordering::Acquire) as *const u8;
        let objective = FAUNA_OBJECTIVE.load(Ordering::Acquire) as *const u8;
        let tip = FAUNA_TIP.load(Ordering::Acquire) as *const u8;
        let mut fixed_planet_identity = [0; 32];
        fixed_planet_identity[..PLANET_OBJECTIVE_ID.len()].copy_from_slice(PLANET_OBJECTIVE_ID);
        assert_eq!(
            unsafe { replacement(fixed_planet_identity.as_ptr()) },
            planet as usize
        );
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(system.cast()) }
                .to_str()
                .unwrap(),
            "Enter Yihytaka XII"
        );
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(planet.cast()) }
                .to_str()
                .unwrap(),
            "Land on Giumfi Major"
        );
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
            "The target is a surface-dwelling creature"
        );
        clear();
    }
}

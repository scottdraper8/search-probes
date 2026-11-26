use crate::resident_hook::{HookError, ResidentHook};
use search_probes_model::{
    binding::InputBinding,
    cache::CachedMatch,
    fauna::Domain,
    settings::{MAX_PRESETS, Preset},
    target::TargetKind,
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const FIXED_LOCALISE_RVA: usize = 0x2c493b0;
const FIXED_PREFIX: [u8; 17] = [
    0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x05, 0xa7, 0xa6, 0x98,
    0x00,
];
const STRING_LOCALISE_RVA: usize = 0x2c49580;
const STRING_PREFIX: [u8; 15] = [
    0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec, 0x40,
];
const SYSTEM_DESCRIPTION_ID: &[u8] = b"SQN_SP_SYSTEM_MISSION_DESC";
const SYSTEM_FLAVOUR_ID: &[u8] = b"SQN_SP_SYSTEM_MISSION_FLAVOUR";
const SYSTEM_OBJECTIVE_ID: &[u8] = b"SQN_SP_SYSTEM_MISSION_OBJ";
const PLANET_OBJECTIVE_ID: &[u8] = b"SQN_SP_NAV_MISSION_OBJ";
const FAUNA_OBJECTIVE_ID: &[u8] = b"SQN_SP_FAUNA_MISSION_OBJ";
const FAUNA_TIP_ID: &[u8] = b"SQN_SP_FAUNA_MISSION_OBJ_TIP";
const FAUNA_STATUS_ID: &[u8] = b"SQN_SP_FAUNA_SCAN";
const GUIDE_BODY_ID: &[u8] = b"SQN_SP_BODY";
const PRESET_IDS: [&[u8]; MAX_PRESETS] = [
    b"SQN_SP_SLOT1",
    b"SQN_SP_SLOT2",
    b"SQN_SP_SLOT3",
    b"SQN_SP_SLOT4",
    b"SQN_SP_SLOT5",
    b"SQN_SP_SLOT6",
    b"SQN_SP_SLOT7",
    b"SQN_SP_SLOT8",
    b"SQN_SP_SLOT9",
];

type FixedLocalise = unsafe extern "system" fn(usize, *const u8) -> *const u8;
type StringLocalise = unsafe extern "system" fn(usize, *const u8, *const u8) -> *const u8;

static FIXED_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static STRING_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOKS: Mutex<Option<Hooks>> = Mutex::new(None);
static SYSTEM_DESCRIPTION: AtomicUsize = AtomicUsize::new(0);
static SYSTEM_FLAVOUR: AtomicUsize = AtomicUsize::new(0);
static SYSTEM_OBJECTIVE: AtomicUsize = AtomicUsize::new(0);
static PLANET_OBJECTIVE: AtomicUsize = AtomicUsize::new(0);
static FAUNA_OBJECTIVE: AtomicUsize = AtomicUsize::new(0);
static FAUNA_TIP: AtomicUsize = AtomicUsize::new(0);
static GUIDE_BODY: AtomicUsize = AtomicUsize::new(0);
static PRESET_NAMES: [AtomicUsize; MAX_PRESETS] = [const { AtomicUsize::new(0) }; MAX_PRESETS];

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
    if let Some(slot) = PRESET_IDS
        .iter()
        .position(|expected| unsafe { matches(identity, expected) })
    {
        PRESET_NAMES[slot].load(Ordering::Acquire)
    } else if unsafe { matches(identity, SYSTEM_DESCRIPTION_ID) } {
        SYSTEM_DESCRIPTION.load(Ordering::Acquire)
    } else if unsafe { matches(identity, SYSTEM_FLAVOUR_ID) } {
        SYSTEM_FLAVOUR.load(Ordering::Acquire)
    } else if unsafe { matches(identity, SYSTEM_OBJECTIVE_ID) } {
        SYSTEM_OBJECTIVE.load(Ordering::Acquire)
    } else if unsafe { matches(identity, PLANET_OBJECTIVE_ID) } {
        PLANET_OBJECTIVE.load(Ordering::Acquire)
    } else if unsafe { matches(identity, FAUNA_OBJECTIVE_ID) }
        || unsafe { matches(identity, FAUNA_STATUS_ID) }
    {
        FAUNA_OBJECTIVE.load(Ordering::Acquire)
    } else if unsafe { matches(identity, FAUNA_TIP_ID) } {
        FAUNA_TIP.load(Ordering::Acquire)
    } else if unsafe { matches(identity, GUIDE_BODY_ID) } {
        GUIDE_BODY.load(Ordering::Acquire)
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
    publish(
        &SYSTEM_DESCRIPTION,
        format!("A search probe has identified {name} as a matching system."),
    )?;
    publish(
        &SYSTEM_FLAVOUR,
        format!("Follow Current Mission in the Galaxy Map and enter {name}."),
    )?;
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
    let habitat = format!("The target is a {} creature", domain.dwelling(cave));
    publish(
        &FAUNA_OBJECTIVE,
        format!("Use the Analysis Visor to scan {name}"),
    )?;
    publish(&FAUNA_TIP, habitat)
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

fn binding_label(binding: InputBinding) -> String {
    match binding {
        InputBinding::Keyboard { first, second } => second.map_or_else(
            || first.label().to_owned(),
            |second| format!("{} + {}", first.label(), second.label()),
        ),
        InputBinding::Controller { first, second } => second.map_or_else(
            || first.label().to_owned(),
            |second| format!("{} + {}", first.label(), second.label()),
        ),
    }
}

fn guide_body(hotkey: InputBinding) -> String {
    format!(
        "• Open Search Probes with {} to edit and name up to nine presets\n\
         • Saved presets appear here\n\
         • Launch a preset, then close the Guide to let the search run\n\
         • Choose a result to copy coordinates or launch a Search Probes in-game mission",
        binding_label(hotkey)
    )
}

pub fn set_guide(presets: &[Preset], hotkey: InputBinding) -> Result<(), &'static str> {
    if presets.len() > MAX_PRESETS {
        return Err("too many Guide presets");
    }
    if presets
        .iter()
        .any(|preset| preset.name.as_bytes().contains(&0))
    {
        return Err("Guide preset name contains a null byte");
    }
    for (slot, destination) in PRESET_NAMES.iter().enumerate() {
        if let Some(preset) = presets.get(slot) {
            publish(destination, preset.name.clone())?;
        } else {
            destination.store(0, Ordering::Release);
        }
    }
    publish(&GUIDE_BODY, guide_body(hotkey))?;
    Ok(())
}

fn clear_guide() {
    for preset in &PRESET_NAMES {
        preset.store(0, Ordering::Release);
    }
    GUIDE_BODY.store(0, Ordering::Release);
}

pub fn clear() {
    SYSTEM_DESCRIPTION.store(0, Ordering::Release);
    SYSTEM_FLAVOUR.store(0, Ordering::Release);
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
    clear_guide();
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
        let system_description = SYSTEM_DESCRIPTION.load(Ordering::Acquire) as *const u8;
        let system_flavour = SYSTEM_FLAVOUR.load(Ordering::Acquire) as *const u8;
        let planet = PLANET_OBJECTIVE.load(Ordering::Acquire) as *const u8;
        let objective = FAUNA_OBJECTIVE.load(Ordering::Acquire) as *const u8;
        let tip = FAUNA_TIP.load(Ordering::Acquire) as *const u8;
        let mut fixed_planet_identity = [0; 32];
        fixed_planet_identity[..PLANET_OBJECTIVE_ID.len()].copy_from_slice(PLANET_OBJECTIVE_ID);
        let mut fixed_system_description = [0; 32];
        fixed_system_description[..SYSTEM_DESCRIPTION_ID.len()]
            .copy_from_slice(SYSTEM_DESCRIPTION_ID);
        let mut fixed_system_flavour = [0; 32];
        fixed_system_flavour[..SYSTEM_FLAVOUR_ID.len()].copy_from_slice(SYSTEM_FLAVOUR_ID);
        assert_eq!(
            unsafe { replacement(fixed_system_description.as_ptr()) },
            system_description as usize
        );
        assert_eq!(
            unsafe { replacement(fixed_system_flavour.as_ptr()) },
            system_flavour as usize
        );
        assert_eq!(
            unsafe { replacement(fixed_planet_identity.as_ptr()) },
            planet as usize
        );
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(system_description.cast()) }
                .to_str()
                .unwrap(),
            "A search probe has identified Yihytaka XII as a matching system."
        );
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(system_flavour.cast()) }
                .to_str()
                .unwrap(),
            "Follow Current Mission in the Galaxy Map and enter Yihytaka XII."
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

    #[test]
    fn saved_preset_names_replace_only_visible_guide_slot_labels() {
        set_guide(
            &[Preset {
                name: "Triple-S fauna".into(),
                search: Default::default(),
            }],
            InputBinding::default(),
        )
        .unwrap();
        let mut first = [0; 32];
        first[..PRESET_IDS[0].len()].copy_from_slice(PRESET_IDS[0]);
        let mut second = [0; 32];
        second[..PRESET_IDS[1].len()].copy_from_slice(PRESET_IDS[1]);
        let name = unsafe { replacement(first.as_ptr()) } as *const u8;
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(name.cast()) }
                .to_str()
                .unwrap(),
            "Triple-S fauna"
        );
        assert_eq!(unsafe { replacement(second.as_ptr()) }, 0);
        set_guide(&[], InputBinding::default()).unwrap();
        assert_eq!(unsafe { replacement(first.as_ptr()) }, 0);
    }

    #[test]
    fn guide_body_is_a_bulleted_list_with_the_configured_hotkey() {
        assert_eq!(
            guide_body(InputBinding::Keyboard {
                first: search_probes_model::binding::KeyboardKey::F7,
                second: None,
            },),
            "• Open Search Probes with F7 to edit and name up to nine presets\n\
             • Saved presets appear here\n\
             • Launch a preset, then close the Guide to let the search run\n\
             • Choose a result to copy coordinates or launch a Search Probes in-game mission"
        );
    }
}

use crate::{
    platform,
    resident_hook::{HookError, ResidentHook},
};
use search_probes_model::{
    address::Address,
    target::{Target, TargetKind},
};
use std::{
    cell::Cell,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

type FindBuilding =
    unsafe extern "system" fn(usize, usize, usize, bool, bool, usize, bool, bool, usize) -> usize;
type ResolveScanEvent = unsafe extern "system" fn(
    usize,
    usize,
    u32,
    usize,
    usize,
    bool,
    f32,
    usize,
    usize,
    bool,
    bool,
    bool,
    usize,
    bool,
) -> usize;
type PlanetFilter = unsafe extern "system" fn(usize, usize, bool, u8) -> bool;
type PlanetAddress = unsafe extern "system" fn(usize) -> u64;
const PLANET_ADDRESS_OFFSET: usize = 0x16ae410;
const PLANET_ADDRESS_PREFIX: [u8; 12] = [
    0x48, 0x83, 0xec, 0x38, 0x4c, 0x8b, 0xd1, 0x48, 0x8d, 0x4c, 0x24, 0x20,
];
static ORIGINAL_RESOLVE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_BUILDING: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_FILTER: AtomicUsize = AtomicUsize::new(0);
static PLANET_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static HOOKS: Mutex<Vec<ResidentHook>> = Mutex::new(Vec::new());
static PUBLISHED: Mutex<Option<Target>> = Mutex::new(None);
static ERROR: Mutex<Option<&'static str>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct Selection {
    event: usize,
    target: Target,
}

thread_local! {
    static SELECTION: Cell<Option<Selection>> = const { Cell::new(None) };
}

struct SelectionGuard(Option<Selection>);

impl Drop for SelectionGuard {
    fn drop(&mut self) {
        SELECTION.set(self.0);
    }
}

pub fn reset() {
    *PUBLISHED.lock().unwrap_or_else(|error| error.into_inner()) = None;
    *ERROR.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

fn fail(error: &'static str) {
    *ERROR.lock().unwrap_or_else(|error| error.into_inner()) = Some(error);
}

pub fn published() -> Result<Option<Target>, &'static str> {
    if let Some(error) = ERROR
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        return Err(error);
    }
    Ok(*PUBLISHED.lock().unwrap_or_else(|error| error.into_inner()))
}

fn owned(event: usize) -> Option<TargetKind> {
    let actual = platform::read::<32>(event.saturating_add(0x228)).ok()?;
    TargetKind::ALL.into_iter().find(|kind| {
        let mut expected = [0; 32];
        expected[..kind.event().len()].copy_from_slice(kind.event().as_bytes());
        actual == expected
    })
}

fn select(event: usize, context: usize) -> Result<Selection, &'static str> {
    let kind = owned(event).ok_or("unrecognized Search Probes destination event")?;
    let identity = platform::read::<16>(context)?;
    let Some((mission_kind, fauna_role)) = TargetKind::from_mission(&identity)? else {
        return Err("Search Probes received an incompatible destination mission");
    };
    if mission_kind != kind
        || platform::read::<1>(context + 0x18)? != [1]
        || platform::u32_at(event + 0x48c)? != 4
        || platform::u32_at(event + 0x430)? != 1
    {
        return Err("Search Probes received an incompatible destination event");
    }
    let seed = u64::from_le_bytes(platform::read(context + 0x10)?);
    let target = crate::destination::from_seed(kind, fauna_role, seed)?;
    let text = platform::pointer(platform::pointer(event + 0x428)?)?;
    let previous = platform::read::<17>(text)?;
    if previous[16] != 0 || !previous[..16].iter().all(u8::is_ascii_hexdigit) {
        return Err("Search Probes destination buffer is invalid");
    }
    unsafe { platform::write(text, &crate::destination::encode(target.address)) }?;
    Ok(Selection { event, target })
}

unsafe fn planet_address(planet: usize) -> Result<Address, &'static str> {
    platform::read::<2>(planet + 0x50)?;
    let original = unsafe {
        std::mem::transmute::<usize, PlanetAddress>(PLANET_ADDRESS.load(Ordering::Acquire))
    };
    Address::from_raw(unsafe { original(planet) })
}

unsafe extern "system" fn building_detour(
    manager: usize,
    event: usize,
    callback: usize,
    update_callback: bool,
    allow_fallback: bool,
    context: usize,
    dry_run: bool,
    pending: bool,
    position: usize,
) -> usize {
    let selection = if owned(event).is_some() {
        match select(event, context) {
            Ok(selection) => Some(selection),
            Err(error) => {
                fail(error);
                return 0;
            }
        }
    } else {
        None
    };
    let _guard = SelectionGuard(SELECTION.replace(selection));
    let original = unsafe {
        std::mem::transmute::<usize, FindBuilding>(ORIGINAL_BUILDING.load(Ordering::Acquire))
    };
    let building = unsafe {
        original(
            manager,
            event,
            callback,
            update_callback,
            allow_fallback,
            context,
            dry_run,
            pending,
            position,
        )
    };
    if let Some(selection) = SELECTION.get().filter(|selection| selection.event == event) {
        if building != 0 {
            let actual = platform::pointer(building + 0x70)
                .and_then(|planet| unsafe { planet_address(planet) });
            let matches_target = match (selection.target.kind, actual) {
                (TargetKind::Planet | TargetKind::Fauna, Ok(actual)) => {
                    actual == selection.target.address
                }
                (TargetKind::System, Ok(actual)) => actual.system() == selection.target.address,
                (_, Err(_)) => false,
            };
            if !matches_target {
                fail(
                    "NMS could not place a marker on the selected planet. Abandon Search Probes in the Log before retrying.",
                );
                return 0;
            }
            *PUBLISHED.lock().unwrap_or_else(|error| error.into_inner()) = Some(selection.target);
        }
    }
    building
}

unsafe extern "system" fn resolve_detour(
    manager: usize,
    result: usize,
    mode: u32,
    event: usize,
    argument_5: usize,
    argument_6: bool,
    argument_7: f32,
    context: usize,
    destination: usize,
    argument_10: bool,
    argument_11: bool,
    argument_12: bool,
    argument_13: usize,
    argument_14: bool,
) -> usize {
    let selection = if owned(event).is_some() {
        match select(event, context) {
            Ok(selection) => Some(selection),
            Err(error) => {
                fail(error);
                None
            }
        }
    } else {
        None
    };
    let _guard = SelectionGuard(SELECTION.replace(selection));
    let original = unsafe {
        std::mem::transmute::<usize, ResolveScanEvent>(ORIGINAL_RESOLVE.load(Ordering::Acquire))
    };
    unsafe {
        original(
            manager,
            result,
            mode,
            event,
            argument_5,
            argument_6,
            argument_7,
            context,
            destination,
            argument_10,
            argument_11,
            argument_12,
            argument_13,
            argument_14,
        )
    }
}

unsafe extern "system" fn filter_detour(
    planet: usize,
    parameters: usize,
    allow_dead: bool,
    class: u8,
) -> bool {
    if let Some(selection) = SELECTION.get().filter(|selection| {
        matches!(
            selection.target.kind,
            TargetKind::Planet | TargetKind::Fauna
        )
            // FindBuilding passes its event-data pointer through unchanged as argument two.
            && parameters == selection.event
    }) {
        return match unsafe { planet_address(planet) } {
            Ok(address) => address == selection.target.address,
            Err(error) => {
                fail(error);
                false
            }
        };
    }
    let original = unsafe {
        std::mem::transmute::<usize, PlanetFilter>(ORIGINAL_FILTER.load(Ordering::Acquire))
    };
    unsafe { original(planet, parameters, allow_dead, class) }
}

/// # Safety
/// The seed owns the destination across saves; scoped local filtering returns only native-owned
/// building records.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut hooks = HOOKS.lock().map_err(|_| HookError::StatePoisoned)?;
    if hooks.is_empty() {
        let planet_address = base + PLANET_ADDRESS_OFFSET;
        if unsafe {
            std::slice::from_raw_parts(planet_address as *const u8, PLANET_ADDRESS_PREFIX.len())
        } != PLANET_ADDRESS_PREFIX
        {
            return Err(HookError::PrefixChanged);
        }
        let definitions: [(usize, usize, &[u8], &AtomicUsize); 3] = [
            (
                0x1305bf0,
                resolve_detour as *const () as usize,
                &[
                    0x44, 0x89, 0x44, 0x24, 0x18, 0x48, 0x89, 0x4c, 0x24, 0x08, 0x55, 0x53, 0x56,
                    0x57, 0x41, 0x54,
                ],
                &ORIGINAL_RESOLVE,
            ),
            (
                0x130d3b0,
                building_detour as *const () as usize,
                &[
                    0x44, 0x88, 0x4c, 0x24, 0x20, 0x4c, 0x89, 0x44, 0x24, 0x18, 0x48, 0x89, 0x54,
                    0x24, 0x10,
                ],
                &ORIGINAL_BUILDING,
            ),
            (
                0x1310680,
                filter_detour as *const () as usize,
                &[
                    0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74,
                    0x24, 0x18,
                ],
                &ORIGINAL_FILTER,
            ),
        ];
        let mut prepared = Vec::with_capacity(definitions.len());
        for (offset, detour, prefix, original) in definitions {
            let hook = unsafe { ResidentHook::prepare(base + offset, detour, prefix) }?;
            original.store(hook.trampoline(), Ordering::Release);
            prepared.push(hook);
        }
        PLANET_ADDRESS.store(planet_address, Ordering::Release);
        *hooks = prepared;
    }
    for hook in hooks.iter_mut().rev() {
        unsafe { hook.set_enabled(true) }?;
    }
    Ok(())
}

/// # Safety
/// All gameplay navigation calls must have returned before disabling the hooks.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut hooks = HOOKS.lock().map_err(|_| HookError::StatePoisoned)?;
    for hook in hooks.iter_mut() {
        unsafe { hook.set_enabled(false) }?;
    }
    reset();
    Ok(())
}

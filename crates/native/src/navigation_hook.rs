use crate::{
    navigation::{MISSION_ID, TARGET_EVENT},
    platform,
    resident_hook::{HookError, ResidentHook},
};
use search_probes_model::address::Address;
use std::{
    cell::Cell,
    sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

type Scan = unsafe extern "system" fn(
    usize,
    usize,
    i32,
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
type FindBuilding =
    unsafe extern "system" fn(usize, usize, usize, bool, bool, usize, bool, bool, usize) -> usize;
type PlanetFilter = unsafe extern "system" fn(usize, usize, bool, u8) -> bool;
type PlanetAddress = unsafe extern "system" fn(usize) -> u64;

static ORIGINAL_SCAN: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_BUILDING: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_FILTER: AtomicUsize = AtomicUsize::new(0);
static PLANET_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static HOOKS: Mutex<Vec<ResidentHook>> = Mutex::new(Vec::new());
static PUBLISHED: AtomicU64 = AtomicU64::new(0);
static ERROR: Mutex<Option<&'static str>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct Selection {
    event: usize,
    target: Address,
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
    PUBLISHED.store(0, Ordering::Release);
    *ERROR.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

fn fail(error: &'static str) {
    *ERROR.lock().unwrap_or_else(|error| error.into_inner()) = Some(error);
}

pub fn published() -> Result<Option<Address>, &'static str> {
    if let Some(error) = ERROR
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        return Err(error);
    }
    match PUBLISHED.load(Ordering::Acquire) {
        0 => Ok(None),
        value => Address::from_raw(value).map(Some),
    }
}

fn owned(event: usize) -> bool {
    let mut expected = [0; 32];
    expected[..TARGET_EVENT.len()].copy_from_slice(TARGET_EVENT);
    platform::read::<32>(event.saturating_add(0x228)) == Ok(expected)
}

fn select(event: usize, context: usize) -> Result<Selection, &'static str> {
    let mut identity = [0; 16];
    identity[..MISSION_ID.len()].copy_from_slice(MISSION_ID);
    if platform::read::<16>(context)? != identity
        || platform::read::<1>(context + 0x18)? != [1]
        || platform::u32_at(event + 0x47c)? != 4
        || platform::u32_at(event + 0x420)? != 1
    {
        return Err("Search Probes received an incompatible destination event");
    }
    let seed = u64::from_le_bytes(platform::read(context + 0x10)?);
    let target = crate::destination::from_seed(seed)?;
    let text = platform::pointer(platform::pointer(event + 0x418)?)?;
    let previous = platform::read::<17>(text)?;
    if previous[16] != 0 || !previous[..16].iter().all(u8::is_ascii_hexdigit) {
        return Err("Search Probes destination buffer is invalid");
    }
    unsafe { platform::write(text, &crate::destination::encode(target)) }?;
    Ok(Selection { event, target })
}

unsafe extern "system" fn scan_detour(
    manager: usize,
    result: usize,
    table: i32,
    event: usize,
    callback: usize,
    update_callback: bool,
    delay: f32,
    context: usize,
    output_address: usize,
    allow_fallback: bool,
    dry_run: bool,
    pending: bool,
    position: usize,
    force: bool,
) -> usize {
    let selection = if table == 3 && owned(event) {
        match select(event, context) {
            Ok(selection) => Some(selection),
            Err(error) => {
                fail(error);
                let mut failed = [0; 12];
                failed[8..].copy_from_slice(&3u32.to_le_bytes());
                if let Err(error) = unsafe { platform::write(result, &failed) } {
                    fail(error);
                }
                return result;
            }
        }
    } else {
        None
    };
    let _guard = SelectionGuard(SELECTION.replace(selection));
    let original =
        unsafe { std::mem::transmute::<usize, Scan>(ORIGINAL_SCAN.load(Ordering::Acquire)) };
    let returned = unsafe {
        original(
            manager,
            result,
            table,
            event,
            callback,
            update_callback,
            delay,
            context,
            output_address,
            allow_fallback,
            dry_run,
            pending,
            position,
            force,
        )
    };
    if let Some(selection) = selection {
        if !dry_run && matches!(platform::u32_at(result + 8), Ok(0 | 1)) {
            PUBLISHED.store(selection.target.raw(), Ordering::Release);
        }
    }
    returned
}

unsafe fn planet_address(planet: usize) -> Result<Address, &'static str> {
    platform::read::<4>(planet + 0x50)?;
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
    let selection = if owned(event) {
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
            if actual != Ok(selection.target) {
                fail(
                    "NMS could not place a marker on the selected planet. Abandon Search Probe Target in the Log before retrying.",
                );
                return 0;
            }
        }
    }
    building
}

unsafe extern "system" fn filter_detour(
    planet: usize,
    parameters: usize,
    allow_dead: bool,
    class: u8,
) -> bool {
    if let Some(selection) = SELECTION
        .get()
        .filter(|selection| parameters == selection.event || parameters == selection.event + 0xc0)
    {
        return match unsafe { planet_address(planet) } {
            Ok(address) => address == selection.target,
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
/// Exact-build scan arguments include the native output buffer and mission context. The seed owns
/// the destination across saves; scoped local filtering returns only native-owned building records.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut hooks = HOOKS.lock().map_err(|_| HookError::StatePoisoned)?;
    if hooks.is_empty() {
        let definitions: [(usize, usize, &[u8], &AtomicUsize); 3] = [
            (
                0x12d40e0,
                scan_detour as *const () as usize,
                &[
                    0x44, 0x89, 0x44, 0x24, 0x18, 0x48, 0x89, 0x4c, 0x24, 0x08, 0x55, 0x56, 0x57,
                    0x41, 0x54, 0x41, 0x55,
                ],
                &ORIGINAL_SCAN,
            ),
            (
                0x12db840,
                building_detour as *const () as usize,
                &[
                    0x44, 0x88, 0x4c, 0x24, 0x20, 0x4c, 0x89, 0x44, 0x24, 0x18, 0x48, 0x89, 0x54,
                    0x24, 0x10,
                ],
                &ORIGINAL_BUILDING,
            ),
            (
                0x12de8a0,
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
        PLANET_ADDRESS.store(base + 0x166b2d0, Ordering::Release);
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

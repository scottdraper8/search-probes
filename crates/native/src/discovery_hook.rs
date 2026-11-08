use crate::resident_hook::{HookError, ResidentHook};
use search_probes_model::address::Address;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const SUBMIT_RVA: usize = 0x4677d0;
const SUBMIT_PREFIX: [u8; 20] = [
    0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x48,
    0x89, 0x7c, 0x24, 0x20,
];
const ANIMAL_DISCOVERY: u32 = 3;
const MAX_PENDING: usize = 64;

type Submit = unsafe extern "system" fn(usize, *const u8, *mut bool, bool) -> bool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Discovery {
    pub address: Address,
    pub identity: [u64; 2],
}

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
static PENDING: Mutex<Vec<Discovery>> = Mutex::new(Vec::new());
static ERROR: Mutex<Option<&'static str>> = Mutex::new(None);

pub fn reset() {
    PENDING
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    *ERROR.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

pub fn take() -> Result<Vec<Discovery>, &'static str> {
    if let Some(error) = ERROR
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        return Err(error);
    }
    Ok(std::mem::take(
        &mut *PENDING.lock().unwrap_or_else(|error| error.into_inner()),
    ))
}

unsafe extern "system" fn detour(
    manager: usize,
    data: *const u8,
    submitted: *mut bool,
    synchronize: bool,
) -> bool {
    let original =
        unsafe { std::mem::transmute::<usize, Submit>(ORIGINAL.load(Ordering::Acquire)) };
    let inserted = unsafe { original(manager, data, submitted, synchronize) };
    let accepted = inserted || (!submitted.is_null() && unsafe { submitted.read() });
    if !accepted || data.is_null() {
        return inserted;
    }
    let kind = unsafe { data.add(0x38).cast::<u32>().read_unaligned() };
    let count = unsafe { data.add(0x30).cast::<u32>().read_unaligned() };
    if kind != ANIMAL_DISCOVERY || !(4..=5).contains(&count) {
        return inserted;
    }
    let raw_address = unsafe { data.cast::<u64>().read_unaligned() };
    let identity = [
        unsafe { data.add(0x18).cast::<u64>().read_unaligned() },
        unsafe { data.add(0x20).cast::<u64>().read_unaligned() },
    ];
    let Ok(address) = Address::from_raw(raw_address) else {
        return inserted;
    };
    let mut pending = PENDING.lock().unwrap_or_else(|error| error.into_inner());
    if pending.len() == MAX_PENDING {
        *ERROR.lock().unwrap_or_else(|error| error.into_inner()) = Some(
            "too many creature discoveries were submitted before navigation could process them",
        );
    } else {
        pending.push(Discovery { address, identity });
    }
    inserted
}

/// # Safety
/// The verified NMS image remains loaded and the hook stays resident for process lifetime.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut hook = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if hook.is_none() {
        let prepared = unsafe {
            ResidentHook::prepare(
                base + SUBMIT_RVA,
                detour as *const () as usize,
                &SUBMIT_PREFIX,
            )?
        };
        ORIGINAL.store(prepared.trampoline(), Ordering::Release);
        *hook = Some(prepared);
    }
    unsafe {
        hook.as_mut()
            .expect("prepared discovery hook")
            .set_enabled(true)
    }
}

/// # Safety
/// All discovery submission calls must have returned before disabling the hook.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut hook = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = hook.as_mut() {
        unsafe { hook.set_enabled(false)? };
    }
    reset();
    Ok(())
}

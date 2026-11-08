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
const DISCOVERY_DATA_BYTES: usize = 0x40;
const ANIMAL_DISCOVERY: u32 = 3;
const MAX_LOCAL_IDS: usize = 5;
const MAX_PENDING: usize = 64;

type Submit = unsafe extern "system" fn(usize, *const u8, *mut bool, bool) -> bool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Discovery {
    pub address: Address,
    pub local_ids: [u64; MAX_LOCAL_IDS],
    pub count: u8,
}

impl Discovery {
    pub fn first_local_id(&self) -> Option<u64> {
        (self.count != 0).then_some(self.local_ids[0])
    }
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

fn decode(data: &[u8; DISCOVERY_DATA_BYTES]) -> Option<Discovery> {
    let kind = u32::from_le_bytes(data[0x38..0x3c].try_into().unwrap());
    let count = u32::from_le_bytes(data[0x30..0x34].try_into().unwrap());
    if kind != ANIMAL_DISCOVERY || !(4..=MAX_LOCAL_IDS as u32).contains(&count) {
        return None;
    }
    let address = Address::from_raw(u64::from_le_bytes(data[..8].try_into().unwrap())).ok()?;
    let mut local_ids = [0; MAX_LOCAL_IDS];
    for (index, local_id) in local_ids.iter_mut().enumerate().take(count as usize) {
        let start = 8 + index * 8;
        *local_id = u64::from_le_bytes(data[start..start + 8].try_into().unwrap());
    }
    Some(Discovery {
        address,
        local_ids,
        count: count as u8,
    })
}

fn retain(discovery: Discovery) {
    let mut pending = PENDING.lock().unwrap_or_else(|error| error.into_inner());
    if pending.len() == MAX_PENDING {
        *ERROR.lock().unwrap_or_else(|error| error.into_inner()) =
            Some("too many creature scans were completed before navigation could process them");
    } else {
        pending.push(discovery);
    }
}

unsafe extern "system" fn detour(
    manager: usize,
    data: *const u8,
    submitted: *mut bool,
    synchronize: bool,
) -> bool {
    let original =
        unsafe { std::mem::transmute::<usize, Submit>(ORIGINAL.load(Ordering::Acquire)) };
    let snapshot = (!data.is_null()).then(|| {
        let mut snapshot = [0; DISCOVERY_DATA_BYTES];
        unsafe { std::ptr::copy_nonoverlapping(data, snapshot.as_mut_ptr(), snapshot.len()) };
        snapshot
    });
    let inserted = unsafe { original(manager, data, submitted, synchronize) };
    let accepted = inserted || (!submitted.is_null() && unsafe { submitted.read() });
    if accepted
        && let Some(snapshot) = snapshot
        && let Some(discovery) = decode(&snapshot)
    {
        retain(discovery);
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

#[cfg(test)]
mod tests {
    use super::*;
    use search_probes_model::address::Coordinates;

    fn address() -> Address {
        Address::new(Coordinates {
            x: 1,
            y: 2,
            z: 3,
            galaxy: 0,
            system: 4,
            planet: 1,
        })
        .unwrap()
    }

    #[test]
    fn animal_scan_retains_every_local_identity_component() {
        let mut data = [0; DISCOVERY_DATA_BYTES];
        data[..8].copy_from_slice(&address().raw().to_le_bytes());
        for (index, value) in [11_u64, 22, 33, 44, 55].into_iter().enumerate() {
            let start = 8 + index * 8;
            data[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
        data[0x30..0x34].copy_from_slice(&5_u32.to_le_bytes());
        data[0x38..0x3c].copy_from_slice(&ANIMAL_DISCOVERY.to_le_bytes());

        let discovery = decode(&data).unwrap();
        assert_eq!(discovery.address, address());
        assert_eq!(discovery.local_ids, [11, 22, 33, 44, 55]);
        assert_eq!(discovery.first_local_id(), Some(11));
    }

    #[test]
    fn non_animal_submission_is_not_a_creature_scan() {
        let mut data = [0; DISCOVERY_DATA_BYTES];
        data[..8].copy_from_slice(&address().raw().to_le_bytes());
        data[0x30..0x34].copy_from_slice(&4_u32.to_le_bytes());
        data[0x38..0x3c].copy_from_slice(&2_u32.to_le_bytes());
        assert_eq!(decode(&data), None);
    }
}

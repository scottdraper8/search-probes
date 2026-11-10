use crate::{
    lifecycle_hook, platform,
    resident_hook::{HookError, ResidentHook},
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

type StartMission = unsafe extern "system" fn(usize, usize, usize, bool, bool) -> bool;
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
static PENDING: Mutex<Option<Request>> = Mutex::new(None);
static OVERFLOW: AtomicBool = AtomicBool::new(false);
const PREFIX: [u8; 26] = [
    0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x55, 0x57, 0x41, 0x56, 0x48, 0x8d,
    0x6c, 0x24, 0xc1, 0x48, 0x81, 0xec, 0x00, 0x01, 0x00, 0x00,
];
const REALITY_MANAGER: usize = 0x60;
const WIKI: usize = 0x248;
const CATEGORY_STRIDE: usize = 0xa0;
const CATEGORY_TOPICS: usize = 0x80;
const TOPIC_STRIDE: usize = 0xb8;
const TOPIC_ID: usize = 0x40;
const TOPIC_MISSION: usize = 0x90;
const CATEGORY_ID: &[u8] = b"SQN_SP_CATEGORY";

pub struct Topics {
    count: usize,
    original: u32,
}

fn fixed<const N: usize>(address: usize, expected: &[u8]) -> Result<bool, &'static str> {
    if expected.is_empty() || expected.len() >= N {
        return Err("invalid Guide identity");
    }
    let value = platform::read::<N>(address)?;
    Ok(&value[..expected.len()] == expected
        && value[expected.len()] == 0
        && value[expected.len() + 1..].iter().all(|byte| *byte == 0))
}

fn array(address: usize, maximum: usize) -> Result<(usize, usize, usize), &'static str> {
    let capacity = platform::u32_at(address)? as usize;
    let count = platform::u32_at(address + 4)? as usize;
    if count > capacity || capacity > maximum {
        return Err("unsupported Guide collection");
    }
    let pointer = if capacity == 0 {
        0
    } else {
        platform::pointer(address + 8)?
    };
    Ok((pointer, count, capacity))
}

impl Topics {
    /// # Safety
    /// The verified gameplay snapshot owns the loaded Wiki data for this callback.
    pub unsafe fn locate(application: usize) -> Result<Self, &'static str> {
        let wiki = platform::pointer(application + REALITY_MANAGER + WIKI)?;
        let (categories, count, _) = array(wiki, 128)?;
        let mut category = None;
        for index in 0..count {
            let candidate = categories + index * CATEGORY_STRIDE;
            if fixed::<32>(candidate, CATEGORY_ID)? && category.replace(candidate).is_some() {
                return Err("Search Probes Guide category is ambiguous");
            }
        }
        let category = category.ok_or("Search Probes Guide category is unavailable")?;
        let topics = category + CATEGORY_TOPICS;
        let (pointer, original, capacity) = array(topics, 128)?;
        if capacity < search_probes_model::settings::MAX_PRESETS
            || original > search_probes_model::settings::MAX_PRESETS
        {
            return Err("Search Probes Guide slots are unavailable");
        }
        for slot in 0..search_probes_model::settings::MAX_PRESETS {
            let topic = pointer + slot * TOPIC_STRIDE;
            let topic_id = search_probes_model::guide::topic(slot)?;
            let mission = search_probes_model::guide::mission(slot)?;
            if !fixed::<32>(topic + TOPIC_ID, topic_id.as_bytes())?
                || !fixed::<16>(topic + TOPIC_MISSION, mission.as_bytes())?
            {
                return Err("Search Probes Guide slots are not in authored order");
            }
        }
        Ok(Self {
            count: topics + 4,
            original: original as u32,
        })
    }

    /// # Safety
    /// The loaded Wiki data remains writable for the current gameplay callback.
    pub unsafe fn show(&self, presets: usize) -> Result<(), &'static str> {
        if presets > search_probes_model::settings::MAX_PRESETS {
            return Err("too many Guide presets");
        }
        if platform::u32_at(self.count)? != presets as u32 {
            unsafe { platform::write(self.count, &(presets as u32).to_le_bytes()) }?;
        }
        Ok(())
    }

    /// Restore the authored vector length before NMS releases or replaces its Wiki allocation.
    ///
    /// # Safety
    /// Dispatch is closed and the application transition has not released gameplay data yet.
    pub unsafe fn restore(self) -> Result<(), &'static str> {
        if platform::u32_at(self.count)? != self.original {
            unsafe { platform::write(self.count, &self.original.to_le_bytes()) }?;
        }
        Ok(())
    }
}

pub struct Request {
    pub slot: usize,
    pub epoch: u64,
}

pub fn take() -> (Option<Request>, bool) {
    (
        PENDING
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take(),
        OVERFLOW.swap(false, Ordering::AcqRel),
    )
}

unsafe extern "system" fn detour(
    manager: usize,
    mission: usize,
    seed: usize,
    restart: bool,
    selected: bool,
) -> bool {
    if let Ok(name) = platform::read::<16>(mission) {
        if let Some(slot) = search_probes_model::guide::mission_slot(&name) {
            let mut pending = PENDING.lock().unwrap_or_else(|error| error.into_inner());
            if pending.is_none() {
                *pending = Some(Request {
                    slot,
                    epoch: lifecycle_hook::epoch(),
                });
            } else {
                OVERFLOW.store(true, Ordering::Release);
            }
            return true;
        }
    }
    let original =
        unsafe { std::mem::transmute::<usize, StartMission>(ORIGINAL.load(Ordering::Acquire)) };
    unsafe { original(manager, mission, seed, restart, selected) }
}

/// # Safety
/// Only exact preset mission IDs are consumed.
pub unsafe fn enable(base: usize) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        let hook = unsafe {
            ResidentHook::prepare(base + 0x9cebb0, detour as *const () as usize, &PREFIX)
        }?;
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
        *state = Some(hook);
    }
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(true) }?;
    }
    Ok(())
}

/// # Safety
/// No request may be dispatched after the runtime has stopped.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(false) }?;
    }
    take();
    Ok(())
}

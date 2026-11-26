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
const CATEGORY_TYPE: usize = 0x90;
const CATEGORY_UNLOCKED_COUNT: usize = 0x94;
const CATEGORY_UNSEEN_COUNT: usize = 0x98;
const GUIDE_TYPE: u32 = 20;
const ARRAY_LAYOUT_TAG: u8 = 1;
const TOPIC_STRIDE: usize = 0xb8;
const TOPIC_ID: usize = 0x40;
const TOPIC_MISSION: usize = 0x90;
const TOPIC_SEEN: usize = 0xb4;
const TOPIC_UNLOCKED: usize = 0xb5;
const CATEGORY_ID: &[u8] = b"SQN_SP_CATEGORY";
const MAX_PRESETS: usize = search_probes_model::settings::MAX_PRESETS;

pub struct Topics {
    seen: [usize; MAX_PRESETS],
    unlocked: [usize; MAX_PRESETS],
    original_unlocked: [u8; MAX_PRESETS],
    unlocked_count: usize,
    unseen_count: usize,
    original_unlocked_count: u32,
    original_unseen_count: u32,
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

fn flag(address: usize) -> Result<u8, &'static str> {
    let value = platform::read::<1>(address)?[0];
    if value > 1 {
        return Err("unsupported Guide flag");
    }
    Ok(value)
}

fn array(address: usize, maximum: usize) -> Result<(usize, usize), &'static str> {
    let count = platform::u32_at(address + 8)? as usize;
    // cTkDynamicArray ends with one allocation byte, one layout byte, then two padding bytes.
    // Both allocation states are valid: resource-backed Wiki arrays set the first byte while a
    // heap copy clears it. Resource data may also poison the two padding bytes with 0xaa.
    let metadata = platform::read::<4>(address + 12)?;
    if !supported_array_metadata(count, maximum, metadata) {
        return Err("unsupported Guide collection");
    }
    let pointer = if count == 0 {
        0
    } else {
        platform::pointer(address)?
    };
    Ok((pointer, count))
}

fn supported_array_metadata(count: usize, maximum: usize, metadata: [u8; 4]) -> bool {
    count <= maximum && metadata[0] <= 1 && metadata[1] == ARRAY_LAYOUT_TAG
}

impl Topics {
    /// # Safety
    /// The verified gameplay snapshot owns the loaded Wiki data for this callback.
    pub unsafe fn locate(application: usize) -> Result<Self, &'static str> {
        let wiki = platform::pointer(application + REALITY_MANAGER + WIKI)?;
        let (categories, count) = array(wiki, 128)?;
        let mut category = None;
        for index in 0..count {
            let candidate = categories + index * CATEGORY_STRIDE;
            if fixed::<32>(candidate, CATEGORY_ID)? && category.replace(candidate).is_some() {
                return Err("Search Probes Guide category is ambiguous");
            }
        }
        let category = category.ok_or("Search Probes Guide category is unavailable")?;
        if platform::u32_at(category + CATEGORY_TYPE)? != GUIDE_TYPE {
            return Err("Search Probes Guide category has the wrong type");
        }
        let topics = category + CATEGORY_TOPICS;
        let (pointer, count) = array(topics, 128)?;
        if count != MAX_PRESETS {
            return Err("Search Probes Guide slots are unavailable");
        }
        let mut seen = [0; MAX_PRESETS];
        let mut unlocked = [0; MAX_PRESETS];
        let mut original_unlocked = [0; MAX_PRESETS];
        for slot in 0..MAX_PRESETS {
            let topic = pointer + slot * TOPIC_STRIDE;
            let topic_id = search_probes_model::guide::topic(slot)?;
            let mission = search_probes_model::guide::mission(slot)?;
            if !fixed::<32>(topic + TOPIC_ID, topic_id.as_bytes())?
                || !fixed::<16>(topic + TOPIC_MISSION, mission.as_bytes())?
            {
                return Err("Search Probes Guide slots are not in authored order");
            }
            seen[slot] = topic + TOPIC_SEEN;
            flag(seen[slot])?;
            unlocked[slot] = topic + TOPIC_UNLOCKED;
            original_unlocked[slot] = flag(unlocked[slot])?;
        }
        let original_unlocked_count = platform::u32_at(category + CATEGORY_UNLOCKED_COUNT)?;
        let original_unseen_count = platform::u32_at(category + CATEGORY_UNSEEN_COUNT)?;
        if original_unlocked_count as usize > count
            || original_unseen_count > original_unlocked_count
        {
            return Err("unsupported Search Probes Guide counters");
        }
        Ok(Self {
            seen,
            unlocked,
            original_unlocked,
            unlocked_count: category + CATEGORY_UNLOCKED_COUNT,
            unseen_count: category + CATEGORY_UNSEEN_COUNT,
            original_unlocked_count,
            original_unseen_count,
        })
    }

    /// # Safety
    /// The loaded Wiki data remains writable for the current gameplay callback.
    pub unsafe fn sync(&self, presets: usize) -> Result<(), &'static str> {
        if presets > MAX_PRESETS {
            return Err("too many Guide presets");
        }
        let mut unseen = 0u32;
        for (slot, (seen, unlocked)) in self
            .seen
            .iter()
            .copied()
            .zip(self.unlocked.iter().copied())
            .enumerate()
        {
            let desired = u8::from(slot < presets);
            if flag(unlocked)? != desired {
                unsafe { platform::write(unlocked, &[desired]) }?;
            }
            if desired != 0 && flag(seen)? == 0 {
                unseen += 1;
            }
        }
        let presets = presets as u32;
        if platform::u32_at(self.unlocked_count)? != presets {
            unsafe { platform::write(self.unlocked_count, &presets.to_le_bytes()) }?;
        }
        if platform::u32_at(self.unseen_count)? != unseen {
            unsafe { platform::write(self.unseen_count, &unseen.to_le_bytes()) }?;
        }
        Ok(())
    }

    /// Restore the authored lock state before NMS releases or replaces its Wiki allocation.
    ///
    /// # Safety
    /// Dispatch is closed and the application transition has not released gameplay data yet.
    pub unsafe fn restore(self) -> Result<(), &'static str> {
        for (address, original) in self.unlocked.into_iter().zip(self.original_unlocked) {
            if flag(address)? != original {
                unsafe { platform::write(address, &[original]) }?;
            }
        }
        if platform::u32_at(self.unlocked_count)? != self.original_unlocked_count {
            unsafe {
                platform::write(
                    self.unlocked_count,
                    &self.original_unlocked_count.to_le_bytes(),
                )
            }?;
        }
        if platform::u32_at(self.unseen_count)? != self.original_unseen_count {
            unsafe {
                platform::write(self.unseen_count, &self.original_unseen_count.to_le_bytes())
            }?;
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
    if let Ok(name) = platform::read::<16>(mission)
        && let Some(slot) = search_probes_model::guide::mission_slot(&name)
    {
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
            ResidentHook::prepare(base + 0x9cff30, detour as *const () as usize, &PREFIX)
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

#[cfg(test)]
mod tests {
    use super::supported_array_metadata;

    #[test]
    fn guide_arrays_accept_heap_and_resource_allocation_states() {
        assert!(supported_array_metadata(9, 128, [0, 1, 0, 0]));
        assert!(supported_array_metadata(9, 128, [1, 1, 0xaa, 0xaa]));
    }

    #[test]
    fn guide_arrays_reject_invalid_metadata_and_counts() {
        assert!(!supported_array_metadata(129, 128, [0, 1, 0, 0]));
        assert!(!supported_array_metadata(9, 128, [2, 1, 0, 0]));
        assert!(!supported_array_metadata(9, 128, [0, 0, 0, 0]));
    }
}

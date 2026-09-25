use crate::resident_hook::{HookError, ResidentHook};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

type Update = unsafe extern "system" fn(*mut u8, f32);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static BASE: AtomicUsize = AtomicUsize::new(0);
static THREAD: AtomicU32 = AtomicU32::new(0);
static OWNER_EPOCH: AtomicU64 = AtomicU64::new(0);
static UNEXPECTED_THREAD: AtomicU32 = AtomicU32::new(0);
static BUSY: AtomicBool = AtomicBool::new(false);
static FAILED: AtomicU8 = AtomicU8::new(0);
static CALLBACK: OnceLock<fn()> = OnceLock::new();
static HOOK: Mutex<Option<ResidentHook>> = Mutex::new(None);
const PREFIX: [u8; 18] = [
    0xf3, 0x0f, 0x11, 0x4c, 0x24, 0x10, 0x4c, 0x8b, 0xdc, 0x55, 0x56, 0x49, 0x8d, 0xab, 0xf8, 0xfe,
    0xff, 0xff,
];

pub fn failure() -> Option<String> {
    match FAILED.load(Ordering::Acquire) {
        0 => None,
        1 => Some(format!(
            "Gameplay changed from thread {} to {} without a game-session transition. Restart the game.",
            THREAD.load(Ordering::Acquire),
            UNEXPECTED_THREAD.load(Ordering::Acquire)
        )),
        _ => {
            Some("The gameplay callback encountered an internal failure. Restart the game.".into())
        }
    }
}
pub fn thread_id() -> u32 {
    THREAD.load(Ordering::Acquire)
}

unsafe extern "system" fn detour(communicator: *mut u8, timestep: f32) {
    let original =
        unsafe { std::mem::transmute::<usize, Update>(ORIGINAL.load(Ordering::Acquire)) };
    unsafe {
        original(communicator, timestep);
    }
    if FAILED.load(Ordering::Acquire) != 0 {
        return;
    }
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let thread = unsafe { GetCurrentThreadId() };
    let epoch = crate::lifecycle_hook::epoch();
    if OWNER_EPOCH.load(Ordering::Acquire) != epoch {
        THREAD.store(thread, Ordering::Release);
        OWNER_EPOCH.store(epoch, Ordering::Release);
    } else if THREAD.load(Ordering::Acquire) != thread {
        UNEXPECTED_THREAD.store(thread, Ordering::Release);
        FAILED.store(1, Ordering::Release);
        BUSY.store(false, Ordering::Release);
        return;
    }
    if let Some(callback) = CALLBACK.get() {
        if catch_unwind(AssertUnwindSafe(*callback)).is_err() {
            FAILED.store(2, Ordering::Release);
        }
    }
    BUSY.store(false, Ordering::Release);
}

/// # Safety
/// The verified gameplay entry must be unpatched. The callback may call native search only within
/// this hook; all borrowed worker contexts must have joined and dropped before it returns.
pub unsafe fn enable(base: usize, callback: fn()) -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if state.is_none() {
        let hook = unsafe {
            ResidentHook::prepare(base + 0x13eae90, detour as *const () as usize, &PREFIX)
        }?;
        if CALLBACK.set(callback).is_err() {
            return Err(HookError::StatePoisoned);
        }
        ORIGINAL.store(hook.trampoline(), Ordering::Release);
        BASE.store(base, Ordering::Release);
        *state = Some(hook);
    }
    if BASE.load(Ordering::Acquire) != base
        || CALLBACK
            .get()
            .is_none_or(|existing| *existing as usize != callback as usize)
    {
        return Err(HookError::PrefixChanged);
    }
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(true) }?;
    }
    Ok(())
}

/// # Safety
/// Stop and join every dispatched search batch before disabling the gameplay hook.
pub unsafe fn disable() -> Result<(), HookError> {
    let mut state = HOOK.lock().map_err(|_| HookError::StatePoisoned)?;
    if let Some(hook) = state.as_mut() {
        unsafe { hook.set_enabled(false) }?;
    }
    Ok(())
}

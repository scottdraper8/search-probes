use crate::resident_hook::ResidentHook;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    Memory::{
        MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_READWRITE, VirtualAlloc, VirtualProtect,
    },
    Threading::GetCurrentProcess,
};

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn detour() -> u32 {
    let original = unsafe {
        std::mem::transmute::<usize, unsafe extern "system" fn() -> u32>(
            ORIGINAL.load(Ordering::Acquire),
        )
    };
    unsafe { original() + 1 }
}

/// # Safety
/// Run once per investigation module, outside DllMain; output must hold four writable u32 values.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_hook_self_test(output: *mut u32) -> i32 {
    if output.is_null() {
        return -1;
    }
    let allocation = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            4096,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if allocation.is_null() {
        return -2;
    }
    let mut code = [0x90u8; 15];
    code[..5].copy_from_slice(&[0xb8, 42, 0, 0, 0]);
    code[14] = 0xc3;
    let mut protection = 0;
    unsafe {
        std::ptr::copy_nonoverlapping(code.as_ptr(), allocation.cast::<u8>(), code.len());
        if VirtualProtect(allocation, 4096, PAGE_EXECUTE_READ, &mut protection) == 0 {
            return -3;
        }
        if FlushInstructionCache(GetCurrentProcess(), allocation, code.len()) == 0 {
            return -4;
        }
    }
    let function = unsafe {
        std::mem::transmute::<usize, unsafe extern "system" fn() -> u32>(allocation as usize)
    };
    unsafe {
        output.write(function());
    }
    let Ok(mut hook) = (unsafe {
        ResidentHook::prepare(
            allocation as usize,
            detour as *const () as usize,
            &code[..14],
        )
    }) else {
        return -5;
    };
    ORIGINAL.store(hook.trampoline(), Ordering::Release);
    if unsafe { hook.set_enabled(true) }.is_err() {
        return -6;
    }
    unsafe {
        output.add(1).write(function());
    }
    let worker = std::thread::spawn(move || unsafe { function() });
    let Ok(value) = worker.join() else {
        return -7;
    };
    unsafe {
        output.add(2).write(value);
    }
    if unsafe { hook.set_enabled(false) }.is_err() {
        return -8;
    }
    unsafe {
        output.add(3).write(function());
    }
    if unsafe { std::slice::from_raw_parts(allocation.cast::<u8>(), code.len()) } != code {
        return -9;
    }
    0
}

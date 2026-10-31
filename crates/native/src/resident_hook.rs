use crate::{
    hook_code::{self, CodeError},
    hook_threads::{FrozenThreads, ThreadError},
};
use std::ffi::c_void;
use windows_sys::Win32::{
    Foundation::GetLastError,
    System::{
        Diagnostics::Debug::FlushInstructionCache,
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW,
        },
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
            PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect,
        },
        Threading::GetCurrentProcess,
    },
};

#[derive(Debug)]
pub enum HookError {
    Code(CodeError),
    Thread(ThreadError),
    PrefixChanged,
    StatePoisoned,
    Allocation(u32),
    Pin(u32),
    Protection(u32),
    Flush(u32),
    Capacity,
}

pub struct ResidentHook {
    target: usize,
    original: Vec<u8>,
    patched: Vec<u8>,
    trampoline: usize,
    enabled: bool,
}

impl ResidentHook {
    /// # Safety
    /// Target and destination must belong to loaded modules. Prefix covers complete straight-line
    /// instructions with no other entry points. This hook lives until process exit.
    pub unsafe fn prepare(
        target: usize,
        destination: usize,
        prefix: &[u8],
    ) -> Result<Self, HookError> {
        if !(hook_code::JUMP_SIZE..=64).contains(&prefix.len()) {
            return Err(HookError::Code(CodeError::PrefixLength));
        }
        if unsafe { std::slice::from_raw_parts(target as *const u8, prefix.len()) } != prefix {
            return Err(HookError::PrefixChanged);
        }
        let mut module = std::ptr::null_mut();
        if unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                destination as *const u16,
                &mut module,
            )
        } == 0
        {
            return Err(HookError::Pin(unsafe { GetLastError() }));
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
            return Err(HookError::Allocation(unsafe { GetLastError() }));
        }
        let prepared = (|| {
            let code = hook_code::relocate_prefix(target as u64, prefix, allocation as u64)
                .map_err(HookError::Code)?;
            if code.len() > 4096 {
                return Err(HookError::Capacity);
            }
            let mut old = 0;
            unsafe {
                std::ptr::copy_nonoverlapping(code.as_ptr(), allocation.cast::<u8>(), code.len());
            }
            if unsafe { VirtualProtect(allocation, 4096, PAGE_EXECUTE_READ, &mut old) } == 0 {
                return Err(HookError::Protection(unsafe { GetLastError() }));
            }
            if unsafe { FlushInstructionCache(GetCurrentProcess(), allocation, code.len()) } == 0 {
                return Err(HookError::Flush(unsafe { GetLastError() }));
            }
            let mut patched = vec![0x90; prefix.len()];
            patched[..hook_code::JUMP_SIZE]
                .copy_from_slice(&hook_code::absolute_jump(destination as u64));
            Ok(Self {
                target,
                original: prefix.to_vec(),
                patched,
                trampoline: allocation as usize,
                enabled: false,
            })
        })();
        if prepared.is_err() {
            unsafe {
                VirtualFree(allocation, 0, MEM_RELEASE);
            }
        }
        prepared
    }

    pub fn trampoline(&self) -> usize {
        self.trampoline
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// # Safety
    /// Target/module lifetimes and exclusive hook ownership must still hold. Query workers must
    /// be joined before disabling; trampolines remain allocated even after disable.
    pub unsafe fn set_enabled(&mut self, enabled: bool) -> Result<(), HookError> {
        if self.enabled == enabled {
            return Ok(());
        }
        let expected = if enabled {
            &self.original
        } else {
            &self.patched
        };
        let replacement = if enabled {
            &self.patched
        } else {
            &self.original
        };
        let frozen =
            FrozenThreads::outside(self.target, expected.len()).map_err(HookError::Thread)?;
        if unsafe { std::slice::from_raw_parts(self.target as *const u8, expected.len()) }
            != expected
        {
            return Err(HookError::PrefixChanged);
        }
        let mut protection = 0;
        let target = self.target as *mut c_void;
        if unsafe {
            VirtualProtect(
                target,
                replacement.len(),
                PAGE_EXECUTE_READWRITE,
                &mut protection,
            )
        } == 0
        {
            return Err(HookError::Protection(unsafe { GetLastError() }));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                replacement.as_ptr(),
                target.cast::<u8>(),
                replacement.len(),
            );
        }
        self.enabled = enabled;
        let mut failure = None;
        if unsafe { FlushInstructionCache(GetCurrentProcess(), target, replacement.len()) } == 0 {
            failure = Some(HookError::Flush(unsafe { GetLastError() }));
            unsafe {
                std::ptr::copy_nonoverlapping(
                    expected.as_ptr(),
                    target.cast::<u8>(),
                    expected.len(),
                );
                FlushInstructionCache(GetCurrentProcess(), target, expected.len());
            }
            self.enabled = !enabled;
        }
        let mut discarded = 0;
        if unsafe { VirtualProtect(target, replacement.len(), protection, &mut discarded) } == 0 {
            failure = Some(HookError::Protection(unsafe { GetLastError() }));
        }
        frozen.resume().map_err(HookError::Thread)?;
        failure.map_or(Ok(()), Err)
    }
}

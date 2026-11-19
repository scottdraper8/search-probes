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
            MEM_COMMIT, MEM_FREE, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION,
            PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
            VirtualProtect, VirtualQuery,
        },
        SystemInformation::{GetSystemInfo, SYSTEM_INFO},
        Threading::GetCurrentProcess,
    },
};

const TRAMPOLINE_CAPACITY: usize = 4096;

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|value| value / alignment * alignment)
}

fn align_down(value: usize, alignment: usize) -> usize {
    value / alignment * alignment
}

/// Return free allocation-granularity addresses within rel32 reach, nearest first. Trying every
/// suitable region lets the instruction relocator choose a trampoline that can still address each
/// RIP-relative operand in the verified prefix.
unsafe fn nearby_allocations(target: usize) -> Vec<usize> {
    let mut system = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut system) };
    let granularity = system.dwAllocationGranularity as usize;
    if granularity == 0 {
        return Vec::new();
    }
    let reach = i32::MAX as usize - granularity;
    let minimum = (system.lpMinimumApplicationAddress as usize).max(target.saturating_sub(reach));
    let maximum = (system.lpMaximumApplicationAddress as usize)
        .saturating_add(1)
        .min(target.saturating_add(reach));
    let mut candidates = Vec::new();
    let mut cursor = minimum;
    while cursor < maximum {
        let mut region = MEMORY_BASIC_INFORMATION::default();
        if unsafe {
            VirtualQuery(
                cursor as *const c_void,
                &mut region,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        } == 0
        {
            break;
        }
        let base = region.BaseAddress as usize;
        let end = base.saturating_add(region.RegionSize);
        if region.State == MEM_FREE {
            let start = base.max(minimum);
            let end = end.min(maximum);
            if let Some(first) = align_up(start, granularity)
                && first
                    .checked_add(TRAMPOLINE_CAPACITY)
                    .is_some_and(|allocation_end| allocation_end <= end)
            {
                candidates.push(first);
                let last = align_down(end - TRAMPOLINE_CAPACITY, granularity);
                if last != first {
                    candidates.push(last);
                }
            }
        }
        let next = end.max(cursor.saturating_add(granularity));
        if next <= cursor {
            break;
        }
        cursor = next;
    }
    candidates.sort_unstable_by_key(|candidate| candidate.abs_diff(target));
    candidates
}

unsafe fn prepare_trampoline(target: usize, prefix: &[u8]) -> Result<(usize, Vec<u8>), HookError> {
    let mut allocation_error = None;
    let mut relocation_error = None;
    let mut candidates = unsafe { nearby_allocations(target) };
    // Preserve support for position-independent prefixes if the address space has no nearby hole.
    candidates.push(0);
    for candidate in candidates {
        let allocation = unsafe {
            VirtualAlloc(
                candidate as *const c_void,
                TRAMPOLINE_CAPACITY,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if allocation.is_null() {
            allocation_error = Some(unsafe { GetLastError() });
            continue;
        }
        let code = match hook_code::relocate_prefix(target as u64, prefix, allocation as u64) {
            Ok(code) => code,
            Err(CodeError::Relocation) => {
                relocation_error = Some(CodeError::Relocation);
                unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
                continue;
            }
            Err(error) => {
                unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
                return Err(HookError::Code(error));
            }
        };
        if code.len() > TRAMPOLINE_CAPACITY {
            unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
            return Err(HookError::Capacity);
        }
        return Ok((allocation as usize, code));
    }
    if let Some(error) = relocation_error {
        Err(HookError::Code(error))
    } else {
        Err(HookError::Allocation(allocation_error.unwrap_or(0)))
    }
}

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
        let (allocation, code) = unsafe { prepare_trampoline(target, prefix)? };
        let allocation = allocation as *mut c_void;
        let prepared = (|| {
            let mut old = 0;
            unsafe {
                std::ptr::copy_nonoverlapping(code.as_ptr(), allocation.cast::<u8>(), code.len());
            }
            if unsafe {
                VirtualProtect(allocation, TRAMPOLINE_CAPACITY, PAGE_EXECUTE_READ, &mut old)
            } == 0
            {
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

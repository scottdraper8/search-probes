#![deny(unsafe_op_in_unsafe_fn)]
#![cfg(windows)]

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::{arch::global_asm, ffi::CStr};
use std::{fs, os::windows::ffi::OsStrExt, path::PathBuf};
use windows_sys::Win32::{
    Foundation::{GetLastError, HMODULE},
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleHandleExW, GetModuleHandleW, GetProcAddress, LoadLibraryW,
        },
        Memory::{PAGE_READWRITE, VirtualProtect},
        SystemInformation::GetSystemDirectoryW,
        Threading::GetCurrentProcess,
    },
};

use search_probes_model::supported::{EXECUTABLE_BYTES, GAME_VERSION, STEAM_BUILD};

enum WinmmExport {
    Name(&'static CStr),
    Ordinal(u16),
}

include!(concat!(env!("OUT_DIR"), "/winmm_exports.rs"));
global_asm!(
    include_str!(concat!(env!("OUT_DIR"), "/winmm_stubs.s")),
    table = sym SP_WINMM_TABLE,
    resolver = sym sp_resolve_winmm,
    time_begin = sym search_probes_time_begin_period,
    time_end = sym search_probes_time_end_period,
    options(att_syntax)
);

type Period = unsafe extern "system" fn(u32) -> u32;
type ProcAddress = unsafe extern "system" fn(
    HMODULE,
    windows_sys::core::PCSTR,
) -> windows_sys::Win32::Foundation::FARPROC;
static WINMM_RESOLVED: OnceLock<()> = OnceLock::new();
static STARTUP_TRIGGER: OnceLock<Result<(), String>> = OnceLock::new();
static ORIGINAL_GET_PROC_ADDRESS: OnceLock<ProcAddress> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);

static SP_WINMM_TABLE: [AtomicUsize; WINMM_EXPORT_COUNT] =
    [const { AtomicUsize::new(0) }; WINMM_EXPORT_COUNT];

extern "system" fn sp_winmm_missing() -> usize {
    0
}

fn load_system_winmm() -> HMODULE {
    let mut proxy = std::ptr::null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    let located = unsafe {
        GetModuleHandleExW(
            flags,
            load_system_winmm as *const () as windows_sys::core::PCWSTR,
            &mut proxy,
        )
    };
    if located == 0 {
        return std::ptr::null_mut();
    }
    let mut path = [0_u16; 32_768];
    let length = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
    let suffix: Vec<_> = "\\winmm.dll\0".encode_utf16().collect();
    if length == 0 || length + suffix.len() > path.len() {
        return std::ptr::null_mut();
    }
    path[length..length + suffix.len()].copy_from_slice(&suffix);
    let real = unsafe { LoadLibraryW(path.as_ptr()) };
    if real == proxy {
        std::ptr::null_mut()
    } else {
        real
    }
}

fn resolve_winmm() {
    WINMM_RESOLVED.get_or_init(|| {
        let real = load_system_winmm();
        for (index, export) in WINMM_EXPORTS.iter().enumerate() {
            let address = if real.is_null() {
                None
            } else {
                let key = match export {
                    WinmmExport::Name(name) => name.as_ptr().cast(),
                    WinmmExport::Ordinal(ordinal) => *ordinal as usize as windows_sys::core::PCSTR,
                };
                unsafe { GetProcAddress(real, key) }
            };
            let address = address
                .map(|function| function as usize)
                .unwrap_or(sp_winmm_missing as *const () as usize);
            SP_WINMM_TABLE[index].store(address, Ordering::Release);
        }
    });
}

extern "C" fn sp_resolve_winmm() {
    resolve_winmm();
}

fn period(index: usize) -> Option<Period> {
    resolve_winmm();
    let address = SP_WINMM_TABLE[index].load(Ordering::Acquire);
    if address == sp_winmm_missing as *const () as usize {
        None
    } else {
        Some(unsafe { std::mem::transmute::<usize, Period>(address) })
    }
}

fn user_directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("SearchProbes"))
}

fn clear_startup_failure() {
    if let Some(directory) = user_directory() {
        let _ = fs::remove_file(directory.join("startup-error.txt"));
    }
}

fn report_startup_failure(error: &str) {
    let Some(directory) = user_directory() else {
        return;
    };
    let _ = fs::create_dir_all(&directory);
    let message: String = error.chars().take(4096).collect();
    let _ = fs::write(directory.join("startup-error.txt"), message);
}

fn executable_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("Cannot locate NMS.exe: {error}"))
}

fn preflight_executable(path: &PathBuf) -> Result<(), String> {
    if fs::metadata(path)
        .map_err(|error| format!("Cannot inspect NMS.exe: {error}"))?
        .len()
        != EXECUTABLE_BYTES
    {
        return Err(format!(
            "Search Probes requires No Man's Sky {GAME_VERSION} (Steam build {STEAM_BUILD})."
        ));
    }
    Ok(())
}

fn runtime_path(executable: &std::path::Path) -> Result<PathBuf, String> {
    let binaries = executable
        .parent()
        .ok_or("NMS.exe has no Binaries directory")?;
    Ok(binaries.join("SearchProbes").join("SearchProbes.dll"))
}

fn initialize() -> Result<(), String> {
    // This function is reached from NMS's first Vulkan lookup, after process-loader
    // initialization. Keep all filesystem and dynamic-library work on this deferred path.
    clear_startup_failure();
    let executable = executable_path()?;
    if executable
        .file_name()
        .is_none_or(|name| !name.to_string_lossy().eq_ignore_ascii_case("NMS.exe"))
    {
        return Ok(());
    }
    preflight_executable(&executable)?;
    let path = runtime_path(&executable)?;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let runtime = unsafe { LoadLibraryW(wide.as_ptr()) };
    if runtime.is_null() {
        return Err(format!(
            "Cannot load {} (Windows error {}).",
            path.display(),
            unsafe { GetLastError() }
        ));
    }
    let start = unsafe { GetProcAddress(runtime, c"SearchProbesStart".as_ptr().cast()) }
        .ok_or("SearchProbes.dll has no SearchProbesStart entry point")?;
    let start: unsafe extern "system" fn() =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, _>(start) };
    unsafe { start() };
    Ok(())
}

fn read<const N: usize>(address: usize) -> Result<[u8; N], String> {
    if address < 0x10000
        || address
            .checked_add(N)
            .is_none_or(|end| end > 0x0000800000000000)
    {
        return Err("native address outside user memory".into());
    }
    let mut bytes = [0; N];
    let mut copied = 0;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const _,
            bytes.as_mut_ptr().cast(),
            N,
            &mut copied,
        )
    };
    if ok == 0 || copied != N {
        return Err("native memory is unavailable".into());
    }
    Ok(bytes)
}

fn u32_at(address: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(read(address)?))
}

fn install_startup_trigger() -> Result<(), String> {
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    if base == 0 {
        return Err("NMS.exe is not mapped".into());
    }
    let pe = base + u32_at(base + 60)? as usize;
    let optional = pe + 24;
    let size = u32_at(optional + 56)? as usize;
    let import_rva = u32_at(optional + 120)? as usize;
    let import_size = u32_at(optional + 124)? as usize;
    let bounded = |rva: usize, length: usize| -> Result<usize, String> {
        if rva == 0 || rva.checked_add(length).is_none_or(|end| end > size) {
            return Err("invalid NMS import extent".into());
        }
        Ok(base + rva)
    };
    let mut found = None;
    for offset in (0..import_size).step_by(20) {
        let descriptor = bounded(import_rva + offset, 20)?;
        let original = u32_at(descriptor)? as usize;
        let name = u32_at(descriptor + 12)? as usize;
        let first = u32_at(descriptor + 16)? as usize;
        if name == 0 {
            break;
        }
        if original == 0 {
            return Err("NMS import names are unavailable".into());
        }
        for index in 0..4096 {
            let entry = u64::from_le_bytes(read(bounded(original + index * 8, 8)?)?);
            if entry == 0 {
                break;
            }
            if entry & (1 << 63) != 0 {
                continue;
            }
            let address = bounded(entry as usize + 2, 128)?;
            let bytes = read::<128>(address)?;
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .ok_or("unterminated NMS import name")?;
            if &bytes[..end] != b"GetProcAddress" {
                continue;
            }
            if found.is_some() {
                return Err("NMS imports GetProcAddress more than once".into());
            }
            found = Some(bounded(first + index * 8, 8)?);
        }
    }
    let slot = found.ok_or("NMS has no GetProcAddress import")?;
    if slot % 8 != 0 {
        return Err("unaligned NMS GetProcAddress import".into());
    }
    let original = u64::from_le_bytes(read::<8>(slot)?) as usize;
    let original: ProcAddress = unsafe { std::mem::transmute(original) };
    ORIGINAL_GET_PROC_ADDRESS
        .set(original)
        .map_err(|_| "NMS startup trigger is already installed")?;
    let mut old = 0;
    if unsafe { VirtualProtect(slot as *const _, 8, PAGE_READWRITE, &mut old) } == 0 {
        return Err("cannot protect the NMS startup import".into());
    }
    let exchanged = unsafe { &*(slot as *const AtomicUsize) }.compare_exchange(
        original as usize,
        deferred_get_proc_address as *const () as usize,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
    let mut discarded = 0;
    let restored = unsafe { VirtualProtect(slot as *const _, 8, old, &mut discarded) };
    if exchanged.is_err() || restored == 0 {
        return Err("NMS GetProcAddress import changed during startup".into());
    }
    Ok(())
}

fn ensure_startup_trigger() -> Result<(), String> {
    STARTUP_TRIGGER.get_or_init(install_startup_trigger).clone()
}

fn is_vulkan_name(name: windows_sys::core::PCSTR) -> bool {
    if name.is_null() || name as usize <= 0xffff {
        return false;
    }
    unsafe { CStr::from_ptr(name.cast()) }
        .to_bytes()
        .starts_with(b"vk")
}

unsafe extern "system" fn deferred_get_proc_address(
    module: HMODULE,
    name: windows_sys::core::PCSTR,
) -> windows_sys::Win32::Foundation::FARPROC {
    if is_vulkan_name(name) {
        start();
    }
    ORIGINAL_GET_PROC_ADDRESS
        .get()
        .and_then(|original| unsafe { original(module, name) })
}

fn start() {
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if let Err(error) = initialize() {
        report_startup_failure(&error);
    }
}

extern "system" fn search_probes_time_begin_period(period_value: u32) -> u32 {
    let result = period(WINMM_TIME_BEGIN_PERIOD)
        .map(|function| unsafe { function(period_value) })
        .unwrap_or(97);
    // This may be called while Windows is still initializing process DLLs. The trigger
    // deliberately performs no filesystem access and loads no additional library here.
    let _ = ensure_startup_trigger();
    result
}

extern "system" fn search_probes_time_end_period(period_value: u32) -> u32 {
    let Some(function) = period(WINMM_TIME_END_PERIOD) else {
        return 97;
    };
    unsafe { function(period_value) }
}

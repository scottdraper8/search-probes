#![deny(unsafe_op_in_unsafe_fn)]
#![cfg(windows)]

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::System::{
    LibraryLoader::{GetProcAddress, LoadLibraryW},
    SystemInformation::GetSystemDirectoryW,
};

type Period = unsafe extern "system" fn(u32) -> u32;
struct Periods {
    begin: Period,
    end: Period,
}
static PERIODS: OnceLock<Option<Periods>> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);

fn periods() -> Option<&'static Periods> {
    PERIODS
        .get_or_init(|| {
            let mut path = vec![0u16; 32768];
            let length =
                unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
            if length == 0 || length >= path.len() {
                return None;
            }
            path.truncate(length);
            path.extend("\\winmm.dll".encode_utf16());
            path.push(0);
            let module = unsafe { LoadLibraryW(path.as_ptr()) };
            if module.is_null() {
                return None;
            }
            let begin = unsafe { GetProcAddress(module, c"timeBeginPeriod".as_ptr().cast()) }?;
            let end = unsafe { GetProcAddress(module, c"timeEndPeriod".as_ptr().cast()) }?;
            Some(Periods {
                begin: unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, Period>(begin)
                },
                end: unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, Period>(end)
                },
            })
        })
        .as_ref()
}

fn start() {
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if std::thread::Builder::new()
        .name("Search Probes startup".into())
        .spawn(
            || match std::panic::catch_unwind(search_probes_native::runtime::initialize) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => search_probes_native::runtime::report_startup_failure(&error),
                Err(_) => search_probes_native::runtime::report_startup_failure(
                    "Search Probes initialization panicked. Restart the game.",
                ),
            },
        )
        .is_err()
    {
        STARTED.store(false, Ordering::Release);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn timeBeginPeriod(period: u32) -> u32 {
    let Some(periods) = periods() else {
        return 97;
    };
    let result = unsafe { (periods.begin)(period) };
    start();
    result
}

#[unsafe(no_mangle)]
pub extern "system" fn timeEndPeriod(period: u32) -> u32 {
    let Some(periods) = periods() else {
        return 97;
    };
    unsafe { (periods.end)(period) }
}

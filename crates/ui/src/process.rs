use std::io;
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_TIMEOUT},
    System::Threading::{CreateMutexW, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    UI::WindowsAndMessaging::{FindWindowW, SW_SHOW, SetForegroundWindow, ShowWindow},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, RECT},
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, SW_HIDE,
    },
};

pub struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn single_instance() -> io::Result<Option<Handle>> {
    let name: Vec<_> = "Local\\SearchProbes.UI".encode_utf16().chain([0]).collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let duplicate = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = Handle(handle);
    if duplicate {
        Ok(None)
    } else {
        Ok(Some(handle))
    }
}

pub struct Owner(Handle);

impl Owner {
    pub fn open(pid: u32) -> io::Result<Self> {
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(Handle(handle)))
        }
    }
    pub fn alive(&self) -> bool {
        (unsafe { WaitForSingleObject(self.0.0, 0) }) == WAIT_TIMEOUT
    }
}

fn window(pid: u32) -> Option<HWND> {
    struct Search {
        pid: u32,
        window: HWND,
        area: i64,
    }
    unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> i32 {
        let search = unsafe { &mut *(data as *mut Search) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut pid);
        }
        if pid != search.pid || unsafe { IsWindowVisible(window) } == 0 {
            return 1;
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(window, &mut rect) } != 0 {
            let area = (i64::from(rect.right) - i64::from(rect.left))
                * (i64::from(rect.bottom) - i64::from(rect.top));
            if area > search.area {
                search.area = area;
                search.window = window;
            }
        }
        1
    }
    let mut search = Search {
        pid,
        window: std::ptr::null_mut(),
        area: 0,
    };
    unsafe {
        EnumWindows(Some(visit), (&mut search as *mut Search) as LPARAM);
    }
    (!search.window.is_null()).then_some(search.window)
}

fn ui_window() -> io::Result<HWND> {
    let title: Vec<_> = "Search Probes".encode_utf16().chain([0]).collect();
    let window = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, &mut pid);
    }
    if window.is_null() || pid != std::process::id() {
        return Err(io::Error::other("Search Probes window is unavailable"));
    }
    Ok(window)
}

pub fn hide(pid: u32) -> io::Result<()> {
    let ui = ui_window()?;
    let game = window(pid).ok_or_else(|| io::Error::other("The game window is unavailable"))?;
    unsafe {
        ShowWindow(ui, SW_HIDE);
        SetForegroundWindow(game);
    }
    Ok(())
}

pub fn interactive() -> io::Result<()> {
    let ui = ui_window()?;
    unsafe {
        ShowWindow(ui, SW_SHOW);
        SetForegroundWindow(ui);
    }
    Ok(())
}

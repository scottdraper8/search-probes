use ash::vk;
use search_probes_ui::egui::{self, Event, Key, Modifiers, PointerButton};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
    UI::{
        HiDpi::GetDpiForWindow,
        Input::{
            GetRawInputData, KeyboardAndMouse::*, RAWINPUT, RAWINPUTHEADER, RID_INPUT,
            RIM_TYPEMOUSE,
        },
        WindowsAndMessaging::*,
    },
};
use windows_sys::{Win32::Graphics::Gdi::ScreenToClient, core::BOOL};

static INPUT: Mutex<Input> = Mutex::new(Input::new());
static ACTIVE: AtomicBool = AtomicBool::new(false);
static WINDOW: AtomicUsize = AtomicUsize::new(0);
static UI_CURSOR: AtomicUsize = AtomicUsize::new(0);

struct Input {
    procedures: BTreeMap<usize, (isize, bool)>,
    visible: bool,
    hotkey: bool,
    events: Vec<Event>,
    buttons: [bool; 3],
    clipped: Option<RECT>,
    cursor: usize,
    surrogate: Option<u16>,
    wheel: Option<(i32, i16, bool)>,
}
impl Input {
    const fn new() -> Self {
        Self {
            procedures: BTreeMap::new(),
            visible: false,
            hotkey: false,
            events: Vec::new(),
            buttons: [false; 3],
            clipped: None,
            cursor: 0,
            surrogate: None,
            wheel: None,
        }
    }
    fn wheel(&mut self, delta: i16, horizontal: bool) {
        let time = unsafe { GetMessageTime() };
        if self.wheel == Some((time, delta, horizontal)) {
            return;
        }
        self.wheel = Some((time, delta, horizontal));
        self.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            phase: egui::TouchPhase::Move,
            delta: if horizontal {
                egui::vec2(-f32::from(delta) / 120., 0.)
            } else {
                egui::vec2(0., f32::from(delta) / 120.)
            },
            modifiers: modifiers(),
        });
    }
    fn button(&mut self, window: HWND, index: usize, pressed: bool) {
        if self.buttons[index] == pressed {
            return;
        }
        self.buttons[index] = pressed;
        self.events.push(Event::PointerButton {
            pos: pointer_position(window),
            button: [
                PointerButton::Primary,
                PointerButton::Secondary,
                PointerButton::Middle,
            ][index],
            pressed,
            modifiers: modifiers(),
        });
    }
}

pub fn install(hwnd: usize) -> Result<(), String> {
    let mut input = INPUT.lock().map_err(|_| "overlay input poisoned")?;
    if !input.procedures.contains_key(&hwnd) {
        let window = hwnd as HWND;
        unsafe {
            windows_sys::Win32::Foundation::SetLastError(0);
        }
        let previous =
            unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, procedure as *const () as isize) };
        if previous == 0 {
            return Err("cannot observe the game window's input".into());
        }
        let unicode = unsafe { IsWindowUnicode(window) } != 0;
        input.procedures.insert(hwnd, (previous, unicode));
    }
    WINDOW.store(hwnd, Ordering::Release);
    Ok(())
}

fn modifiers() -> Modifiers {
    let down = |key| unsafe { GetKeyState(key) } as u16 & 0x8000 != 0;
    Modifiers {
        alt: down(i32::from(VK_MENU)),
        ctrl: down(i32::from(VK_CONTROL)),
        shift: down(i32::from(VK_SHIFT)),
        mac_cmd: false,
        command: down(i32::from(VK_CONTROL)),
    }
}
fn restore(input: &mut Input) -> Option<(RECT, usize)> {
    ACTIVE.store(false, Ordering::Release);
    let previous = input.clipped.take().map(|rect| (rect, input.cursor));
    input.buttons = [false; 3];
    input.events.clear();
    input.surrogate = None;
    previous
}
fn restore_cursor(previous: Option<(RECT, usize)>, focused: bool) {
    if focused && let Some((rect, cursor)) = previous {
        unsafe {
            ClipCursor(&rect);
            SetCursor(cursor as HCURSOR);
        }
    }
}
pub fn release() {
    let previous = restore(&mut INPUT.lock().unwrap_or_else(|e| e.into_inner()));
    let focused = unsafe { GetForegroundWindow() } as usize == WINDOW.load(Ordering::Acquire);
    restore_cursor(previous, focused);
}

fn dpi_scale(window: HWND) -> f32 {
    (unsafe { GetDpiForWindow(window) } as f32 / 96.0).max(1.0)
}
fn pointer_position(window: HWND) -> egui::Pos2 {
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point);
        ScreenToClient(window, &mut point);
    }
    let dpi = dpi_scale(window);
    egui::pos2(point.x as f32 / dpi, point.y as f32 / dpi)
}

pub fn take(hwnd: usize, extent: vk::Extent2D, time: f64) -> Option<egui::RawInput> {
    let window = hwnd as HWND;
    let focused = unsafe { GetForegroundWindow() } == window;
    let iconic = unsafe { IsIconic(window) } != 0;
    let down = unsafe { GetAsyncKeyState(i32::from(VK_F7)) } as u16 & 0x8000 != 0;
    let mut clipped = RECT::default();
    let has_clip = unsafe { GetClipCursor(&mut clipped) } != 0;
    let cursor = unsafe { GetCursor() } as usize;
    let mut rect = RECT::default();
    unsafe {
        GetClientRect(window, &mut rect);
    }
    let scale = extent.width as f32 / (rect.right.max(1)) as f32 * dpi_scale(window);
    let position = pointer_position(window);
    let modifiers = modifiers();
    let buttons = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .map(|key| unsafe { GetAsyncKeyState(i32::from(key)) } as u16 & 0x8000 != 0);
    let mut input = INPUT.lock().unwrap_or_else(|e| e.into_inner());
    if focused && down && !input.hotkey {
        input.visible = !input.visible;
    }
    input.hotkey = down;
    if !input.visible || !focused || iconic || rect.right <= 0 || rect.bottom <= 0 {
        let previous = restore(&mut input);
        drop(input);
        restore_cursor(previous, focused);
        return None;
    }
    let activated = !ACTIVE.swap(true, Ordering::AcqRel);
    if activated {
        input.clipped = has_clip.then_some(clipped);
        input.cursor = cursor;
    }
    input.events.push(Event::PointerMoved(position));
    input.events.push(Event::ModifiersChanged(modifiers));
    for (index, pressed) in buttons.into_iter().enumerate() {
        if input.buttons[index] != pressed {
            input.buttons[index] = pressed;
            input.events.push(Event::PointerButton {
                pos: position,
                button: [
                    PointerButton::Primary,
                    PointerButton::Secondary,
                    PointerButton::Middle,
                ][index],
                pressed,
                modifiers,
            });
        }
    }
    let screen_rect = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(extent.width as f32 / scale, extent.height as f32 / scale),
    );
    let mut raw = egui::RawInput {
        screen_rect: Some(screen_rect),
        time: Some(time),
        events: std::mem::take(&mut input.events),
        focused: true,
        ..Default::default()
    };
    drop(input);
    // These Win32 calls may synchronously re-enter the window procedure.
    if activated {
        unsafe {
            ClipCursor(std::ptr::null());
            ReleaseCapture();
            let cursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            UI_CURSOR.store(cursor as usize, Ordering::Release);
            SetCursor(cursor);
        }
    }
    if let Some(viewport) = raw.viewports.get_mut(&egui::ViewportId::ROOT) {
        viewport.native_pixels_per_point = Some(scale);
        viewport.inner_rect = Some(screen_rect);
        viewport.focused = Some(true);
    }
    Some(raw)
}

fn key(value: WPARAM) -> Option<Key> {
    Some(match value as u16 {
        VK_TAB => Key::Tab,
        VK_BACK => Key::Backspace,
        VK_RETURN => Key::Enter,
        VK_ESCAPE => Key::Escape,
        VK_SPACE => Key::Space,
        VK_INSERT => Key::Insert,
        VK_DELETE => Key::Delete,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_PRIOR => Key::PageUp,
        VK_NEXT => Key::PageDown,
        VK_LEFT => Key::ArrowLeft,
        VK_RIGHT => Key::ArrowRight,
        VK_UP => Key::ArrowUp,
        VK_DOWN => Key::ArrowDown,
        0x41 => Key::A,
        0x42 => Key::B,
        0x43 => Key::C,
        0x44 => Key::D,
        0x45 => Key::E,
        0x46 => Key::F,
        0x47 => Key::G,
        0x48 => Key::H,
        0x49 => Key::I,
        0x4a => Key::J,
        0x4b => Key::K,
        0x4c => Key::L,
        0x4d => Key::M,
        0x4e => Key::N,
        0x4f => Key::O,
        0x50 => Key::P,
        0x51 => Key::Q,
        0x52 => Key::R,
        0x53 => Key::S,
        0x54 => Key::T,
        0x55 => Key::U,
        0x56 => Key::V,
        0x57 => Key::W,
        0x58 => Key::X,
        0x59 => Key::Y,
        0x5a => Key::Z,
        _ => return None,
    })
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let processed = std::panic::catch_unwind(|| {
        let active =
            ACTIVE.load(Ordering::Acquire) && WINDOW.load(Ordering::Acquire) == hwnd as usize;
        let pasted = if active
            && matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN)
            && wparam == 0x56
            && modifiers().command
        {
            paste(hwnd)
        } else {
            None
        };
        let mut input = INPUT.lock().unwrap_or_else(|e| e.into_inner());
        let previous = input.procedures.get(&(hwnd as usize)).copied();
        let keyboard = matches!(
            message,
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR
        );
        if keyboard
            && wparam == usize::from(VK_F7)
            && WINDOW.load(Ordering::Acquire) == hwnd as usize
        {
            let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
            if down && !input.hotkey && unsafe { GetForegroundWindow() } == hwnd {
                input.visible = !input.visible;
            }
            input.hotkey = down;
            return (previous, true);
        }
        if matches!(message, WM_SYSKEYDOWN | WM_SYSKEYUP) && wparam == usize::from(VK_F4) {
            return (previous, false);
        }
        if message == WM_KILLFOCUS {
            restore(&mut input);
        }
        if active {
            if input.events.len() >= 4096 {
                input.events.clear();
            }
            match message {
                WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                    if let Some(key) = key(wparam) {
                        let modifiers = modifiers();
                        let pressed = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
                        if modifiers.command && pressed && key == Key::V {
                            if let Some(text) = pasted {
                                input.events.push(Event::Paste(text));
                            }
                        } else if modifiers.command && pressed && key == Key::C {
                            input.events.push(Event::Copy);
                        } else if modifiers.command && pressed && key == Key::X {
                            input.events.push(Event::Cut);
                        } else {
                            input.events.push(Event::Key {
                                key,
                                physical_key: None,
                                pressed,
                                repeat: pressed && lparam & (1 << 30) != 0,
                                modifiers,
                            });
                        }
                    }
                }
                WM_CHAR => {
                    let unit = wparam as u16;
                    if (0xd800..=0xdbff).contains(&unit) {
                        input.surrogate = Some(unit);
                    } else {
                        let value = if (0xdc00..=0xdfff).contains(&unit) {
                            input.surrogate.take().map(|high| {
                                0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(unit)
                                    - 0xdc00
                            })
                        } else {
                            input.surrogate = None;
                            Some(u32::from(unit))
                        };
                        if let Some(ch) =
                            value.and_then(char::from_u32).filter(|ch| !ch.is_control())
                        {
                            input.events.push(Event::Text(ch.to_string()));
                        }
                    }
                }
                WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
                | WM_MBUTTONUP => {
                    let index = match message {
                        WM_LBUTTONDOWN | WM_LBUTTONUP => 0,
                        WM_RBUTTONDOWN | WM_RBUTTONUP => 1,
                        _ => 2,
                    };
                    input.button(
                        hwnd,
                        index,
                        matches!(message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN),
                    );
                }
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    input.wheel((wparam >> 16) as i16, message == WM_MOUSEHWHEEL)
                }
                WM_INPUT => {
                    let mut raw = RAWINPUT::default();
                    let mut bytes = std::mem::size_of::<RAWINPUT>() as u32;
                    if unsafe {
                        GetRawInputData(
                            lparam as _,
                            RID_INPUT,
                            (&mut raw as *mut RAWINPUT).cast(),
                            &mut bytes,
                            std::mem::size_of::<RAWINPUTHEADER>() as u32,
                        )
                    } != u32::MAX
                        && raw.header.dwType == RIM_TYPEMOUSE
                    {
                        let buttons = unsafe { raw.data.mouse.Anonymous.Anonymous };
                        for (index, down, up) in [(0, 1, 2), (1, 4, 8), (2, 16, 32)] {
                            if buttons.usButtonFlags & down != 0 {
                                input.button(hwnd, index, true);
                            }
                            if buttons.usButtonFlags & up != 0 {
                                input.button(hwnd, index, false);
                            }
                        }
                        if buttons.usButtonFlags & 0x400 != 0 {
                            input.wheel(buttons.usButtonData as i16, false);
                        }
                        if buttons.usButtonFlags & 0x800 != 0 {
                            input.wheel(buttons.usButtonData as i16, true);
                        }
                    }
                    return (previous, true);
                }
                WM_SETCURSOR => {
                    drop(input);
                    unsafe {
                        SetCursor(UI_CURSOR.load(Ordering::Acquire) as HCURSOR);
                    }
                    return (previous, true);
                }
                _ => {}
            }
        }
        if message == WM_NCDESTROY {
            input.procedures.remove(&(hwnd as usize));
            restore(&mut input);
        }
        (
            previous,
            active && (keyboard || (WM_MOUSEFIRST..=WM_MOUSELAST).contains(&message)),
        )
    });
    let (previous, consumed) = match processed {
        Ok(value) => value,
        Err(_) => (None, false),
    };
    if consumed {
        if message == WM_INPUT {
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
        return 0;
    }
    if let Some((previous, unicode)) = previous {
        let callback = unsafe { std::mem::transmute::<isize, WNDPROC>(previous) };
        if unicode {
            unsafe { CallWindowProcW(callback, hwnd, message, wparam, lparam) }
        } else {
            unsafe { CallWindowProcA(callback, hwnd, message, wparam, lparam) }
        }
    } else {
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
}

fn paste(hwnd: HWND) -> Option<String> {
    if unsafe { OpenClipboard(hwnd) } == 0 {
        return None;
    }
    let handle = unsafe { GetClipboardData(13) };
    let result = if handle.is_null() {
        None
    } else {
        let pointer = unsafe { GlobalLock(handle) }.cast::<u16>();
        if pointer.is_null() {
            None
        } else {
            let mut text = Vec::new();
            for index in 0..(unsafe { GlobalSize(handle) } / 2).min(65536) {
                let ch = unsafe { *pointer.add(index) };
                if ch == 0 {
                    break;
                }
                text.push(ch);
            }
            unsafe {
                GlobalUnlock(handle);
            }
            Some(String::from_utf16_lossy(&text))
        }
    };
    unsafe {
        CloseClipboard();
    }
    result
}
fn copy(hwnd: HWND, text: &str) {
    let text: Vec<_> = text.encode_utf16().take(65535).chain([0]).collect();
    if unsafe { OpenClipboard(hwnd) } == 0 {
        return;
    }
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, text.len() * 2);
        if !handle.is_null() {
            let pointer = GlobalLock(handle);
            if !pointer.is_null() {
                std::ptr::copy_nonoverlapping(text.as_ptr(), pointer.cast::<u16>(), text.len());
                GlobalUnlock(handle);
                if EmptyClipboard() == 0 || SetClipboardData(13, handle).is_null() {
                    windows_sys::Win32::Foundation::GlobalFree(handle);
                }
            } else {
                windows_sys::Win32::Foundation::GlobalFree(handle);
            }
        }
        CloseClipboard();
    }
}
pub fn output(hwnd: usize, output: &egui::PlatformOutput) {
    let cursor = match output.cursor_icon {
        egui::CursorIcon::None => std::ptr::null_mut(),
        icon => unsafe {
            LoadCursorW(
                std::ptr::null_mut(),
                match icon {
                    egui::CursorIcon::PointingHand => IDC_HAND,
                    egui::CursorIcon::Text | egui::CursorIcon::VerticalText => IDC_IBEAM,
                    egui::CursorIcon::ResizeHorizontal | egui::CursorIcon::ResizeColumn => {
                        IDC_SIZEWE
                    }
                    egui::CursorIcon::ResizeVertical | egui::CursorIcon::ResizeRow => IDC_SIZENS,
                    egui::CursorIcon::ResizeNwSe
                    | egui::CursorIcon::ResizeNorthWest
                    | egui::CursorIcon::ResizeSouthEast => IDC_SIZENWSE,
                    egui::CursorIcon::ResizeNeSw
                    | egui::CursorIcon::ResizeNorthEast
                    | egui::CursorIcon::ResizeSouthWest => IDC_SIZENESW,
                    egui::CursorIcon::Grab
                    | egui::CursorIcon::Grabbing
                    | egui::CursorIcon::Move
                    | egui::CursorIcon::AllScroll => IDC_SIZEALL,
                    egui::CursorIcon::Crosshair => IDC_CROSS,
                    egui::CursorIcon::Wait | egui::CursorIcon::Progress => IDC_WAIT,
                    egui::CursorIcon::NotAllowed | egui::CursorIcon::NoDrop => IDC_NO,
                    _ => IDC_ARROW,
                },
            )
        },
    };
    UI_CURSOR.store(cursor as usize, Ordering::Release);
    if ACTIVE.load(Ordering::Acquire) {
        unsafe {
            SetCursor(cursor);
        }
    }
    for command in &output.commands {
        if let egui::OutputCommand::CopyText(text) = command {
            copy(hwnd as HWND, text);
        }
    }
}
pub unsafe extern "system" fn game_async_key(key: i32) -> i16 {
    if ACTIVE.load(Ordering::Acquire) || key == i32::from(VK_F7) {
        0
    } else {
        unsafe { GetAsyncKeyState(key) }
    }
}
pub unsafe extern "system" fn game_key(key: i32) -> i16 {
    if ACTIVE.load(Ordering::Acquire) || key == i32::from(VK_F7) {
        0
    } else {
        unsafe { GetKeyState(key) }
    }
}
pub unsafe extern "system" fn game_keyboard(keys: *mut u8) -> BOOL {
    if ACTIVE.load(Ordering::Acquire) {
        unsafe {
            std::ptr::write_bytes(keys, 0, 256);
        }
        1
    } else {
        unsafe { GetKeyboardState(keys) }
    }
}
pub unsafe extern "system" fn game_cursor_pos(x: i32, y: i32) -> BOOL {
    if ACTIVE.load(Ordering::Acquire) {
        1
    } else {
        unsafe { SetCursorPos(x, y) }
    }
}
pub unsafe extern "system" fn game_clip(rect: *const RECT) -> BOOL {
    if ACTIVE.load(Ordering::Acquire) {
        1
    } else {
        unsafe { ClipCursor(rect) }
    }
}
pub unsafe extern "system" fn game_cursor(cursor: HCURSOR) -> HCURSOR {
    if ACTIVE.load(Ordering::Acquire) {
        unsafe { SetCursor(UI_CURSOR.load(Ordering::Acquire) as HCURSOR) }
    } else {
        unsafe { SetCursor(cursor) }
    }
}

pub fn diagnostics() -> serde_json::Value {
    let input = INPUT.lock().unwrap_or_else(|e| e.into_inner());
    serde_json::json!({"visible":input.visible,"active":ACTIVE.load(Ordering::Acquire),"window":WINDOW.load(Ordering::Acquire)})
}

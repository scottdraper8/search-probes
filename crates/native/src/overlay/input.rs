use ash::vk;
use search_probes_model::binding::{BindingDevice, ControllerButton, InputBinding, KeyboardKey};
use search_probes_ui::egui::{self, Event, Key, Modifiers, PointerButton};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
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
            RIM_TYPEMOUSE, XboxController::*,
        },
        WindowsAndMessaging::*,
    },
};
use windows_sys::{Win32::Graphics::Gdi::ScreenToClient, core::BOOL};

static INPUT: Mutex<Input> = Mutex::new(Input::new());
static ACTIVE: AtomicBool = AtomicBool::new(false);
static WINDOW: AtomicUsize = AtomicUsize::new(0);
static UI_CURSOR: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_KEYS: AtomicU32 = AtomicU32::new(0);
static HOTKEY_DOWN: AtomicBool = AtomicBool::new(false);

const CONTROLLER_BUTTONS: [ControllerButton; 16] = [
    ControllerButton::FaceBottom,
    ControllerButton::FaceRight,
    ControllerButton::FaceLeft,
    ControllerButton::FaceTop,
    ControllerButton::DpadDown,
    ControllerButton::DpadLeft,
    ControllerButton::DpadRight,
    ControllerButton::DpadUp,
    ControllerButton::LeftBumper,
    ControllerButton::RightBumper,
    ControllerButton::LeftTrigger,
    ControllerButton::RightTrigger,
    ControllerButton::LeftStick,
    ControllerButton::RightStick,
    ControllerButton::View,
    ControllerButton::Menu,
];

struct Input {
    procedures: BTreeMap<usize, (isize, bool)>,
    visible: bool,
    hotkey: bool,
    binding: Option<InputBinding>,
    controllers: [u32; XUSER_MAX_COUNT as usize],
    events: Vec<Event>,
    buttons: [bool; 3],
    game: Option<GameInput>,
    surrogate: Option<u16>,
    wheel: Option<(i32, i16, bool)>,
}

#[derive(Clone, Copy)]
struct GameInput {
    clipped: Option<RECT>,
    cursor: usize,
    position: Option<POINT>,
}

impl Input {
    const fn new() -> Self {
        Self {
            procedures: BTreeMap::new(),
            visible: false,
            hotkey: false,
            binding: None,
            controllers: [0; XUSER_MAX_COUNT as usize],
            events: Vec::new(),
            buttons: [false; 3],
            game: None,
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

pub struct InputFrame {
    pub raw: egui::RawInput,
    pub controller: Vec<(u8, ControllerButton, bool)>,
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
fn deactivate(input: &mut Input) {
    ACTIVE.store(false, Ordering::Release);
    HOTKEY_DOWN.store(false, Ordering::Release);
    input.buttons = [false; 3];
    input.events.clear();
    input.surrogate = None;
}
fn restore(input: &mut Input) -> Option<GameInput> {
    deactivate(input);
    input.game.take()
}
fn restore_game_input(game: Option<GameInput>, focused: bool) {
    if focused && let Some(game) = game {
        unsafe {
            if let Some(position) = game.position {
                SetCursorPos(position.x, position.y);
            }
            if let Some(rect) = game.clipped {
                ClipCursor(&rect);
            } else {
                ClipCursor(std::ptr::null());
            }
            SetCursor(game.cursor as HCURSOR);
        }
    }
}
pub fn release() {
    let game = restore(&mut INPUT.lock().unwrap_or_else(|e| e.into_inner()));
    let focused = unsafe { GetForegroundWindow() } as usize == WINDOW.load(Ordering::Acquire);
    restore_game_input(game, focused);
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

fn virtual_key(key: KeyboardKey) -> i32 {
    i32::from(match key {
        KeyboardKey::ArrowDown => VK_DOWN,
        KeyboardKey::ArrowLeft => VK_LEFT,
        KeyboardKey::ArrowRight => VK_RIGHT,
        KeyboardKey::ArrowUp => VK_UP,
        KeyboardKey::Escape => VK_ESCAPE,
        KeyboardKey::Tab => VK_TAB,
        KeyboardKey::Backspace => VK_BACK,
        KeyboardKey::Enter => VK_RETURN,
        KeyboardKey::Space => VK_SPACE,
        KeyboardKey::Insert => VK_INSERT,
        KeyboardKey::Delete => VK_DELETE,
        KeyboardKey::Home => VK_HOME,
        KeyboardKey::End => VK_END,
        KeyboardKey::PageUp => VK_PRIOR,
        KeyboardKey::PageDown => VK_NEXT,
        KeyboardKey::Semicolon => VK_OEM_1,
        KeyboardKey::Comma => VK_OEM_COMMA,
        KeyboardKey::Backslash => VK_OEM_5,
        KeyboardKey::Slash => VK_OEM_2,
        KeyboardKey::OpenBracket => VK_OEM_4,
        KeyboardKey::CloseBracket => VK_OEM_6,
        KeyboardKey::Backtick => VK_OEM_3,
        KeyboardKey::Minus => VK_OEM_MINUS,
        KeyboardKey::Period => VK_OEM_PERIOD,
        KeyboardKey::Equals => VK_OEM_PLUS,
        KeyboardKey::Quote => VK_OEM_7,
        KeyboardKey::Num0 => 0x30,
        KeyboardKey::Num1 => 0x31,
        KeyboardKey::Num2 => 0x32,
        KeyboardKey::Num3 => 0x33,
        KeyboardKey::Num4 => 0x34,
        KeyboardKey::Num5 => 0x35,
        KeyboardKey::Num6 => 0x36,
        KeyboardKey::Num7 => 0x37,
        KeyboardKey::Num8 => 0x38,
        KeyboardKey::Num9 => 0x39,
        KeyboardKey::A => 0x41,
        KeyboardKey::B => 0x42,
        KeyboardKey::C => 0x43,
        KeyboardKey::D => 0x44,
        KeyboardKey::E => 0x45,
        KeyboardKey::F => 0x46,
        KeyboardKey::G => 0x47,
        KeyboardKey::H => 0x48,
        KeyboardKey::I => 0x49,
        KeyboardKey::J => 0x4a,
        KeyboardKey::K => 0x4b,
        KeyboardKey::L => 0x4c,
        KeyboardKey::M => 0x4d,
        KeyboardKey::N => 0x4e,
        KeyboardKey::O => 0x4f,
        KeyboardKey::P => 0x50,
        KeyboardKey::Q => 0x51,
        KeyboardKey::R => 0x52,
        KeyboardKey::S => 0x53,
        KeyboardKey::T => 0x54,
        KeyboardKey::U => 0x55,
        KeyboardKey::V => 0x56,
        KeyboardKey::W => 0x57,
        KeyboardKey::X => 0x58,
        KeyboardKey::Y => 0x59,
        KeyboardKey::Z => 0x5a,
        KeyboardKey::F1 => VK_F1,
        KeyboardKey::F2 => VK_F2,
        KeyboardKey::F3 => VK_F3,
        KeyboardKey::F4 => VK_F4,
        KeyboardKey::F5 => VK_F5,
        KeyboardKey::F6 => VK_F6,
        KeyboardKey::F7 => VK_F7,
        KeyboardKey::F8 => VK_F8,
        KeyboardKey::F9 => VK_F9,
        KeyboardKey::F10 => VK_F10,
        KeyboardKey::F11 => VK_F11,
        KeyboardKey::F12 => VK_F12,
        KeyboardKey::F13 => VK_F13,
        KeyboardKey::F14 => VK_F14,
        KeyboardKey::F15 => VK_F15,
        KeyboardKey::F16 => VK_F16,
        KeyboardKey::F17 => VK_F17,
        KeyboardKey::F18 => VK_F18,
        KeyboardKey::F19 => VK_F19,
        KeyboardKey::F20 => VK_F20,
        KeyboardKey::F21 => VK_F21,
        KeyboardKey::F22 => VK_F22,
        KeyboardKey::F23 => VK_F23,
        KeyboardKey::F24 => VK_F24,
        KeyboardKey::ShiftLeft => VK_LSHIFT,
        KeyboardKey::ShiftRight => VK_RSHIFT,
        KeyboardKey::ControlLeft => VK_LCONTROL,
        KeyboardKey::ControlRight => VK_RCONTROL,
        KeyboardKey::AltLeft => VK_LMENU,
        KeyboardKey::AltRight => VK_RMENU,
        KeyboardKey::SuperLeft => VK_LWIN,
        KeyboardKey::SuperRight => VK_RWIN,
        KeyboardKey::IntlBackslash => VK_OEM_102,
    })
}

fn keyboard_down(key: KeyboardKey) -> bool {
    (unsafe { GetAsyncKeyState(virtual_key(key)) }) as u16 & 0x8000 != 0
}

fn controller_pressed(gamepad: &XINPUT_GAMEPAD, button: ControllerButton) -> bool {
    let flag = match button {
        ControllerButton::FaceBottom => XINPUT_GAMEPAD_A,
        ControllerButton::FaceRight => XINPUT_GAMEPAD_B,
        ControllerButton::FaceLeft => XINPUT_GAMEPAD_X,
        ControllerButton::FaceTop => XINPUT_GAMEPAD_Y,
        ControllerButton::DpadDown => XINPUT_GAMEPAD_DPAD_DOWN,
        ControllerButton::DpadLeft => XINPUT_GAMEPAD_DPAD_LEFT,
        ControllerButton::DpadRight => XINPUT_GAMEPAD_DPAD_RIGHT,
        ControllerButton::DpadUp => XINPUT_GAMEPAD_DPAD_UP,
        ControllerButton::LeftBumper => XINPUT_GAMEPAD_LEFT_SHOULDER,
        ControllerButton::RightBumper => XINPUT_GAMEPAD_RIGHT_SHOULDER,
        ControllerButton::LeftStick => XINPUT_GAMEPAD_LEFT_THUMB,
        ControllerButton::RightStick => XINPUT_GAMEPAD_RIGHT_THUMB,
        ControllerButton::View => XINPUT_GAMEPAD_BACK,
        ControllerButton::Menu => XINPUT_GAMEPAD_START,
        ControllerButton::LeftTrigger => {
            return gamepad.bLeftTrigger > XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8;
        }
        ControllerButton::RightTrigger => {
            return gamepad.bRightTrigger > XINPUT_GAMEPAD_TRIGGER_THRESHOLD as u8;
        }
    };
    gamepad.wButtons & flag != 0
}

fn controller_mask(gamepad: &XINPUT_GAMEPAD) -> u32 {
    CONTROLLER_BUTTONS
        .iter()
        .enumerate()
        .fold(0, |mask, (index, &button)| {
            mask | (u32::from(controller_pressed(gamepad, button)) << index)
        })
}

fn read_controllers() -> [u32; XUSER_MAX_COUNT as usize] {
    let mut controllers = [0; XUSER_MAX_COUNT as usize];
    for (index, mask) in controllers.iter_mut().enumerate() {
        let mut state = XINPUT_STATE::default();
        if unsafe { XInputGetState(index as u32, &mut state) } == 0 {
            *mask = controller_mask(&state.Gamepad);
        }
    }
    controllers
}

fn controller_bit(button: ControllerButton) -> u32 {
    1 << CONTROLLER_BUTTONS
        .iter()
        .position(|&candidate| candidate == button)
        .expect("controller button is in the canonical list")
}

fn binding_down(binding: InputBinding, controllers: &[u32; XUSER_MAX_COUNT as usize]) -> bool {
    match binding {
        InputBinding::Keyboard { first, second } => {
            keyboard_down(first) && second.is_none_or(keyboard_down)
        }
        InputBinding::Controller { first, second } => {
            let required = controller_bit(first) | second.map_or(0, controller_bit);
            controllers
                .iter()
                .any(|controller| controller & required == required)
        }
    }
}

fn binding_keys(binding: InputBinding) -> u32 {
    match binding {
        InputBinding::Keyboard { first, second } => {
            virtual_key(first) as u32 | (second.map_or(0, virtual_key) as u32) << 16
        }
        InputBinding::Controller { .. } => 0,
    }
}

fn controller_events(
    previous: &[u32; XUSER_MAX_COUNT as usize],
    current: &[u32; XUSER_MAX_COUNT as usize],
) -> Vec<(u8, ControllerButton, bool)> {
    let mut events = Vec::new();
    for (controller, (&previous, &current)) in previous.iter().zip(current).enumerate() {
        let changed = previous ^ current;
        for (index, &button) in CONTROLLER_BUTTONS.iter().enumerate() {
            let bit = 1 << index;
            if changed & bit != 0 {
                events.push((controller as u8, button, current & bit != 0));
            }
        }
    }
    events
}

pub fn take(
    hwnd: usize,
    extent: vk::Extent2D,
    time: f64,
    binding: InputBinding,
    capture_device: Option<BindingDevice>,
) -> Option<InputFrame> {
    let window = hwnd as HWND;
    let focused = unsafe { GetForegroundWindow() } == window;
    let iconic = unsafe { IsIconic(window) } != 0;
    let poll_controllers = binding.device() == BindingDevice::Controller
        || capture_device == Some(BindingDevice::Controller);
    let controllers = if poll_controllers {
        read_controllers()
    } else {
        [0; XUSER_MAX_COUNT as usize]
    };
    let down = binding_down(binding, &controllers);
    HOTKEY_KEYS.store(binding_keys(binding), Ordering::Release);
    HOTKEY_DOWN.store(down, Ordering::Release);
    let mut clipped = RECT::default();
    let has_clip = unsafe { GetClipCursor(&mut clipped) } != 0;
    let cursor = unsafe { GetCursor() } as usize;
    let mut cursor_position = POINT::default();
    let has_cursor_position = unsafe { GetCursorPos(&mut cursor_position) } != 0;
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
    let controller = controller_events(&input.controllers, &controllers);
    input.controllers = controllers;
    let binding_changed = input.binding != Some(binding);
    input.binding = Some(binding);
    if focused && capture_device.is_none() && !binding_changed && down && !input.hotkey {
        input.visible = !input.visible;
    }
    input.hotkey = down;
    if !focused {
        deactivate(&mut input);
        return None;
    }
    if !input.visible || iconic || rect.right <= 0 || rect.bottom <= 0 {
        let game = restore(&mut input);
        drop(input);
        restore_game_input(game, true);
        return None;
    }
    let activated = !ACTIVE.swap(true, Ordering::AcqRel);
    if activated && input.game.is_none() {
        input.game = Some(GameInput {
            clipped: has_clip.then_some(clipped),
            cursor,
            position: has_cursor_position.then_some(cursor_position),
        });
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
    Some(InputFrame { raw, controller })
}

fn key(value: WPARAM, lparam: LPARAM) -> Option<Key> {
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
        VK_SHIFT => {
            if (lparam >> 16) & 0xff == 0x36 {
                Key::ShiftRight
            } else {
                Key::ShiftLeft
            }
        }
        VK_CONTROL => {
            if lparam & (1 << 24) != 0 {
                Key::ControlRight
            } else {
                Key::ControlLeft
            }
        }
        VK_MENU => {
            if lparam & (1 << 24) != 0 {
                Key::AltRight
            } else {
                Key::AltLeft
            }
        }
        VK_LSHIFT => Key::ShiftLeft,
        VK_RSHIFT => Key::ShiftRight,
        VK_LCONTROL => Key::ControlLeft,
        VK_RCONTROL => Key::ControlRight,
        VK_LMENU => Key::AltLeft,
        VK_RMENU => Key::AltRight,
        VK_LWIN => Key::SuperLeft,
        VK_RWIN => Key::SuperRight,
        VK_OEM_1 => Key::Semicolon,
        VK_OEM_COMMA => Key::Comma,
        VK_OEM_5 => Key::Backslash,
        VK_OEM_2 => Key::Slash,
        VK_OEM_4 => Key::OpenBracket,
        VK_OEM_6 => Key::CloseBracket,
        VK_OEM_3 => Key::Backtick,
        VK_OEM_MINUS => Key::Minus,
        VK_OEM_PERIOD => Key::Period,
        VK_OEM_PLUS => Key::Equals,
        VK_OEM_7 => Key::Quote,
        VK_OEM_102 => Key::IntlBackslash,
        0x30 => Key::Num0,
        0x31 => Key::Num1,
        0x32 => Key::Num2,
        0x33 => Key::Num3,
        0x34 => Key::Num4,
        0x35 => Key::Num5,
        0x36 => Key::Num6,
        0x37 => Key::Num7,
        0x38 => Key::Num8,
        0x39 => Key::Num9,
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
        VK_F1 => Key::F1,
        VK_F2 => Key::F2,
        VK_F3 => Key::F3,
        VK_F4 => Key::F4,
        VK_F5 => Key::F5,
        VK_F6 => Key::F6,
        VK_F7 => Key::F7,
        VK_F8 => Key::F8,
        VK_F9 => Key::F9,
        VK_F10 => Key::F10,
        VK_F11 => Key::F11,
        VK_F12 => Key::F12,
        VK_F13 => Key::F13,
        VK_F14 => Key::F14,
        VK_F15 => Key::F15,
        VK_F16 => Key::F16,
        VK_F17 => Key::F17,
        VK_F18 => Key::F18,
        VK_F19 => Key::F19,
        VK_F20 => Key::F20,
        VK_F21 => Key::F21,
        VK_F22 => Key::F22,
        VK_F23 => Key::F23,
        VK_F24 => Key::F24,
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
            && hotkey_virtual_key(wparam as i32)
            && HOTKEY_DOWN.load(Ordering::Acquire)
            && WINDOW.load(Ordering::Acquire) == hwnd as usize
        {
            return (previous, true);
        }
        if matches!(message, WM_SYSKEYDOWN | WM_SYSKEYUP) && wparam == usize::from(VK_F4) {
            return (previous, false);
        }
        if message == WM_KILLFOCUS {
            input.visible = false;
            deactivate(&mut input);
        }
        if active {
            if input.events.len() >= 4096 {
                input.events.clear();
            }
            match message {
                WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                    if let Some(key) = key(wparam, lparam) {
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
                                physical_key: Some(key),
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

fn hotkey_virtual_key(key: i32) -> bool {
    let keys = HOTKEY_KEYS.load(Ordering::Acquire);
    key == (keys & 0xffff) as i32 || (keys >> 16 != 0 && key == (keys >> 16) as i32)
}

pub unsafe extern "system" fn game_async_key(key: i32) -> i16 {
    if ACTIVE.load(Ordering::Acquire)
        || (HOTKEY_DOWN.load(Ordering::Acquire) && hotkey_virtual_key(key))
    {
        0
    } else {
        unsafe { GetAsyncKeyState(key) }
    }
}
pub unsafe extern "system" fn game_key(key: i32) -> i16 {
    if ACTIVE.load(Ordering::Acquire)
        || (HOTKEY_DOWN.load(Ordering::Acquire) && hotkey_virtual_key(key))
    {
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
        return 1;
    }
    let result = unsafe { GetKeyboardState(keys) };
    if result != 0 && HOTKEY_DOWN.load(Ordering::Acquire) {
        let hotkeys = HOTKEY_KEYS.load(Ordering::Acquire);
        unsafe {
            *keys.add((hotkeys & 0xff) as usize) = 0;
            if hotkeys >> 16 != 0 {
                *keys.add(((hotkeys >> 16) & 0xff) as usize) = 0;
            }
        }
    }
    result
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
    serde_json::json!({"visible":input.visible,"active":ACTIVE.load(Ordering::Acquire),"restore_pending":input.game.is_some(),"window":WINDOW.load(Ordering::Acquire)})
}

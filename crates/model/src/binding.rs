use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingDevice {
    Keyboard,
    Controller,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyboardKey {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Escape,
    Tab,
    Backspace,
    Enter,
    Space,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Comma,
    Backslash,
    Slash,
    OpenBracket,
    CloseBracket,
    Backtick,
    Minus,
    Period,
    Equals,
    Semicolon,
    Quote,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    SuperLeft,
    SuperRight,
    IntlBackslash,
}

impl KeyboardKey {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ArrowDown => "Down",
            Self::ArrowLeft => "Left",
            Self::ArrowRight => "Right",
            Self::ArrowUp => "Up",
            Self::Escape => "Esc",
            Self::Tab => "Tab",
            Self::Backspace => "Backspace",
            Self::Enter => "Enter",
            Self::Space => "Space",
            Self::Insert => "Insert",
            Self::Delete => "Delete",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "Page Up",
            Self::PageDown => "Page Down",
            Self::Comma => ",",
            Self::Backslash => "\\",
            Self::Slash => "/",
            Self::OpenBracket => "[",
            Self::CloseBracket => "]",
            Self::Backtick => "`",
            Self::Minus => "-",
            Self::Period => ".",
            Self::Equals => "=",
            Self::Semicolon => ";",
            Self::Quote => "'",
            Self::Num0 => "0",
            Self::Num1 => "1",
            Self::Num2 => "2",
            Self::Num3 => "3",
            Self::Num4 => "4",
            Self::Num5 => "5",
            Self::Num6 => "6",
            Self::Num7 => "7",
            Self::Num8 => "8",
            Self::Num9 => "9",
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::H => "H",
            Self::I => "I",
            Self::J => "J",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::U => "U",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
            Self::F13 => "F13",
            Self::F14 => "F14",
            Self::F15 => "F15",
            Self::F16 => "F16",
            Self::F17 => "F17",
            Self::F18 => "F18",
            Self::F19 => "F19",
            Self::F20 => "F20",
            Self::F21 => "F21",
            Self::F22 => "F22",
            Self::F23 => "F23",
            Self::F24 => "F24",
            Self::ShiftLeft => "Left Shift",
            Self::ShiftRight => "Right Shift",
            Self::ControlLeft => "Left Ctrl",
            Self::ControlRight => "Right Ctrl",
            Self::AltLeft => "Left Alt",
            Self::AltRight => "Right Alt",
            Self::SuperLeft => "Left Super",
            Self::SuperRight => "Right Super",
            Self::IntlBackslash => "Intl \\",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerButton {
    FaceBottom,
    FaceRight,
    FaceLeft,
    FaceTop,
    DpadDown,
    DpadLeft,
    DpadRight,
    DpadUp,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    LeftStick,
    RightStick,
    View,
    Menu,
}

impl ControllerButton {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FaceBottom => "Bottom face button",
            Self::FaceRight => "Right face button",
            Self::FaceLeft => "Left face button",
            Self::FaceTop => "Top face button",
            Self::DpadDown => "D-pad down",
            Self::DpadLeft => "D-pad left",
            Self::DpadRight => "D-pad right",
            Self::DpadUp => "D-pad up",
            Self::LeftBumper => "Left bumper",
            Self::RightBumper => "Right bumper",
            Self::LeftTrigger => "Left trigger",
            Self::RightTrigger => "Right trigger",
            Self::LeftStick => "Left stick click",
            Self::RightStick => "Right stick click",
            Self::View => "View / Select",
            Self::Menu => "Menu / Start",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "device", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputBinding {
    Keyboard {
        first: KeyboardKey,
        second: Option<KeyboardKey>,
    },
    Controller {
        first: ControllerButton,
        second: Option<ControllerButton>,
    },
}

impl Default for InputBinding {
    fn default() -> Self {
        Self::Keyboard {
            first: KeyboardKey::F7,
            second: None,
        }
    }
}

impl InputBinding {
    pub const fn device(self) -> BindingDevice {
        match self {
            Self::Keyboard { .. } => BindingDevice::Keyboard,
            Self::Controller { .. } => BindingDevice::Controller,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Keyboard {
                first,
                second: Some(second),
            } if first == second => Err("a binding cannot contain the same key twice"),
            Self::Controller {
                first,
                second: Some(second),
            } if first == second => Err("a binding cannot contain the same button twice"),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_chord_rejects_the_same_key_twice() {
        assert_eq!(
            InputBinding::Keyboard {
                first: KeyboardKey::F7,
                second: Some(KeyboardKey::F7),
            }
            .validate(),
            Err("a binding cannot contain the same key twice")
        );
    }

    #[test]
    fn controller_chord_rejects_the_same_button_twice() {
        assert_eq!(
            InputBinding::Controller {
                first: ControllerButton::LeftBumper,
                second: Some(ControllerButton::LeftBumper),
            }
            .validate(),
            Err("a binding cannot contain the same button twice")
        );
    }

    #[test]
    fn keyboard_and_controller_chords_accept_two_distinct_inputs() {
        assert!(
            InputBinding::Keyboard {
                first: KeyboardKey::ShiftLeft,
                second: Some(KeyboardKey::Z),
            }
            .validate()
            .is_ok()
        );
        assert!(
            InputBinding::Controller {
                first: ControllerButton::LeftBumper,
                second: Some(ControllerButton::RightBumper),
            }
            .validate()
            .is_ok()
        );
    }
}

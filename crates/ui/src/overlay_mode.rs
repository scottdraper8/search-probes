/// Passive feedback may draw over gameplay without activating any input interception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Interactive,
    Passive,
}

impl OverlayMode {
    pub fn select(
        focused: bool,
        drawable: bool,
        window_visible: bool,
        notifications: bool,
    ) -> Option<Self> {
        if !focused || !drawable {
            None
        } else if window_visible {
            Some(Self::Interactive)
        } else if notifications {
            Some(Self::Passive)
        } else {
            None
        }
    }

    pub fn captures_input(self) -> bool {
        self == Self::Interactive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_notifications_never_capture_gameplay_input() {
        assert!(
            !OverlayMode::select(true, true, false, true)
                .unwrap()
                .captures_input()
        );
        assert!(
            OverlayMode::select(true, true, true, true)
                .unwrap()
                .captures_input()
        );
        assert_eq!(OverlayMode::select(true, true, false, false), None);
        for visible in [false, true] {
            for notifications in [false, true] {
                assert_eq!(
                    OverlayMode::select(false, true, visible, notifications),
                    None
                );
                assert_eq!(
                    OverlayMode::select(true, false, visible, notifications),
                    None
                );
            }
        }
    }
}

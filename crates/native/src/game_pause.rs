use crate::platform;
use std::time::{Duration, Instant};

// Exact-build cGcDebugOptions global and libMBIN field offsets.
const DEBUG_OPTIONS_RVA: usize = 0x0534_A240;
const ALLOW_PAUSE_OFFSET: usize = 0x224B;
const ALWAYS_HAVE_FOCUS_OFFSET: usize = 0x2256;

#[derive(Clone, Copy, PartialEq, Eq)]
struct PauseState {
    allow_pause: u8,
    always_have_focus: u8,
}

impl PauseState {
    const FORCED_RUNNING: Self = Self {
        allow_pause: 0,
        always_have_focus: 1,
    };
}

pub struct GamePauseControl {
    allow_pause: usize,
    always_have_focus: usize,
    original: Option<PauseState>,
    last_maintenance: Instant,
}

impl GamePauseControl {
    pub fn new(image_base: usize) -> Result<Self, &'static str> {
        let options = image_base
            .checked_add(DEBUG_OPTIONS_RVA)
            .ok_or("debug options address overflow")?;
        Ok(Self {
            allow_pause: options + ALLOW_PAUSE_OFFSET,
            always_have_focus: options + ALWAYS_HAVE_FOCUS_OFFSET,
            original: None,
            last_maintenance: Instant::now(),
        })
    }

    fn read_bool(address: usize) -> Result<u8, &'static str> {
        let [value] = platform::read(address)?;
        if value > 1 {
            return Err("NMS pause setting is not a boolean");
        }
        Ok(value)
    }

    fn read(&self) -> Result<PauseState, &'static str> {
        Ok(PauseState {
            allow_pause: Self::read_bool(self.allow_pause)?,
            always_have_focus: Self::read_bool(self.always_have_focus)?,
        })
    }

    fn write(&self, from: PauseState, to: PauseState) -> Result<(), &'static str> {
        if from.always_have_focus != to.always_have_focus {
            unsafe { platform::write(self.always_have_focus, &[to.always_have_focus])? };
        }
        if from.allow_pause != to.allow_pause
            && let Err(error) = unsafe { platform::write(self.allow_pause, &[to.allow_pause]) }
        {
            if from.always_have_focus != to.always_have_focus {
                let _ =
                    unsafe { platform::write(self.always_have_focus, &[from.always_have_focus]) };
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn sync(&mut self, keep_game_running: bool) -> Result<(), &'static str> {
        if keep_game_running {
            let current = self.read()?;
            let activating = self.original.is_none();
            if activating {
                self.original = Some(current);
            }
            if let Err(error) = self.write(current, PauseState::FORCED_RUNNING) {
                if activating {
                    self.original = None;
                }
                return Err(error);
            }
            self.last_maintenance = Instant::now();
            return Ok(());
        }
        self.restore()
    }

    pub fn maintain(&mut self) -> Result<(), &'static str> {
        if self.original.is_some() && self.last_maintenance.elapsed() >= Duration::from_secs(1) {
            let current = self.read()?;
            self.write(current, PauseState::FORCED_RUNNING)?;
            self.last_maintenance = Instant::now();
        }
        Ok(())
    }

    pub fn restore(&mut self) -> Result<(), &'static str> {
        let Some(original) = self.original else {
            return Ok(());
        };
        let current = self.read()?;
        if current != PauseState::FORCED_RUNNING && current != original {
            return Err("NMS pause settings changed outside Search Probes; restart the game");
        }
        if current != original {
            self.write(current, original)?;
        }
        self.original = None;
        Ok(())
    }
}

impl Drop for GamePauseControl {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::PathBuf};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

use search_probes_model::supported::{EXECUTABLE_BYTES, STEAM_BUILD};
pub use search_probes_model::supported::{EXECUTABLE_SHA256, GAME_VERSION};

pub struct Image {
    base: usize,
    pub path: PathBuf,
}

impl Image {
    pub fn verify() -> Result<Self, String> {
        let path = crate::platform::executable_path().map_err(|error| error.to_string())?;
        let mut file = File::open(&path).map_err(|error| error.to_string())?;
        if file.metadata().map_err(|error| error.to_string())?.len() != EXECUTABLE_BYTES {
            return Err(format!(
                "Search Probes requires No Man's Sky {GAME_VERSION} (Steam build {STEAM_BUILD})."
            ));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let actual: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != EXECUTABLE_SHA256 {
            return Err(format!(
                "Unsupported NMS executable: {actual}. Search Probes requires {GAME_VERSION}, Steam build {STEAM_BUILD}."
            ));
        }
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        if base == 0 {
            return Err("The NMS executable is not mapped.".into());
        }
        Ok(Self { base, path })
    }
    pub fn base(&self) -> usize {
        self.base
    }
}

use crate::platform;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::System::Memory::{PAGE_READWRITE, VirtualProtect};

pub struct Import {
    slot: usize,
    original: usize,
    replacement: usize,
}

impl Drop for Import {
    fn drop(&mut self) {
        let mut old = 0;
        unsafe {
            if VirtualProtect(self.slot as *const _, 8, PAGE_READWRITE, &mut old) != 0 {
                let slot = &*(self.slot as *const AtomicUsize);
                let _ = slot.compare_exchange(
                    self.replacement,
                    self.original,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                let mut discarded = 0;
                VirtualProtect(self.slot as *const _, 8, old, &mut discarded);
            }
        }
    }
}

pub fn install(base: usize, names: &[(&[u8], usize)]) -> Result<Vec<Import>, String> {
    let pe = base + platform::u32_at(base + 60)? as usize;
    let optional = pe + 24;
    let size = platform::u32_at(optional + 56)? as usize;
    let import_rva = platform::u32_at(optional + 120)? as usize;
    let import_size = platform::u32_at(optional + 124)? as usize;
    let bounded = |rva: usize, length: usize| -> Result<usize, String> {
        if rva == 0 || rva.checked_add(length).is_none_or(|end| end > size) {
            return Err("invalid import extent".into());
        }
        Ok(base + rva)
    };
    let mut result = Vec::new();
    for offset in (0..import_size).step_by(20) {
        let descriptor = bounded(import_rva + offset, 20)?;
        let original = platform::u32_at(descriptor)? as usize;
        let name = platform::u32_at(descriptor + 12)? as usize;
        let first = platform::u32_at(descriptor + 16)? as usize;
        if name == 0 {
            break;
        }
        if original == 0 {
            return Err("import names unavailable".into());
        }
        for index in 0..4096 {
            let entry = u64::from_le_bytes(platform::read::<8>(bounded(original + index * 8, 8)?)?);
            if entry == 0 {
                break;
            }
            if entry & (1 << 63) != 0 {
                continue;
            }
            let address = bounded(entry as usize + 2, 128)?;
            let bytes = platform::read::<128>(address)?;
            let end = bytes
                .iter()
                .position(|b| *b == 0)
                .ok_or("unterminated import name")?;
            let Some((_, replacement)) = names.iter().find(|(name, _)| *name == &bytes[..end])
            else {
                continue;
            };
            let slot = bounded(first + index * 8, 8)?;
            if slot % 8 != 0 {
                return Err("unaligned import slot".into());
            }
            let value = u64::from_le_bytes(platform::read::<8>(slot)?) as usize;
            let mut old = 0;
            // The exact-build image owns this aligned IAT slot. Only the pointer is exchanged; executable bytes are untouched.
            unsafe {
                if VirtualProtect(slot as *const _, 8, PAGE_READWRITE, &mut old) == 0 {
                    return Err("cannot protect Vulkan import".into());
                }
                let exchanged = (&*(slot as *const AtomicUsize)).compare_exchange(
                    value,
                    *replacement,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                let mut discarded = 0;
                let restored = VirtualProtect(slot as *const _, 8, old, &mut discarded);
                if exchanged.is_ok() {
                    result.push(Import {
                        slot,
                        original: value,
                        replacement: *replacement,
                    });
                }
                if exchanged.is_err() || restored == 0 {
                    return Err("Vulkan import changed during installation".into());
                }
            }
        }
    }
    if result.len() != names.len() {
        return Err("required Vulkan imports are missing or duplicated".into());
    }
    Ok(result)
}

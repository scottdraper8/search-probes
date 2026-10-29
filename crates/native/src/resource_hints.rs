use search_probes_model::vocabulary::UNIQUE_RESOURCE_SET;

const HINTS: usize = 0x33f0;
const STRIDE: usize = 0x20;
const MAX_HINTS: i32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hints {
    pub unique_resources: u32,
    pub vile_brood: bool,
}

/// # Safety
/// Requires initialized 7.06 GcPlanetData from the query visitor. Its resource-hint array must
/// remain alive and immutable until this call returns. Only owned classifications are retained.
pub unsafe fn capture(planet: *const u8) -> Result<Hints, &'static str> {
    if planet.is_null() {
        return Err("null generated resource hints");
    }
    let count = unsafe { planet.add(HINTS + 8).cast::<i32>().read_unaligned() };
    if !(0..=MAX_HINTS).contains(&count) {
        return Err("invalid generated resource hint count");
    }
    let mut result = Hints {
        unique_resources: UNIQUE_RESOURCE_SET,
        vile_brood: false,
    };
    if count == 0 {
        return Ok(result);
    }
    let pointer = unsafe { planet.add(HINTS).cast::<usize>().read_unaligned() };
    if !(0x10000..=0x0000_7fff_ffff_ffff - count as usize * STRIDE).contains(&pointer) {
        return Err("invalid generated resource hint pointer");
    }
    for index in 0..count as usize {
        let id = unsafe { std::slice::from_raw_parts((pointer + index * STRIDE) as *const u8, 16) };
        let id = &id[..id.iter().position(|&byte| byte == 0).unwrap_or(16)];
        match id {
            b"UI_BONES_HINT" => result.unique_resources |= 1,
            b"UI_SCRAP_HINT" => result.unique_resources |= 2,
            b"UI_B_SWARM_HINT" => result.unique_resources |= 4,
            b"UI_BUGS_HINT" => result.vile_brood = true,
            _ => {}
        }
    }
    Ok(result)
}

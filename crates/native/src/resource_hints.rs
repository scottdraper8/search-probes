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

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(ids: &[&[u8]]) -> Hints {
        let mut rows = vec![0u8; ids.len() * STRIDE];
        for (index, id) in ids.iter().enumerate() {
            rows[index * STRIDE..][..id.len()].copy_from_slice(id);
        }
        let mut planet = vec![0u8; HINTS + 16];
        planet[HINTS..][..8].copy_from_slice(&(rows.as_ptr() as usize).to_le_bytes());
        planet[HINTS + 8..][..4].copy_from_slice(&(ids.len() as i32).to_le_bytes());
        unsafe { capture(planet.as_ptr()) }.unwrap()
    }

    #[test]
    fn uses_discovery_hints_and_keeps_brood_separate_from_resources() {
        for (ids, resources, brood) in [
            (vec![], 0, false),
            (vec![b"UI_BONES_HINT".as_slice(), b"PLANT_LUSH"], 1, false),
            (vec![b"UI_SCRAP_HINT".as_slice()], 2, false),
            (
                vec![b"UI_B_SWARM_HINT".as_slice(), b"UI_SCRAP_HINT"],
                6,
                false,
            ),
            (vec![b"PLANT_LUSH".as_slice(), b"UI_BUGS_HINT"], 0, true),
            (
                vec![
                    b"UI_BONES_HINT".as_slice(),
                    b"UI_BONES_HINT",
                    b"UI_BUGS_HINT",
                ],
                1,
                true,
            ),
            (
                vec![b"UI_SCRAP_HINT_X".as_slice(), b"UI_B_SWARM_OTHER"],
                0,
                false,
            ),
        ] {
            assert_eq!(
                classify(&ids),
                Hints {
                    unique_resources: UNIQUE_RESOURCE_SET | resources,
                    vile_brood: brood
                }
            );
        }
    }

    #[test]
    fn malformed_arrays_fail_instead_of_claiming_absence() {
        assert!(unsafe { capture(std::ptr::null()) }.is_err());
        let mut planet = vec![0u8; HINTS + 16];
        for count in [-1i32, 1, MAX_HINTS + 1] {
            planet[HINTS + 8..][..4].copy_from_slice(&count.to_le_bytes());
            assert!(unsafe { capture(planet.as_ptr()) }.is_err());
        }
        planet[HINTS + 8..][..4].copy_from_slice(&1i32.to_le_bytes());
        planet[HINTS..][..8].copy_from_slice(&usize::MAX.to_le_bytes());
        assert!(unsafe { capture(planet.as_ptr()) }.is_err());
    }
}

const ROLE_TABLE: usize = 0x3180;
const ROLE_STRIDE: usize = 0x5d8;
const MAX_ROLES: i32 = search_probes_model::fauna::MAX_SPECIES as i32;
const PLAYER_PREDATOR: u32 = 2;

fn height_cm(metres: f32) -> Result<i32, &'static str> {
    let centimetres = (metres * 100.).round();
    // Check representation, not physical plausibility. The native discovery-height calculation
    // can legitimately produce a negative number for small schooling fish.
    if !centimetres.is_finite()
        || f64::from(centimetres) < f64::from(i32::MIN)
        || f64::from(centimetres) > f64::from(i32::MAX)
    {
        return Err("unrepresentable generated fauna height");
    }
    Ok(centimetres as i32)
}

unsafe fn read<T: Copy>(pointer: *const u8, offset: usize) -> T {
    unsafe { pointer.add(offset).cast::<T>().read_unaligned() }
}

/// # Safety
/// `planet` must be initialized GcPlanetData from the query visitor. Its generated creature role
/// array must remain alive and immutable until this call returns.
pub unsafe fn player_attacking(planet: *const u8) -> Result<bool, &'static str> {
    if planet.is_null() {
        return Err("null generated fauna");
    }
    let roles = unsafe { read::<*const u8>(planet, ROLE_TABLE) };
    let count = unsafe { read::<i32>(planet, ROLE_TABLE + 8) };
    if !(0..=MAX_ROLES).contains(&count)
        || (count != 0
            && !(0x10000..=0x0000_7fff_ffff_ffff - count as usize * ROLE_STRIDE)
                .contains(&(roles as usize)))
    {
        return Err("invalid generated fauna role array");
    }
    let mut present = false;
    for index in 0..count as usize {
        let role = unsafe { roles.add(index * ROLE_STRIDE) };
        let kind = unsafe { read::<u32>(role, 0x57c) };
        let density = unsafe { read::<f32>(role, 0x5c4) };
        if kind > 10 || !density.is_finite() || density < 0. {
            return Err("invalid generated fauna role");
        }
        present |= kind == PLAYER_PREDATOR && density > 0.;
    }
    Ok(present)
}

#[derive(Debug)]
pub struct Observation {
    pub species: Vec<search_probes_model::fauna::Species>,
    pub player_attacking: bool,
    pub sandworms: bool,
}

/// # Safety
/// Same observation window as `player_attacking`. No native pointer is retained.
pub unsafe fn capture(planet: *const u8) -> Result<Observation, &'static str> {
    use search_probes_model::fauna::{Domain, Family, Species};
    let player_attacking = unsafe { player_attacking(planet)? };
    let sandworms = unsafe { sandworms(planet)? };
    let count = unsafe { read::<i32>(planet, ROLE_TABLE + 8) } as usize;
    let pointer = unsafe { read::<*const u8>(planet, ROLE_TABLE) };
    let mut species = Vec::with_capacity(count);
    for index in 0..count {
        let role = unsafe { pointer.add(index * ROLE_STRIDE) };
        if unsafe { read::<f32>(role, 0x5c4) } == 0. {
            continue;
        }
        let id = unsafe { std::slice::from_raw_parts(role.add(0x5a0), 16) };
        let id = &id[..id.iter().position(|b| *b == 0).unwrap_or(16)];
        let family = Family::from_id(id).ok_or("unclassified generated fauna body family")?;
        let tile = unsafe { read::<u32>(role, 0x5cc) };
        let activity = unsafe { read::<u32>(role, 0x558) };
        let rarity = unsafe { read::<u32>(role, 0x8c) };
        let hemisphere = unsafe { read::<u32>(role, 0x5c8) };
        if tile > 8 || activity > 4 || rarity > 2 || hemisphere > 2 {
            return Err("invalid generated fauna classification");
        }
        let domain = if tile == 4 {
            Domain::Aquatic
        } else if family.domains() == Domain::Airborne.bit() {
            Domain::Airborne
        } else {
            Domain::Land
        };
        if family.domains() & domain.bit() == 0 {
            return Err("unexpected fauna body family placement");
        }
        let mut heights_cm = [0; 2];
        for (n, height) in heights_cm.iter_mut().enumerate() {
            let metres = unsafe { read::<f32>(role, 0x84 + n * 4) };
            *height = height_cm(metres)?;
        }
        species.push(Species {
            family,
            domain,
            activity: activity as u8,
            rarity: rarity as u8,
            cave: tile == 5,
            hemisphere: hemisphere as u8,
            heights_cm,
        });
    }
    Ok(Observation {
        species,
        player_attacking,
        sandworms,
    })
}

/// # Safety
/// Requires live initialized GcPlanetData in the query visitor.
pub unsafe fn sandworms(planet: *const u8) -> Result<bool, &'static str> {
    if planet.is_null() {
        return Err("null generated fauna");
    }
    let present = unsafe { read::<u8>(planet, ROLE_TABLE + 0x18) };
    let frequency = unsafe { read::<f32>(planet, ROLE_TABLE + 0x14) };
    if present > 1 || !frequency.is_finite() || frequency < 0. {
        return Err("invalid generated sandworm presence");
    }
    Ok(present != 0)
}

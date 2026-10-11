const ROLE_TABLE: usize = 0x3180;
const ROLE_STRIDE: usize = 0x5d8;
const MAX_ROLES: i32 = search_probes_model::fauna::MAX_SPECIES as i32;
const PLAYER_PREDATOR: u32 = 2;

unsafe fn read<T: Copy>(pointer: *const u8, offset: usize) -> T {
    unsafe { pointer.add(offset).cast::<T>().read_unaligned() }
}

/// # Safety
/// `planet` must be initialized 7.06 GcPlanetData from the query visitor. Its generated creature
/// role array must remain alive and immutable until this call returns.
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
        if tile > 8 || activity > 4 || rarity > 2 {
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
            if !metres.is_finite() || !(0.0..=1000.0).contains(&metres) {
                return Err("invalid generated fauna height");
            }
            *height = (metres * 100.).round() as u32;
        }
        species.push(Species {
            family,
            domain,
            activity: activity as u8,
            rarity: rarity as u8,
            cave: tile == 5,
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
/// Requires live initialized 7.06 GcPlanetData in the query visitor.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(entries: &[(u32, f32)]) -> Result<bool, &'static str> {
        let mut roles = vec![0u8; entries.len() * ROLE_STRIDE];
        for (index, (kind, density)) in entries.iter().enumerate() {
            roles[index * ROLE_STRIDE + 0x57c..][..4].copy_from_slice(&kind.to_le_bytes());
            roles[index * ROLE_STRIDE + 0x5c4..][..4].copy_from_slice(&density.to_le_bytes());
        }
        let mut planet = vec![0u8; ROLE_TABLE + 16];
        planet[ROLE_TABLE..][..8].copy_from_slice(&(roles.as_ptr() as usize).to_le_bytes());
        planet[ROLE_TABLE + 8..][..4].copy_from_slice(&(entries.len() as i32).to_le_bytes());
        unsafe { player_attacking(planet.as_ptr()) }
    }

    #[test]
    fn requires_a_spawnable_player_predator_not_an_animal_hunter() {
        assert_eq!(classify(&[(1, 120.), (7, 36.), (3, 60.)]), Ok(false));
        assert_eq!(classify(&[(2, 0.), (1, 120.)]), Ok(false));
        assert_eq!(classify(&[(1, 120.), (2, 30.), (3, 60.)]), Ok(true));
        assert_eq!(classify(&[]), Ok(false));
    }

    #[test]
    fn invalid_roles_do_not_become_negative_matches() {
        for entry in [(11, 30.), (2, f32::NAN), (2, f32::INFINITY), (2, -1.)] {
            assert!(classify(&[(2, 30.), entry]).is_err());
        }
        assert!(unsafe { player_attacking(std::ptr::null()) }.is_err());
        let mut planet = vec![0u8; ROLE_TABLE + 16];
        for count in [-1i32, 1, MAX_ROLES + 1] {
            planet[ROLE_TABLE + 8..][..4].copy_from_slice(&count.to_le_bytes());
            assert!(unsafe { player_attacking(planet.as_ptr()) }.is_err());
        }
    }
    #[test]
    fn species_reader_uses_actual_tiles_and_body_routing_not_behaviour_type() {
        use search_probes_model::fauna::{Domain, Family};
        let mut roles = vec![0u8; 3 * ROLE_STRIDE];
        for (i, (id, tile)) in [
            (b"CRAB".as_slice(), 4u32),
            (b"BIRD".as_slice(), 1),
            (b"PLOUGH".as_slice(), 1),
        ]
        .into_iter()
        .enumerate()
        {
            let r = &mut roles[i * ROLE_STRIDE..][..ROLE_STRIDE];
            r[0x5a0..][..id.len()].copy_from_slice(id);
            r[0x57c..][..4].copy_from_slice(&4u32.to_le_bytes());
            r[0x5c4..][..4].copy_from_slice(&30f32.to_le_bytes());
            r[0x5cc..][..4].copy_from_slice(&tile.to_le_bytes());
            r[0x84..][..4].copy_from_slice(&1.25f32.to_le_bytes());
            r[0x88..][..4].copy_from_slice(&1.5f32.to_le_bytes());
        }
        let mut planet = vec![0u8; ROLE_TABLE + 32];
        planet[ROLE_TABLE..][..8].copy_from_slice(&(roles.as_ptr() as usize).to_le_bytes());
        planet[ROLE_TABLE + 8..][..4].copy_from_slice(&3i32.to_le_bytes());
        planet[ROLE_TABLE + 0x18] = 1;
        let observed = unsafe { capture(planet.as_ptr()) }.unwrap();
        assert!(observed.sandworms);
        assert!(!observed.player_attacking);
        assert_eq!(
            observed
                .species
                .iter()
                .map(|s| s.domain)
                .collect::<Vec<_>>(),
            vec![Domain::Aquatic, Domain::Airborne, Domain::Land]
        );
        assert_eq!(observed.species[0].family, Family::Crabs);
        assert_eq!(observed.species[0].heights_cm, [125, 150]);
        roles[0x84..][..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(unsafe { capture(planet.as_ptr()) }.is_err());
        roles[0x84..][..4].copy_from_slice(&1.25f32.to_le_bytes());
        roles[0x558..][..4].copy_from_slice(&5u32.to_le_bytes());
        assert!(unsafe { capture(planet.as_ptr()) }.is_err());
        planet[ROLE_TABLE + 0x18] = 2;
        assert!(unsafe { sandworms(planet.as_ptr()) }.is_err());
    }
}

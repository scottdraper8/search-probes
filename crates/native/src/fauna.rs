const ROLE_TABLE: usize = 0x3180;
const ROLE_STRIDE: usize = 0x5d8;
const MAX_ROLES: i32 = 128;
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
    if !(0..=MAX_ROLES).contains(&count) || (count != 0 && (roles as usize) < 0x10000) {
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
}

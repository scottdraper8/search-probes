//! Wild Arena facts are derived without actor spawning, archive scans, live entity registry
//! traversal, or native data pointer retention.
use search_probes_model::address::Address;
use search_probes_model::arena::{Creature, RankEvidence, Ranks};

type Result<T> = std::result::Result<T, &'static str>;
const RANK_FUNCTION_RVA: usize = 0x13d0270;
const RANK_WEIGHTS_RVA: usize = 0x53286e4;
const RANK_FORK_IDS_RVA: usize = 0x4afdf40;
const RANK_ROOT_FORK_ID_RVA: usize = 0x4ba8960;
const RANK_FUNCTION_PREFIX: [u8; 36] = [
    0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x48, 0x89, 0x7c, 0x24, 0x20, 0x44,
    0x89, 0x44, 0x24, 0x18, 0x55, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xec,
    0xe0, 0x02, 0x00, 0x00,
];
const RANK_FORK_IDS: [[u8; 16]; 3] = [
    *b"COMBAT\0\0\0\0\0\0\0\0\0\0",
    *b"SPEED\0\0\0\0\0\0\0\0\0\0\0",
    *b"HEALTH\0\0\0\0\0\0\0\0\0\0",
];
const RANK_ROOT_FORK_ID: [u8; 17] = *b"CORE_STAT_CLASSES";
const RANK_WEIGHTS: [u32; 12] = [30, 30, 25, 15, 30, 30, 25, 15, 30, 30, 25, 15];
type Roll = unsafe extern "system" fn(*mut u32, *const u64, i32, i32) -> u8;
unsafe fn read<T: Copy>(p: *const u8, o: usize) -> T {
    unsafe { p.add(o).cast::<T>().read_unaligned() }
}
fn rank_seed(role: &[u8], planet: u64, size: u8) -> u64 {
    rank_seed_selected(
        role,
        crate::fauna_models::component_id(role[0x5a0..0x5b0].try_into().unwrap()),
        planet,
        size,
    )
}
fn rank_seed_selected(role: &[u8], id: [u8; 16], planet: u64, size: u8) -> u64 {
    let low = u64::from_le_bytes(id[..8].try_into().unwrap());
    let high = u64::from_le_bytes(id[8..].try_into().unwrap());
    let resource = u64::from_le_bytes(role[0x5b0..0x5b8].try_into().unwrap());
    let engine = u32::from_le_bytes(role[0x5d0..0x5d4].try_into().unwrap());
    crate::fauna_models::pair(
        crate::fauna_models::pair(
            size as u64,
            crate::fauna_models::pair(engine as u64, planet),
        ),
        crate::fauna_models::pair(resource, crate::fauna_models::pair(high, low)),
    )
}
fn affinity(
    entry: crate::fauna_models::Entry,
    biome: u32,
    subtype: u32,
    map: [u8; 16],
) -> Result<u8> {
    if biome >= 16 || subtype >= 31 {
        return Err("invalid Arena biome classification");
    }
    let biome = match subtype {
        25 => 12,
        26 => 13,
        _ if (8..=10).contains(&biome) => 7,
        _ => biome,
    };
    let value = if entry.forced_affinity != 0 && entry.forced_affinity != 9 {
        entry.forced_affinity
    } else {
        map[biome as usize]
    };
    if !(1..=8).contains(&value) {
        return Err("unsupported generated Arena affinity");
    }
    Ok(value)
}
pub struct Context {
    catalogue: crate::fauna_models::Catalogue,
    affinities: [u8; 16],
    roll: Roll,
}
impl Context {
    fn ranks(&self, role: &[u8], planet: u64, size: u8) -> Result<Ranks> {
        let seed = [rank_seed(role, planet, size), 1];
        let mut output = [u32::MAX; 3];
        // Construction verifies this pure native function; only this owned output is written.
        let ok = unsafe { (self.roll)(output.as_mut_ptr(), seed.as_ptr(), 0, 3) };
        if ok != 1 || output.iter().any(|&r| r > 3) {
            return Err("invalid native Arena ranks");
        }
        Ok(Ranks {
            health: output[0] as u8,
            agility: output[1] as u8,
            combat: output[2] as u8,
        })
    }
    /// # Safety
    /// The waiting gameplay callback keeps the catalogue and globals fixed. The rank function reads
    /// globals and writes only caller-owned stack output.
    pub unsafe fn from_verified_image(base: usize) -> Result<Self> {
        unsafe {
            let image = base as *const u8;
            let code = image.add(RANK_FUNCTION_RVA);
            if std::slice::from_raw_parts(code, RANK_FUNCTION_PREFIX.len()) != RANK_FUNCTION_PREFIX
            {
                return Err("Arena rank function prefix mismatch");
            }
            if read::<[[u8; 16]; 3]>(image, RANK_FORK_IDS_RVA) != RANK_FORK_IDS
                || read::<[u8; 17]>(image, RANK_ROOT_FORK_ID_RVA) != RANK_ROOT_FORK_ID
            {
                return Err("Arena rank fork IDs do not match 7.13");
            }
            let catalogue = crate::fauna_models::Catalogue::from_verified_image(base)?;
            let affinities = catalogue.affinity_map();
            let weights = read::<[u32; 12]>(image, RANK_WEIGHTS_RVA);
            if weights != RANK_WEIGHTS {
                return Err("Arena rank weights do not match 7.13");
            }
            Ok(Self {
                catalogue,
                affinities,
                roll: std::mem::transmute::<*const u8, Roll>(code),
            })
        }
    }
    fn role(
        &self,
        role_index: usize,
        role: &[u8],
        planet: u64,
        biome: u32,
        subtype: u32,
    ) -> Result<Option<Creature>> {
        let id: [u8; 16] = role[0x5a0..0x5b0].try_into().unwrap();
        let entry = self.catalogue.entry(id)?;
        let component = self
            .catalogue
            .entry(crate::fauna_models::component_id(id))?;
        if !component.eligible {
            return Ok(None);
        }
        let affinity = affinity(component, biome, subtype, self.affinities)?;
        let ranks = if let Some(size) = self.catalogue.arena_size_class(entry, role)? {
            RankEvidence::exact(self.ranks(role, planet, size)?)?
        } else {
            // All four adult classes bound every depth/rounding outcome. The matcher may settle
            // unanimous decisions, but must not claim these classes are all physically reachable.
            let mut possible_ranks = 0;
            for size in 0..4 {
                possible_ranks |= self.ranks(role, planet, size)?.bit();
            }
            RankEvidence::bounded(possible_ranks)?
        };
        let role = u8::try_from(role_index)
            .ok()
            .filter(|role| *role < 30)
            .ok_or("Arena creature role exceeds discovery-table bounds")?;
        Ok(Some(Creature {
            role,
            affinity,
            ranks,
        }))
    }
    /// # Safety
    /// Same initialized immutable query-planet lifetime as `fauna::capture`; no pointers escape.
    /// `address` is the exact planet address supplied to GenerateQueryInfo. The temporary query
    /// planet leaves its loaded-world address field unset, so it cannot supply the Arena seed.
    pub unsafe fn capture(&self, planet: *const u8, address: Address) -> Result<Vec<Creature>> {
        self.capture_inputs(&unsafe { crate::arena_inputs::Planet::capture(planet, address) }?)
    }

    /// Evaluate the same bounded role observations after the query visitor has released its data.
    /// Resource acquisition and placement refinement remain separate from this initial bound.
    pub fn capture_inputs(&self, inputs: &crate::arena_inputs::Planet) -> Result<Vec<Creature>> {
        if !(1..=8).contains(&inputs.address.coordinates().planet) {
            return Err("Arena inputs require an exact generated planet address");
        }
        let mut creatures = Vec::with_capacity(inputs.roles.len());
        for role in &inputs.roles {
            if let Some(creature) = self.role(
                role.index(),
                &role.calculation_bytes(),
                inputs.address.raw(),
                inputs.biome,
                inputs.subtype,
            )? {
                creatures.push(creature);
            }
        }
        Ok(creatures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "system" fn known_roll(
        output: *mut u32,
        _seed: *const u64,
        first: i32,
        last: i32,
    ) -> u8 {
        if first != 0 || last != 3 {
            return 0;
        }
        unsafe { output.copy_from_nonoverlapping([3, 0, 1].as_ptr(), 3) };
        1
    }

    #[test]
    fn rank_outputs_keep_the_game_health_speed_combat_projection() {
        let context = Context {
            catalogue: crate::fauna_models::Catalogue::empty_for_tests(),
            affinities: [1, 4, 3, 6, 2, 5, 5, 7, 7, 7, 7, 1, 4, 3, 7, 1],
            roll: known_roll,
        };
        let role = [0; 0x5d8];

        assert_eq!(
            context.ranks(&role, 0, 0).unwrap(),
            Ranks {
                health: 3,
                agility: 0,
                combat: 1,
            }
        );
    }
}

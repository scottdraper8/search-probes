//! Shared generated-creature catalogue and identity calculations.
//!
//! Search results are produced from temporary generated planet data, before any creature actor or
//! discovery-display record exists. The role and catalogue nevertheless contain every input NMS
//! uses to derive the creature's stable discovery name and Arena size class.

use search_probes_model::{address::Address, fauna::NameIdentity};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, &'static str>;

const ROLE_BYTES: usize = 0x5d8;
const CATALOGUE_ENTRY_BYTES: usize = 0xd0;
const PET_BIOME_AFFINITIES: [u8; 16] = [1, 4, 3, 6, 2, 5, 5, 7, 7, 7, 7, 1, 4, 3, 7, 1];

unsafe fn read<T: Copy>(pointer: *const u8, offset: usize) -> T {
    unsafe { pointer.add(offset).cast::<T>().read_unaligned() }
}

fn pointer(pointer: *const u8, size: usize) -> Result<()> {
    if !(0x10000..=0x0000_7fff_ffff_ffffusize.saturating_sub(size)).contains(&(pointer as usize)) {
        return Err("invalid creature catalogue pointer");
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub struct Entry {
    pub eligible: bool,
    pub forced_affinity: u8,
    source: crate::fauna_model_scalars::Catalogue,
}

fn entry(bytes: &[u8]) -> Result<([u8; 16], Entry)> {
    if bytes.len() != CATALOGUE_ENTRY_BYTES {
        return Err("invalid creature catalogue extent");
    }
    let id = bytes[0x30..0x40].try_into().unwrap();
    let eligible = bytes[0xc9];
    let affinity = bytes[0xcc];
    let minimum = f32::from_le_bytes(bytes[0xa8..0xac].try_into().unwrap());
    let maximum = f32::from_le_bytes(bytes[0xa4..0xa8].try_into().unwrap());
    if eligible > 1
        || affinity > 9
        || !minimum.is_finite()
        || !maximum.is_finite()
        || minimum <= 0.
        || maximum < minimum
    {
        return Err("invalid creature catalogue entry");
    }
    Ok((
        id,
        Entry {
            eligible: eligible != 0,
            forced_affinity: affinity,
            source: crate::fauna_model_scalars::Catalogue::from_native_bytes(
                bytes.try_into().unwrap(),
            ),
        },
    ))
}

#[derive(Clone, Copy)]
struct SizeRules {
    maximum: f32,
    multiplier: f32,
    thresholds: [f32; 3],
    quarter: f32,
    uniform: f64,
    infant_multiplier: f32,
}

impl SizeRules {
    fn current_7_14() -> Self {
        Self {
            maximum: 8.0,
            multiplier: 1.0,
            thresholds: [1.2, 2.5, 4.5],
            quarter: 0.25,
            uniform: 1.0 / 4_294_967_296.0,
            infant_multiplier: 1.0,
        }
    }

    fn validate(self) -> Result<Self> {
        if ![
            self.maximum,
            self.multiplier,
            self.quarter,
            self.infant_multiplier,
        ]
        .into_iter()
        .chain(self.thresholds)
        .all(|value| value.is_finite() && value > 0.)
            || !self.uniform.is_finite()
            || self.uniform <= 0.
            || self.thresholds[0] >= self.thresholds[1]
            || self.thresholds[1] >= self.thresholds[2]
        {
            return Err("invalid creature size rules");
        }
        Ok(self)
    }

    fn scale(self, entry: Entry, role: &[u8]) -> Result<f32> {
        let role: &[u8; ROLE_BYTES] = role
            .try_into()
            .map_err(|_| "invalid generated creature role extent")?;
        let scale = crate::fauna_model_scalars::calculate(
            role,
            entry.source,
            crate::fauna_model_scalars::Parameters {
                maximum: self.maximum,
                quarter: self.quarter,
                uniform: self.uniform,
                infant_multiplier: self.infant_multiplier,
            },
        )?
        .adult_scale
            * self.multiplier;
        if !scale.is_finite() || scale <= 0. {
            return Err("invalid generated creature scale");
        }
        Ok(scale)
    }

    fn exact_class(self, entry: Entry, role: &[u8]) -> Result<u8> {
        let scale = self.scale(entry, role)?;
        Ok(self
            .thresholds
            .iter()
            .filter(|&&threshold| scale >= threshold)
            .count() as u8)
    }

    fn conservative_class(self, entry: Entry, role: &[u8]) -> Result<Option<u8>> {
        let scale = self.scale(entry, role)?;
        // Arena ranks use the transformed actor basis. Underwater placement and float rounding at
        // a boundary cannot be proven from the remote query, so retain the existing rank bound.
        let tile = u32::from_le_bytes(role[0x5cc..0x5d0].try_into().unwrap());
        if tile == 4
            || self.thresholds.iter().any(|&threshold| {
                (scale - threshold).abs() <= 64. * f32::EPSILON * scale.max(threshold)
            })
        {
            return Ok(None);
        }
        Ok(Some(
            self.thresholds
                .iter()
                .filter(|&&threshold| scale >= threshold)
                .count() as u8,
        ))
    }
}

pub struct Catalogue {
    entries: BTreeMap<[u8; 16], Entry>,
    size: SizeRules,
}

impl Catalogue {
    /// # Safety
    /// `base` is the verified NMS 7.14 image and its application catalogue remains live.
    pub unsafe fn from_verified_image(base: usize) -> Result<Self> {
        unsafe {
            let image = base as *const u8;
            let application = read::<*const u8>(image, 0x6f6c288);
            pointer(application, 0x5c5550)?;
            let count = read::<i32>(application, 0x5c5548);
            if !(1..=32).contains(&count) {
                return Err("invalid creature catalogue groups");
            }
            let mut entries = BTreeMap::new();
            for group in 0..count as usize {
                let header = read::<*const u8>(application, 0x5c5448 + group * 8);
                pointer(header, 16)?;
                let data = read::<*const u8>(header, 0);
                let length = read::<i32>(header, 8);
                if !(0..=512).contains(&length) || entries.len() + length as usize > 1024 {
                    return Err("creature catalogue exceeds bound");
                }
                if length != 0 {
                    pointer(data, length as usize * CATALOGUE_ENTRY_BYTES)?;
                }
                for index in 0..length as usize {
                    let (id, value) = entry(std::slice::from_raw_parts(
                        data.add(index * CATALOGUE_ENTRY_BYTES),
                        CATALOGUE_ENTRY_BYTES,
                    ))?;
                    if entries.insert(id, value).is_some() {
                        return Err("duplicate creature catalogue ID");
                    }
                }
            }
            Ok(Self {
                entries,
                size: SizeRules::current_7_14().validate()?,
            })
        }
    }

    pub fn entry(&self, id: [u8; 16]) -> Result<Entry> {
        self.entries
            .get(&id)
            .copied()
            .ok_or("missing generated creature catalogue entry")
    }

    pub fn affinity_map(&self) -> [u8; 16] {
        PET_BIOME_AFFINITIES
    }

    pub fn arena_size_class(&self, entry: Entry, role: &[u8]) -> Result<Option<u8>> {
        self.size.conservative_class(entry, role)
    }

    pub fn naming_identity(&self, role: &[u8], address: Address) -> Result<NameIdentity> {
        if role.len() != ROLE_BYTES || !(1..=8).contains(&address.coordinates().planet) {
            return Err("creature naming requires an exact generated planet role");
        }
        let id: [u8; 16] = role[0x5a0..0x5b0].try_into().unwrap();
        let entry = self.entry(id)?;
        let size = self.size.exact_class(entry, role)?;
        let completion = u64::from_le_bytes(role[0x5b0..0x5b8].try_into().unwrap());
        let valid = role[0x5b8];
        let creature_type = u32::from_le_bytes(role[0x5d0..0x5d4].try_into().unwrap());
        if valid != 1 || completion == 0 {
            return Err("generated creature has no stable naming identity");
        }
        Ok(identity_from_parts(
            component_id(id),
            completion,
            creature_type,
            address.raw(),
            size,
        ))
    }

    #[cfg(test)]
    pub fn empty_for_tests() -> Self {
        Self {
            entries: BTreeMap::new(),
            size: SizeRules::current_7_14(),
        }
    }
}

pub fn pair(a: u64, b: u64) -> u64 {
    const MULTIPLIER: u64 = 0x9ddfea08eb382d69;
    let a = (a ^ b).wrapping_mul(MULTIPLIER);
    let a = a ^ (a >> 47);
    let b = (b ^ a).wrapping_mul(MULTIPLIER);
    let b = b ^ (b >> 47);
    b.wrapping_mul(MULTIPLIER)
}

pub fn component_id(role_id: [u8; 16]) -> [u8; 16] {
    match role_id.as_slice() {
        b"CRAB\0\0\0\0\0\0\0\0\0\0\0\0" | b"HERMITCRAB\0\0\0\0\0\0" => {
            *b"SPIDER\0\0\0\0\0\0\0\0\0\0"
        }
        b"MANTAGLOW\0\0\0\0\0\0\0" => *b"MANTA\0\0\0\0\0\0\0\0\0\0\0",
        b"SEAHORSEGLOW\0\0\0\0" => *b"SEAHORSE\0\0\0\0\0\0\0\0",
        _ => role_id,
    }
}

fn identity_from_parts(
    id: [u8; 16],
    completion: u64,
    creature_type: u32,
    planet: u64,
    size: u8,
) -> NameIdentity {
    let low = u64::from_le_bytes(id[..8].try_into().unwrap());
    let high = u64::from_le_bytes(id[8..].try_into().unwrap());
    let node = pair(high, low);
    NameIdentity {
        species_seed: pair(completion, node),
        genus_seed: pair(size as u64, pair(creature_type as u64, planet)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_triceratops_identity_matches_the_games_display_record() {
        let identity = identity_from_parts(
            *b"TRICERATOPS\0\0\0\0\0",
            0xf03f_105d_6fdf_0cbc,
            0x1a,
            0x0031_083a_267f_f7fd,
            1,
        );
        assert_eq!(identity.species_seed, 0x3468_4bae_4a89_cca1);
        assert_eq!(identity.genus_seed, 0x73cb_086a_4707_0866);
    }

    #[test]
    fn generated_role_produces_the_observed_triceratops_identity() {
        let id = *b"TRICERATOPS\0\0\0\0\0";
        let mut entries = BTreeMap::new();
        entries.insert(
            id,
            Entry {
                eligible: true,
                forced_affinity: 0,
                source: crate::fauna_model_scalars::Catalogue {
                    minimum_scale: 2.0,
                    maximum_scale: 2.0,
                    dependency_probability: 0.0,
                },
            },
        );
        let catalogue = Catalogue {
            entries,
            size: SizeRules::current_7_14(),
        };
        let mut role = [0u8; ROLE_BYTES];
        role[0x5a0..0x5b0].copy_from_slice(&id);
        role[0x5b0..0x5b8].copy_from_slice(&0xf03f_105d_6fdf_0cbcu64.to_le_bytes());
        role[0x5b8] = 1;
        role[0x5d0..0x5d4].copy_from_slice(&0x1au32.to_le_bytes());
        let address = Address::from_raw(0x0031_083a_267f_f7fd).unwrap();

        let identity = catalogue.naming_identity(&role, address).unwrap();

        assert_eq!(identity.species_seed, 0x3468_4bae_4a89_cca1);
        assert_eq!(identity.genus_seed, 0x73cb_086a_4707_0866);
    }

    #[test]
    fn component_aliases_match_native_scene_selection() {
        assert_eq!(
            component_id(*b"HERMITCRAB\0\0\0\0\0\0"),
            *b"SPIDER\0\0\0\0\0\0\0\0\0\0"
        );
        assert_eq!(
            component_id(*b"MANTAGLOW\0\0\0\0\0\0\0"),
            *b"MANTA\0\0\0\0\0\0\0\0\0\0\0"
        );
    }
}

//! Wild Arena facts from generated roles and the loaded 7.06 catalogue. No actor spawning, archive
//! scans, live entity registry traversal, or native data pointer retention is required.
use search_probes_model::address::Address;
use search_probes_model::arena::{Creature, RankEvidence, Ranks};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, &'static str>;
pub(crate) const RANK_WEIGHTS_RVA: usize = 0x531e024;
pub(crate) const AFFINITIES_RVA: usize = 0x531e5dc;
type Roll = unsafe extern "system" fn(*mut u32, *const u64, i32, i32) -> u8;
unsafe fn read<T: Copy>(p: *const u8, o: usize) -> T {
    unsafe { p.add(o).cast::<T>().read_unaligned() }
}
fn pointer(p: *const u8, size: usize) -> Result<()> {
    if !(0x10000..=0x0000_7fff_ffff_ffffusize.saturating_sub(size)).contains(&(p as usize)) {
        return Err("invalid Arena data pointer");
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Catalogue {
    eligible: bool,
    forced_affinity: u8,
    source: crate::fauna_model_scalars::Catalogue,
}
fn catalogue(bytes: &[u8]) -> Result<([u8; 16], Catalogue)> {
    if bytes.len() != 0xd0 {
        return Err("invalid Arena catalogue extent");
    }
    let id = bytes[0x30..0x40].try_into().unwrap();
    let eligible = bytes[0xc9];
    let affinity = bytes[0xcc];
    let min = f32::from_le_bytes(bytes[0xa8..0xac].try_into().unwrap());
    let max = f32::from_le_bytes(bytes[0xa4..0xa8].try_into().unwrap());
    if eligible > 1
        || affinity > 9
        || !min.is_finite()
        || !max.is_finite()
        || min <= 0.
        || max < min
    {
        return Err("invalid Arena catalogue entry");
    }
    Ok((
        id,
        Catalogue {
            eligible: eligible != 0,
            forced_affinity: affinity,
            source: crate::fauna_model_scalars::Catalogue::from_native_bytes(
                bytes.try_into().unwrap(),
            ),
        },
    ))
}
#[derive(Clone, Copy)]
struct SizeGlobals {
    maximum: f32,
    multiplier: f32,
    thresholds: [f32; 3],
    quarter: f32,
    uniform: f64,
    infant_multiplier: f32,
}
impl SizeGlobals {
    fn validate(self) -> Result<Self> {
        if ![
            self.maximum,
            self.multiplier,
            self.quarter,
            self.infant_multiplier,
        ]
        .into_iter()
        .chain(self.thresholds)
        .all(|v| v.is_finite() && v > 0.)
            || !self.uniform.is_finite()
            || self.uniform <= 0.
            || self.thresholds[0] >= self.thresholds[1]
            || self.thresholds[1] >= self.thresholds[2]
        {
            return Err("invalid Arena size globals");
        }
        Ok(self)
    }
    fn model_scalars(
        self,
        entry: Catalogue,
        role: &[u8],
    ) -> Result<crate::fauna_model_scalars::ModelScalars> {
        crate::fauna_model_scalars::calculate(
            role.try_into().map_err(|_| "invalid Arena role extent")?,
            entry.source,
            crate::fauna_model_scalars::Parameters {
                maximum: self.maximum,
                quarter: self.quarter,
                uniform: self.uniform,
                infant_multiplier: self.infant_multiplier,
            },
        )
    }
    fn class(self, entry: Catalogue, role: &[u8]) -> Result<Option<u8>> {
        let scale = self.model_scalars(entry, role)?.adult_scale * self.multiplier;
        if !scale.is_finite() || scale <= 0. {
            return Err("invalid generated Arena scale");
        }
        // The native actor derives length from its transformed basis; do not assert an exact rank
        // when float rounding around a size-class boundary could change the seed. Underwater
        // depth scaling likewise needs placement information absent from a remote planet query.
        let tile = u32::from_le_bytes(role[0x5cc..0x5d0].try_into().unwrap());
        if tile == 4
            || self
                .thresholds
                .iter()
                .any(|&t| (scale - t).abs() <= 64. * f32::EPSILON * scale.max(t))
        {
            return Ok(None);
        }
        Ok(Some(
            self.thresholds.iter().filter(|&&t| scale >= t).count() as u8,
        ))
    }
}
fn pair(a: u64, b: u64) -> u64 {
    const M: u64 = 0x9ddfea08eb382d69;
    let a = (a ^ b).wrapping_mul(M);
    let a = a ^ (a >> 47);
    let b = (b ^ a).wrapping_mul(M);
    let b = b ^ (b >> 47);
    b.wrapping_mul(M)
}
// The role family selects the scale catalogue, but the native rank hash reads the selected
// GcCreatureComponentData.Id. Current single-component scene assets qualify these aliases;
// CRAB also has live actor/resource/role association and visor rank parity.
fn component_id(role_id: [u8; 16]) -> [u8; 16] {
    match role_id.as_slice() {
        b"CRAB\0\0\0\0\0\0\0\0\0\0\0\0" | b"HERMITCRAB\0\0\0\0\0\0" => {
            *b"SPIDER\0\0\0\0\0\0\0\0\0\0"
        }
        b"MANTAGLOW\0\0\0\0\0\0\0" => *b"MANTA\0\0\0\0\0\0\0\0\0\0\0",
        b"SEAHORSEGLOW\0\0\0\0" => *b"SEAHORSE\0\0\0\0\0\0\0\0",
        _ => role_id,
    }
}
fn rank_seed(role: &[u8], planet: u64, size: u8) -> u64 {
    rank_seed_selected(
        role,
        component_id(role[0x5a0..0x5b0].try_into().unwrap()),
        planet,
        size,
    )
}
fn rank_seed_selected(role: &[u8], id: [u8; 16], planet: u64, size: u8) -> u64 {
    let low = u64::from_le_bytes(id[..8].try_into().unwrap());
    let high = u64::from_le_bytes(id[8..].try_into().unwrap());
    let resource = u64::from_le_bytes(role[0x5b0..0x5b8].try_into().unwrap());
    let engine = u32::from_le_bytes(role[0x5d0..0x5d4].try_into().unwrap());
    pair(
        pair(size as u64, pair(engine as u64, planet)),
        pair(resource, pair(high, low)),
    )
}
fn affinity(entry: Catalogue, biome: u32, subtype: u32, map: [u8; 16]) -> Result<u8> {
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
    catalogue: BTreeMap<[u8; 16], Catalogue>,
    size: SizeGlobals,
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
    /// Exact hash-verified image; the waiting gameplay callback keeps its catalogue and globals
    /// fixed. The audited rank function reads globals and writes only caller-owned stack output.
    pub unsafe fn from_verified_image(base: usize) -> Result<Self> {
        unsafe {
            let image = base as *const u8;
            let code = image.add(0x13cc070);
            if std::slice::from_raw_parts(code, 8)
                != [0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74]
            {
                return Err("Arena rank function prefix mismatch");
            }
            let app = read::<*const u8>(image, 0x6f60828);
            pointer(app, 0x5c5550)?;
            let count = read::<i32>(app, 0x5c5548);
            if !(1..=32).contains(&count) {
                return Err("invalid Arena catalogue groups");
            }
            let mut entries = BTreeMap::new();
            for i in 0..count as usize {
                let header = read::<*const u8>(app, 0x5c5448 + i * 8);
                pointer(header, 16)?;
                let data = read::<*const u8>(header, 0);
                let n = read::<i32>(header, 8);
                if !(0..=512).contains(&n) || entries.len() + n as usize > 1024 {
                    return Err("Arena catalogue exceeds bound");
                }
                if n != 0 {
                    pointer(data, n as usize * 0xd0)?;
                }
                for j in 0..n as usize {
                    let (id, value) =
                        catalogue(std::slice::from_raw_parts(data.add(j * 0xd0), 0xd0))?;
                    if entries.insert(id, value).is_some() {
                        return Err("duplicate Arena catalogue ID");
                    }
                }
            }
            let size = SizeGlobals {
                maximum: read(image, 0x535037c),
                multiplier: read(image, 0x5350108),
                thresholds: [
                    read(image, 0x53500e8),
                    read(image, 0x53500c4),
                    read(image, 0x535007c),
                ],
                quarter: read(image, 0x4bebe00),
                uniform: read(image, 0x4beb558),
                infant_multiplier: read(image, 0x53502c8),
            }
            .validate()?;
            let affinities = read::<[u8; 16]>(image, AFFINITIES_RVA);
            if affinities.iter().any(|v| !(1..=8).contains(v)) {
                return Err("invalid Arena affinity globals");
            }
            let weights = read::<[u32; 12]>(image, RANK_WEIGHTS_RVA);
            if weights.as_chunks::<4>().0.iter().any(|w| {
                w.iter()
                    .try_fold(0i32, |sum, &v| {
                        i32::try_from(v).ok().and_then(|v| sum.checked_add(v))
                    })
                    .is_none_or(|sum| sum == 0)
            }) {
                return Err("invalid Arena rank weights");
            }
            Ok(Self {
                catalogue: entries,
                size,
                affinities,
                roll: std::mem::transmute::<*const u8, Roll>(code),
            })
        }
    }
    fn role(&self, role: &[u8], planet: u64, biome: u32, subtype: u32) -> Result<Option<Creature>> {
        let id: [u8; 16] = role[0x5a0..0x5b0].try_into().unwrap();
        let entry = *self
            .catalogue
            .get(&id)
            .ok_or("missing generated Arena catalogue entry")?;
        let component = *self
            .catalogue
            .get(&component_id(id))
            .ok_or("missing Arena component catalogue entry")?;
        if !component.eligible {
            return Ok(None);
        }
        let affinity = affinity(component, biome, subtype, self.affinities)?;
        let ranks = if let Some(size) = self.size.class(entry, role)? {
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
        Ok(Some(Creature { affinity, ranks }))
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

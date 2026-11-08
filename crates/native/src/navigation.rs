use crate::{
    discovery_hook, game_state::Snapshot, identity::Image, mission_text_hook, navigation_hook,
    platform,
};
use search_probes_model::{
    address::Address,
    fauna::{Domain, Family},
    target::{FAUNA_SCAN_READY_PROGRESS, Target, TargetKind},
};
use std::time::{Duration, Instant};

const PLANET_COUNT: usize = 0x2544;
const PLANET_TABLE: usize = 0x2e30;
const PLANET_STRIDE: usize = 0xd9170;
const PLANET_ADDRESS: usize = 0x8;
const PLANET_DATA: usize = 0x60;
const ROLE_TABLE: usize = 0x3180;
const ROLE_STRIDE: usize = 0x5d8;
const ROLE_RARITY: usize = 0x8c;
const ROLE_ACTIVITY: usize = 0x558;
const ROLE_FAMILY: usize = 0x5a0;
const ROLE_SEED: usize = 0x5b0;
const ROLE_SEED_VALID: usize = 0x5b8;
const ROLE_DENSITY: usize = 0x5c4;
const ROLE_TILE: usize = 0x5cc;
const CREATURE_TABLE: usize = 0xe1130;
const CREATURE_STRIDE: usize = 0x24a0;
const CREATURE_COMPLETION_ID: usize = 0x2390;
const CREATURE_COMPLETION_ID_VALID: usize = 0x2398;
const CREATURE_NODE_SEED: usize = 0x23a0;
const CREATURE_SPECIES_SEED: usize = 0x23a8;
const CREATURE_GENUS_SEED: usize = 0x23b0;
const SIGNAL_MISSION: &[u8] = b"SQN_SP_SIGNAL";
const MAX_RETAINED_DISCOVERIES: usize = 64;
const SELECT_MISSION_RVA: usize = 0x9d7de0;
const SELECT_MISSION_PREFIX: [u8; 26] = [
    0x48, 0x8b, 0xc4, 0x44, 0x88, 0x40, 0x18, 0x53, 0x56, 0x57, 0x41, 0x54, 0x48, 0x81, 0xec, 0x58,
    0x01, 0x00, 0x00, 0x45, 0x33, 0xe4, 0x41, 0x0f, 0xb6, 0xf8,
];

#[repr(C)]
struct Seed {
    value: u64,
    valid: bool,
}

fn fixed<const N: usize>(value: &[u8]) -> Result<[u8; N], &'static str> {
    if value.is_empty() || value.len() >= N || value.contains(&0) {
        return Err("invalid native identifier");
    }
    let mut result = [0; N];
    result[..value.len()].copy_from_slice(value);
    Ok(result)
}

fn vector(address: usize) -> Result<(usize, usize), &'static str> {
    let capacity = platform::u32_at(address)? as usize;
    let count = platform::u32_at(address + 4)? as usize;
    if count > capacity || capacity > 4096 {
        return Err("unsupported mission collection");
    }
    Ok((
        if count > 0 {
            platform::pointer(address + 8)?
        } else {
            0
        },
        count,
    ))
}

#[derive(Clone, Copy)]
struct Active {
    index: usize,
    mission: usize,
    target: Target,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct MissionKey {
    identity: [u8; 16],
    seed: [u8; 16],
}

fn mission_key(mission: usize) -> Result<MissionKey, &'static str> {
    Ok(MissionKey {
        identity: platform::read(mission + 8)?,
        seed: platform::read(mission + 0x100)?,
    })
}

fn incomplete(mission: usize) -> Result<bool, &'static str> {
    let stages = platform::u32_at(mission + 0x1c)?;
    let progress = platform::u32_at(mission + 0x48)? as i32;
    Ok(progress >= 0 && (progress as u32) < stages)
}

fn selected(manager: usize) -> Result<Option<(usize, usize)>, &'static str> {
    let (pointer, count) = vector(manager + 0x1d8)?;
    let index = platform::u32_at(manager + 0x268)? as usize;
    if index >= count {
        return Ok(None);
    }
    Ok(Some((index, platform::pointer(pointer + index * 8)?)))
}

fn selected_return(manager: usize) -> Result<Option<MissionKey>, &'static str> {
    let Some((_, mission)) = selected(manager)? else {
        return Ok(None);
    };
    let key = mission_key(mission)?;
    Ok((incomplete(mission)? && TargetKind::from_mission(&key.identity)?.is_none()).then_some(key))
}

fn previous(manager: usize) -> Result<Option<MissionKey>, &'static str> {
    let key = MissionKey {
        identity: platform::read(manager + 0x248)?,
        seed: platform::read(manager + 0x258)?,
    };
    Ok(key.identity.iter().any(|byte| *byte != 0).then_some(key))
}

fn resolve(manager: usize, key: MissionKey) -> Result<Option<(usize, usize)>, &'static str> {
    let (pointer, count) = vector(manager + 0x1d8)?;
    let mut result = None;
    for index in 0..count {
        let mission = platform::pointer(pointer + index * 8)?;
        if mission_key(mission)? == key
            && incomplete(mission)?
            && result.replace((index, mission)).is_some()
        {
            return Err("previous mission identity is ambiguous");
        }
    }
    Ok(result)
}

fn fallback_mission(manager: usize) -> Result<Option<(usize, usize)>, &'static str> {
    let (pointer, count) = vector(manager + 0x1d8)?;
    for index in 0..count {
        let mission = platform::pointer(pointer + index * 8)?;
        let identity = platform::read::<16>(mission + 8)?;
        let identity = identity.split(|byte| *byte == 0).next().unwrap_or_default();
        if !identity.starts_with(b"SQN_SP_") && incomplete(mission)? {
            return Ok(Some((index, mission)));
        }
    }
    Ok(None)
}

unsafe fn select_mission(
    image: &Image,
    manager: usize,
    index: usize,
    mission: usize,
) -> Result<(), &'static str> {
    let address = image.base() + SELECT_MISSION_RVA;
    if unsafe { std::slice::from_raw_parts(address as *const u8, SELECT_MISSION_PREFIX.len()) }
        != SELECT_MISSION_PREFIX
    {
        return Err("NMS mission selection function does not match this build");
    }
    type Select = unsafe extern "system" fn(usize, usize, bool);
    let select = unsafe { std::mem::transmute::<usize, Select>(address) };
    unsafe { select(manager, mission, false) };
    if platform::u32_at(manager + 0x268)? as usize != index {
        return Err("NMS did not select the requested mission");
    }
    Ok(())
}

fn active(manager: usize) -> Result<Option<Active>, &'static str> {
    let (pointer, count) = vector(manager + 0x1d8)?;
    let mut result = None;
    for index in 0..count {
        let mission = platform::pointer(pointer + index * 8)?;
        let identity = platform::read::<16>(mission + 8)?;
        if platform::read::<1>(mission + 0x108)? != [1] {
            continue;
        }
        let stages = platform::u32_at(mission + 0x1c)?;
        let progress = platform::u32_at(mission + 0x48)? as i32;
        if progress >= 0 && (progress as u32) < stages {
            let Some((kind, fauna_role)) = TargetKind::from_mission(&identity)? else {
                continue;
            };
            let seed = u64::from_le_bytes(platform::read(mission + 0x100)?);
            let target = crate::destination::from_seed(kind, fauna_role, seed)?;
            if result
                .replace(Active {
                    index,
                    mission,
                    target,
                })
                .is_some()
            {
                return Err("multiple Search Probes navigation missions are active");
            }
        }
    }
    Ok(result)
}

fn pending(manager: usize) -> Result<bool, &'static str> {
    for offset in [0x198, 0x1a8] {
        let (pointer, count) = vector(manager + offset)?;
        for index in 0..count {
            let identity = platform::read::<16>(pointer + index * 0x28)?;
            if TargetKind::from_mission(&identity)?.is_some() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn route(application: usize) -> Result<Option<Target>, &'static str> {
    let manager = platform::pointer(application + 0x4deab0)?;
    let count = platform::u32_at(manager + 0xd4)? as usize;
    if count > 1024 {
        return Err("unsupported active scan-event collection");
    }
    let pointer = if count > 0 {
        platform::pointer(manager + 0xd8)?
    } else {
        0
    };
    let mut result = None;
    for index in 0..count {
        let record = pointer + index * 0x70;
        let identity = platform::read::<16>(record + 0x40)?;
        let Some((kind, fauna_role)) = TargetKind::from_mission(&identity)? else {
            continue;
        };
        if platform::read::<32>(record + 0x10)? != fixed::<32>(kind.event().as_bytes())?
            || platform::u32_at(record + 0x64)? != 3
        {
            return Err("unexpected Search Probes route");
        }
        let raw_address = u64::from_le_bytes(platform::read(record + 0x50)?);
        let seed = u64::from_le_bytes(platform::read(record + 0x58)?);
        let target = crate::destination::from_seed(kind, fauna_role, seed)?;
        if raw_address != 0 {
            let address = Address::from_raw(raw_address)?;
            if target.address != address {
                return Err("Search Probes route does not match its selected destination");
            }
        }
        if result.replace(target).is_some() {
            return Err("multiple Search Probes routes are active");
        }
    }
    Ok(result)
}

struct FaunaTarget {
    completion_id: u64,
    domain: Domain,
    cave: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FaunaIdentity {
    species_seed: u64,
    genus_seed: u64,
}

fn fauna_identity(
    application: usize,
    completion_id: u64,
) -> Result<Option<FaunaIdentity>, &'static str> {
    let mut result = None;
    // Search roles and display records are independently ordered. Join them by the completion
    // identity copied into both records; a role index is never a display-table slot index.
    for slot in 0..usize::from(search_probes_model::target::FAUNA_ROLES) {
        let record = application + CREATURE_TABLE + slot * CREATURE_STRIDE;
        let valid = platform::read::<1>(record + CREATURE_COMPLETION_ID_VALID)?[0];
        if valid > 1 {
            return Err("invalid generated creature completion identity");
        }
        if valid == 0
            || u64::from_le_bytes(platform::read(record + CREATURE_COMPLETION_ID)?) != completion_id
        {
            continue;
        }
        let node_seed = u64::from_le_bytes(platform::read(record + CREATURE_NODE_SEED)?);
        let identity = FaunaIdentity {
            species_seed: u64::from_le_bytes(platform::read(record + CREATURE_SPECIES_SEED)?),
            genus_seed: u64::from_le_bytes(platform::read(record + CREATURE_GENUS_SEED)?),
        };
        if node_seed == 0 || identity.species_seed == 0 || identity.genus_seed == 0 {
            return Err("generated creature has an incomplete naming identity");
        }
        if result.replace(identity).is_some() {
            return Err("generated creature completion identity is ambiguous");
        }
    }
    Ok(result)
}

fn fauna_target(snapshot: &Snapshot, target: Target) -> Result<FaunaTarget, &'static str> {
    let planet = usize::from(target.address.coordinates().planet);
    let count = platform::u32_at(snapshot.root + PLANET_COUNT)? as usize;
    if planet == 0 || planet > count || count > 8 {
        return Err("fauna destination does not identify a loaded planet");
    }
    let object = snapshot.root + PLANET_TABLE + (planet - 1) * PLANET_STRIDE;
    let address = u64::from_le_bytes(platform::read(object + PLANET_ADDRESS)?);
    if address != target.address.raw() {
        return Err("loaded fauna planet does not match the selected destination");
    }
    let data = object + PLANET_DATA;
    let roles = platform::pointer(data + ROLE_TABLE)?;
    let count = platform::u32_at(data + ROLE_TABLE + 8)? as usize;
    let role = usize::from(target.fauna_role.ok_or("fauna destination has no role")?);
    if role >= count || count > search_probes_model::fauna::MAX_SPECIES {
        return Err("fauna destination role is not loaded");
    }
    let role = roles + role * ROLE_STRIDE;
    let completion_id = u64::from_le_bytes(platform::read(role + ROLE_SEED)?);
    if platform::read::<1>(role + ROLE_SEED_VALID)? != [1] || completion_id == 0 {
        return Err("fauna destination role has no stable completion identity");
    }
    let density = f32::from_le_bytes(platform::read(role + ROLE_DENSITY)?);
    let tile = platform::u32_at(role + ROLE_TILE)?;
    let activity = platform::u32_at(role + ROLE_ACTIVITY)?;
    let rarity = platform::u32_at(role + ROLE_RARITY)?;
    if !density.is_finite() || density <= 0. || tile > 8 || activity > 4 || rarity > 2 {
        return Err("invalid fauna destination role");
    }
    let identity = platform::read::<16>(role + ROLE_FAMILY)?;
    let end = identity.iter().position(|byte| *byte == 0).unwrap_or(16);
    let family =
        Family::from_id(&identity[..end]).ok_or("unclassified fauna destination body family")?;
    let domain = if tile == 4 {
        Domain::Aquatic
    } else if family.domains() == Domain::Airborne.bit() {
        Domain::Airborne
    } else {
        Domain::Land
    };
    if family.domains() & domain.bit() == 0 {
        return Err("unexpected fauna destination placement");
    }
    Ok(FaunaTarget {
        completion_id,
        domain,
        cave: tile == 5,
    })
}

#[derive(Default)]
pub struct Navigation {
    requested: Option<(Target, Instant)>,
    destination: Option<Target>,
    discoveries: Vec<discovery_hook::Discovery>,
    signaled: Option<Target>,
    described: Option<(Target, Option<FaunaIdentity>)>,
    return_to: Option<MissionKey>,
}

impl Navigation {
    pub fn destination(&self) -> Option<Target> {
        self.destination
    }

    /// # Safety
    /// Mission and scan managers plus mod-owned definitions remain live for this callback.
    pub unsafe fn start(
        &mut self,
        image: &Image,
        snapshot: &Snapshot,
        target: Target,
    ) -> Result<(), &'static str> {
        let address = target.address;
        target.validate()?;
        if address.coordinates().galaxy != snapshot.address.coordinates().galaxy {
            return Err("navigation requires a destination in the current galaxy");
        }
        let manager = snapshot.application + 0x86e3d0;
        if self.requested.is_some() || pending(manager)? {
            return Err("a Search Probes destination is still starting");
        }
        if active(manager)?.is_some() {
            return Err(
                "abandon the Search Probes mission in the Log before choosing another destination",
            );
        }
        let return_to = selected_return(manager)?;
        navigation_hook::reset();
        discovery_hook::reset();
        mission_text_hook::clear();
        self.discoveries.clear();
        self.signaled = None;
        self.described = None;
        if target.kind == TargetKind::Fauna && snapshot.address.system() == target.address.system()
        {
            unsafe { self.describe_fauna(image, snapshot, target)? };
        }
        let mission = target.mission()?;
        type Start = unsafe extern "system" fn(usize, *const u8, *const Seed, bool, bool) -> bool;
        let start = unsafe { std::mem::transmute::<usize, Start>(image.base() + 0x9ced00) };
        let accepted = unsafe {
            start(
                manager,
                fixed::<16>(mission.as_bytes())?.as_ptr(),
                &Seed {
                    value: crate::destination::seed(target)?,
                    valid: true,
                },
                false,
                true,
            )
        };
        if !accepted {
            return Err("NMS declined the Search Probes request");
        }
        self.requested = Some((target, Instant::now()));
        self.return_to = return_to;
        Ok(())
    }

    unsafe fn describe_fauna(
        &mut self,
        image: &Image,
        snapshot: &Snapshot,
        target: Target,
    ) -> Result<(), &'static str> {
        let fauna = fauna_target(snapshot, target)?;
        let identity = fauna_identity(snapshot.application, fauna.completion_id)?;
        if self.described == Some((target, identity)) {
            return Ok(());
        }
        let name = if let Some(identity) = identity {
            let context = unsafe { crate::fauna::Context::from_verified_image(image.base()) }?;
            Some(unsafe { context.name(identity.species_seed, identity.genus_seed) }?)
        } else {
            None
        };
        mission_text_hook::set_fauna(name.as_deref(), fauna.domain, fauna.cave)?;
        self.described = Some((target, identity));
        Ok(())
    }

    unsafe fn update_fauna_text(
        &mut self,
        image: &Image,
        snapshot: &Snapshot,
        active: Option<Active>,
    ) -> Result<(), &'static str> {
        let Some(active) = active.filter(|active| active.target.kind == TargetKind::Fauna) else {
            if self.described.take().is_some() {
                mission_text_hook::clear();
            }
            return Ok(());
        };
        let target = active.target;
        if snapshot.address.system() != target.address.system() {
            if self.described.take().is_some() {
                mission_text_hook::clear();
            }
            return Ok(());
        }
        unsafe { self.describe_fauna(image, snapshot, target) }
    }

    unsafe fn restore_completed_selection(
        &mut self,
        image: &Image,
        manager: usize,
    ) -> Result<(), &'static str> {
        let Some((_, completed)) = selected(manager)? else {
            return Ok(());
        };
        let identity = platform::read::<16>(completed + 8)?;
        let stages = platform::u32_at(completed + 0x1c)?;
        let progress = platform::u32_at(completed + 0x48)?;
        if TargetKind::from_mission(&identity)?.is_none() || stages == 0 || progress != stages {
            return Ok(());
        }
        let mut candidate = if let Some(key) = previous(manager)? {
            resolve(manager, key)?
        } else {
            None
        };
        if candidate.is_none()
            && let Some(key) = self.return_to
        {
            candidate = resolve(manager, key)?;
        }
        if candidate.is_none() {
            candidate = fallback_mission(manager)?;
        }
        if let Some((index, mission)) = candidate {
            unsafe { select_mission(image, manager, index, mission) }?;
        }
        self.return_to = None;
        Ok(())
    }

    unsafe fn update_fauna_completion(
        &mut self,
        image: &Image,
        snapshot: &Snapshot,
        manager: usize,
        active: Option<Active>,
    ) -> Result<(), &'static str> {
        let incoming = discovery_hook::take()?;
        let Some(active) = active.filter(|active| active.target.kind == TargetKind::Fauna) else {
            self.discoveries.clear();
            self.signaled = None;
            return Ok(());
        };
        let target = active.target;
        self.discoveries.extend(
            incoming
                .into_iter()
                .filter(|discovery| discovery.address == target.address),
        );
        if self.discoveries.len() > MAX_RETAINED_DISCOVERIES {
            self.discoveries
                .drain(..self.discoveries.len() - MAX_RETAINED_DISCOVERIES);
        }
        if self.signaled == Some(target) {
            self.discoveries.clear();
            return Ok(());
        }
        if platform::u32_at(active.mission + 0x48)? != FAUNA_SCAN_READY_PROGRESS
            || snapshot.address != target.address
        {
            return Ok(());
        }
        let expected = fauna_target(snapshot, target)?.completion_id;
        let matched = self.discoveries.iter().any(|discovery| {
            discovery.address == target.address && discovery.first_local_id() == Some(expected)
        });
        self.discoveries.clear();
        if !matched {
            return Ok(());
        }
        type Start = unsafe extern "system" fn(usize, *const u8, *const Seed, bool, bool) -> bool;
        let start = unsafe { std::mem::transmute::<usize, Start>(image.base() + 0x9ced00) };
        let accepted = unsafe {
            start(
                manager,
                fixed::<16>(SIGNAL_MISSION)?.as_ptr(),
                &Seed {
                    value: crate::destination::seed(target)?,
                    valid: true,
                },
                false,
                false,
            )
        };
        if !accepted {
            return Err("NMS declined the creature-scan completion signal");
        }
        self.signaled = Some(target);
        Ok(())
    }

    /// # Safety
    /// Mission selection runs only on the gameplay callback with current manager ownership.
    pub unsafe fn update(
        &mut self,
        image: &Image,
        snapshot: &Snapshot,
    ) -> Result<(), &'static str> {
        let manager = snapshot.application + 0x86e3d0;
        let published = match navigation_hook::published() {
            Ok(address) => address,
            Err(error) => {
                self.requested = None;
                return Err(error);
            }
        };
        let mission = active(manager)?;
        let routed = route(snapshot.application)?;
        if let (Some(active), Some(routed)) = (mission, routed)
            && active.target != routed
        {
            return Err("Search Probes route does not match its active mission");
        }
        self.destination = mission.map(|active| active.target);
        unsafe {
            self.update_fauna_text(image, snapshot, mission)?;
            self.update_fauna_completion(image, snapshot, manager, mission)?;
            self.restore_completed_selection(image, manager)?;
        }
        if let Some((expected, started)) = self.requested {
            if let Some(active) =
                mission.filter(|active| active.target == expected && routed == Some(expected))
            {
                unsafe { select_mission(image, manager, active.index, active.mission) }?;
                self.requested = None;
            } else if published == Some(expected) && mission.is_none() && !pending(manager)? {
                self.requested = None;
            } else if started.elapsed() > Duration::from_secs(30) {
                self.requested = None;
                return Err(
                    "NMS has not activated the destination. Check Search Probes in the Log.",
                );
            }
        }
        Ok(())
    }
}

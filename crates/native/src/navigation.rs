use crate::{game_state::Snapshot, identity::Image, navigation_hook, platform};
use search_probes_model::{
    address::Address,
    target::{Target, TargetKind},
};
use std::time::{Duration, Instant};

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

fn active(manager: usize) -> Result<Option<(usize, usize)>, &'static str> {
    let (pointer, count) = vector(manager + 0x1c0)?;
    let mut result = None;
    for index in 0..count {
        let mission = platform::pointer(pointer + index * 8)?;
        let identity = platform::read::<16>(mission + 8)?;
        if !TargetKind::ALL
            .into_iter()
            .any(|kind| fixed::<16>(kind.mission().as_bytes()) == Ok(identity))
            || platform::read::<1>(mission + 0x108)? != [1]
        {
            continue;
        }
        let stages = platform::u32_at(mission + 0x1c)?;
        let progress = platform::u32_at(mission + 0x48)? as i32;
        if progress >= 0 && (progress as u32) < stages {
            if result.replace((index, mission)).is_some() {
                return Err("multiple Search Probe Target missions are active");
            }
        }
    }
    Ok(result)
}

fn pending(manager: usize) -> Result<bool, &'static str> {
    for offset in [0x180, 0x190] {
        let (pointer, count) = vector(manager + offset)?;
        for index in 0..count {
            let identity = platform::read::<16>(pointer + index * 0x28)?;
            if TargetKind::ALL
                .into_iter()
                .any(|kind| fixed::<16>(kind.mission().as_bytes()) == Ok(identity))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn route(application: usize) -> Result<Option<Target>, &'static str> {
    let manager = platform::pointer(application + 0x4de450)?;
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
        let Some(kind) = TargetKind::ALL
            .into_iter()
            .find(|kind| fixed::<16>(kind.mission().as_bytes()) == Ok(identity))
        else {
            continue;
        };
        if platform::read::<32>(record + 0x10)? != fixed::<32>(kind.event().as_bytes())?
            || platform::u32_at(record + 0x64)? != 3
        {
            return Err("unexpected Search Probe Target route");
        }
        let address = Address::from_raw(u64::from_le_bytes(platform::read(record + 0x50)?))?;
        let seed = u64::from_le_bytes(platform::read(record + 0x58)?);
        let target = crate::destination::from_seed(kind, seed)?;
        if target.address != address {
            return Err("Search Probe Target route does not match its selected destination");
        }
        if result.replace(target).is_some() {
            return Err("multiple Search Probe Target routes are active");
        }
    }
    Ok(result)
}

#[derive(Default)]
pub struct Navigation {
    requested: Option<(Target, Instant)>,
    destination: Option<Target>,
}

impl Navigation {
    pub fn destination(&self) -> Option<Target> {
        self.destination
    }

    /// # Safety
    /// The verified game's mission/scan managers and mod-owned definitions are live on this callback.
    pub unsafe fn start(
        &mut self,
        image: &Image,
        snapshot: &Snapshot,
        target: Target,
    ) -> Result<(), &'static str> {
        let address = target.address;
        let kind = target.kind;
        target.validate()?;
        if address.coordinates().galaxy != snapshot.address.coordinates().galaxy {
            return Err("navigation requires a destination in the current galaxy");
        }
        let manager = snapshot.application + 0x847bb0;
        if self.requested.is_some() || pending(manager)? {
            return Err("a Search Probe Target is still starting");
        }
        if active(manager)?.is_some() {
            return Err(
                "abandon Search Probe Target in the Log before choosing another destination",
            );
        }
        type Lookup = unsafe extern "system" fn(usize, *const u8, i32) -> usize;
        let lookup = unsafe { std::mem::transmute::<usize, Lookup>(image.base() + 0x4e6bc0) };
        let name = fixed::<32>(kind.event().as_bytes())?;
        let scan_manager = platform::pointer(snapshot.application + 0x4de450)?;
        let table = platform::pointer(scan_manager + 0x30)?;
        platform::read::<16>(table)?;
        let event = unsafe { lookup(scan_manager, name.as_ptr(), 3) };
        if event == 0 {
            return Err(
                "Search Probes navigation assets are not loaded. Enable mods in NMS and restart the game.",
            );
        }
        if platform::read::<32>(event + 0x228)? != name || platform::u32_at(event + 0x48c)? != 4 {
            return Err("Search Probes navigation assets are missing or incompatible");
        }
        if platform::u32_at(event + 0x430)? != 1 {
            return Err("navigation definition must contain one destination");
        }
        let entry = platform::pointer(event + 0x428)?;
        let text = platform::pointer(entry)?;
        let previous = platform::read::<17>(text)?;
        if previous[16] != 0 || !previous[..16].iter().all(u8::is_ascii_hexdigit) {
            return Err("navigation definition has an invalid destination buffer");
        }
        navigation_hook::reset();
        #[repr(C)]
        struct Seed {
            value: u64,
            valid: bool,
        }
        type Start = unsafe extern "system" fn(usize, *const u8, *const Seed, bool, bool) -> bool;
        let start = unsafe { std::mem::transmute::<usize, Start>(image.base() + 0x9b8e00) };
        let accepted = unsafe {
            start(
                manager,
                fixed::<16>(kind.mission().as_bytes())?.as_ptr(),
                &Seed {
                    value: address.raw(),
                    valid: true,
                },
                false,
                true,
            )
        };
        if !accepted {
            return Err("NMS declined the Search Probe Target request");
        }
        self.requested = Some((target, Instant::now()));
        Ok(())
    }

    /// # Safety
    /// Mission selection runs only on the verified gameplay callback with current manager ownership.
    pub unsafe fn update(
        &mut self,
        image: &Image,
        snapshot: &Snapshot,
    ) -> Result<(), &'static str> {
        let manager = snapshot.application + 0x847bb0;
        let published = match navigation_hook::published() {
            Ok(address) => address,
            Err(error) => {
                self.requested = None;
                return Err(error);
            }
        };
        let mission = active(manager)?;
        let routed = route(snapshot.application)?;
        self.destination = mission.and(routed);
        if let Some((expected, started)) = self.requested {
            if let Some((index, mission)) = mission.filter(|_| routed == Some(expected)) {
                type Select = unsafe extern "system" fn(usize, usize, bool);
                let select =
                    unsafe { std::mem::transmute::<usize, Select>(image.base() + 0x9c15f0) };
                unsafe { select(manager, mission, false) };
                if platform::u32_at(manager + 0x250)? as usize != index {
                    return Err("NMS did not select Search Probe Target");
                }
                self.requested = None;
            } else if published == Some(expected) && mission.is_none() && !pending(manager)? {
                self.requested = None;
            } else if started.elapsed() > Duration::from_secs(30) {
                self.requested = None;
                return Err(
                    "NMS has not activated the destination. Check Search Probe Target in the Log.",
                );
            }
        }
        Ok(())
    }
}

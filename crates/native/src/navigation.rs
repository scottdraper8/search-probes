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
    let (pointer, count) = vector(manager + 0x1d8)?;
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
        let Some(kind) = TargetKind::ALL
            .into_iter()
            .find(|kind| fixed::<16>(kind.mission().as_bytes()) == Ok(identity))
        else {
            continue;
        };
        if platform::read::<32>(record + 0x10)? != fixed::<32>(kind.event().as_bytes())?
            || platform::u32_at(record + 0x64)? != 3
        {
            return Err("unexpected Search Probes route");
        }
        let address = Address::from_raw(u64::from_le_bytes(platform::read(record + 0x50)?))?;
        let seed = u64::from_le_bytes(platform::read(record + 0x58)?);
        let target = crate::destination::from_seed(kind, seed)?;
        if target.address != address {
            return Err("Search Probes route does not match its selected destination");
        }
        if result.replace(target).is_some() {
            return Err("multiple Search Probes routes are active");
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
    /// Mission and scan managers plus mod-owned definitions remain live for this callback.
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
        let manager = snapshot.application + 0x86e3d0;
        if self.requested.is_some() || pending(manager)? {
            return Err("a Search Probes destination is still starting");
        }
        if active(manager)?.is_some() {
            return Err(
                "abandon the Search Probes mission in the Log before choosing another destination",
            );
        }
        navigation_hook::reset();
        #[repr(C)]
        struct Seed {
            value: u64,
            valid: bool,
        }
        type Start = unsafe extern "system" fn(usize, *const u8, *const Seed, bool, bool) -> bool;
        let start = unsafe { std::mem::transmute::<usize, Start>(image.base() + 0x9ced00) };
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
            return Err("NMS declined the Search Probes request");
        }
        self.requested = Some((target, Instant::now()));
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
        self.destination = mission.and(routed);
        if let Some((expected, started)) = self.requested {
            if let Some((index, mission)) = mission.filter(|_| routed == Some(expected)) {
                type Select = unsafe extern "system" fn(usize, usize, bool);
                let select_address = image.base() + 0x9d7de0;
                const SELECT_PREFIX: [u8; 26] = [
                    0x48, 0x8b, 0xc4, 0x44, 0x88, 0x40, 0x18, 0x53, 0x56, 0x57, 0x41, 0x54, 0x48,
                    0x81, 0xec, 0x58, 0x01, 0x00, 0x00, 0x45, 0x33, 0xe4, 0x41, 0x0f, 0xb6, 0xf8,
                ];
                if unsafe {
                    std::slice::from_raw_parts(select_address as *const u8, SELECT_PREFIX.len())
                } != SELECT_PREFIX
                {
                    return Err("NMS mission selection function does not match this build");
                }
                let select = unsafe { std::mem::transmute::<usize, Select>(select_address) };
                unsafe { select(manager, mission, false) };
                if platform::u32_at(manager + 0x268)? as usize != index {
                    return Err("NMS did not select Search Probes");
                }
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

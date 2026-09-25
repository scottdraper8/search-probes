use crate::{identity::Image, platform};
use search_probes_model::address::{Address, Coordinates};

#[derive(Clone, PartialEq, Eq)]
pub struct Epoch {
    transition: u64,
    application: usize,
    root: usize,
    language: u32,
    difficulty: u32,
    object_cache: u32,
    solar_resources: [u8; 16],
    configuration: [u8; 0xbb0],
}

pub struct Snapshot {
    pub epoch: Epoch,
    pub address: Address,
    pub application: usize,
    pub planet: usize,
    pub difficulty: usize,
}

#[cfg(feature = "investigation")]
impl Epoch {
    pub fn differences(&self, other: &Self) -> Vec<&'static str> {
        let mut changed = Vec::new();
        macro_rules! compare {
            ($($field:ident),+) => {$(if self.$field != other.$field { changed.push(stringify!($field)); })+};
        }
        compare!(
            transition,
            application,
            root,
            language,
            difficulty,
            object_cache,
            solar_resources,
            configuration
        );
        changed
    }
}

impl Snapshot {
    pub fn read(image: &Image, transition: u64) -> Result<Self, &'static str> {
        let base = image.base();
        let application = platform::pointer(base + 0x6e7aae8)?;
        let root = platform::pointer(application + 0x71af70)?;
        let raw = platform::read::<24>(application + 0xc1a0)?;
        let components: [i32; 6] = std::array::from_fn(|index| {
            i32::from_le_bytes(
                raw[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("coordinate word"),
            )
        });
        let address = Address::new(Coordinates {
            planet: components[0]
                .try_into()
                .map_err(|_| "invalid current planet")?,
            system: components[1]
                .try_into()
                .map_err(|_| "invalid current system")?,
            x: components[2]
                .try_into()
                .map_err(|_| "invalid current region X")?,
            y: components[3]
                .try_into()
                .map_err(|_| "invalid current region Y")?,
            z: components[4]
                .try_into()
                .map_err(|_| "invalid current region Z")?,
            galaxy: components[5]
                .try_into()
                .map_err(|_| "invalid current galaxy")?,
        })?;
        if address.coordinates().system == 0 {
            return Err("no loaded system");
        }
        let difficulty = platform::u32_at(application + 0x315a2c)?;
        if difficulty > 3 {
            return Err("unsupported combat difficulty");
        }
        let planet = root + 0x520570;
        let mut configuration = platform::read::<0xbb0>(planet)?;
        configuration[0xba4..0xbac].fill(0);
        let solar_resources = platform::read::<16>(root + 0x521180 + 0x6c0)?;
        if solar_resources
            .chunks_exact(8)
            .any(|bytes| u64::from_le_bytes(bytes.try_into().expect("resource pointer")) < 0x10000)
        {
            return Err("solar generator resources are not ready");
        }
        let object_cache = platform::u32_at(base + 0x7376be8)?;
        if object_cache == 0 || object_cache == u32::MAX {
            return Err("object resources are not ready");
        }
        let epoch = Epoch {
            transition,
            application,
            root,
            language: platform::u32_at(base + 0x6e01558)?,
            difficulty,
            object_cache,
            solar_resources,
            configuration,
        };
        Ok(Self {
            epoch,
            address,
            application,
            planet,
            difficulty: difficulty as usize,
        })
    }
}

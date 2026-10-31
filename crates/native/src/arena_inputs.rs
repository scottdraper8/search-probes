//! Detached inputs for model selection and wild Arena calculations. These are scalar/string
//! fields, not copied native objects. They may outlive the planet-query visitor.
use search_probes_model::address::Address;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Role {
    index: usize,
    minimum: [u8; 4],
    maximum: [u8; 4],
    behaviour: [u8; 4],
    filter: [u8; 32],
    family: [u8; 16],
    seed: [u8; 8],
    seed_valid: u8,
    tile: [u8; 4],
    engine: [u8; 4],
    active: bool,
}

impl Role {
    fn copy(index: usize, bytes: &[u8; 0x5d8]) -> Self {
        Self {
            index,
            minimum: bytes[0x574..0x578].try_into().unwrap(),
            maximum: bytes[0x56c..0x570].try_into().unwrap(),
            behaviour: bytes[0x57c..0x580].try_into().unwrap(),
            filter: bytes[0x580..0x5a0].try_into().unwrap(),
            family: bytes[0x5a0..0x5b0].try_into().unwrap(),
            seed: bytes[0x5b0..0x5b8].try_into().unwrap(),
            seed_valid: bytes[0x5b8],
            tile: bytes[0x5cc..0x5d0].try_into().unwrap(),
            engine: bytes[0x5d0..0x5d4].try_into().unwrap(),
            active: f32::from_le_bytes(bytes[0x5c4..0x5c8].try_into().unwrap()) > 0.,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Population construction keeps every source slot for the native RNG sequence. Only
    /// positive-density roles appear in the planet's generated fauna list; an inactive slot
    /// must not become a creature witness merely because its source model can be prepared.
    pub fn active(&self) -> bool {
        self.active
    }

    /// Compatibility input for Models::source, Context::source_scalars and the Arena role
    /// calculators only. All fields those routines consume are preserved bit-for-bit. This is
    /// deliberately NOT a GcCreatureRoleData suitable for passing to a native spawn routine.
    pub(crate) fn calculation_bytes(&self) -> [u8; 0x5d8] {
        let mut bytes = [0; 0x5d8];
        bytes[0x574..0x578].copy_from_slice(&self.minimum);
        bytes[0x56c..0x570].copy_from_slice(&self.maximum);
        bytes[0x57c..0x580].copy_from_slice(&self.behaviour);
        bytes[0x580..0x5a0].copy_from_slice(&self.filter);
        bytes[0x5a0..0x5b0].copy_from_slice(&self.family);
        bytes[0x5b0..0x5b8].copy_from_slice(&self.seed);
        bytes[0x5b8] = self.seed_valid;
        bytes[0x5cc..0x5d0].copy_from_slice(&self.tile);
        bytes[0x5d0..0x5d4].copy_from_slice(&self.engine);
        bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planet {
    pub address: Address,
    pub biome: u32,
    pub subtype: u32,
    pub roles: Vec<Role>,
}

unsafe fn read<T: Copy>(p: *const u8, offset: usize) -> T {
    unsafe { p.add(offset).cast::<T>().read_unaligned() }
}

impl Planet {
    /// # Safety
    /// `planet` and its role array remain initialized and immutable for this visitor.
    /// `address` is the exact planet argument supplied to the query, never the temporary planet's
    /// loaded-world address. The returned object contains no native addresses or borrowed data.
    pub unsafe fn capture(planet: *const u8, address: Address) -> Result<Self> {
        unsafe { Self::capture_roles(planet, address, false) }
    }

    /// Capture the complete role array for population construction, including zero-density slots.
    /// Their indices and total count participate in the native population seed sequence.
    /// # Safety
    /// Same immutable completed native query and exact-address contract as `capture`.
    pub unsafe fn capture_for_population(planet: *const u8, address: Address) -> Result<Self> {
        unsafe { Self::capture_roles(planet, address, true) }
    }

    unsafe fn capture_roles(planet: *const u8, address: Address, all: bool) -> Result<Self> {
        unsafe { crate::fauna::player_attacking(planet)? };
        let index = unsafe { read::<i32>(planet, 0x353c) };
        if !(0..8).contains(&index) || i32::from(address.coordinates().planet) != index + 1 {
            return Err("Arena query address does not identify the generated planet");
        }
        let pointer = unsafe { read::<*const u8>(planet, 0x3180) };
        let count = unsafe { read::<i32>(planet, 0x3188) } as usize;
        let mut roles = Vec::with_capacity(count);
        for index in 0..count {
            let row = unsafe { pointer.add(index * 0x5d8) };
            if !all && unsafe { read::<f32>(row, 0x5c4) } == 0. {
                continue;
            }
            let row = unsafe { &*row.cast::<[u8; 0x5d8]>() };
            roles.push(Role::copy(index, row));
        }
        Ok(Self {
            address,
            biome: unsafe { read(planet, 0x32b8) },
            subtype: unsafe { read(planet, 0x32bc) },
            roles,
        })
    }
}

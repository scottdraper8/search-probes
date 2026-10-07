use search_probes_model::address::Address;
use std::{marker::PhantomData, rc::Rc};

type Lifecycle = unsafe extern "system" fn(*mut u8);
type Populate = unsafe extern "system" fn(u64, *mut u8, *const f32);

pub struct RegionGenerator {
    construct: Lifecycle,
    populate: Populate,
    destroy: Lifecycle,
    _thread_bound: PhantomData<Rc<()>>,
}

#[repr(C, align(16))]
struct Storage([u8; 0x260]);

#[repr(C, align(16))]
struct Root([f32; 4]);

struct Voxel<'a> {
    storage: Box<Storage>,
    bindings: &'a RegionGenerator,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct RegionSystem {
    pub address: u64,
    pub position: [f32; 3],
}

impl RegionGenerator {
    /// # Safety
    /// Requires a verified 7.06 image. Calls are confined to a loaded gameplay callback.
    pub unsafe fn from_verified_image(base: usize) -> Result<Self, &'static str> {
        unsafe {
            for (rva, prefix) in [
                (0x4c5620, &[0x66, 0x0f, 0x6f, 0x05][..]),
                (0x1367810, &[0x48, 0x8b, 0xc4][..]),
                (0x4c5420, &[0x48, 0x89, 0x5c, 0x24, 0x08][..]),
            ] {
                if std::slice::from_raw_parts((base + rva) as *const u8, prefix.len()) != prefix {
                    return Err("native region function prefix mismatch");
                }
            }
            Ok(Self {
                construct: std::mem::transmute::<usize, Lifecycle>(base + 0x4c5620),
                populate: std::mem::transmute::<usize, Populate>(base + 0x1367810),
                destroy: std::mem::transmute::<usize, Lifecycle>(base + 0x4c5420),
                _thread_bound: PhantomData,
            })
        }
    }

    /// # Safety
    /// The gameplay callback holds loaded game resources and current unlock state valid.
    pub unsafe fn generate(&self, region: Address) -> Result<Vec<RegionSystem>, &'static str> {
        if region.region() != region {
            return Err("region has nonzero system or planet fields");
        }
        let mut voxel = Voxel {
            storage: Box::new(Storage([0; 0x260])),
            bindings: self,
        };
        let pointer = voxel.storage.0.as_mut_ptr();
        unsafe {
            (self.construct)(pointer);
            let root = Root([0.0, 0.0, 0.0, 1.0]);
            (self.populate)(region.raw(), pointer, root.0.as_ptr());
            let count = pointer.add(0x254).cast::<u32>().read_unaligned() as usize;
            let positions_count = pointer.add(0x214).cast::<u32>().read_unaligned() as usize;
            if count != positions_count || count > 4095 {
                return Err("invalid native region count");
            }
            if count == 0 {
                return Ok(Vec::new());
            }
            let addresses = pointer.add(0x258).cast::<*const u64>().read_unaligned();
            let positions = pointer
                .add(0x218)
                .cast::<*const [f32; 3]>()
                .read_unaligned();
            if addresses.is_null() || positions.is_null() {
                return Err("null native region output");
            }
            let mut seen = [0u64; 64];
            let mut result = Vec::with_capacity(count);
            for index in 0..count {
                let address = Address::from_raw(addresses.add(index).read_unaligned())?;
                let position = positions.add(index).read_unaligned();
                let system = usize::from(address.coordinates().system);
                if address.region() != region
                    || address.system() != address
                    || system == 0
                    || !position.iter().all(|value| value.is_finite())
                {
                    return Err("invalid native region system");
                }
                let bit = 1u64 << (system % 64);
                if seen[system / 64] & bit != 0 {
                    return Err("duplicate native region system");
                }
                seen[system / 64] |= bit;
                result.push(RegionSystem {
                    address: address.raw(),
                    position,
                });
            }
            Ok(result)
        }
    }
}

impl Drop for Voxel<'_> {
    fn drop(&mut self) {
        unsafe { (self.bindings.destroy)(self.storage.0.as_mut_ptr()) };
    }
}

use std::{marker::PhantomData, rc::Rc};

const CONTEXT_SIZE: usize = 0xc10;
const RNG_START: usize = 0xba4;
const RNG_END: usize = 0xbac;
pub(crate) const CONFIGURATION_SIZE: usize = 0xbb0;
const DESCRIPTION_CACHE_START: usize = CONFIGURATION_SIZE;
type Deallocate = unsafe extern "system" fn(*mut u8, *mut u8, i32);
const RESOURCE_POINTER_OFFSETS: [usize; 18] = [
    0, 8, 0x50, 0x58, 0x60, 0x68, 0x70, 0x78, 0x80, 0x4f8, 0x510, 0x518, 0x520, 0x528, 0x530,
    0x538, 0x540, 0x548,
];

#[cfg(any(windows, test))]
pub(crate) fn configuration_identity(
    mut configuration: [u8; CONFIGURATION_SIZE],
) -> [u8; CONFIGURATION_SIZE] {
    // The live generator's pending-task vector is not borrowed by query contexts.
    configuration[0xb90..0xba0].fill(0);
    configuration[RNG_START..RNG_END].fill(0);
    configuration
}

#[repr(C, align(16))]
struct Storage([u8; CONTEXT_SIZE]);

#[repr(C)]
#[derive(Clone, Copy)]
struct ResourceVector {
    capacity: u32,
    count: u32,
    data: *const u8,
}

pub struct PlanetQueryContext {
    storage: Box<Storage>,
    initial: [u8; CONTEXT_SIZE],
    allocator: *mut u8,
    deallocate: Deallocate,
    _gameplay: PhantomData<Rc<()>>,
}

impl PlanetQueryContext {
    /// # Safety
    /// Exact 7.06 image verification and a gameplay lease are required. The caller keeps the live
    /// generator and its configuration graph unchanged until this context is dropped. An audited
    /// scoped worker may construct and use its own context while the gameplay callback waits.
    /// This context permits GenerateQueryInfo only; full generation/object resolution is excluded.
    pub unsafe fn borrow_query_configuration(
        base: usize,
        live: *const u8,
    ) -> Result<Self, &'static str> {
        if live.is_null() {
            return Err("null live planet generator");
        }
        let construct_address = base + 0x13b8c90;
        let prefix = [0x48, 0x89, 0x5c, 0x24, 0x08, 0x57];
        if unsafe { std::slice::from_raw_parts(construct_address as *const u8, prefix.len()) }
            != prefix
        {
            return Err("planet context constructor prefix mismatch");
        }
        let mut storage = Box::new(Storage([0; CONTEXT_SIZE]));
        unsafe {
            let construct =
                std::mem::transmute::<usize, unsafe extern "system" fn(*mut u8)>(construct_address);
            let private = storage.0.as_mut_ptr();
            construct(private);
            for offset in RESOURCE_POINTER_OFFSETS {
                let resource = live.add(offset).cast::<usize>().read_unaligned();
                private
                    .add(offset)
                    .cast::<usize>()
                    .write_unaligned(resource);
            }
            std::ptr::copy_nonoverlapping(live.add(0x10), private.add(0x10), 0x3f);
            std::ptr::copy_nonoverlapping(live.add(0x660), private.add(0x660), 0x420);
            private.add(0xba0).write(live.add(0xba0).read());
            for offset in [0x88, 0x198, 0x2a8, 0x3b8, 0x550, 0xa80]
                .into_iter()
                .flat_map(|base| (0..17).map(move |i| base + i * 16))
                .chain([0x500])
            {
                let vector = live.add(offset).cast::<ResourceVector>().read_unaligned();
                if vector.count > vector.capacity
                    || vector.capacity > 100_000
                    || (vector.count > 0 && vector.data.is_null())
                {
                    return Err("invalid borrowed resource vector");
                }
                private
                    .add(offset)
                    .cast::<ResourceVector>()
                    .write_unaligned(vector);
            }
        }
        let initial = storage.0;
        Ok(Self {
            storage,
            initial,
            allocator: (base + 0x500a050) as *mut u8,
            deallocate: unsafe { std::mem::transmute::<usize, Deallocate>(base + 0x2c1f570) },
            _gameplay: PhantomData,
        })
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.storage.0.as_mut_ptr()
    }

    pub fn borrowed_configuration_unchanged(&self) -> bool {
        self.storage.0[..RNG_START] == self.initial[..RNG_START]
            && self.storage.0[RNG_END..DESCRIPTION_CACHE_START]
                == self.initial[RNG_END..DESCRIPTION_CACHE_START]
    }

    #[cfg(all(windows, feature = "investigation"))]
    pub fn snapshots(&self) -> (&[u8], &[u8]) {
        (&self.initial, &self.storage.0)
    }
}

impl Drop for PlanetQueryContext {
    fn drop(&mut self) {
        unsafe {
            let cache = self.storage.0.as_mut_ptr().add(DESCRIPTION_CACHE_START);
            let mut chunk = cache.add(0x10).cast::<*mut u8>().read_unaligned();
            while !chunk.is_null() {
                let next = chunk.cast::<*mut u8>().read_unaligned();
                (self.deallocate)(self.allocator, chunk, -1);
                chunk = next;
            }
            let table = cache.add(0x20).cast::<*mut u8>().read_unaligned();
            if table != cache.add(0x38) && !table.is_null() {
                (self.deallocate)(self.allocator, table, -1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_tasks_and_rng_preserve_configuration_identity() {
        let mut configuration = [0x5a; CONFIGURATION_SIZE];
        let expected = configuration_identity(configuration);
        for (capacity, count, pointer, rng) in [
            (0u32, 0u32, 0u64, 1u64),
            (511, 173, 0x12345678, 0xfedcba98),
            (511, 142, 0x12345678, 0xfedcba97),
            (1024, 0, 0x87654321, 0),
        ] {
            configuration[0xb90..0xb94].copy_from_slice(&capacity.to_le_bytes());
            configuration[0xb94..0xb98].copy_from_slice(&count.to_le_bytes());
            configuration[0xb98..0xba0].copy_from_slice(&pointer.to_le_bytes());
            configuration[RNG_START..RNG_END].copy_from_slice(&rng.to_le_bytes());
            assert_eq!(configuration_identity(configuration), expected);
        }
    }

    #[test]
    fn resource_and_generation_option_changes_invalidate_configuration_identity() {
        let configuration = [0x5a; CONFIGURATION_SIZE];
        let expected = configuration_identity(configuration);
        for offset in RESOURCE_POINTER_OFFSETS.into_iter().chain([
            0x10, 0x660, 0xa7f, 0x88, 0x8c, 0x90, 0xb80, 0xb84, 0xb88, 0xba0,
        ]) {
            let mut changed = configuration;
            changed[offset] ^= 1;
            assert_ne!(
                configuration_identity(changed),
                expected,
                "offset {offset:#x}"
            );
        }
    }

    unsafe extern "system" fn record_release(allocator: *mut u8, pointer: *mut u8, tag: i32) {
        assert_eq!(tag, -1);
        unsafe { &mut *allocator.cast::<Vec<usize>>() }.push(pointer as usize);
    }

    fn empty_context(releases: &mut Vec<usize>) -> PlanetQueryContext {
        let mut context = PlanetQueryContext {
            storage: Box::new(Storage([0; CONTEXT_SIZE])),
            initial: [0; CONTEXT_SIZE],
            allocator: (releases as *mut Vec<usize>).cast(),
            deallocate: record_release,
            _gameplay: PhantomData,
        };
        unsafe {
            let cache = context.storage.0.as_mut_ptr().add(DESCRIPTION_CACHE_START);
            cache
                .add(0x20)
                .cast::<*mut u8>()
                .write_unaligned(cache.add(0x38));
        }
        context
    }

    #[test]
    fn empty_description_cache_has_no_allocations() {
        let mut releases = Vec::new();
        drop(empty_context(&mut releases));
        assert!(releases.is_empty());
    }

    #[test]
    fn description_cache_releases_each_pool_and_active_table() {
        let mut releases = Vec::new();
        let mut context = empty_context(&mut releases);
        let mut tail = [0usize; 2];
        let mut head = [tail.as_mut_ptr() as usize, 0];
        let mut table = [0usize; 8];
        unsafe {
            let cache = context.storage.0.as_mut_ptr().add(DESCRIPTION_CACHE_START);
            cache
                .add(0x10)
                .cast::<usize>()
                .write_unaligned(head.as_mut_ptr() as usize);
            cache
                .add(0x20)
                .cast::<usize>()
                .write_unaligned(table.as_mut_ptr() as usize);
        }
        drop(context);
        assert_eq!(
            releases,
            [
                head.as_ptr() as usize,
                tail.as_ptr() as usize,
                table.as_ptr() as usize
            ]
        );
    }
}

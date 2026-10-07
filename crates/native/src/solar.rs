use std::{marker::PhantomData, rc::Rc};

type Initialize = unsafe extern "system" fn(*mut u8);
type ReleaseResource = unsafe extern "system" fn(*mut u8, *mut u8);
type Deallocate = unsafe extern "system" fn(*mut u8, *mut u8, i32);

#[repr(C, align(16))]
struct Storage([u8; 0x6e0]);

pub struct SolarContext {
    storage: Box<Storage>,
    resource_cache: *mut u8,
    allocator: *mut u8,
    release: ReleaseResource,
    deallocate: Deallocate,
    _gameplay: PhantomData<Rc<()>>,
}

impl SolarContext {
    /// # Safety
    /// The caller verifies the exact 7.06 image and holds initialized game resources through drop.
    /// Use the gameplay thread or an audited scoped worker with valid NMS allocator TLS.
    pub unsafe fn new(base: usize) -> Result<Self, &'static str> {
        unsafe fn checked(base: usize, va: usize, prefix: &[u8]) -> Result<usize, &'static str> {
            let address = base + va - 0x140000000;
            if unsafe { std::slice::from_raw_parts(address as *const u8, prefix.len()) } != prefix {
                return Err("solar context function prefix mismatch");
            }
            Ok(address)
        }
        unsafe {
            let initialize = std::mem::transmute::<usize, Initialize>(checked(
                base,
                0x141649ae0,
                &[0x48, 0x89, 0x5c, 0x24, 0x10],
            )?);
            let release = std::mem::transmute::<usize, ReleaseResource>(checked(
                base,
                0x1404b2df0,
                &[0x40, 0x53, 0x56, 0x57, 0x48, 0x83, 0xec, 0x20],
            )?);
            let deallocate = std::mem::transmute::<usize, Deallocate>(checked(
                base,
                0x142c1f570,
                &[0x48, 0x85, 0xd2, 0x0f, 0x84],
            )?);
            let mut result = Self {
                storage: Box::new(Storage([0; 0x6e0])),
                resource_cache: (base + 0x7489480) as *mut u8,
                allocator: (base + 0x500a050) as *mut u8,
                release,
                deallocate,
                _gameplay: PhantomData,
            };
            let ptr = result.storage.0.as_mut_ptr();
            for (header, inline, capacity) in
                [(0x500, 0, 16u64), (0x5e0, 0x520, 6), (0x6b0, 0x5f0, 6)]
            {
                ptr.add(header).cast::<u64>().write_unaligned(capacity);
                ptr.add(header + 8)
                    .cast::<*mut u8>()
                    .write_unaligned(ptr.add(inline));
            }
            initialize(ptr);
            if ptr.add(0x6c0).cast::<usize>().read_unaligned() == 0
                || ptr.add(0x6c8).cast::<usize>().read_unaligned() == 0
            {
                return Err("solar resource acquisition failed");
            }
            Ok(result)
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.storage.0.as_mut_ptr()
    }
}

impl Drop for SolarContext {
    fn drop(&mut self) {
        let ptr = self.storage.0.as_mut_ptr();
        unsafe {
            for offset in [0x6c0, 0x6c8] {
                let resource = ptr.add(offset).cast::<*mut u8>().read_unaligned();
                if !resource.is_null() {
                    (self.release)(self.resource_cache, resource);
                }
            }
            for (header, inline) in [(0x500, 0), (0x5e0, 0x520), (0x6b0, 0x5f0)] {
                let allocation = ptr.add(header + 8).cast::<*mut u8>().read_unaligned();
                if !allocation.is_null() && allocation != ptr.add(inline) {
                    (self.deallocate)(self.allocator, allocation, -1);
                }
            }
        }
    }
}

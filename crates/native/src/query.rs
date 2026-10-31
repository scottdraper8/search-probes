use search_probes_model::address::Address;
use std::{marker::PhantomData, rc::Rc};

pub const QUERY_SIZE: usize = 0x36c0;
pub const METADATA_OFFSET: usize = 0x1050;
pub const PLANET_QUERY_OFFSET: usize = 0x790;
pub const PLANET_QUERY_SIZE: usize = 0x170;
pub const PLANET_INPUT_OFFSET: usize = METADATA_OFFSET + 0x2180;
pub const PLANET_INPUT_SIZE: usize = 0x58;

type Lifecycle = unsafe extern "system" fn(*mut u8);
type Classify = unsafe extern "system" fn(*mut u8, u64);
type Solar = unsafe extern "system" fn(*mut u8, *mut u8, *mut u8, *mut u8);
type Planet = unsafe extern "system" fn(*mut u8, *mut u8, *mut u8, u64, bool);

pub struct Bindings {
    construct: Lifecycle,
    destroy: Lifecycle,
    classify: Classify,
    solar: Solar,
    planet: Planet,
    _thread_bound: PhantomData<Rc<()>>,
}

impl Bindings {
    /// # Safety
    /// `base` must remain loaded. Native calls require a gameplay callback or an audited worker while
    /// that callback holds game state still.
    pub unsafe fn from_verified_image(base: usize) -> Result<Self, &'static str> {
        unsafe fn check(
            base: usize,
            preferred: usize,
            prefix: &[u8],
        ) -> Result<usize, &'static str> {
            let address = base + preferred - 0x140000000;
            let actual = unsafe { std::slice::from_raw_parts(address as *const u8, prefix.len()) };
            if actual != prefix {
                return Err("NMS native function prefix mismatch");
            }
            Ok(address)
        }
        unsafe {
            Ok(Self {
                construct: std::mem::transmute::<usize, Lifecycle>(check(
                    base,
                    0x140457d60,
                    &[0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c],
                )?),
                destroy: std::mem::transmute::<usize, Lifecycle>(check(
                    base,
                    0x14031d7b0,
                    &[0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74],
                )?),
                classify: std::mem::transmute::<usize, Classify>(check(
                    base,
                    0x1413527b0,
                    &[0x48, 0x89, 0x5c, 0x24, 0x10, 0x57, 0x48, 0x83],
                )?),
                solar: std::mem::transmute::<usize, Solar>(check(
                    base,
                    0x14167bd90,
                    &[0x48, 0x89, 0x5c, 0x24, 0x08],
                )?),
                planet: std::mem::transmute::<usize, Planet>(check(
                    base,
                    0x1416bedb0,
                    &[0x4c, 0x89, 0x4c, 0x24, 0x20],
                )?),
                _thread_bound: PhantomData,
            })
        }
    }
}

#[repr(C, align(16))]
struct Storage([u8; QUERY_SIZE]);

pub struct Query<'a> {
    storage: Box<Storage>,
    bindings: &'a Bindings,
    address: Address,
    count: usize,
    generated: u8,
}

struct PlanetCall {
    function: Planet,
    generator: *mut u8,
    storage: *mut u8,
    index: usize,
    address: u64,
}

impl PlanetCall {
    unsafe fn invoke(&self) {
        unsafe {
            (self.function)(
                self.generator,
                self.storage
                    .add(PLANET_QUERY_OFFSET + self.index * PLANET_QUERY_SIZE),
                self.storage
                    .add(PLANET_INPUT_OFFSET + self.index * PLANET_INPUT_SIZE),
                self.address,
                false,
            );
        }
    }
}

impl<'a> Query<'a> {
    /// # Safety
    /// `solar` is a fully initialized native solar generator, exclusively usable on this thread.
    /// Its resource graph and referenced gameplay state must remain valid and quiescent.
    pub unsafe fn begin(
        bindings: &'a Bindings,
        solar: *mut u8,
        address: Address,
    ) -> Result<Self, &'static str> {
        if solar.is_null() {
            return Err("null solar generator");
        }
        let mut storage = Box::new(Storage([0; QUERY_SIZE]));
        unsafe {
            (bindings.construct)(storage.0.as_mut_ptr());
        }
        let mut result = Self {
            storage,
            bindings,
            address: address.system(),
            count: 0,
            generated: 0,
        };
        let ptr = result.storage.0.as_mut_ptr();
        unsafe {
            (ptr as *mut u32).write_unaligned(1);
            ptr.add(8)
                .cast::<u64>()
                .write_unaligned(result.address.raw());
            ptr.add(0x10)
                .cast::<u64>()
                .write_unaligned(result.address.raw());
            ptr.add(0x18).write(1);
            (bindings.classify)(ptr.add(0x20), result.address.raw());
            (bindings.solar)(solar, ptr.add(0x10), ptr.add(0x20), ptr.add(0x1030));
            if ptr.add(0x1038).cast::<usize>().read_unaligned() != ptr.add(METADATA_OFFSET) as usize
            {
                return Err("native query metadata ownership changed");
            }
            let count = ptr
                .add(0x760)
                .cast::<i32>()
                .read_unaligned()
                .checked_add(ptr.add(0x764).cast::<i32>().read_unaligned())
                .ok_or("planet count overflow")?;
            if !(1..=8).contains(&count) {
                return Err("native planet count outside 1..8");
            }
            result.count = count as usize;
        }
        Ok(result)
    }

    pub fn planet_count(&self) -> usize {
        self.count
    }
    pub fn bytes(&self) -> &[u8] {
        &self.storage.0
    }

    /// # Safety
    /// `planet` is an exclusively usable initialized native planet generator on this thread.
    /// The caller owns this call's capture callback and holds the game's shared query state still.
    pub unsafe fn generate_planet(
        &mut self,
        planet: *mut u8,
        index: usize,
    ) -> Result<(), &'static str> {
        let call = self.prepare_planet(planet, index)?;
        unsafe {
            call.invoke();
        }
        Ok(())
    }

    /// # Safety
    /// The generator is exclusively owned and the query hook is active. The visitor sees the live
    /// planet and completed query fields while native generation is suspended before destruction.
    pub unsafe fn visit_planet(
        &mut self,
        planet: *mut u8,
        index: usize,
        mut visitor: impl FnMut(*const u8, &[u8]) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        let call = self.prepare_planet(planet, index)?;
        let mut observed = 0;
        unsafe {
            crate::capture_session::visit_planet(
                Address::from_raw(call.address)?,
                |live| {
                    observed += 1;
                    if observed != 1 {
                        return Err("multiple captures for one generated planet");
                    }
                    visitor(live, std::slice::from_raw_parts(call.storage, QUERY_SIZE))
                },
                || call.invoke(),
            )?;
        }
        if observed != 1 {
            return Err("generated planet was not captured");
        }
        Ok(())
    }

    fn prepare_planet(
        &mut self,
        planet: *mut u8,
        index: usize,
    ) -> Result<PlanetCall, &'static str> {
        if index >= self.count {
            return Err("planet index outside generated system");
        }
        if planet.is_null() {
            return Err("null planet generator");
        }
        if self.generated & (1 << index) != 0 {
            return Err("planet already generated in this query");
        }
        let address = self.address.planet(index)?.raw();
        self.generated |= 1 << index;
        Ok(PlanetCall {
            function: self.bindings.planet,
            generator: planet,
            storage: self.storage.0.as_mut_ptr(),
            index,
            address,
        })
    }
}

impl Drop for Query<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.bindings.destroy)(self.storage.0.as_mut_ptr());
        }
    }
}

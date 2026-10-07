use crate::object_cache::{MAX_FILENAME_BYTES, ObjectTableCache, TableFacts};
use std::{marker::PhantomData, rc::Rc};

type Acquire = unsafe extern "system" fn(*mut u8, *const u8, bool) -> *mut u8;
type Release = unsafe extern "system" fn(*mut u8, *mut u8);

pub struct ObjectTables {
    cache: *mut u8,
    acquire: Acquire,
    release: Release,
    _thread_bound: PhantomData<Rc<()>>,
}

struct Resource<'a> {
    owner: &'a ObjectTables,
    pointer: *mut u8,
}

impl Drop for Resource<'_> {
    fn drop(&mut self) {
        unsafe { (self.owner.release)(self.owner.cache, self.pointer) };
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct ObjectFlags {
    pub floating_islands: u32,
    pub loaded_tables: u32,
    pub objects: u32,
    pub missing_tables: u32,
}

impl ObjectTables {
    /// # Safety
    /// Requires the exact verified 7.06 image and initialized game resources held valid through use.
    pub unsafe fn from_verified_image(base: usize) -> Result<Self, &'static str> {
        let acquire = base + 0x16c7660;
        let release = base + 0x4b2df0;
        unsafe {
            if std::slice::from_raw_parts(acquire as *const u8, 5) != [0x44, 0x88, 0x44, 0x24, 0x18]
                || std::slice::from_raw_parts(release as *const u8, 4) != [0x40, 0x53, 0x56, 0x57]
            {
                return Err("object resource function prefix mismatch");
            }
            let epoch = ((base + 0x7489858) as *const i32).read_unaligned();
            if epoch == 0 || epoch == -1 {
                return Err("object resource cache is not initialized");
            }
            Ok(Self {
                cache: (base + 0x7489710) as *mut u8,
                acquire: std::mem::transmute::<usize, Acquire>(acquire),
                release: std::mem::transmute::<usize, Release>(release),
                _thread_bound: PhantomData,
            })
        }
    }

    /// # Safety
    /// `planet` is live until return. Summaries belong to its current resource/configuration epoch;
    /// source object counts and IsFloatingIsland are immutable within that epoch.
    pub unsafe fn flags(
        &self,
        planet: *const u8,
        summaries: &ObjectTableCache,
    ) -> Result<ObjectFlags, &'static str> {
        if planet.is_null() {
            return Err("null planet object source");
        }
        let mut result = ObjectFlags::default();
        unsafe {
            let (sources, count) = dynamic_array(planet.add(0x3210), 4096)?;
            for index in 0..count {
                let source = sources.add(index * 0x58);
                let (options, count) = dynamic_array(source.add(0x10), 4096)?;
                if count == 0 || options.add(8).cast::<i32>().read_unaligned() <= 0 {
                    continue;
                }
                let filename = options.cast::<*const u8>().read_unaligned();
                if filename.is_null() {
                    continue;
                }
                let length = options.add(8).cast::<i32>().read_unaligned() as usize;
                if length > MAX_FILENAME_BYTES {
                    return Err("object-table filename exceeds the native path bound");
                }
                let bytes = std::slice::from_raw_parts(filename, length);
                let key = &bytes[..bytes.iter().position(|&byte| byte == 0).unwrap_or(length)];
                let facts = summaries.resolve(key, || self.load(filename))?;
                if let Some(facts) = facts {
                    result.loaded_tables += 1;
                    result.objects = result
                        .objects
                        .checked_add(facts.objects)
                        .ok_or("object record count overflow")?;
                    result.floating_islands |= u32::from(facts.floating_islands);
                } else {
                    result.missing_tables += 1;
                }
            }
        }
        Ok(result)
    }
    unsafe fn load(&self, filename: *const u8) -> Result<Option<TableFacts>, &'static str> {
        let pointer = unsafe { (self.acquire)(self.cache, filename, false) };
        if pointer.is_null() {
            return Ok(None);
        }
        let resource = Resource {
            owner: self,
            pointer,
        };
        let mut facts = TableFacts::default();
        for offset in [0x10, 0x40, 0x30, 0x20] {
            let (objects, count) = unsafe { dynamic_array(resource.pointer.add(offset), 65_536) }?;
            facts.objects = facts
                .objects
                .checked_add(count as u32)
                .ok_or("object record count overflow")?;
            for index in 0..count {
                facts.floating_islands |= unsafe { objects.add(index * 0x168 + 0x15f).read() != 0 };
            }
        }
        Ok(Some(facts))
    }
}

unsafe fn dynamic_array(
    header: *const u8,
    bound: usize,
) -> Result<(*const u8, usize), &'static str> {
    let pointer = unsafe { header.cast::<*const u8>().read_unaligned() };
    let count = unsafe { header.add(8).cast::<i32>().read_unaligned() };
    if count < 0 || count as usize > bound || (count > 0 && pointer.is_null()) {
        return Err("invalid native object array");
    }
    Ok((pointer, count as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_array_bounds_reject_unknown_storage() {
        let mut header = [0u8; 16];
        assert_eq!(unsafe { dynamic_array(header.as_ptr(), 4) }.unwrap().1, 0);
        header[8..12].copy_from_slice(&1i32.to_le_bytes());
        assert!(unsafe { dynamic_array(header.as_ptr(), 4) }.is_err());
        header[8..12].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(unsafe { dynamic_array(header.as_ptr(), 4) }.is_err());
    }
}

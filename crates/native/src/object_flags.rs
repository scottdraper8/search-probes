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
    pub ground_grass: u32,
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
                    let decoration = key
                        .windows(b"/BIOMES/OBJECTS/".len())
                        .any(|part| part.eq_ignore_ascii_case(b"/BIOMES/OBJECTS/"));
                    result.ground_grass |= u32::from(facts.ground_grass && !decoration);
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
                let object = unsafe { objects.add(index * 0x168) };
                facts.floating_islands |= unsafe { object.add(0x15f).read() != 0 };
                facts.ground_grass |= unsafe { ground_grass(object) }?;
            }
        }
        Ok(Some(facts))
    }
}

unsafe fn ground_grass(object: *const u8) -> Result<bool, &'static str> {
    let (filename, length) = unsafe { dynamic_array(object.add(0x58), MAX_FILENAME_BYTES) }?;
    if length == 0 {
        return Ok(false);
    }
    let path = unsafe { std::slice::from_raw_parts(filename, length) };
    let path = &path[..path.iter().position(|&b| b == 0).unwrap_or(path.len())];
    const MODELS: &[&[u8]] = &[
        b"MODELS/PLANETS/BIOMES/COMMON/GRASS/NEWCROSSGRASS.SCENE.MBIN",
        b"MODELS/PLANETS/BIOMES/COMMON/GRASS/NEWLUSHGRASS.SCENE.MBIN",
        b"MODELS/PLANETS/BIOMES/COMMON/GRASS/BUBBLELUSHGRASS.SCENE.MBIN",
        b"MODELS/PLANETS/BIOMES/BARREN/HQ/FOLIAGE/BARRENGRASSLARGE.SCENE.MBIN",
    ];
    if !MODELS.iter().any(|model| path.eq_ignore_ascii_case(model)) {
        return Ok(false);
    }
    let (variants, count) = unsafe { dynamic_array(object.add(0xe0), 64) }?;
    for i in 0..count {
        let variant = unsafe { variants.add(i * 0x48) };
        let values = [0x24, 0x34, 0x40]
            .map(|offset| unsafe { variant.add(offset).cast::<f32>().read_unaligned() });
        if values.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err("invalid grass spawn density");
        }
        if values[0] > 0. && (values[1] > 0. || values[2] > 0.) {
            return Ok(true);
        }
    }
    Ok(false)
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
    fn grass_requires_a_ground_grass_model_and_nonzero_spawn_density() {
        let mut object = vec![0; 0x168];
        let mut variant = [0u8; 0x48];
        let model = b"MODELS/PLANETS/BIOMES/COMMON/GRASS/NEWLUSHGRASS.SCENE.MBIN";
        object[0x58..0x60].copy_from_slice(&(model.as_ptr() as usize).to_ne_bytes());
        object[0x60..0x64].copy_from_slice(&(model.len() as i32).to_ne_bytes());
        object[0xe0..0xe8].copy_from_slice(&(variant.as_ptr() as usize).to_ne_bytes());
        object[0xe8..0xec].copy_from_slice(&1i32.to_ne_bytes());
        assert!(!unsafe { ground_grass(object.as_ptr()) }.unwrap());
        variant[0x24..0x28].copy_from_slice(&1f32.to_ne_bytes());
        assert!(!unsafe { ground_grass(object.as_ptr()) }.unwrap());
        variant[0x34..0x38].copy_from_slice(&0.2f32.to_ne_bytes());
        assert!(unsafe { ground_grass(object.as_ptr()) }.unwrap());
        object[0x58..0x68].fill(0);
        let model = b"MODELS/PLANETS/BIOMES/COMMON/FLOWERS/BUTTERCUP.SCENE.MBIN";
        object[0x58..0x60].copy_from_slice(&(model.as_ptr() as usize).to_ne_bytes());
        object[0x60..0x64].copy_from_slice(&(model.len() as i32).to_ne_bytes());
        assert!(!unsafe { ground_grass(object.as_ptr()) }.unwrap());
        object[0x58..0x60].fill(0);
        assert!(unsafe { ground_grass(object.as_ptr()) }.is_err());
    }
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

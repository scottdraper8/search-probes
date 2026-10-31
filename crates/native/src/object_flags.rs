use crate::object_cache::{MAX_FILENAME_BYTES, MAX_OWNED_BYTES, ObjectTableCache, TableFacts};
use search_probes_model::grass::GrassModel;
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
    pub titan_worms: bool,
    pub loaded_tables: u32,
    pub objects: u32,
    pub missing_tables: u32,
    pub ground_grass: u32,
    pub glowing_grass: bool,
    pub glowing_flora: bool,
}

impl ObjectTables {
    /// # Safety
    /// Game resources must remain initialized and valid through use.
    pub unsafe fn from_verified_image(base: usize) -> Result<Self, &'static str> {
        let acquire = base + 0x16f9e10;
        let release = base + 0x4bb8a0;
        unsafe {
            if std::slice::from_raw_parts(acquire as *const u8, 5) != [0x44, 0x88, 0x44, 0x24, 0x18]
                || std::slice::from_raw_parts(release as *const u8, 4) != [0x40, 0x53, 0x56, 0x57]
            {
                return Err("object resource function prefix mismatch");
            }
            let epoch = ((base + 0x755f6b8) as *const i32).read_unaligned();
            if epoch == 0 || epoch == -1 {
                return Err("object resource cache is not initialized");
            }
            Ok(Self {
                cache: (base + 0x755f570) as *mut u8,
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
        need_glow: u8,
        mut flora_models: Option<&mut Vec<crate::flora_appearance::Spawn>>,
    ) -> Result<ObjectFlags, &'static str> {
        if planet.is_null() {
            return Err("null planet object source");
        }
        let mut result = ObjectFlags::default();
        unsafe {
            let (sources, count) = dynamic_array(planet.add(0x3210), 4096)?;
            for index in 0..count {
                let source = sources.add(index * 0x58);
                if source.add(0x53).read() != 0 {
                    continue;
                }
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
                let surface = ![
                    b"/BIOMES/OBJECTS/".as_slice(),
                    b"/BIOMES/CAVE/",
                    b"/BIOMES/UNDERWATER/",
                ]
                .iter()
                .any(|part| {
                    key.windows(part.len())
                        .any(|w| w.eq_ignore_ascii_case(part))
                });
                let glow = if surface { need_glow } else { 0 };
                let facts = summaries.resolve(key, glow, || self.load(filename, glow))?;
                if let Some(facts) = facts {
                    result.loaded_tables += 1;
                    result.titan_worms |= facts.titan_worms;
                    result.objects = result
                        .objects
                        .checked_add(facts.objects)
                        .ok_or("object record count overflow")?;
                    if source.add(0x53).read() == 0 {
                        result.floating_islands |= u32::from(facts.floating_islands);
                    }
                    let decoration = key
                        .windows(b"/BIOMES/OBJECTS/".len())
                        .any(|part| part.eq_ignore_ascii_case(b"/BIOMES/OBJECTS/"));
                    if !decoration {
                        result.ground_grass |= facts.ground_grass;
                    }
                    if glow & 4 != 0 {
                        if let Some(models) = flora_models.as_deref_mut() {
                            let ground_index = source.add(0x4c).cast::<i32>().read_unaligned();
                            models.extend(facts.flora_models.iter().cloned().map(|mut spawn| {
                                spawn.source_ground_index = ground_index;
                                spawn
                            }));
                        }
                    }
                    if glow != 0 {
                        result.glowing_grass |= facts.glowing_grass;
                        result.glowing_flora |= facts.glowing_flora;
                    }
                } else {
                    result.missing_tables += 1;
                }
            }
        }
        Ok(result)
    }
    unsafe fn load(
        &self,
        filename: *const u8,
        glow: u8,
    ) -> Result<Option<TableFacts>, &'static str> {
        let pointer = unsafe { (self.acquire)(self.cache, filename, false) };
        if pointer.is_null() {
            return Ok(None);
        }
        let resource = Resource {
            owner: self,
            pointer,
        };
        let mut facts = TableFacts {
            glow_known: glow,
            ..Default::default()
        };
        let mut flora_models = Vec::new();
        let mut flora_bytes = 0usize;
        for offset in [0x10, 0x40, 0x30, 0x20] {
            let (objects, count) = unsafe { dynamic_array(resource.pointer.add(offset), 65_536) }?;
            facts.objects = facts
                .objects
                .checked_add(count as u32)
                .ok_or("object record count overflow")?;
            for index in 0..count {
                let object = unsafe { objects.add(index * 0x168) };
                facts.floating_islands |= unsafe { floating_island(object) }?;
                facts.titan_worms |= unsafe { titan_worm(object) }?;
                let grass = unsafe { ground_grass(object) }?;
                facts.ground_grass |= grass;
                if glow != 0 && unsafe { spawns(object) }? {
                    let path = unsafe { model_path(object) }?;
                    if !path.is_empty() {
                        if glow & 4 != 0 && grass == 0 {
                            let spawn = unsafe { flora_spawn(object, path) };
                            flora_bytes = flora_bytes
                                .checked_add(spawn.owned_bytes() + std::mem::size_of_val(&spawn))
                                .filter(|&bytes| bytes <= MAX_OWNED_BYTES)
                                .ok_or("object-table flora data exceeds byte bound")?;
                            flora_models.push(spawn);
                        }
                        let grass = grass != 0;
                        if (grass && glow & 1 != 0 && !facts.glowing_grass)
                            || (!grass && glow & 2 != 0 && !facts.glowing_flora)
                        {
                            let model = crate::bioluminescence::get(path)?;
                            if grass {
                                facts.glowing_grass |= model.emissive;
                            } else {
                                facts.glowing_flora |= model.glowing_flora;
                            }
                        }
                    }
                }
            }
        }
        flora_models.sort_unstable();
        flora_models.dedup();
        facts.flora_models = flora_models.into();
        Ok(Some(facts))
    }
}

unsafe fn flora_spawn(object: *const u8, path: &[u8]) -> crate::flora_appearance::Spawn {
    crate::flora_appearance::Spawn {
        model: path.to_vec(),
        match_ground_colour: unsafe { object.add(0x160).read() != 0 },
        ground_colour_index: unsafe { object.add(0x100).cast::<u32>().read_unaligned() },
        swap_primary_for_random: unsafe { object.add(0x164).read() != 0 },
        swap_primary_for_secondary: unsafe { object.add(0x165).read() != 0 },
        ..Default::default()
    }
}

unsafe fn ground_grass(object: *const u8) -> Result<u32, &'static str> {
    let path = unsafe { model_path(object) }?;
    let Some(model) = GrassModel::ALL
        .into_iter()
        .find(|model| path.eq_ignore_ascii_case(model.path().as_bytes()))
    else {
        return Ok(0);
    };
    Ok(if unsafe { spawns(object) }? {
        model.mask()
    } else {
        0
    })
}

unsafe fn floating_island(object: *const u8) -> Result<bool, &'static str> {
    Ok(unsafe { object.add(0x15f).read() != 0 && spawns(object)? })
}

unsafe fn titan_worm(object: *const u8) -> Result<bool, &'static str> {
    Ok(unsafe { model_path(object) }?.eq_ignore_ascii_case(
        b"MODELS/PLANETS/CREATURES/SANDWORMMINI/GROUNDWORMSPAWNER.SCENE.MBIN",
    ) && unsafe { spawns(object) }?)
}

/// # Safety
/// The returned filename must not outlive the acquired native object resource.
unsafe fn model_path<'a>(object: *const u8) -> Result<&'a [u8], &'static str> {
    let (filename, length) = unsafe { dynamic_array(object.add(0x58), MAX_FILENAME_BYTES) }?;
    if length == 0 {
        return Ok(&[]);
    }
    let path = unsafe { std::slice::from_raw_parts(filename, length) };
    let path = &path[..path.iter().position(|&b| b == 0).unwrap_or(path.len())];
    Ok(path)
}

unsafe fn spawns(object: *const u8) -> Result<bool, &'static str> {
    let (variants, count) = unsafe { dynamic_array(object.add(0xe0), 64) }?;
    for i in 0..count {
        let variant = unsafe { variants.add(i * 0x48) };
        let values = [0x24, 0x34, 0x40]
            .map(|offset| unsafe { variant.add(offset).cast::<f32>().read_unaligned() });
        if values.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err("invalid vegetation spawn density");
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

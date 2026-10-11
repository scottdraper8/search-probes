pub const WEATHER: u32 = 1;
pub const METADATA: u32 = 2;
pub const NAME: u32 = 4;
pub const SCREEN_FILTER: u32 = 8;
pub const INDEX: u32 = 16;
pub const COLOUR_COUNT: usize = 10;
pub const COLOURS: u32 = ((1 << COLOUR_COUNT) - 1) << 8;
pub const ALL: u32 = 31 | COLOURS;

#[repr(usize)]
#[derive(Clone, Copy, Debug)]
pub enum Colour {
    Grass,
    Flora,
    LeafPalette,
    Cloud,
    SkyFog,
    SkyHeightFog,
    SkyHorizon,
    Sky,
    SkyNight,
    Water,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metadata {
    pub biome: u32,
    pub biome_subtype: u32,
    pub creature_life_level: u32,
    pub inhabiting_race: u32,
    pub life_level: u32,
    pub resource_level: u32,
    pub sentinel_levels: [u32; 4],
    pub in_abandoned_system: u32,
    pub in_empty_system: u32,
    pub in_gas_giant_system: u32,
}

pub const fn colour(index: usize) -> u32 {
    if index < COLOUR_COUNT {
        1 << (index + 8)
    } else {
        0
    }
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    pub present: u32,
    pub index: i32,
    pub weather: [u32; 3],
    pub metadata: Metadata,
    pub screen_filter: i32,
    pub name: [u8; 128],
    pub colours: [[f64; 4]; COLOUR_COUNT],
    pub grass_terrain: [[f64; 4]; 2],
}

impl Default for Facts {
    fn default() -> Self {
        Self {
            present: 0,
            index: -1,
            weather: [0; 3],
            metadata: Metadata::default(),
            screen_filter: -1,
            name: [0; 128],
            colours: [[0.; 4]; COLOUR_COUNT],
            grass_terrain: [[0.; 4]; 2],
        }
    }
}

unsafe fn read<T: Copy>(p: *const u8, offset: usize) -> T {
    unsafe { p.add(offset).cast::<T>().read_unaligned() }
}

fn pointer(value: usize) -> Result<*const u8, &'static str> {
    if !(0x10000..=0x0000_7fff_ffff_ffff).contains(&value) {
        return Err("invalid native capture pointer");
    }
    Ok(value as *const u8)
}

unsafe fn record(header: *const u8, index: i32, stride: usize) -> Result<*const u8, &'static str> {
    let count: i32 = unsafe { read(header, 8) };
    if !(0..count).contains(&index) || count > 4096 {
        return Err("selected colour index outside bounded native table");
    }
    let data = pointer(unsafe { read(header, 0) })?;
    Ok(unsafe { data.add(index as usize * stride) })
}

pub fn water_reflectance(
    scattering: [f32; 3],
    transmission: [f32; 3],
    distances: [f32; 2],
) -> Result<[f64; 4], &'static str> {
    if distances.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("invalid water optical distance");
    }
    let mut rgb = [0., 0., 0., 1.];
    for i in 0..3 {
        let s = scattering[i] as f64;
        let t = transmission[i] as f64;
        if !s.is_finite() || !t.is_finite() || !(0. ..=1.).contains(&s) || !(0. ..=1.).contains(&t)
        {
            return Err("invalid water optical coefficient");
        }
        let scatter = -s.max(0.01).ln() / distances[0] as f64;
        let absorb = -t.max(0.01).ln() / distances[1] as f64;
        if scatter + absorb == 0. {
            return Err("water has no scattering or absorption");
        }
        let albedo = scatter / (scatter + absorb);
        let a = ((1. - albedo) / (1. - 0.8 * albedo)).sqrt();
        rgb[i] = 5. * (1. - 0.139 * a) * (1. - a) / (1. + 1.17 * a);
    }
    Ok(rgb)
}

/// # Safety
/// Requires the same immutable, initialized generator and live planet ownership as capture.
/// Reads the selected in-memory hazard overrides/defaults; retains no native reference.
pub unsafe fn storm_profile(
    planet: *const u8,
    generator: *const u8,
) -> Result<search_probes_model::weather::StormHazards, &'static str> {
    if planet.is_null() {
        return Err("null live planet");
    }
    let mut profile = [[[0.; 2]; 2]; 3];
    unsafe {
        if generator.is_null() {
            return Err("null storm configuration generator");
        }
        let index: u32 = read(planet, 0x1e54);
        let count: u32 = read(generator, 0x504);
        if index >= count || count > 17 {
            return Err("storm weather selection outside bounded native table");
        }
        let table = pointer(read(generator, 0x508))?;
        let properties = pointer(read(table, index as usize * 8))?;
        let defaults = pointer(read(generator, 0x4f8))?;
        // Exact 7.06 native hazard selector: override booleans and two six-state
        // value arrays, with [extreme, normal] floats in each state.
        for (slot, (flag, offset, default_offset)) in profile.iter_mut().zip([
            (0xbe0, 0xad0, 0x110), // Temperature
            (0xbe1, 0xb30, 0x140), // Toxicity
            (0xbe2, 0xb60, 0x170), // Radiation
        ]) {
            let (source, offset) = if read::<u8>(properties, flag) != 0 {
                (properties, offset)
            } else {
                (defaults, default_offset)
            };
            *slot = [0, 3].map(|state| read(source, offset + state * 8));
        }
        search_probes_model::weather::extreme_storm_profile(&profile)?;
    }
    Ok(profile)
}

/// # Safety
/// Every requested field must be readable from a live current-build planet. Requested sky/water
/// tables require its initialized generator and live resource graph. Call before planet destruction,
/// without concurrent mutation. An empty mask does not read pointers.
pub unsafe fn capture(
    planet: *const u8,
    generator: *const u8,
    wanted: u32,
) -> Result<Facts, &'static str> {
    if wanted & !ALL != 0 {
        return Err("unknown capture requirement");
    }
    let mut facts = Facts::default();
    if wanted == 0 {
        return Ok(facts);
    }
    if planet.is_null() {
        return Err("null live planet");
    }
    unsafe {
        if wanted & INDEX != 0 {
            facts.index = read(planet, 0x353c);
        }
        if wanted & WEATHER != 0 {
            for (slot, offset) in facts.weather.iter_mut().zip([0x1e48, 0x1e50, 0x1e54]) {
                *slot = read(planet, offset);
            }
        }
        if wanted & METADATA != 0 {
            facts.metadata = Metadata {
                biome: read(planet, 0x32b8),
                biome_subtype: read(planet, 0x32bc),
                creature_life_level: read(planet, 0x352c),
                inhabiting_race: read(planet, 0x3534),
                life_level: read(planet, 0x3538),
                resource_level: read(planet, 0x3540),
                sentinel_levels: [0x34cc, 0x34e4, 0x34fc, 0x3514]
                    .map(|offset| read(planet, offset)),
                in_abandoned_system: u32::from(read::<u8>(planet, 0x3acf) != 0),
                in_empty_system: u32::from(read::<u8>(planet, 0x3ad0) != 0),
                in_gas_giant_system: u32::from(read::<u8>(planet, 0x3ad1) != 0),
            };
        }
        if wanted & NAME != 0 {
            facts.name = read(planet, 0x3a4e);
        }
        if wanted & (SCREEN_FILTER | COLOURS) != 0 {
            facts.screen_filter = read(planet, 0x1e44);
        }
        if wanted & colour(0) != 0 {
            facts.grass_terrain =
                [1, 3].map(|index| read::<[f32; 4]>(planet, 0x1e60 + index * 16).map(f64::from));
        }
        for i in 0..3 {
            if wanted & colour(i) != 0 {
                facts.colours[i] = read::<[f32; 4]>(planet, i * 0x70).map(f64::from);
            }
        }
        if wanted & (colour(3) | colour(4) | colour(5) | colour(6) | colour(7) | colour(8)) != 0 {
            if generator.is_null() {
                return Err("null sky generator");
            }
            let mut biome: i32 = read(planet, 0x32b8);
            let subtype: i32 = read(planet, 0x32bc);
            if !(0..17).contains(&biome) {
                return Err("invalid sky biome");
            }
            if subtype == 25 {
                biome = 12;
            } else if subtype == 26 {
                biome = 13;
            }
            for (table_offset, index_offset, fields) in [
                (
                    0x518,
                    0x1e34,
                    &[(3, 0), (4, 0x20), (5, 0x30), (6, 0x40), (7, 0x70)][..],
                ),
                (0x528, 0x1e3c, &[(8, 0x70)][..]),
            ] {
                if !fields.iter().any(|(i, _)| wanted & colour(*i) != 0) {
                    continue;
                }
                let table = pointer(read(generator, table_offset))?;
                let mut header = table.add(biome as usize * 16);
                if read::<i32>(header, 8) <= 0 {
                    header = table.add(0x120);
                }
                let row = record(header, read(planet, index_offset), 0xe0)?;
                for &(i, offset) in fields {
                    if wanted & colour(i) != 0 {
                        facts.colours[i] = read::<[f32; 4]>(row, offset).map(f64::from);
                    }
                }
            }
        }
        if wanted & colour(9) != 0 {
            if generator.is_null() {
                return Err("null water generator");
            }
            let header = pointer(read(generator, 0x530))?;
            let row = record(header, read(planet, 0x3518), 0x80)?;
            facts.colours[9] =
                water_reflectance(read(row, 0x40), read(row, 0x50), read(row, 0x68))?;
        }
    }
    facts.present = wanted
        | if wanted & COLOURS != 0 {
            SCREEN_FILTER
        } else {
            0
        };
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storm_profiles_read_selected_loaded_overrides_and_defaults_without_colour_capture() {
        let mut planet = vec![0u8; 0x3b00];
        let mut generator = vec![0u8; 0x600];
        let mut selected = vec![0u8; 0xbf0];
        let mut defaults = vec![0u8; 0x200];
        let rows = [selected.as_ptr() as usize; 17];
        generator[0x504..0x508].copy_from_slice(&17u32.to_le_bytes());
        generator[0x508..0x510].copy_from_slice(&(rows.as_ptr() as usize).to_le_bytes());
        generator[0x4f8..0x500].copy_from_slice(&(defaults.as_ptr() as usize).to_le_bytes());
        planet[0x1e48..0x1e4c].copy_from_slice(&2u32.to_le_bytes());
        planet[0x1e50..0x1e54].copy_from_slice(&1u32.to_le_bytes());
        planet[0x1e54..0x1e58].copy_from_slice(&2u32.to_le_bytes());
        // An invalid colour filter and absent sky/water tables must be irrelevant.
        planet[0x1e44..0x1e48].copy_from_slice(&(-1i32).to_le_bytes());
        for (offset, values) in [
            (0xad0, [20f32, 12.]),
            (0xae8, [140., 92.]),
            (0xb30, [0., 0.]),
            (0xb48, [0., 0.]),
            (0xb60, [0., 0.]),
            (0xb78, [0., 0.]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                selected[offset + index * 4..offset + index * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        // Toxicity comes from the default table, not the selected zero override.
        for (offset, value) in [(0x140, 92f32), (0x144, 50.), (0x158, 100.), (0x15c, 92.)] {
            defaults[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        selected[0xbe0] = 1;
        selected[0xbe1] = 0;
        selected[0xbe2] = 1;
        let wanted = WEATHER;
        let captured = unsafe { capture(planet.as_ptr(), generator.as_ptr(), wanted) }.unwrap();
        assert_eq!(captured.present, wanted);
        assert_eq!(captured.screen_filter, -1);
        assert_eq!(captured.weather, [2, 1, 2]);
        let profile = unsafe { storm_profile(planet.as_ptr(), generator.as_ptr()) }.unwrap();
        assert_eq!(
            profile,
            [
                [[20., 12.], [140., 92.]],
                [[92., 50.], [100., 92.]],
                [[0.; 2]; 2],
            ]
        );
        let mut facts = search_probes_model::facts::Facts::default();
        facts.set_extreme_storm_profile(
            search_probes_model::weather::extreme_storm_profile(&profile).unwrap(),
        );
        crate::snapshot::apply_capture(&mut facts, &captured, 0).unwrap();
        assert_eq!(
            facts.planet_flag(search_probes_model::criteria::PlanetFlag::ExtremeStorms),
            Some(true)
        );
        assert!(unsafe { storm_profile(planet.as_ptr(), std::ptr::null()) }.is_err());
        planet[0x1e54..0x1e58].copy_from_slice(&17u32.to_le_bytes());
        assert!(unsafe { storm_profile(planet.as_ptr(), generator.as_ptr()) }.is_err());
        planet[0x1e54..0x1e58].copy_from_slice(&2u32.to_le_bytes());
        generator[0x504..0x508].copy_from_slice(&18u32.to_le_bytes());
        assert!(unsafe { storm_profile(planet.as_ptr(), generator.as_ptr()) }.is_err());
    }
    #[test]
    fn any_does_not_read_planet_or_generator() {
        let facts = unsafe { capture(std::ptr::null(), std::ptr::null(), 0) }.unwrap();
        assert_eq!(facts, Facts::default());
    }
    #[test]
    fn palette_only_does_not_read_native_tables() {
        let mut planet = [0u8; 0x1e48];
        for (chunk, value) in planet[0x70..]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip([0.1f32, 0.2, 0.3, 1.])
        {
            chunk.copy_from_slice(&value.to_ne_bytes());
        }
        let facts = unsafe { capture(planet.as_ptr(), std::ptr::null(), colour(1)) }.unwrap();
        assert_eq!(facts.present, colour(1) | SCREEN_FILTER);
        assert_eq!(facts.colours[1], [0.1f32, 0.2, 0.3, 1.].map(f64::from));
    }
    #[test]
    fn grass_reads_render_bound_tiles_and_selected_grading() {
        let mut planet = vec![0; 0x3ae0];
        for (slot, rgba) in [(1, [0.2f32, 0.3, 0.4, 1.]), (3, [0.5f32, 0.6, 0.7, 1.])] {
            for (i, v) in rgba.iter().enumerate() {
                let offset = 0x1e60 + slot * 16 + i * 4;
                planet[offset..offset + 4].copy_from_slice(&v.to_ne_bytes());
            }
        }
        planet[0x1e44..0x1e48].copy_from_slice(&13i32.to_ne_bytes());
        let captured = unsafe { capture(planet.as_ptr(), std::ptr::null(), colour(0)) }.unwrap();
        assert_eq!(
            captured.grass_terrain,
            [[0.2f32, 0.3, 0.4, 1.], [0.5f32, 0.6, 0.7, 1.]].map(|c| c.map(f64::from))
        );
        assert_eq!(captured.screen_filter, 13);
        assert_eq!(captured.present, colour(0) | SCREEN_FILTER);
        assert_eq!(captured.colours[0], [0.; 4]);
    }
    #[test]
    fn night_and_water_use_their_native_tables_and_compiled_slots() {
        use search_probes_model::criteria::{Criteria, Field};

        fn put<T: Copy>(bytes: &mut [u8], offset: usize, value: T) {
            assert!(offset + std::mem::size_of::<T>() <= bytes.len());
            unsafe {
                bytes
                    .as_mut_ptr()
                    .add(offset)
                    .cast::<T>()
                    .write_unaligned(value)
            };
        }

        let mut planet = vec![0; 0x3540];
        let mut generator = vec![0; 0x540];
        let mut night_headers = vec![0; 19 * 16];
        let mut night_rows = vec![0; 2 * 0xe0];
        let mut water_header = [0; 16];
        let mut water_row = [0; 0x80];
        put(&mut planet, 0x32b8, 13i32);
        put(&mut planet, 0x32bc, 1i32);
        put(&mut planet, 0x1e3c, 1i32);
        put(&mut night_rows, 0xe0 + 0x70, [0.3f32, 0.4, 0.5, 1.]);
        put(&mut night_headers, 0x120, night_rows.as_ptr() as usize);
        put(&mut night_headers, 0x128, 2i32);
        put(&mut generator, 0x528, night_headers.as_ptr() as usize);
        put(&mut water_row, 0x40, [0.8f32, 0.6, 0.4]);
        put(&mut water_row, 0x50, [0.2f32, 0.4, 0.6]);
        put(&mut water_row, 0x68, [1.0f32, 2.]);
        put(&mut water_header, 0, water_row.as_ptr() as usize);
        put(&mut water_header, 8, 1i32);
        put(&mut generator, 0x530, water_header.as_ptr() as usize);

        let mut criteria = Criteria::default();
        criteria.choices.insert(Field::NightSky, 1 << 5);
        criteria.choices.insert(Field::WaterColour, 1 << 3);
        let wanted = u32::from(criteria.compile().unwrap().requirements.colours) << 8;
        assert_eq!(
            wanted,
            colour(Colour::SkyNight as usize) | colour(Colour::Water as usize)
        );
        let facts = unsafe { capture(planet.as_ptr(), generator.as_ptr(), wanted) }.unwrap();
        assert_eq!(facts.present, wanted | SCREEN_FILTER);
        assert_eq!(facts.colours[8], [0.3f32, 0.4, 0.5, 1.].map(f64::from));
        assert_eq!(
            facts.colours[9],
            water_reflectance([0.8, 0.6, 0.4], [0.2, 0.4, 0.6], [1., 2.]).unwrap()
        );
    }
    #[test]
    fn invalid_optics_fail_explicitly() {
        assert!(water_reflectance([1.; 3], [1.; 3], [1.; 2]).is_err());
        assert!(water_reflectance([0.5; 3], [0.5; 3], [0., 1.]).is_err());
        assert!(water_reflectance([f32::NAN; 3], [0.5; 3], [1.; 2]).is_err());
    }
}

/// # Safety
/// Requires the complete generated colour data of a live current-build planet.
pub unsafe fn flora_palettes(planet: *const u8) -> [[[f32; 4]; 5]; 66] {
    std::array::from_fn(|palette| {
        std::array::from_fn(|alternative| unsafe {
            read(planet, palette * 0x70 + alternative * 16)
        })
    })
}

/// # Safety
/// Requires the complete generated colour data of a live current-build planet.
pub unsafe fn flora_ground_colours(planet: *const u8) -> [[f32; 4]; 24] {
    std::array::from_fn(|index| unsafe { read(planet, 0x1e60 + index * 16) })
}

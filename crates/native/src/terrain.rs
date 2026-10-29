use search_probes_model::terrain::{NoiseLayer, Shape, Terrain};

unsafe fn read<T: Copy>(planet: *const u8, offset: usize) -> T {
    unsafe { planet.add(offset).cast::<T>().read_unaligned() }
}

fn positive_finite(values: &[f32]) -> Result<(), &'static str> {
    if values.iter().any(|v| !v.is_finite() || *v < 0.) {
        return Err("invalid generated terrain dimensions");
    }
    Ok(())
}

unsafe fn shape(planet: *const u8, offset: usize) -> Result<Shape, &'static str> {
    if unsafe { read::<u8>(planet, offset + 0x3c) } == 0 {
        return Ok(Shape::default());
    }
    let [height, width, ratio, region_size] =
        [0x4, 0x38, 0x20, 0x24].map(|i| unsafe { read::<f32>(planet, offset + i) });
    positive_finite(&[height, width, ratio, region_size])?;
    let kind = unsafe { read::<u32>(planet, offset) };
    if kind > 1 {
        return Err("unsupported generated terrain shape");
    }
    Ok(Shape {
        active: true,
        subtract: unsafe { read::<u8>(planet, offset + 0x3d) } != 0,
        trench: unsafe { read::<u8>(planet, offset + 0x3e) } != 0,
        kind,
        height,
        width,
        ratio,
        region_size,
    })
}

/// # Safety
/// `planet` is initialized GcPlanetData from the verified 7.06 query visitor, valid until return.
pub unsafe fn features(planet: *const u8) -> Result<u32, &'static str> {
    if planet.is_null() {
        return Err("null generated terrain");
    }
    let mut terrain = Terrain::default();
    for (i, layer) in terrain.noise.iter_mut().enumerate() {
        let offset = 0x2af0 + i * 0x84;
        if unsafe { read::<u8>(planet, offset + 0x80) } == 0 {
            continue;
        }
        let [height, width, region_ratio, region_scale, plateau_stratas] =
            [0x40, 0x7c, 0x60, 0x64, 0x58].map(|i| unsafe { read::<f32>(planet, offset + i) });
        positive_finite(&[height, width, region_ratio, region_scale, plateau_stratas])?;
        *layer = NoiseLayer {
            active: true,
            subtract: unsafe { read::<u8>(planet, offset + 0x81) } != 0,
            height,
            width,
            region_ratio,
            region_scale,
            plateau_stratas,
        };
    }
    for (i, output) in terrain.shapes.iter_mut().enumerate() {
        *output = unsafe { shape(planet, 0x2f10 + i * 0x40) }?;
    }
    for (i, output) in terrain.caves.iter_mut().enumerate() {
        *output = unsafe { shape(planet, 0x30d0 + i * 0x40) }?;
    }
    Ok(terrain.features())
}

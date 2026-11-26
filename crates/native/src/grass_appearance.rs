use crate::{colour_lut, game_assets};
use search_probes_model::{
    grass::{self, GrassModel},
    vocabulary::GRASS_COLOUR_SET,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

type Result<T> = std::result::Result<T, &'static str>;
type Grade = Box<dyn Fn([f64; 3]) -> [f64; 3] + Send + Sync>;

pub struct Prepared {
    variants: Vec<grass::PrimaryColour<Grade>>,
}

impl Prepared {
    pub fn fixed_colour(&self) -> Option<u32> {
        self.variants
            .iter()
            .try_fold(GRASS_COLOUR_SET, |colours, variant| {
                variant.fixed_colour().map(|colour| colours | colour)
            })
    }
}

pub fn texture_brightness(bytes: &[u8]) -> Result<f64> {
    if bytes.len() < 128 || &bytes[..4] != b"DDS " {
        return Err("invalid grass texture header");
    }
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let (width, height) = (word(16) as usize, word(12) as usize);
    if word(4) != 124
        || word(76) != 32
        || word(80) & 4 == 0
        || width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || width % 4 != 0
        || height % 4 != 0
        || word(24) > 1
        || word(112) & 0x20fe00 != 0
    {
        return Err("unsupported grass texture dimensions");
    }
    let (start, format) = match &bytes[84..88] {
        b"DX10"
            if bytes.len() >= 148
                && matches!(word(128), 98 | 99)
                && word(132) == 3
                && word(136) & 4 == 0
                && word(140) == 1 =>
        {
            (148, 7)
        }
        b"DXT5" => (128, 3),
        b"DXT1" => (128, 1),
        _ => return Err("unsupported grass texture format"),
    };
    let block_size = if format == 1 { 8 } else { 16 };
    let blocks = bytes
        .get(start..start + width * height / 16 * block_size)
        .ok_or("truncated grass texture")?;
    let mut histogram = [0u64; 256];
    for block in blocks.chunks_exact(block_size) {
        let mut rgba = [0u8; 64];
        if format == 7 {
            if block[0] == 0 {
                return Err("invalid BC7 grass texture block");
            }
            bcdec_rs::bc7(block, &mut rgba, 16);
        } else if format == 3 {
            bcdec_rs::bc3(block, &mut rgba, 16);
        } else {
            bcdec_rs::bc1(block, &mut rgba, 16);
        }
        for &[r, g, b, alpha] in rgba.as_chunks::<4>().0 {
            histogram[r.max(g).max(b) as usize] += u64::from(alpha);
        }
    }
    let weight: u64 = histogram.iter().sum();
    if weight == 0 {
        return Err("grass texture has no visible texels");
    }
    let mut cumulative = 0;
    let median = histogram
        .iter()
        .position(|&count| {
            cumulative += count;
            cumulative > weight / 2
        })
        .ok_or("invalid grass texture histogram")?;
    let srgb = median as f64 / 255.;
    let linear = if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    };
    Ok((2. * linear).min(1.))
}

pub fn material_brightness(root: &Path, model: GrassModel) -> Result<Vec<f64>> {
    let (archive, textures): (&str, &[&str]) = match model {
        GrassModel::Cross => (
            "NMSARC.TexBiomesCOMMON.pak",
            &[
                "lushgrass7.base.dds",
                "lushgrass3.base.dds",
                "lushgrass4.base.dds",
                "lushgrass5.base.dds",
                "lushgrass6.base.dds",
                "curlygrass.base.dds",
                "dandegrass.base.dds",
                "seaweedgrass.base.dds",
                "tentaclegrass.base.dds",
            ],
        ),
        GrassModel::Lush => ("NMSARC.TexBiomesCOMMON.pak", &["newlushgrass.dds"]),
        GrassModel::Bubble => ("NMSARC.TexBiomesCOMMON.pak", &["bubblegrass.base.dds"]),
        GrassModel::Barren => ("NMSARC.TexBiomesBARREN.pak", &["thickgrassclump.dds"]),
    };
    textures
        .iter()
        .map(|texture| {
            let directory = if model == GrassModel::Barren {
                "textures/planets/biomes/barren/hq/foliage"
            } else {
                "textures/planets/biomes/common/grass"
            };
            let path = format!("{directory}/{texture}");
            texture_brightness(&game_assets::read(root, archive, &path, 32 * 1024 * 1024)?)
        })
        .collect()
}

fn classify(
    primary: &grass::PrimaryColour<impl Fn([f64; 3]) -> [f64; 3]>,
    models: u32,
    brightness: impl Fn(GrassModel) -> Result<Arc<Vec<f64>>>,
) -> Result<u32> {
    if models & !0xf != 0 {
        return Err("unsupported grass model");
    }
    if models == 0 {
        return Ok(GRASS_COLOUR_SET);
    }
    let mut common = None;
    for model in GrassModel::ALL {
        if models & model.mask() == 0 {
            continue;
        }
        let materials = brightness(model)?;
        if materials.is_empty() {
            return Err("missing grass materials");
        }
        for &factor in materials.iter() {
            let colour = primary.material_colour(factor)?;
            common = Some(common.map_or(colour, |previous| previous & colour));
        }
    }
    Ok(common.unwrap_or(GRASS_COLOUR_SET))
}

pub fn from_game(root: &Path, primary: [f64; 4], models: u32, filter: i32) -> Result<u32> {
    if models == 0 {
        return Ok(GRASS_COLOUR_SET);
    }
    let prepared = prepare_with_world(primary, colour_lut::from_game_world_grade(root, filter)?)?;
    prepared
        .variants
        .iter()
        .try_fold(GRASS_COLOUR_SET, |colours, primary| {
            classify(primary, models, |model| {
                Ok(Arc::new(material_brightness(root, model)?))
            })
            .map(|classified| colours | classified)
        })
}

fn prepare_with_world(primary: [f64; 4], world: colour_lut::WorldGrade) -> Result<Prepared> {
    let variants = world
        .strengths()
        .iter()
        .copied()
        .map(|strength| {
            let world = world.clone();
            let grade: Grade = Box::new(move |rgb| world.sample(rgb, strength));
            grass::PrimaryColour::new(primary, grade)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Prepared { variants })
}

pub fn prepare(primary: [f64; 4], filter: i32) -> Result<Prepared> {
    prepare_with_world(primary, colour_lut::world_grade(filter)?)
}

pub fn get(primary: &Prepared, models: u32) -> Result<u32> {
    if models == 0 {
        return Ok(GRASS_COLOUR_SET);
    }
    static CACHE: OnceLock<Mutex<BTreeMap<GrassModel, Arc<Vec<f64>>>>> = OnceLock::new();
    primary
        .variants
        .iter()
        .try_fold(GRASS_COLOUR_SET, |colours, variant| {
            classify(variant, models, |model| {
                let mut cache = CACHE
                    .get_or_init(Default::default)
                    .lock()
                    .map_err(|_| "grass material cache poisoned")?;
                if let Some(value) = cache.get(&model) {
                    return Ok(value.clone());
                }
                let exe = std::env::current_exe().map_err(|_| "cannot locate NMS executable")?;
                let root = exe
                    .parent()
                    .and_then(Path::parent)
                    .ok_or("cannot locate NMS game data")?;
                let value = Arc::new(material_brightness(root, model)?);
                cache.insert(model, value.clone());
                Ok(value)
            })
            .map(|classified| colours | classified)
        })
}

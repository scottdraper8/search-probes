use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(PartialEq)]
pub struct Lut {
    size: usize,
    rgb: Vec<[f64; 3]>,
}

impl Lut {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let u32_at = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        if bytes.len() < 128 || &bytes[..4] != b"DDS " || u32_at(4) != 124 {
            return Err("invalid world colour LUT header");
        }
        let size = u32_at(16) as usize;
        if ![16, 32].contains(&size)
            || u32_at(12) as usize != size
            || u32_at(24) as usize != size
            || u32_at(80) & 0x40 == 0
            || u32_at(84) != 0
            || u32_at(88) != 32
            || [92, 96, 100, 104].map(u32_at) != [0xff0000, 0xff00, 0xff, 0xff000000]
            || bytes.len() != 128 + size.pow(3) * 4
        {
            return Err("unsupported world colour LUT format");
        }
        let rgb = bytes[128..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[2], p[1], p[0]].map(|v| f64::from(v) / 255.))
            .collect();
        Ok(Self { size, rgb })
    }

    pub fn sample(&self, rgb: [f64; 3]) -> [f64; 3] {
        let position = rgb.map(|v| (v * self.size as f64 - 0.5).clamp(0., (self.size - 1) as f64));
        let low = position.map(|v| v as usize);
        let fraction = std::array::from_fn::<_, 3, _>(|i| position[i] - low[i] as f64);
        let mut result = [0.; 3];
        for corner in 0..8 {
            let high = [corner & 1 != 0, corner & 2 != 0, corner & 4 != 0];
            let index = std::array::from_fn::<_, 3, _>(|i| {
                (low[i] + usize::from(high[i])).min(self.size - 1)
            });
            let weight: f64 = (0..3)
                .map(|i| {
                    if high[i] {
                        fraction[i]
                    } else {
                        1. - fraction[i]
                    }
                })
                .product();
            let value = self.rgb[(index[2] * self.size + index[1]) * self.size + index[0]];
            for i in 0..3 {
                result[i] += value[i] * weight;
            }
        }
        result.map(|v| v.clamp(0., 1.))
    }
}

pub fn from_game(root: &Path, filter: i32) -> Result<Lut> {
    let name = filename(filter).ok_or("unsupported generated world colour filter")?;
    Lut::decode(&crate::game_assets::read(
        root,
        "NMSARC.TexMisc.pak",
        name,
        256 * 1024,
    )?)
}

pub fn get(filter: i32) -> Result<Arc<Lut>> {
    static CACHE: OnceLock<Mutex<BTreeMap<i32, Arc<Lut>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "world colour cache poisoned")?;
    if let Some(lut) = cache.get(&filter) {
        return Ok(lut.clone());
    }
    let exe = std::env::current_exe().map_err(|_| "cannot locate NMS executable")?;
    let root = exe
        .parent()
        .and_then(Path::parent)
        .ok_or("cannot locate NMS game data")?;
    let lut = Arc::new(from_game(root, filter)?);
    cache.insert(filter, lut.clone());
    Ok(lut)
}

fn filename(filter: i32) -> Option<&'static str> {
    const FILES: [&str; 85] = [
        "textures/lut/filters/default.dds",
        "textures/lut/filters/defaultstorm.dds",
        "textures/lut/filters/frozendefault.dds",
        "textures/lut/filters/frozenstorm.dds",
        "textures/lut/filters/toxicdefault.dds",
        "textures/lut/filters/toxicstorm.dds",
        "textures/lut/filters/radiodefault.dds",
        "textures/lut/filters/radiostorm.dds",
        "textures/lut/filters/scorchdefault.dds",
        "textures/lut/filters/scorchstorm.dds",
        "textures/lut/filters/barrendefault.dds",
        "textures/lut/filters/barrenstorm.dds",
        "textures/lut/filters/weird1.dds",
        "textures/lut/filters/weird2.dds",
        "textures/lut/filters/weird3.dds",
        "textures/lut/filters/weird4.dds",
        "textures/lut/filters/weird5.dds",
        "textures/lut/filters/weird6.dds",
        "textures/lut/filters/weird7.dds",
        "textures/lut/filters/weird8.dds",
        "textures/lut/filters/vintage.dds",
        "textures/lut/filters/hyperreal.dds",
        "textures/lut/filters/desaturated.dds",
        "textures/lut/filters/amber.dds",
        "textures/lut/filters/gb_green.dds",
        "textures/lut/filters/apocalypse.dds",
        "textures/lut/filters/diffusion.dds",
        "textures/lut/filters/green.dds",
        "textures/lut/filters/blackandwhite.dds",
        "textures/lut/filters/isolation.dds",
        "textures/lut/filters/sepia.dds",
        "textures/lut/filters/filmic.dds",
        "textures/lut/filters/gb_grey.dds",
        "textures/lut/filters/binoculars.dds",
        "textures/lut/filters/surveying1.dds",
        "textures/lut/filters/petmode.dds",
        "textures/lut/filters/nexus.dds",
        "textures/lut/filters/default.dds",
        "textures/lut/filters/default.dds",
        "textures/lut/filters/afdefault.dds",
        "textures/lut/filters/default.dds",
        "textures/lut/filters/missionsurvey.dds",
        "textures/lut/filters/vibrant.dds",
        "textures/lut/filters/vibrantbright.dds",
        "textures/lut/filters/vibrantwarm.dds",
        "textures/lut/filters/retrobright.dds",
        "textures/lut/filters/retrowash.dds",
        "textures/lut/filters/drama.dds",
        "textures/lut/filters/memorybold.dds",
        "textures/lut/filters/memory.dds",
        "textures/lut/filters/memorywash.dds",
        "textures/lut/filters/autumn.dds",
        "textures/lut/filters/autumnfade.dds",
        "textures/lut/filters/classicbright.dds",
        "textures/lut/filters/classic.dds",
        "textures/lut/filters/classicwash.dds",
        "textures/lut/filters/blackandwhitedream.dds",
        "textures/lut/filters/colourtouchb.dds",
        "textures/lut/filters/colourtouchc.dds",
        "textures/lut/filters/negativeprint.dds",
        "textures/lut/filters/sepiaextreme.dds",
        "textures/lut/filters/solarise.dds",
        "textures/lut/filters/twotonestrong.dds",
        "textures/lut/filters/twotone.dds",
        "textures/lut/filters/dramatic.dds",
        "textures/lut/filters/fuchsia.dds",
        "textures/lut/filters/violet.dds",
        "textures/lut/filters/cyan.dds",
        "textures/lut/filters/greenb.dds",
        "textures/lut/filters/amberb.dds",
        "textures/lut/filters/red.dds",
        "textures/lut/filters/hueshifta.dds",
        "textures/lut/filters/hueshiftb.dds",
        "textures/lut/filters/hueshiftc.dds",
        "textures/lut/filters/hueshiftd.dds",
        "textures/lut/filters/warmstripe.dds",
        "textures/lut/filters/nmsretroa.dds",
        "textures/lut/filters/nmsretrob.dds",
        "textures/lut/filters/nmsretroc.dds",
        "textures/lut/filters/nmsretrod.dds",
        "textures/lut/filters/nmsretroe.dds",
        "textures/lut/filters/nmsretrof.dds",
        "textures/lut/filters/nmsretrog.dds",
        "textures/lut/filters/corruptsentinels.dds",
        "textures/lut/filters/default.dds",
    ];
    usize::try_from(filter)
        .ok()
        .and_then(|i| FILES.get(i).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_filter_bounds_and_post_biome_corruption_override() {
        assert_eq!(
            filename(83),
            Some("textures/lut/filters/corruptsentinels.dds")
        );
        assert_eq!(filename(84), Some("textures/lut/filters/default.dds"));
        for invalid in [-1, 85, i32::MAX] {
            assert_eq!(filename(invalid), None);
        }
    }

    fn dds() -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(b"DDS ");
        for (offset, value) in [
            (4, 124u32),
            (12, 16),
            (16, 16),
            (24, 16),
            (80, 0x41),
            (88, 32),
            (92, 0xff0000),
            (96, 0xff00),
            (100, 0xff),
            (104, 0xff000000),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for b in 0..16 {
            for g in 0..16 {
                for r in 0..16 {
                    bytes.extend_from_slice(&[b * 17, g * 17, r * 17, 255]);
                }
            }
        }
        bytes
    }

    #[test]
    fn dds_axes_texel_centres_interpolation_and_bounds() {
        let mut bytes = dds();
        let lut = Lut::decode(&bytes).unwrap();
        for rgb in [
            [0.; 3],
            [1.; 3],
            [1., 0., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [0.5; 3],
        ] {
            for (actual, expected) in lut.sample(rgb).into_iter().zip(rgb) {
                assert!((actual - expected).abs() < 1e-12);
            }
        }
        assert_eq!(lut.sample([-1., 2., 0.]), [0., 1., 0.]);
        assert_eq!(lut.sample([0.5 / 16., 0., 0.]), [0.; 3]);
        bytes[24..28].copy_from_slice(&32u32.to_le_bytes());
        assert!(Lut::decode(&bytes).is_err());
        assert!(Lut::decode(&dds()[..160]).is_err());
        bytes = dds();
        bytes[92..96].copy_from_slice(&0xffu32.to_le_bytes());
        assert!(Lut::decode(&bytes).is_err());
    }
}

use std::{
    cell::RefCell,
    collections::BTreeMap,
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex, OnceLock, Weak},
};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(PartialEq)]
pub struct Lut {
    size: usize,
    rgb: Vec<[f64; 3]>,
}

#[derive(Clone)]
pub struct WorldGrade {
    base: Arc<Lut>,
    selected: Arc<Lut>,
    fade_distance: f64,
}

const SAMPLE_CACHE_CAPACITY: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SampleKey {
    base: usize,
    selected: usize,
    rgb: [u64; 3],
    strength: u64,
}

struct CachedSample {
    _base: Weak<Lut>,
    _selected: Weak<Lut>,
    value: [f64; 3],
}

#[derive(Default)]
struct SampleCache {
    entries: BTreeMap<SampleKey, CachedSample>,
    order: VecDeque<SampleKey>,
}

impl SampleCache {
    fn get(&self, key: SampleKey) -> Option<[f64; 3]> {
        self.entries.get(&key).map(|sample| sample.value)
    }

    fn insert(&mut self, key: SampleKey, grade: &WorldGrade, value: [f64; 3]) {
        if self.entries.contains_key(&key) {
            return;
        }
        while self.entries.len() >= SAMPLE_CACHE_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(
            key,
            CachedSample {
                _base: Arc::downgrade(&grade.base),
                _selected: Arc::downgrade(&grade.selected),
                value,
            },
        );
        self.order.push_back(key);
    }
}

thread_local! {
    static SAMPLE_CACHE: RefCell<SampleCache> = RefCell::new(SampleCache::default());
}

impl WorldGrade {
    const FULL_STRENGTH: [f64; 1] = [1.0];
    const DISTANCE_STRENGTHS: [f64; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];

    pub fn fade_distance(&self) -> f64 {
        self.fade_distance
    }

    pub fn strengths(&self) -> &'static [f64] {
        if self.fade_distance > 0.0 {
            &Self::DISTANCE_STRENGTHS
        } else {
            &Self::FULL_STRENGTH
        }
    }

    pub fn sample(&self, rgb: [f64; 3], strength: f64) -> [f64; 3] {
        let strength = strength.clamp(0.0, 1.0);
        let key = SampleKey {
            base: Arc::as_ptr(&self.base) as usize,
            selected: Arc::as_ptr(&self.selected) as usize,
            rgb: rgb.map(f64::to_bits),
            strength: strength.to_bits(),
        };
        if let Some(value) = SAMPLE_CACHE.with(|cache| cache.borrow().get(key)) {
            return value;
        }
        let value = if strength <= 0.0 {
            self.base.sample(rgb)
        } else if strength >= 1.0 {
            self.selected.sample(rgb)
        } else {
            let base = self.base.sample(rgb);
            let selected = self.selected.sample(rgb);
            std::array::from_fn(|channel| {
                base[channel] + (selected[channel] - base[channel]) * strength
            })
        };
        SAMPLE_CACHE.with(|cache| cache.borrow_mut().insert(key, self, value));
        value
    }

    pub fn full_sample(&self, rgb: [f64; 3]) -> [f64; 3] {
        self.sample(rgb, 1.0)
    }
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

pub fn from_game_world_grade(root: &Path, filter: i32) -> Result<WorldGrade> {
    Ok(WorldGrade {
        base: Arc::new(from_game(root, 0)?),
        selected: Arc::new(from_game(root, filter)?),
        fade_distance: fade_distance(filter),
    })
}

pub fn world_grade(filter: i32) -> Result<WorldGrade> {
    Ok(WorldGrade {
        base: get(0)?,
        selected: get(filter)?,
        fade_distance: fade_distance(filter),
    })
}

/// Distance at which the generated screen filter reaches full strength. Zero-valued filters are
/// authored as full-screen grades. These are the generated-world entries in the current game
/// screen-filter table; non-world/photo-mode values correctly fall back to full strength.
pub const fn fade_distance(filter: i32) -> f64 {
    match filter {
        15 | 60 => 100.0,
        76..=82 => 150.0,
        71..=74 => 250.0,
        18 => 300.0,
        _ => 0.0,
    }
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

    fn constant(rgb: [f64; 3]) -> Arc<Lut> {
        Arc::new(Lut {
            size: 2,
            rgb: vec![rgb; 8],
        })
    }

    #[test]
    fn distance_grade_blends_base_and_selected_luts() {
        let grade = WorldGrade {
            base: constant([0.0, 0.0, 0.0]),
            selected: constant([1.0, 0.5, 0.25]),
            fade_distance: 250.0,
        };
        assert_eq!(grade.strengths().first(), Some(&0.0));
        assert_eq!(grade.strengths().last(), Some(&1.0));
        assert_eq!(grade.sample([0.2, 0.4, 0.6], 0.5), [0.5, 0.25, 0.125]);
        assert_eq!(grade.full_sample([0.2, 0.4, 0.6]), [1.0, 0.5, 0.25]);
    }

    #[test]
    fn generated_filter_fade_distances_match_the_game_table() {
        assert_eq!(fade_distance(73), 250.0);
        assert_eq!(fade_distance(76), 150.0);
        assert_eq!(fade_distance(15), 100.0);
        assert_eq!(fade_distance(18), 300.0);
        assert_eq!(fade_distance(42), 0.0);
    }
}

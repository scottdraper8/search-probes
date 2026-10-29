use crate::{
    bioluminescence::{Assets as Source, Mbin},
    colour_lut,
    flora_geometry::{Assets as Geometry, VertexSample},
};
use search_probes_model::{appearance, vocabulary::GRASS_COLOUR_SET};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

type Result<T> = std::result::Result<T, &'static str>;
pub type Palettes = [[[f32; 4]; 5]; 66];
#[derive(Clone)]
pub struct Context {
    pub palettes: Palettes,
    pub ground_colours: [[f32; 4]; 24],
    pub palette_mode: u32,
}
impl Context {
    fn ground(&self, spawn: &Spawn) -> [f32; 4] {
        let index = spawn.source_ground_index.checked_mul(2).and_then(|index| {
            index.checked_sub(if spawn.ground_colour_index == 2 { 2 } else { 1 })
        });
        index
            .and_then(|index| usize::try_from(index).ok())
            .filter(|&index| index < 23)
            .and_then(|index| self.ground_colours.get(index).copied())
            .unwrap_or([0., 0., 1., 1.])
    }
    fn palette(&self, spawn: &Spawn, palette: usize, alternative: usize) -> Result<[f32; 4]> {
        let alternative = alternative.min(3);
        if self.palette_mode == 0
            || !spawn.swap_primary_for_secondary && !spawn.swap_primary_for_random
        {
            return Ok(self.palettes[palette][alternative]);
        }
        if self.palette_mode == 1 {
            return Ok([1., 0., 0., 1.]);
        }
        if spawn.swap_primary_for_random {
            return Err("native random palette rotation requires source seed");
        }
        let index = if spawn.swap_primary_for_secondary {
            [1, 0, 3, 4, 2][alternative]
        } else {
            alternative
        };
        Ok(self.palettes[palette][index])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub colours: u32,
    pub known: u32,
    pub complete: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Spawn {
    pub source_ground_index: i32,
    pub match_ground_colour: bool,
    pub ground_colour_index: u32,
    pub swap_primary_for_random: bool,
    pub swap_primary_for_secondary: bool,
    pub model: Vec<u8>,
}
impl Spawn {
    pub fn owned_bytes(&self) -> usize {
        self.model.capacity()
    }
}

fn asset_name(mbin: &Mbin<'_>, offset: usize) -> Result<Vec<u8>> {
    let bytes = mbin.bytes(offset, 16)?;
    Ok(bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(16)].to_vec())
}
struct Texture {
    average_valid: bool,
    pixels: Vec<[f64; 4]>,
    average: [f64; 3],
    recolour: Vec<appearance::RecolourPixel>,
}
struct OptionTexture {
    texture: Arc<Texture>,
    palette: usize,
    alternative: usize,
    average: [f64; 3],
    multiply: bool,
}
struct Material {
    procedural: bool,
    secondary: Option<Arc<Material>>,
    constant: Option<[f64; 4]>,
    srgb: bool,
    alpha_flags: u32,
    vertex_flags: u32,
    opacity: f64,
    layers: Vec<Vec<OptionTexture>>,
    optional: Vec<bool>,
    layer_keys: Vec<(Vec<u8>, Vec<u8>)>,
    palettes: Vec<(usize, usize)>,
}
struct PreparedMaterial {
    constant: Option<[f64; 4]>,
    layers: Vec<Vec<Vec<[f64; 5]>>>,
    optional: Vec<bool>,
    combinations: usize,
    srgb: bool,
}
impl PreparedMaterial {
    fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 32
            + self.optional.capacity()
            + self.layers.capacity() * std::mem::size_of::<Vec<Vec<[f64; 5]>>>()
            + self
                .layers
                .iter()
                .map(|layer| {
                    layer.capacity() * std::mem::size_of::<Vec<[f64; 5]>>()
                        + layer
                            .iter()
                            .map(|option| option.capacity() * std::mem::size_of::<[f64; 5]>())
                            .sum::<usize>()
                })
                .sum::<usize>()
    }
    fn recipe(&self, combination: usize) -> Vec<[f64; 4]> {
        if let Some(colour) = self.constant {
            return vec![colour; 1024];
        }
        let mut choice = combination;
        let mut pixels = vec![[0.; 4]; 1024];
        let mut base = true;
        for (layer_index, layer) in self.layers.iter().enumerate() {
            let count = layer.len() + usize::from(self.optional[layer_index]);
            let selected = choice % count;
            choice /= count;
            if selected == layer.len() {
                continue;
            }
            let option = &layer[selected];
            for (pixel, source) in pixels.iter_mut().zip(option) {
                for channel in 0..3 {
                    pixel[channel] = if base {
                        source[channel]
                    } else {
                        pixel[channel] * (1. - source[3]) + source[channel] * source[3]
                    };
                }
                if base {
                    pixel[3] = source[4];
                }
            }
            base = false;
        }
        if base {
            return Vec::new();
        }
        for pixel in &mut pixels {
            for channel in 0..3 {
                let diffuse = pixel[channel].powf(1. / 2.2);
                pixel[channel] = if self.srgb {
                    appearance::srgb_to_linear(diffuse)
                } else {
                    diffuse
                };
            }
        }
        pixels
    }
}
#[derive(Default)]
struct TexturePreparation {
    materials: HashMap<(usize, usize), Arc<PreparedMaterial>>,
    recipes: HashMap<(usize, usize, usize), Arc<[[f64; 4]]>>,
    bytes: usize,
}
impl TexturePreparation {
    const MAX_BYTES: usize = 16 * 1024 * 1024;
    fn reserve(&mut self, bytes: usize) -> bool {
        if bytes > Self::MAX_BYTES {
            return false;
        }
        if self.bytes + bytes > Self::MAX_BYTES {
            self.materials = HashMap::new();
            self.recipes = HashMap::new();
            self.bytes = 0;
        }
        self.bytes += bytes;
        true
    }
    fn material(
        &mut self,
        key: (usize, usize),
        prepare: impl FnOnce() -> Result<PreparedMaterial>,
    ) -> Result<Arc<PreparedMaterial>> {
        if let Some(prepared) = self.materials.get(&key) {
            return Ok(prepared.clone());
        }
        let prepared = Arc::new(prepare()?);
        if self.reserve(prepared.owned_bytes() + 96) {
            self.materials.insert(key, prepared.clone());
        }
        Ok(prepared)
    }
    fn recipe(
        &mut self,
        material_key: (usize, usize),
        prepared: &PreparedMaterial,
        combination: usize,
    ) -> Arc<[[f64; 4]]> {
        let key = (material_key.0, material_key.1, combination);
        if let Some(pixels) = self.recipes.get(&key) {
            return pixels.clone();
        }
        let pixels: Arc<[[f64; 4]]> = prepared.recipe(combination).into();
        if self.reserve(std::mem::size_of_val(pixels.as_ref()) + 128) {
            self.recipes.insert(key, pixels.clone());
        }
        pixels
    }
}

#[derive(Default)]
struct Preparation {
    textures: TexturePreparation,
    classified: HashMap<[u64; 3], u32>,
}

pub struct Model {
    materials: Vec<Arc<Material>>,
    samples: Vec<Arc<[VertexSample]>>,
    optional_layers: [std::collections::BTreeSet<(Vec<u8>, Vec<u8>)>; 2],
}
impl Model {
    fn metadata_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 32
            + self.materials.capacity() * std::mem::size_of::<Arc<Material>>()
            + self.samples.capacity() * std::mem::size_of::<Arc<[VertexSample]>>()
            + self
                .optional_layers
                .iter()
                .flat_map(|set| set.iter())
                .map(|(group, layer)| 64 + group.capacity() + layer.capacity())
                .sum::<usize>()
    }
    fn owned_bytes(&self) -> usize {
        let mut total = self.metadata_bytes();
        let mut samples = std::collections::BTreeSet::new();
        let mut materials = std::collections::BTreeSet::new();
        let mut textures = std::collections::BTreeSet::new();
        self.account_bytes(&mut samples, &mut materials, &mut textures, &mut total);
        total
    }
    fn account_bytes(
        &self,
        samples: &mut std::collections::BTreeSet<usize>,
        materials: &mut std::collections::BTreeSet<usize>,
        textures: &mut std::collections::BTreeSet<usize>,
        total: &mut usize,
    ) {
        for sample in &self.samples {
            if samples.insert(Arc::as_ptr(sample) as *const VertexSample as usize) {
                *total += 32 + std::mem::size_of_val(sample.as_ref());
            }
        }
        for material in &self.materials {
            material.account_bytes(materials, textures, total);
        }
    }
}

pub struct Assets {
    root: PathBuf,
    luts: BTreeMap<i32, Arc<colour_lut::Lut>>,
    source: Source,
    geometry: Geometry,
    models: BTreeMap<String, Arc<Model>>,
    model_bytes: usize,
    materials: BTreeMap<String, Arc<Material>>,
    textures: BTreeMap<String, Arc<Texture>>,
    texture_bytes: usize,
    material_bytes: usize,
    colours: BTreeMap<(Spawn, Vec<u32>), Observation>,
    colour_bytes: usize,
}

fn path(name: &str) -> Result<String> {
    let path = name.replace('\\', "/").to_ascii_lowercase();
    if path.len() > 4096 || path.starts_with('/') || path.split('/').any(|part| part == "..") {
        return Err("invalid flora asset path");
    }
    Ok(path)
}

impl Assets {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.into(),
            luts: BTreeMap::new(),
            source: Source::new(root),
            geometry: Geometry::new(root),
            models: BTreeMap::new(),
            model_bytes: 0,
            materials: BTreeMap::new(),
            textures: BTreeMap::new(),
            texture_bytes: 0,
            material_bytes: 0,
            colours: BTreeMap::new(),
            colour_bytes: 0,
        }
    }
    fn texture(&mut self, name: &str) -> Result<Arc<Texture>> {
        let name = path(name)?;
        if let Some(texture) = self.textures.get(&name) {
            return Ok(texture.clone());
        }
        let archive = if let Some(biome) = name
            .strip_prefix("textures/planets/biomes/")
            .and_then(|name| name.split('/').next())
        {
            format!("NMSARC.TexBiomes{}.pak", biome.to_ascii_uppercase())
        } else if name.starts_with("textures/common/spacecraft/") {
            "NMSARC.TexSpacecraft.pak".into()
        } else {
            return Err("unsupported flora diffuse archive");
        };
        let texture = Arc::new(decode_texture(&self.source.read(
            &archive,
            &name,
            32 * 1024 * 1024,
        )?)?);
        let bytes = std::mem::size_of::<Texture>()
            + name.len()
            + 64
            + texture.pixels.capacity() * std::mem::size_of::<[f64; 4]>()
            + texture.recolour.capacity() * std::mem::size_of::<appearance::RecolourPixel>();
        if bytes <= 128 * 1024 * 1024 {
            if self.texture_bytes + bytes > 128 * 1024 * 1024 {
                self.textures.clear();
                self.texture_bytes = 0;
            }
            self.texture_bytes += bytes;
            self.textures.insert(name, texture.clone());
        }
        Ok(texture)
    }
    fn material(&mut self, name: &str) -> Result<Arc<Material>> {
        self.material_sampler(name, 0)
    }
    fn material_sampler(&mut self, name: &str, sampler: usize) -> Result<Arc<Material>> {
        let original = path(name)?;
        let name = if sampler == 0 {
            original.clone()
        } else {
            format!("{original}#secondary")
        };
        if let Some(material) = self.materials.get(&name) {
            return Ok(material.clone());
        }
        let bytes = self
            .source
            .read("NMSARC.MetadataEtc.pak", &original, 4 * 1024 * 1024)?;
        let mbin = Mbin(&bytes);
        mbin.header(0x4737d48a)?;
        let mut diffuse = None;
        let mut second = false;
        let mut vertex = false;
        for flag in mbin.array(0x20, 4, 128)?.step_by(4) {
            second |= mbin.word(flag)? == 15;
            vertex |= mbin.word(flag)? == 20;
        }
        let mut srgb = true;
        for offset in mbin.array(0x70, 0x50, 256)?.step_by(0x50) {
            if mbin.string(offset + 0x30)?
                == if sampler == 0 {
                    "gDiffuseMap"
                } else {
                    "gDiffuse2Map"
                }
            {
                diffuse = Some(path(mbin.string(offset + 0x20)?)?);
                srgb = mbin.bytes(offset + 0x4d, 1)?[0] != 0;
            }
        }
        let mut alpha_flags = 0;
        let mut vertex_flags = 0;
        for offset in mbin.array(0xa0, 0x30, 256)?.step_by(0x30) {
            if mbin.string(offset + 0x20)? == "gDynamicFlags" {
                alpha_flags = mbin.word(offset + 4)?;
                if vertex {
                    vertex_flags = mbin.word(offset)?;
                }
            }
        }
        let mut uniform_colour = None;
        let mut opacity = 1.;
        for offset in mbin.array(0x90, 0x30, 256)?.step_by(0x30) {
            if mbin.string(offset + 0x20)? == "gMaterialColourVec4" {
                let colour = std::array::from_fn(|channel| {
                    f64::from(f32::from_bits(mbin.word(offset + channel * 4).unwrap()))
                });
                opacity = colour[3];
                uniform_colour = Some(colour);
            }
        }
        if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) {
            return Err("invalid flora material opacity");
        }
        let mut material = Material {
            procedural: false,
            secondary: None,
            constant: if diffuse.is_none() {
                Some(uniform_colour.ok_or("flora constant colour uniform is missing")?)
            } else {
                None
            },
            srgb,
            alpha_flags,
            vertex_flags,
            opacity,
            layers: Vec::new(),
            optional: Vec::new(),
            layer_keys: Vec::new(),
            palettes: Vec::new(),
        };
        if let Some(diffuse) = diffuse {
            let (directory, filename) = diffuse
                .rsplit_once('/')
                .ok_or("unsupported flora diffuse path")?;
            let stem = filename
                .split('.')
                .next()
                .ok_or("unsupported flora diffuse suffix")?;
            let descriptor = format!("{directory}/{stem}.texture.mbin");
            match self
                .source
                .read("NMSARC.Precache.pak", &descriptor, 4 * 1024 * 1024)
            {
                Ok(bytes) => {
                    let mbin = Mbin(&bytes);
                    mbin.header(0x4cf0c331)?;
                    material.procedural = true;

                    for index in (0..8).rev() {
                        let layer = 0x20 + index * 0x48;
                        let probability = f32::from_bits(mbin.word(layer + 0x40)?);
                        if !probability.is_finite() || probability < 0. {
                            return Err("invalid flora layer probability");
                        }
                        if probability == 0. {
                            continue;
                        }
                        let mut variants = Vec::new();
                        for option in mbin.array(layer + 0x30, 0x60, 256)?.step_by(0x60) {
                            let probability = f32::from_bits(mbin.word(option + 0x4c)?);
                            if !probability.is_finite() || probability < 0. {
                                return Err("invalid flora texture probability");
                            }
                            let palette = mbin.word(option + 0x48)? as usize;
                            let alternative = mbin.word(option + 0x40)? as usize;
                            if palette >= 66 || alternative > 7 {
                                return Err("invalid flora palette binding");
                            }
                            let texture = self.texture(mbin.string(option + 0x30)?)?;
                            let average = if mbin.bytes(option + 0x55, 1)?[0] != 0 {
                                std::array::from_fn(|channel| {
                                    f64::from(f32::from_bits(
                                        mbin.word(option + channel * 4).unwrap(),
                                    ))
                                })
                            } else {
                                if !texture.average_valid && alternative != 7 {
                                    return Err(
                                        "flora diffuse average requires unsupported fallback",
                                    );
                                }
                                texture.average
                            };
                            if average
                                .iter()
                                .any(|value| !value.is_finite() || !(0. ..=1.).contains(value))
                            {
                                return Err("invalid flora texture average");
                            }
                            if alternative < 6 {
                                material.palettes.push((palette, alternative.min(3)));
                            }
                            variants.push(OptionTexture {
                                texture,
                                palette,
                                alternative,
                                average,
                                multiply: mbin.bytes(option + 0x54, 1)?[0] != 0,
                            });
                        }
                        if !variants.is_empty() {
                            material.layers.push(variants);
                            material.optional.push(probability < 1.);
                            material.layer_keys.push((
                                asset_name(&mbin, layer)?,
                                asset_name(&mbin, layer + 0x20)?,
                            ));
                        }
                    }
                }
                Err("missing vegetation asset") => {
                    let texture = self.texture(&diffuse)?;
                    material.optional.push(false);
                    material.layer_keys.push((Vec::new(), Vec::new()));
                    material.layers.push(vec![OptionTexture {
                        average: texture.average,
                        texture,
                        palette: 0,
                        alternative: 7,
                        multiply: false,
                    }]);
                }
                Err(error) => return Err(error),
            }
        }
        if sampler == 0 && second {
            let secondary = self.material_sampler(&original, 1)?;
            material.palettes.extend(secondary.palettes.iter().copied());
            material.secondary = Some(secondary);
        }
        material.palettes.sort_unstable();
        material.palettes.dedup();
        let material = Arc::new(material);
        let bytes = material.owned_bytes() + name.len() + 64;
        if bytes <= 32 * 1024 * 1024 {
            if self.material_bytes + bytes > 32 * 1024 * 1024 {
                self.materials.clear();
                self.material_bytes = 0;
            }
            self.material_bytes += bytes;
            self.materials.insert(name, material.clone());
        }
        Ok(material)
    }
    pub fn model(&mut self, name: &str) -> Result<Arc<Model>> {
        let name = path(name)?;
        if let Some(model) = self.models.get(&name) {
            return Ok(model.clone());
        }
        let mut materials = Vec::new();
        let mut samples = Vec::new();
        for mesh in self.geometry.meshes(&name)? {
            let material = self.material(&mesh.material)?;
            if material.constant.is_some() || !material.layers.is_empty() {
                materials.push(material);
                samples.push(mesh.samples);
            }
        }
        let mut optional_layers: [std::collections::BTreeSet<(Vec<u8>, Vec<u8>)>; 2] =
            Default::default();
        for name in self.source.flora_materials(&name, 0)? {
            let material = self.material(&name)?;
            material.collect_optional(&mut optional_layers);
        }
        let model = Arc::new(Model {
            materials,
            samples,
            optional_layers,
        });
        self.retain_model(name, model.clone(), 256 * 1024 * 1024);
        Ok(model)
    }
    fn retained_model_bytes(&self) -> usize {
        let mut models = std::collections::BTreeSet::new();
        let mut samples = std::collections::BTreeSet::new();
        let mut materials = std::collections::BTreeSet::new();
        let mut textures = std::collections::BTreeSet::new();
        let mut total = 0;
        for (name, model) in &self.models {
            total += name.capacity() + 64;
            if models.insert(Arc::as_ptr(model) as usize) {
                total += model.metadata_bytes();
                model.account_bytes(&mut samples, &mut materials, &mut textures, &mut total);
            }
        }
        total
    }
    fn retain_model(&mut self, name: String, model: Arc<Model>, max_bytes: usize) {
        if model.owned_bytes() + name.capacity() + 64 > max_bytes {
            return;
        }
        self.models.insert(name, model);
        self.model_bytes = self.retained_model_bytes();
        while self.models.len() > 16384 || self.model_bytes > max_bytes {
            if self.models.pop_first().is_none() {
                break;
            }
            self.model_bytes = self.retained_model_bytes();
        }
    }
    fn prepare(
        &mut self,
        spawn: &Spawn,
        context: &Context,
        filter: i32,
        wanted: u32,
    ) -> Result<(
        Arc<Model>,
        Option<Arc<colour_lut::Lut>>,
        Vec<u32>,
        Option<Observation>,
    )> {
        let name =
            path(std::str::from_utf8(&spawn.model).map_err(|_| "invalid flora model filename")?)?;
        let model = self.model(&name)?;
        let mut key = vec![filter as u32, context.palette_mode];
        key.extend(context.ground(spawn).map(f32::to_bits));
        for material in &model.materials {
            for &(palette, alternative) in &material.palettes {
                key.extend(
                    context
                        .palette(spawn, palette, alternative)?
                        .map(f32::to_bits),
                );
            }
        }
        if let Some(&colours) = self.colours.get(&(spawn.clone(), key.clone())) {
            if colours.complete
                || colours.colours & wanted != 0
                || wanted != 0 && wanted & !colours.known == 0
            {
                return Ok((model, None, key, Some(colours)));
            }
        }
        if !self.luts.contains_key(&filter) {
            self.luts
                .insert(filter, Arc::new(colour_lut::from_game(&self.root, filter)?));
        }
        let lut = self.luts.get(&filter).unwrap().clone();
        Ok((model, Some(lut), key, None))
    }
    fn retain(&mut self, spawn: &Spawn, key: Vec<u32>, mut colours: Observation) {
        const MAX_BYTES: usize = 16 * 1024 * 1024;
        let bytes = spawn.owned_bytes()
            + key.capacity() * 4
            + std::mem::size_of::<((Spawn, Vec<u32>), Observation)>()
            + 64;
        if bytes > MAX_BYTES {
            return;
        }
        if self.colours.len() >= 16384 || self.colour_bytes + bytes > MAX_BYTES {
            self.colours.clear();
            self.colour_bytes = 0;
        }
        if let Some(old) = self.colours.get(&(spawn.clone(), key.clone())) {
            colours.colours |= old.colours;
            colours.known |= old.known;
            colours.complete |= old.complete;
        }
        if self.colours.insert((spawn.clone(), key), colours).is_none() {
            self.colour_bytes += bytes;
        }
    }
}

impl Model {
    fn observe(
        &self,
        spawn: &Spawn,
        context: &Context,
        lut: &colour_lut::Lut,
        wanted: u32,
    ) -> Result<Observation> {
        let mut preparation = Preparation::default();
        let mut colours = GRASS_COLOUR_SET;
        let mut absent = 0x7ff;
        for (index, material) in self.materials.iter().enumerate() {
            let observed = material.observe_prepared(
                context,
                lut,
                spawn,
                &self.samples[index],
                &self.optional_layers,
                wanted,
                &mut preparation,
            )?;
            colours |= observed.colours & !GRASS_COLOUR_SET;
            absent &= observed.known & !observed.colours;
            if colours & wanted != 0 && index + 1 < self.materials.len() {
                return Ok(Observation {
                    colours,
                    known: colours & 0x7ff,
                    complete: false,
                });
            }
        }
        let known = absent | (colours & 0x7ff);
        Ok(Observation {
            colours,
            known,
            complete: known == 0x7ff,
        })
    }
}

impl Material {
    fn combinations(
        &self,
        optional_layers: &std::collections::BTreeSet<(Vec<u8>, Vec<u8>)>,
    ) -> Result<usize> {
        self.layers
            .iter()
            .enumerate()
            .try_fold(1usize, |count, (index, layer)| {
                count
                    .checked_mul(
                        layer.len()
                            + usize::from(
                                self.optional[index]
                                    || optional_layers.contains(&self.layer_keys[index]),
                            ),
                    )
                    .filter(|count| *count <= 4096)
                    .ok_or("flora texture combination bound exceeded")
            })
    }
    fn collect_optional(&self, sets: &mut [std::collections::BTreeSet<(Vec<u8>, Vec<u8>)>; 2]) {
        self.optional_keys(&mut sets[0]);
        if let Some(secondary) = &self.secondary {
            secondary.optional_keys(&mut sets[1]);
        }
    }
    fn optional_keys(&self, set: &mut std::collections::BTreeSet<(Vec<u8>, Vec<u8>)>) {
        for (&optional, key) in self.optional.iter().zip(&self.layer_keys) {
            if optional {
                set.insert(key.clone());
            }
        }
    }

    fn owned_bytes(&self) -> usize {
        let mut total = 0;
        self.account_bytes(&mut Default::default(), &mut Default::default(), &mut total);
        total
    }
    fn account_bytes(
        &self,
        materials: &mut std::collections::BTreeSet<usize>,
        textures: &mut std::collections::BTreeSet<usize>,
        total: &mut usize,
    ) {
        if !materials.insert(self as *const Self as usize) {
            return;
        }
        *total += 32
            + std::mem::size_of::<Self>()
            + self.optional.capacity()
            + self.layer_keys.capacity() * std::mem::size_of::<(Vec<u8>, Vec<u8>)>()
            + self
                .layer_keys
                .iter()
                .map(|(group, layer)| group.capacity() + layer.capacity())
                .sum::<usize>()
            + self.palettes.capacity() * std::mem::size_of::<(usize, usize)>()
            + self.layers.capacity() * std::mem::size_of::<Vec<OptionTexture>>()
            + self
                .layers
                .iter()
                .map(|v| v.capacity() * std::mem::size_of::<OptionTexture>())
                .sum::<usize>();
        for option in self.layers.iter().flatten() {
            if textures.insert(Arc::as_ptr(&option.texture) as usize) {
                *total += 32
                    + std::mem::size_of::<Texture>()
                    + option.texture.pixels.capacity() * std::mem::size_of::<[f64; 4]>()
                    + option.texture.recolour.capacity()
                        * std::mem::size_of::<appearance::RecolourPixel>();
            }
        }
        if let Some(secondary) = &self.secondary {
            secondary.account_bytes(materials, textures, total);
        }
    }

    fn surface_weight(&self, alpha: f64, sample: &VertexSample) -> f64 {
        let enabled = self.alpha_flags & 1073741825 != 0;
        let mut weight = if enabled {
            (if self.alpha_flags & 1073741828 != 0 {
                alpha
            } else {
                1.
            }) * (if self.alpha_flags & 1073741832 != 0 {
                self.opacity
            } else {
                1.
            })
        } else {
            1.
        };
        if self.alpha_flags & 1073741888 != 0 {
            weight = 1. - weight;
        }
        if self.vertex_flags & 2048 != 0 {
            weight *= sample.colour[3];
        }
        let cutoff = if self.alpha_flags & 1073741840 != 0 {
            0.35
        } else {
            0.0001
        };
        if enabled && weight < cutoff {
            0.
        } else {
            weight.max(0.)
                * sample.weight
                * if self.vertex_flags & 4096 != 0 {
                    sample.colour[3].min(1.)
                } else {
                    1.
                }
        }
    }
    fn prepare_variants(
        &self,
        context: &Context,
        spawn: &Spawn,
        optional_layers: &std::collections::BTreeSet<(Vec<u8>, Vec<u8>)>,
    ) -> Result<PreparedMaterial> {
        if let Some(colour) = self.constant {
            if colour.iter().any(|value| !value.is_finite()) {
                return Err("invalid flora constant colour");
            }
            let linear = std::array::from_fn::<_, 3, _>(|channel| {
                let value = colour[channel];
                let magnitude = value.abs();
                (if magnitude > 1. {
                    magnitude.powf(2.4000000953674316)
                } else {
                    magnitude.powf(2.200000047683716)
                }) * value.signum()
            });
            return Ok(PreparedMaterial {
                constant: Some([linear[0], linear[1], linear[2], colour[3]]),
                layers: Vec::new(),
                optional: Vec::new(),
                combinations: 1,
                srgb: self.srgb,
            });
        }
        let layers: Vec<Vec<&OptionTexture>> = self
            .layers
            .iter()
            .map(|layer| layer.iter().collect())
            .collect();
        let optional: Vec<bool> = self
            .optional
            .iter()
            .zip(&self.layer_keys)
            .map(|(&optional, key)| optional || optional_layers.contains(key))
            .collect();
        if layers.iter().any(Vec::is_empty) {
            return Err("selected flora texture option is missing");
        }
        let combinations =
            layers
                .iter()
                .enumerate()
                .try_fold(1usize, |count, (index, layer)| {
                    count
                        .checked_mul(layer.len() + usize::from(optional[index]))
                        .filter(|count| *count <= 4096)
                        .ok_or("flora texture combination bound exceeded")
                })?;
        let prepared: Vec<Vec<Vec<[f64; 5]>>> = layers
            .iter()
            .map(|layer| {
                layer
                    .iter()
                    .map(|&option| {
                        let alternative = option.alternative;
                        let palette = option.palette;
                        let force = self.procedural && spawn.match_ground_colour;
                        let target_rgba: [f32; 4] = if force {
                            context.ground(spawn)
                        } else if alternative == 6 {
                            [0., 0., 1., 1.]
                        } else if alternative < 6 {
                            context.palette(spawn, palette, alternative)?
                        } else {
                            [0.; 4]
                        };
                        if target_rgba
                            .iter()
                            .any(|value| !value.is_finite() || !(0. ..=1.).contains(value))
                        {
                            return Err("invalid generated flora colour");
                        }
                        let target =
                            [target_rgba[0], target_rgba[1], target_rgba[2]].map(f64::from);
                        let recolour_weight = f64::from(target_rgba[3]);
                        if target
                            .iter()
                            .any(|value| !value.is_finite() || !(0. ..=1.).contains(value))
                        {
                            return Err("invalid generated flora colour");
                        }
                        let target_hsv = appearance::texture_hsv(target);
                        let average_hsv = appearance::texture_hsv(option.average);
                        Ok(option
                            .texture
                            .pixels
                            .iter()
                            .zip(&option.texture.recolour)
                            .map(|(source, prepared)| {
                                let rgb = [source[0], source[1], source[2]];
                                let rgb = if alternative < 7 || force {
                                    if option.multiply {
                                        std::array::from_fn(|channel| {
                                            rgb[channel] * target[channel]
                                        })
                                    } else {
                                        prepared.apply(average_hsv, target_hsv)
                                    }
                                } else {
                                    rgb
                                };
                                let rgb = std::array::from_fn::<_, 3, _>(|channel| {
                                    let raw = source[channel].powf(2.2);
                                    raw * (1. - recolour_weight)
                                        + rgb[channel].powf(2.2) * recolour_weight
                                });
                                [
                                    rgb[0],
                                    rgb[1],
                                    rgb[2],
                                    (source[3] - 0.004).clamp(0., 1.) * 1.0040160417556763,
                                    source[3],
                                ]
                            })
                            .collect())
                    })
                    .collect::<Result<_>>()
            })
            .collect::<Result<_>>()?;
        Ok(PreparedMaterial {
            constant: None,
            layers: prepared,
            optional,
            combinations,
            srgb: self.srgb,
        })
    }
    fn variants(
        &self,
        context: &Context,
        spawn: &Spawn,
        optional_layers: &std::collections::BTreeSet<(Vec<u8>, Vec<u8>)>,
        sampler: usize,
        preparation: &mut TexturePreparation,
        mut visit: impl FnMut(&[[f64; 4]]) -> Result<bool>,
    ) -> Result<()> {
        let key = (self as *const Self as usize, sampler);
        let prepared = preparation.material(key, || {
            self.prepare_variants(context, spawn, optional_layers)
        })?;
        for combination in 0..prepared.combinations {
            let pixels = preparation.recipe(key, &prepared, combination);
            if !visit(&pixels)? || pixels.is_empty() {
                break;
            }
        }
        Ok(())
    }
    fn observe_prepared(
        &self,
        context: &Context,
        lut: &colour_lut::Lut,
        spawn: &Spawn,
        samples: &[VertexSample],
        optional_layers: &[std::collections::BTreeSet<(Vec<u8>, Vec<u8>)>; 2],
        wanted: u32,
        preparation: &mut Preparation,
    ) -> Result<Observation> {
        let primary_count = self.combinations(&optional_layers[0])?;
        let secondary_count = self
            .secondary
            .as_ref()
            .map(|v| v.combinations(&optional_layers[1]))
            .transpose()?
            .unwrap_or(1);
        if primary_count
            .checked_mul(secondary_count)
            .is_none_or(|count| count > 4096)
            || self.secondary.is_some()
                && secondary_count * 1024 * std::mem::size_of::<[f64; 4]>() > 8 * 1024 * 1024
        {
            return Err("flora combined recipe work bound exceeded");
        }
        let mut secondary = Vec::new();
        if let Some(material) = &self.secondary {
            if samples
                .iter()
                .any(|sample| sample.uv[2..].iter().any(|v| !v.is_finite()))
            {
                return Err("invalid flora secondary texture coordinates");
            }
            material.variants(
                context,
                spawn,
                &optional_layers[1],
                1,
                &mut preparation.textures,
                |pixels| {
                    if secondary.len() >= 4096 {
                        return Err("flora secondary variant bound exceeded");
                    }
                    secondary.push(
                        samples
                            .iter()
                            .map(|sample| bilinear(pixels, [sample.uv[2], sample.uv[3]]))
                            .collect::<Vec<_>>(),
                    );
                    Ok(true)
                },
            )?;
        }
        let requested = if wanted == 0 { 0x7ff } else { wanted & 0x7ff };
        let mut common = requested;
        let classified = &mut preparation.classified;
        let mut evaluated = false;
        let mut partial = false;
        self.variants(
            context,
            spawn,
            &optional_layers[0],
            0,
            &mut preparation.textures,
            |primary| {
                let mut classify = |pixels: &[[f64; 4]]| -> Result<bool> {
                    evaluated = true;
                    let total: f64 = pixels
                        .iter()
                        .zip(samples)
                        .map(|(pixel, sample)| self.surface_weight(pixel[3], sample))
                        .sum();
                    let mut remaining = total;
                    let mut weights = [0.; 11];
                    let mut decided = 0u32;
                    let mut predominant = 0u32;
                    for (pixel, sample) in pixels.iter().zip(samples) {
                        let weight = self.surface_weight(pixel[3], sample);
                        if weight <= 0. {
                            continue;
                        }
                        remaining = (remaining - weight).max(0.);
                        let linear = std::array::from_fn(|channel| {
                            pixel[channel]
                                * if self.vertex_flags & 1024 != 0 {
                                    sample.colour[channel]
                                } else {
                                    1.
                                }
                        });
                        let key = linear.map(f64::to_bits);
                        let colour = if let Some(&colour) = classified.get(&key) {
                            colour
                        } else {
                            let colour = appearance::linear_colour(linear, |rgb| lut.sample(rgb))?;
                            if classified.len() >= 65536 {
                                classified.clear();
                            }
                            classified.insert(key, colour);
                            colour
                        };
                        for (index, value) in weights.iter_mut().enumerate() {
                            let bit = 1 << index;
                            if common & bit == 0 || decided & bit != 0 {
                                continue;
                            }
                            if colour & bit != 0 {
                                *value += weight;
                            }
                            if total > 0. && *value > total * 0.5 {
                                predominant |= bit;
                                decided |= bit;
                            } else if *value + remaining + total * 1e-12 < total * 0.5 {
                                decided |= bit;
                            }
                        }
                        if decided & common == common {
                            break;
                        }
                    }
                    for (index, &weight) in weights.iter().enumerate() {
                        if total > 0. && weight > total * 0.5 {
                            predominant |= 1 << index;
                        }
                    }
                    common &= predominant;
                    if wanted != 0 && common == 0 {
                        partial = true;
                        return Ok(false);
                    }
                    Ok(common != 0)
                };
                let primary: Vec<[f64; 4]> = samples
                    .iter()
                    .map(|sample| bilinear(primary, [sample.uv[0], sample.uv[1]]))
                    .collect();
                if secondary.is_empty() {
                    return classify(&primary);
                }
                for overlay in &secondary {
                    let pixels: Vec<[f64; 4]> = primary
                        .iter()
                        .zip(overlay)
                        .map(|(base, overlay)| {
                            std::array::from_fn(|channel| {
                                if channel == 3 {
                                    base[3]
                                } else {
                                    base[channel] * (1. - overlay[3])
                                        + overlay[channel] * overlay[3]
                                }
                            })
                        })
                        .collect();
                    if !classify(&pixels)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            },
        )?;
        if partial {
            Ok(Observation {
                colours: GRASS_COLOUR_SET,
                known: requested,
                complete: requested == 0x7ff,
            })
        } else {
            Ok(Observation {
                colours: GRASS_COLOUR_SET | if evaluated { common } else { 0 },
                known: requested,
                complete: requested == 0x7ff,
            })
        }
    }
}

fn bilinear(pixels: &[[f64; 4]], uv: [f64; 2]) -> [f64; 4] {
    let xy = uv.map(|v| v.rem_euclid(1.) * 32. - 0.5);
    let base = xy.map(|v| v.floor() as i64);
    let blend = std::array::from_fn::<_, 2, _>(|i| xy[i] - base[i] as f64);
    let texel = |x: i64, y: i64| pixels[y.rem_euclid(32) as usize * 32 + x.rem_euclid(32) as usize];
    let a = texel(base[0], base[1]);
    let b = texel(base[0] + 1, base[1]);
    let c = texel(base[0], base[1] + 1);
    let d = texel(base[0] + 1, base[1] + 1);
    std::array::from_fn(|i| {
        (a[i] * (1. - blend[0]) + b[i] * blend[0]) * (1. - blend[1])
            + (c[i] * (1. - blend[0]) + d[i] * blend[0]) * blend[1]
    })
}

fn decode_texture(bytes: &[u8]) -> Result<Texture> {
    let mbin = Mbin(bytes);
    if mbin.bytes(0, 4)? != b"DDS " || mbin.word(4)? != 124 || mbin.word(76)? != 32 {
        return Err("invalid flora diffuse texture");
    }
    let mut width = mbin.word(16)? as usize;
    let mut height = mbin.word(12)? as usize;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || mbin.word(24)? > 1
        || mbin.word(112)? & 0x20fe00 != 0
    {
        return Err("unsupported flora diffuse dimensions");
    }
    let (mut start, size, format) = match mbin.bytes(84, 4)? {
        b"DXT1" => (128, 8, 1),
        b"DXT5" => (128, 16, 3),
        b"DX10"
            if matches!(mbin.word(128)?, 98 | 99)
                && mbin.word(132)? == 3
                && mbin.word(136)? & 4 == 0
                && mbin.word(140)? == 1 =>
        {
            (148, 16, 7)
        }
        _ => return Err("unsupported flora diffuse encoding"),
    };
    let mut mips = mbin.word(28)?.max(1);
    while width > 32 && height > 32 && mips > 1 {
        start += width.div_ceil(4) * height.div_ceil(4) * size;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
        mips -= 1;
    }
    let blocks = mbin.bytes(start, width.div_ceil(4) * height.div_ceil(4) * size)?;
    let mut decoded = vec![[0.; 4]; width * height];
    for (index, block) in blocks.chunks_exact(size).enumerate() {
        let mut rgba = [0u8; 64];
        match format {
            1 => bcdec_rs::bc1(block, &mut rgba, 16),
            3 => bcdec_rs::bc3(block, &mut rgba, 16),
            _ if block[0] != 0 => bcdec_rs::bc7(block, &mut rgba, 16),
            _ => return Err("invalid flora diffuse block"),
        }
        let bx = index % width.div_ceil(4) * 4;
        let by = index / width.div_ceil(4) * 4;
        for (pixel, value) in rgba.as_chunks::<4>().0.iter().enumerate() {
            let x = bx + pixel % 4;
            let y = by + pixel / 4;
            if x < width && y < height {
                decoded[y * width + x] = value.map(|v| f64::from(v) / 255.);
            }
        }
    }
    let pixels: Vec<[f64; 4]> = (0..1024)
        .map(|index| {
            decoded[(index / 32 * height / 32).min(height - 1) * width
                + (index % 32 * width / 32).min(width - 1)]
        })
        .collect();
    let average = [bytes[0x3a], bytes[0x39], bytes[0x38]].map(|value| f64::from(value) / 255.);
    let recolour = pixels
        .iter()
        .map(|pixel| appearance::RecolourPixel::new([pixel[0], pixel[1], pixel[2]]))
        .collect();
    Ok(Texture {
        pixels,
        average,
        recolour,
        average_valid: mbin.word(0x38)? != 0,
    })
}

pub fn colours(
    models: &[Spawn],
    context: &Context,
    filter: i32,
    wanted: u32,
) -> Result<Observation> {
    static CACHE: OnceLock<Mutex<Assets>> = OnceLock::new();
    let cache = if let Some(cache) = CACHE.get() {
        cache
    } else {
        let exe = std::env::current_exe().map_err(|_| "cannot locate NMS executable")?;
        let root = exe
            .parent()
            .and_then(Path::parent)
            .ok_or("cannot locate NMS game data")?;
        CACHE.get_or_init(|| Mutex::new(Assets::new(root)))
    };
    let mut colours = GRASS_COLOUR_SET;
    let mut absent = 0x7ff;
    for spawn in models {
        let (model, lut, key, cached) = cache
            .lock()
            .map_err(|_| "flora appearance cache poisoned")?
            .prepare(spawn, context, filter, wanted)?;
        let observed = if let Some(cached) = cached {
            cached
        } else {
            let observed = model.observe(spawn, context, &lut.unwrap(), wanted)?;
            cache
                .lock()
                .map_err(|_| "flora appearance cache poisoned")?
                .retain(spawn, key, observed);
            observed
        };
        colours |= observed.colours & !GRASS_COLOUR_SET;
        absent &= observed.known & !observed.colours;
        if colours & wanted != 0 {
            return Ok(Observation {
                colours,
                known: colours & 0x7ff,
                complete: false,
            });
        }
    }
    let known = absent | colours & 0x7ff;
    Ok(Observation {
        colours,
        known,
        complete: known == 0x7ff,
    })
}

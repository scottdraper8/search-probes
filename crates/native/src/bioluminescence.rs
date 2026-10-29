use crate::game_assets::Archive;
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ModelFacts {
    pub emissive: bool,
    pub glowing_flora: bool,
}

pub(crate) struct Mbin<'a>(pub(crate) &'a [u8]);

impl<'a> Mbin<'a> {
    pub(crate) fn bytes(&self, offset: usize, size: usize) -> Result<&'a [u8]> {
        self.0
            .get(offset..offset.checked_add(size).ok_or("asset extent overflow")?)
            .ok_or("truncated vegetation asset")
    }
    pub(crate) fn word(&self, offset: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.bytes(offset, 4)?.try_into().unwrap(),
        ))
    }
    pub(crate) fn pointer(&self, offset: usize) -> Result<usize> {
        let relative = i64::from_le_bytes(self.bytes(offset, 8)?.try_into().unwrap());
        offset
            .checked_add_signed(isize::try_from(relative).map_err(|_| "invalid asset pointer")?)
            .filter(|&value| value <= self.0.len())
            .ok_or("asset pointer outside file")
    }
    pub(crate) fn array(
        &self,
        offset: usize,
        stride: usize,
        bound: usize,
    ) -> Result<std::ops::Range<usize>> {
        let count = self.word(offset + 8)? as usize;
        if count > bound {
            return Err("vegetation asset array exceeds bound");
        }
        if count == 0 {
            return Ok(0..0);
        }
        let start = self.pointer(offset)?;
        let size = count.checked_mul(stride).ok_or("asset array overflow")?;
        self.bytes(start, size)?;
        Ok(start..start + size)
    }
    pub(crate) fn string(&self, offset: usize) -> Result<&'a str> {
        let range = self.array(offset, 1, 4096)?;
        let bytes = &self.0[range];
        std::str::from_utf8(bytes.strip_suffix(&[0]).unwrap_or(bytes))
            .map_err(|_| "invalid vegetation asset string")
    }
    pub(crate) fn header(&self, hash: u32) -> Result<()> {
        if self.bytes(0, 8)? != [0xcc; 8] || self.word(12)? != hash {
            return Err("unexpected vegetation asset template");
        }
        Ok(())
    }
}

pub struct Assets {
    root: PathBuf,
    archives: BTreeMap<String, Archive<File>>,
    models: BTreeMap<String, ModelFacts>,
    materials: BTreeMap<String, bool>,
    entities: BTreeMap<String, bool>,
}

fn path(value: &str) -> Result<String> {
    let value = value.replace('\\', "/").to_ascii_lowercase();
    if value.len() > 4096 || value.split('/').any(|part| part == "..") || value.starts_with('/') {
        return Err("invalid vegetation asset path");
    }
    Ok(value)
}

impl Assets {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.into(),
            archives: BTreeMap::new(),
            models: BTreeMap::new(),
            materials: BTreeMap::new(),
            entities: BTreeMap::new(),
        }
    }
    pub(crate) fn read(&mut self, archive: &str, name: &str, bound: usize) -> Result<Vec<u8>> {
        if !self.archives.contains_key(archive) {
            let file = std::fs::read_dir(self.root.join("GAMEDATA/PCBANKS"))
                .map_err(|_| "cannot find vegetation archives")?
                .filter_map(|entry| entry.ok())
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(archive)
                })
                .ok_or("missing vegetation archive")?;
            self.archives.insert(
                archive.into(),
                Archive::open(
                    File::open(file.path()).map_err(|_| "cannot open vegetation archive")?,
                )?,
            );
        }
        self.archives
            .get_mut(archive)
            .unwrap()
            .asset(name, bound)?
            .ok_or("missing vegetation asset")
    }
    pub(crate) fn flora(&mut self, name: &str) -> Result<bool> {
        let name = path(name)?;
        if let Some(&value) = self.entities.get(&name) {
            return Ok(value);
        }
        let bytes = match self.read("NMSARC.Precache.pak", &name, 4 * 1024 * 1024) {
            Ok(bytes) => bytes,
            Err("missing vegetation asset") => {
                self.entities.insert(name, false);
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        let mbin = Mbin(&bytes);
        mbin.header(0x6874ff60)?;
        let mut flora = false;
        for offset in mbin.array(0x30, 0x20, 4096)?.step_by(0x20) {
            if mbin.word(offset + 8)? == 0xb395d494 {
                flora |= mbin.word(mbin.pointer(offset)?)? == 4;
            }
            if mbin.word(offset + 8)? == 0xb395d494 && !mbin.string(offset + 0x10)?.is_empty() {
                return Err("linked flora discovery component is unsupported");
            }
        }
        self.entities.insert(name, flora);
        Ok(flora)
    }
    fn material(&mut self, name: &str) -> Result<bool> {
        let name = path(name)?;
        if let Some(&value) = self.materials.get(&name) {
            return Ok(value);
        }
        let bytes = self.read("NMSARC.MetadataEtc.pak", &name, 4 * 1024 * 1024)?;
        let mbin = Mbin(&bytes);
        mbin.header(0x4737d48a)?;
        let mut flags = 0;
        for offset in mbin.array(0xa0, 0x30, 256)?.step_by(0x30) {
            if mbin.string(offset + 0x20)? == "gDynamicFlags" {
                flags = mbin.word(offset)?;
            }
        }
        let mut strength = 0.;
        for offset in mbin.array(0x90, 0x30, 256)?.step_by(0x30) {
            if mbin.string(offset + 0x20)? == "gMaterialParams2Vec4" {
                strength = f32::from_bits(mbin.word(offset)?);
                if !strength.is_finite() || strength < 0. {
                    return Err("invalid vegetation emission");
                }
            }
        }
        let mut emissive = flags & 16384 != 0 && strength > 0.;
        if emissive && flags & 8 != 0 {
            let mut mask = None;
            for offset in mbin.array(0x70, 0x50, 256)?.step_by(0x50) {
                if mbin.string(offset + 0x30)? == "gMasksMap" {
                    mask = Some(path(mbin.string(offset + 0x20)?)?);
                }
            }
            let mask = mask.ok_or("missing vegetation emission mask")?;
            let biome = mask
                .strip_prefix("textures/planets/biomes/")
                .and_then(|suffix| suffix.split('/').next())
                .ok_or("unsupported vegetation mask path")?;
            let archive = format!("NMSARC.TexBiomes{}.pak", biome.to_ascii_uppercase());
            emissive = emission_mask(&self.read(&archive, &mask, 32 * 1024 * 1024)?)?;
        }
        self.materials.insert(name, emissive);
        Ok(emissive)
    }
    pub(crate) fn flora_materials(&mut self, name: &str, depth: usize) -> Result<Vec<String>> {
        if depth > 32 {
            return Err("flora scene reference depth exceeded");
        }
        let bytes = self.read("NMSARC.EntitySceneMBIN.pak", &path(name)?, 16 * 1024 * 1024)?;
        let mbin = Mbin(&bytes);
        mbin.header(0x3db87e47)?;
        let mut materials = Vec::new();
        let mut entities = Vec::new();
        let mut scenes = Vec::new();
        let mut pending = vec![(0x20, 0)];
        let mut visited = 0;
        while let Some((node, level)) = pending.pop() {
            visited += 1;
            if visited > 65536 || level > 128 {
                return Err("flora scene node bound exceeded");
            }
            for offset in mbin.array(node, 0x20, 4096)?.step_by(0x20) {
                let key = mbin.bytes(offset, 16)?;
                let key = &key[..key.iter().position(|&byte| byte == 0).unwrap_or(16)];
                let value = mbin.string(offset + 0x10)?;
                if value.is_empty() {
                    continue;
                }
                match key {
                    b"MATERIAL" if mbin.bytes(node + 0x40, 16)?.starts_with(b"MESH\0") => {
                        materials.push(path(value)?)
                    }
                    b"ATTACHMENT" => entities.push(path(value)?),
                    b"SCENEGRAPH" => scenes.push(path(value)?),
                    _ => {}
                }
            }
            pending.extend(
                mbin.array(node + 0x10, 0x80, 65536)?
                    .step_by(0x80)
                    .map(|child| (child, level + 1)),
            );
        }
        let mut flora = false;
        for entity in entities {
            flora |= self.flora(&entity)?;
        }
        if !flora {
            materials.clear();
        }
        for scene in scenes {
            materials.extend(self.flora_materials(&scene, depth + 1)?);
        }
        materials.sort_unstable();
        materials.dedup();
        Ok(materials)
    }
    pub fn model(&mut self, name: &str) -> Result<ModelFacts> {
        self.model_at(name, 0)
    }
    fn model_at(&mut self, name: &str, depth: usize) -> Result<ModelFacts> {
        if depth > 32 {
            return Err("vegetation scene reference depth exceeded");
        }
        let name = path(name)?;
        if let Some(&facts) = self.models.get(&name) {
            return Ok(facts);
        }
        if self.models.len() >= 16384 {
            return Err("vegetation model cache exceeds bound");
        }
        let bytes = self.read("NMSARC.EntitySceneMBIN.pak", &name, 16 * 1024 * 1024)?;
        let mbin = Mbin(&bytes);
        mbin.header(0x3db87e47)?;
        let mut materials = Vec::new();
        let mut entities = Vec::new();
        let mut scenes = Vec::new();
        let mut pending = vec![(0x20, 0)];
        let mut visited = 0;
        while let Some((node, level)) = pending.pop() {
            visited += 1;
            if visited > 65536 || level > 128 {
                return Err("vegetation scene node bound exceeded");
            }
            for offset in mbin.array(node, 0x20, 4096)?.step_by(0x20) {
                let key = mbin.bytes(offset, 16)?;
                let key = &key[..key.iter().position(|&b| b == 0).unwrap_or(16)];
                let value = mbin.string(offset + 0x10)?;
                if value.is_empty() {
                    continue;
                }
                match key {
                    b"MATERIAL" => materials.push(value),
                    b"ATTACHMENT" => entities.push(value),
                    b"SCENEGRAPH" => scenes.push(value),
                    _ => {}
                }
            }
            pending.extend(
                mbin.array(node + 0x10, 0x80, 65536)?
                    .step_by(0x80)
                    .map(|child| (child, level + 1)),
            );
        }
        materials.sort_unstable();
        materials.dedup();
        entities.sort_unstable();
        entities.dedup();
        scenes.sort_unstable();
        scenes.dedup();
        let mut flora = false;
        for entity in entities {
            flora |= self.flora(entity)?;
        }
        let mut facts = ModelFacts::default();
        let grass = search_probes_model::grass::GrassModel::ALL
            .iter()
            .any(|model| model.path().eq_ignore_ascii_case(&name));
        if flora || grass {
            for material in materials {
                facts.emissive |= self.material(material)?;
            }
        }
        for scene in scenes {
            let child = self.model_at(scene, depth + 1)?;
            facts.emissive |= child.emissive;
            facts.glowing_flora |= child.glowing_flora;
        }
        facts.glowing_flora |= flora && facts.emissive;
        self.models.insert(name, facts);
        Ok(facts)
    }
}

fn emission_mask(bytes: &[u8]) -> Result<bool> {
    let mbin = Mbin(bytes);
    if mbin.bytes(0, 4)? != b"DDS " || mbin.word(4)? != 124 || mbin.word(76)? != 32 {
        return Err("invalid vegetation mask texture");
    }
    let (width, height) = (mbin.word(16)? as usize, mbin.word(12)? as usize);
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || width % 4 != 0
        || height % 4 != 0
        || mbin.word(24)? > 1
        || mbin.word(112)? & 0x20fe00 != 0
    {
        return Err("invalid vegetation mask dimensions");
    }
    let (start, size, format) = match mbin.bytes(84, 4)? {
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
        _ => return Err("unsupported vegetation mask encoding"),
    };
    let blocks = mbin.bytes(start, width * height / 16 * size)?;
    for block in blocks.chunks_exact(size) {
        let mut rgba = [0; 64];
        match format {
            1 => bcdec_rs::bc1(block, &mut rgba, 16),
            3 => bcdec_rs::bc3(block, &mut rgba, 16),
            _ if block[0] != 0 => bcdec_rs::bc7(block, &mut rgba, 16),
            _ => return Err("invalid vegetation mask block"),
        }
        if rgba.chunks_exact(4).any(|pixel| pixel[2] > 0) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn get(name: &[u8]) -> Result<ModelFacts> {
    static CACHE: OnceLock<Mutex<Assets>> = OnceLock::new();
    let name = std::str::from_utf8(name).map_err(|_| "invalid vegetation model filename")?;
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
    cache
        .lock()
        .map_err(|_| "vegetation asset cache poisoned")?
        .model(name)
}

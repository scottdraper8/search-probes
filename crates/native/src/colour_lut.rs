use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

type Result<T> = std::result::Result<T, &'static str>;
const CHUNK: usize = 65536;

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

struct Archive<R> {
    reader: R,
    data: u64,
    entries: Vec<(u64, usize)>,
    chunks: Vec<(u64, usize)>,
    names: Vec<String>,
}

impl<R: Read + Seek> Archive<R> {
    fn open(mut reader: R) -> Result<Self> {
        let length = reader
            .seek(SeekFrom::End(0))
            .map_err(|_| "cannot size game archive")?;
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| "cannot seek game archive")?;
        let mut header = [0; 48];
        reader
            .read_exact(&mut header)
            .map_err(|_| "cannot read game archive header")?;
        let u64_at = |offset| u64::from_le_bytes(header[offset..offset + 8].try_into().unwrap());
        let files = u64_at(16);
        let count = u64_at(24);
        let data = u64_at(40);
        if &header[..5] != b"HGPAK"
            || u64_at(8) != 2
            || header[32] > 1
            || !(1..=200_000).contains(&files)
            || count > 1_000_000
            || data > length
            || data < 48 + files * 32 + if header[32] == 1 { count * 8 } else { 0 }
        {
            return Err("unsupported or invalid game archive");
        }
        let mut entries = Vec::with_capacity(files as usize);
        for _ in 0..files {
            let mut bytes = [0; 32];
            reader
                .read_exact(&mut bytes)
                .map_err(|_| "truncated game archive index")?;
            let offset = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
            let size = u64::from_le_bytes(bytes[24..].try_into().unwrap());
            entries.push((
                offset,
                usize::try_from(size).map_err(|_| "game asset exceeds address bounds")?,
            ));
        }
        let mut chunks = Vec::new();
        if header[32] == 1 {
            let mut offset = data;
            for _ in 0..count {
                let mut bytes = [0; 8];
                reader
                    .read_exact(&mut bytes)
                    .map_err(|_| "truncated game chunk index")?;
                let size = u64::from_le_bytes(bytes);
                if size == 0
                    || size > 131072
                    || offset.checked_add(size).is_none_or(|end| end > length)
                {
                    return Err("invalid compressed game chunk");
                }
                chunks.push((offset, size as usize));
                offset += (size + 15) & !15;
            }
            if chunks.is_empty() {
                return Err("compressed game archive has no chunks");
            }
        }
        for &(offset, size) in &entries {
            let end = offset
                .checked_add(size as u64)
                .ok_or("game asset extent overflow")?;
            if offset < data
                || (chunks.is_empty() && end > length)
                || (!chunks.is_empty() && end - data > chunks.len() as u64 * CHUNK as u64)
            {
                return Err("game asset outside archive bounds");
            }
        }
        let mut archive = Self {
            reader,
            data,
            entries,
            chunks,
            names: Vec::new(),
        };
        let manifest = archive.read(0, 8 * 1024 * 1024)?;
        let text = std::str::from_utf8(&manifest).map_err(|_| "invalid game archive manifest")?;
        archive.names = text
            .trim_end_matches(['\r', '\n'])
            .split("\r\n")
            .map(|name| name.replace('\\', "/").to_ascii_lowercase())
            .collect();
        if archive.names.len() + 1 != archive.entries.len() {
            return Err("game archive manifest/index mismatch");
        }
        Ok(archive)
    }

    fn read(&mut self, index: usize, bound: usize) -> Result<Vec<u8>> {
        let &(offset, size) = self.entries.get(index).ok_or("missing game asset index")?;
        if size > bound {
            return Err("game asset exceeds read limit");
        }
        if self.chunks.is_empty() {
            self.reader
                .seek(SeekFrom::Start(offset))
                .map_err(|_| "cannot seek game asset")?;
            let mut out = vec![0; size];
            self.reader
                .read_exact(&mut out)
                .map_err(|_| "truncated game asset")?;
            return Ok(out);
        }
        let start = usize::try_from(offset - self.data).map_err(|_| "invalid game asset offset")?;
        let end = start
            .checked_add(size)
            .ok_or("game asset extent overflow")?;
        let mut out = Vec::with_capacity(size);
        for index in start / CHUNK..end.div_ceil(CHUNK) {
            let &(offset, size) = self.chunks.get(index).ok_or("missing game asset chunk")?;
            self.reader
                .seek(SeekFrom::Start(offset))
                .map_err(|_| "cannot seek game chunk")?;
            let mut compressed = vec![0; size];
            self.reader
                .read_exact(&mut compressed)
                .map_err(|_| "truncated game chunk")?;
            let decoded = match zstd::bulk::decompress(&compressed, CHUNK) {
                Ok(bytes) => bytes,
                Err(_) if size == CHUNK => compressed,
                Err(_) => return Err("cannot decompress game colour data"),
            };
            let lo = start.saturating_sub(index * CHUNK);
            let hi = (end - index * CHUNK).min(CHUNK);
            out.extend_from_slice(decoded.get(lo..hi).ok_or("short decoded game chunk")?);
        }
        Ok(out)
    }

    fn asset(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        let matches: Vec<_> = self
            .names
            .iter()
            .enumerate()
            .filter(|(_, n)| *n == name)
            .map(|(i, _)| i + 1)
            .collect();
        match matches.as_slice() {
            [] => Ok(None),
            [index] => self.read(*index, 256 * 1024).map(Some),
            _ => Err("duplicate game colour asset"),
        }
    }
}

fn mod_files(directory: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    if depth > 32 {
        return Err("mod asset directory exceeds depth bound");
    }
    for entry in std::fs::read_dir(directory).map_err(|_| "cannot inspect mod colour overrides")? {
        let entry = entry.map_err(|_| "cannot inspect mod colour overrides")?;
        let kind = entry
            .file_type()
            .map_err(|_| "cannot inspect mod colour overrides")?;
        if kind.is_dir() {
            mod_files(&entry.path(), depth + 1, out)?;
        } else if kind.is_file() {
            out.push(entry.path());
            if out.len() > 50_000 {
                return Err("mod asset count exceeds bound");
            }
        }
    }
    Ok(())
}

pub fn from_game(root: &Path, filter: i32) -> Result<Lut> {
    let name = filename(filter).ok_or("unsupported generated world colour filter")?;
    let mut mods = Vec::new();
    mod_files(&root.join("GAMEDATA/MODS"), 0, &mut mods)?;
    for path in mods {
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
        {
            let archive = Archive::open(File::open(path).map_err(|_| "cannot open mod archive")?)?;
            if archive.names.iter().any(|asset| asset == name) {
                return Err("grass appearance cannot resolve a modded world colour LUT");
            }
        } else if path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
            .ends_with(name)
        {
            return Err("grass appearance cannot resolve a modded world colour LUT");
        }
    }
    let directory = root.join("GAMEDATA/PCBANKS");
    let path = std::fs::read_dir(directory)
        .map_err(|_| "cannot find game colour archive")?
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("NMSARC.TexMisc.pak")
        })
        .ok_or("missing game colour archive")?
        .path();
    let mut archive =
        Archive::open(File::open(path).map_err(|_| "cannot open game colour archive")?)?;
    Lut::decode(&archive.asset(name)?.ok_or("missing game colour LUT")?)
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
    use std::io::Cursor;

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

    fn pak(asset: &[u8], compressed: bool) -> Vec<u8> {
        let manifest = b"textures/lut/filters/default.dds\r\n";
        let asset_offset = (manifest.len() + 15) & !15;
        let mut plain = manifest.to_vec();
        plain.resize(asset_offset, 0);
        plain.extend_from_slice(asset);
        let blocks: Vec<_> = plain
            .chunks(CHUNK)
            .enumerate()
            .map(|(i, chunk)| {
                if i == 1 && chunk.len() == CHUNK {
                    chunk.to_vec()
                } else {
                    zstd::bulk::compress(chunk, 1).unwrap()
                }
            })
            .collect();
        let data = 48 + 64 + if compressed { blocks.len() * 8 } else { 0 };
        let mut out = vec![0; data];
        out[..5].copy_from_slice(b"HGPAK");
        for (offset, value) in [
            (8, 2u64),
            (16, 2),
            (24, blocks.len() as u64),
            (40, data as u64),
            (64, data as u64),
            (72, manifest.len() as u64),
            (96, (data + asset_offset) as u64),
            (104, asset.len() as u64),
        ] {
            out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        out[32] = u8::from(compressed);
        if compressed {
            for (i, block) in blocks.iter().enumerate() {
                out[112 + i * 8..120 + i * 8].copy_from_slice(&(block.len() as u64).to_le_bytes());
                out.extend_from_slice(block);
                out.resize(data + ((out.len() - data + 15) & !15), 0);
            }
        } else {
            out.extend_from_slice(&plain);
        }
        out
    }

    #[test]
    fn archive_reads_cross_chunk_compressed_and_raw_assets_and_rejects_truncation() {
        let asset: Vec<u8> = (0..140_000).map(|i| (i % 251) as u8).collect();
        for compressed in [false, true] {
            let bytes = pak(&asset, compressed);
            let mut archive = Archive::open(Cursor::new(&bytes)).unwrap();
            assert_eq!(
                archive
                    .asset("textures/lut/filters/default.dds")
                    .unwrap()
                    .unwrap(),
                asset
            );
            assert!(
                archive
                    .asset("textures/lut/filters/missing.dds")
                    .unwrap()
                    .is_none()
            );
            assert!(archive.read(1, 100).is_err());
            assert!(Archive::open(Cursor::new(&bytes[..bytes.len() / 2])).is_err());
        }
        let mut invalid = pak(&asset, true);
        invalid[8] = 3;
        assert!(Archive::open(Cursor::new(invalid)).is_err());
    }
}

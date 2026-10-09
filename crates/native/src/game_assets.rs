use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) const CHUNK: usize = 65536;

pub(crate) struct Archive<R> {
    reader: R,
    data: u64,
    entries: Vec<(u64, usize)>,
    chunks: Vec<(u64, usize)>,
    names: Vec<String>,
}

impl<R: Read + Seek> Archive<R> {
    pub(crate) fn open(mut reader: R) -> Result<Self> {
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
                Err(_) => return Err("cannot decompress game asset data"),
            };
            let lo = start.saturating_sub(index * CHUNK);
            let hi = (end - index * CHUNK).min(CHUNK);
            out.extend_from_slice(decoded.get(lo..hi).ok_or("short decoded game chunk")?);
        }
        Ok(out)
    }

    pub(crate) fn asset(&mut self, name: &str, bound: usize) -> Result<Option<Vec<u8>>> {
        let matches: Vec<_> = self
            .names
            .iter()
            .enumerate()
            .filter(|(_, n)| *n == name)
            .map(|(i, _)| i + 1)
            .collect();
        match matches.as_slice() {
            [] => Ok(None),
            [index] => self.read(*index, bound).map(Some),
            _ => Err("duplicate game asset"),
        }
    }
}

pub(crate) fn read(root: &Path, archive_name: &str, name: &str, bound: usize) -> Result<Vec<u8>> {
    let directory = root.join("GAMEDATA/PCBANKS");
    let path = std::fs::read_dir(directory)
        .map_err(|_| "cannot find game asset archive")?
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(archive_name)
        })
        .ok_or("missing game asset archive")?
        .path();
    let mut archive =
        Archive::open(File::open(path).map_err(|_| "cannot open game asset archive")?)?;
    archive.asset(name, bound)?.ok_or("missing game asset")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
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
                    .asset("textures/lut/filters/default.dds", 256 * 1024)
                    .unwrap()
                    .unwrap(),
                asset
            );
            assert!(
                archive
                    .asset("textures/lut/filters/missing.dds", 256 * 1024)
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

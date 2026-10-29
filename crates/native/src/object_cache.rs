use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_TABLES: usize = 1024;
pub const MAX_OWNED_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_FILENAME_BYTES: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableFacts {
    pub floating_islands: bool,
    pub titan_worms: bool,
    pub objects: u32,
    pub ground_grass: u32,
    pub glowing_grass: bool,
    pub glowing_flora: bool,
    pub glow_known: u8,
    pub flora_models: Arc<[crate::flora_appearance::Spawn]>,
}

impl TableFacts {
    pub fn owned_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of_val(self.flora_models.as_ref())
            + self
                .flora_models
                .iter()
                .map(crate::flora_appearance::Spawn::owned_bytes)
                .sum::<usize>()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Statistics {
    pub hits: u64,
    pub loads: u64,
    pub retained: usize,
}

#[derive(Default)]
struct Cache {
    entries: BTreeMap<Vec<u8>, TableFacts>,
    order: VecDeque<Vec<u8>>,
    statistics: Statistics,
    owned_bytes: usize,
}

#[derive(Default)]
pub struct ObjectTableCache {
    cache: RwLock<Cache>,
    hits: AtomicU64,
}

impl ObjectTableCache {
    pub fn resolve(
        &self,
        filename: &[u8],
        need_glow: u8,
        load: impl FnOnce() -> Result<Option<TableFacts>, &'static str>,
    ) -> Result<Option<TableFacts>, &'static str> {
        if filename.is_empty() || filename.len() > MAX_FILENAME_BYTES || filename.contains(&0) {
            return Err("invalid object-table filename");
        }
        {
            let cache = self
                .cache
                .read()
                .map_err(|_| "object-table cache poisoned")?;
            if let Some(facts) = cache
                .entries
                .get(filename)
                .cloned()
                .filter(|facts| facts.glow_known & need_glow == need_glow)
            {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(Some(facts));
            }
        }
        let mut cache = self
            .cache
            .write()
            .map_err(|_| "object-table cache poisoned")?;
        if let Some(facts) = cache
            .entries
            .get(filename)
            .cloned()
            .filter(|facts| facts.glow_known & need_glow == need_glow)
        {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(Some(facts));
        }
        cache.statistics.loads = cache.statistics.loads.saturating_add(1);
        let Some(mut facts) = load()? else {
            return Ok(None);
        };
        if let Some(old) = cache.entries.get(filename) {
            if old.glow_known & 1 != 0 && facts.glow_known & 1 == 0 {
                facts.glowing_grass = old.glowing_grass;
            }
            if old.glow_known & 2 != 0 && facts.glow_known & 2 == 0 {
                facts.glowing_flora = old.glowing_flora;
            }
            if old.glow_known & 4 != 0 && facts.glow_known & 4 == 0 {
                facts.flora_models = old.flora_models.clone();
            }
            facts.glow_known |= old.glow_known;
        }
        let bytes = facts.owned_bytes() + filename.len() * 2;
        if bytes > MAX_OWNED_BYTES {
            return Err("object-table summary exceeds byte bound");
        }
        if let Some(old) = cache.entries.remove(filename) {
            cache.owned_bytes -= old.owned_bytes() + filename.len() * 2;
            cache.order.retain(|key| key.as_slice() != filename);
        }
        while cache.entries.len() >= MAX_TABLES || cache.owned_bytes + bytes > MAX_OWNED_BYTES {
            let oldest = cache
                .order
                .pop_front()
                .ok_or("object-table cache order is invalid")?;
            let old = cache
                .entries
                .remove(&oldest)
                .ok_or("object-table cache entry is invalid")?;
            cache.owned_bytes -= old.owned_bytes() + oldest.len() * 2;
        }
        cache.entries.insert(filename.to_vec(), facts.clone());
        cache.order.push_back(filename.to_vec());
        cache.owned_bytes += bytes;
        cache.statistics.retained = cache.entries.len();
        Ok(Some(facts))
    }

    pub fn statistics(&self) -> Statistics {
        let mut statistics = self
            .cache
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .statistics;
        statistics.hits = self.hits.load(Ordering::Relaxed);
        statistics
    }

    pub fn reset_statistics(&mut self) {
        self.hits.store(0, Ordering::Relaxed);
        let cache = self
            .cache
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        cache.statistics = Statistics {
            retained: cache.entries.len(),
            ..Statistics::default()
        };
    }

    pub fn clear(&mut self) {
        self.hits.store(0, Ordering::Relaxed);
        *self
            .cache
            .get_mut()
            .unwrap_or_else(|error| error.into_inner()) = Cache::default();
    }
}

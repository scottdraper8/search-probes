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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn byte_budget_evicts_owned_recipes_during_upgrades_and_rejects_oversize_tables() {
        let cache = ObjectTableCache::default();
        let facts = |bytes| TableFacts {
            glow_known: 4,
            flora_models: vec![crate::flora_appearance::Spawn {
                model: vec![b'a'; bytes],
                ..Default::default()
            }]
            .into(),
            ..Default::default()
        };
        cache
            .resolve(b"first", 4, || Ok(Some(facts(MAX_OWNED_BYTES / 2))))
            .unwrap();
        cache
            .resolve(b"second", 4, || Ok(Some(facts(MAX_OWNED_BYTES / 2))))
            .unwrap();
        assert_eq!(cache.statistics().retained, 1);
        assert!(cache.cache.read().unwrap().owned_bytes <= MAX_OWNED_BYTES);
        let mut partial = facts(0);
        partial.glow_known = 0;
        cache.resolve(b"upgrade", 0, || Ok(Some(partial))).unwrap();
        cache
            .resolve(b"upgrade", 4, || Ok(Some(facts(MAX_OWNED_BYTES / 2))))
            .unwrap();
        assert_eq!(cache.statistics().retained, 1);
        let bytes = cache.cache.read().unwrap().owned_bytes;
        assert!(
            cache
                .resolve(b"oversize", 4, || Ok(Some(facts(MAX_OWNED_BYTES))))
                .is_err()
        );
        assert_eq!(cache.cache.read().unwrap().owned_bytes, bytes);
    }

    #[test]
    fn glow_requests_upgrade_partial_tables_without_duplicate_retention() {
        let cache = ObjectTableCache::default();
        cache
            .resolve(b"LUSH", 0, || Ok(Some(TableFacts::default())))
            .unwrap();
        assert_eq!(
            cache
                .resolve(b"LUSH", 0, || unreachable!())
                .unwrap()
                .unwrap()
                .glow_known,
            0
        );
        let upgraded = TableFacts {
            glowing_grass: true,
            glow_known: 1,
            ..Default::default()
        };
        assert_eq!(
            cache
                .resolve(b"LUSH", 1, || Ok(Some(upgraded.clone())))
                .unwrap(),
            Some(upgraded.clone())
        );
        assert_eq!(
            cache.resolve(b"LUSH", 1, || unreachable!()).unwrap(),
            Some(upgraded.clone())
        );
        assert_eq!(cache.statistics().retained, 1);
        assert_eq!(cache.statistics().loads, 2);
        let flora = cache
            .resolve(b"LUSH", 2, || {
                Ok(Some(TableFacts {
                    glowing_flora: true,
                    glow_known: 2,
                    ..Default::default()
                }))
            })
            .unwrap()
            .unwrap();
        assert!(flora.glowing_grass && flora.glowing_flora);
        assert_eq!(flora.glow_known, 3);
        assert_eq!(
            cache.resolve(b"LUSH", 3, || unreachable!()).unwrap(),
            Some(flora.clone())
        );
    }

    #[test]
    fn repeated_concurrent_reads_load_once_and_keep_only_owned_facts() {
        let cache = ObjectTableCache::default();
        let loads = AtomicUsize::new(0);
        let start = Barrier::new(16);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    start.wait();
                    for _ in 0..100 {
                        assert_eq!(
                            cache
                                .resolve(b"TABLE.MBIN", 0, || {
                                    loads.fetch_add(1, Ordering::Relaxed);
                                    Ok(Some(TableFacts {
                                        floating_islands: true,
                                        objects: 42,
                                        ground_grass: 1,
                                        ..Default::default()
                                    }))
                                })
                                .unwrap()
                                .unwrap()
                                .objects,
                            42
                        );
                    }
                });
            }
        });
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        assert_eq!(cache.statistics().hits, 1599);
    }

    #[test]
    fn failures_are_not_cached_and_keys_and_retention_are_bounded() {
        let mut cache = ObjectTableCache::default();
        assert!(cache.resolve(&[], 0, || unreachable!()).is_err());
        assert!(
            cache
                .resolve(&[b'x'; MAX_FILENAME_BYTES + 1], 0, || unreachable!())
                .is_err()
        );
        assert_eq!(cache.resolve(b"MISSING", 0, || Ok(None)).unwrap(), None);
        assert!(cache.resolve(b"MISSING", 0, || Err("unavailable")).is_err());
        assert_eq!(cache.statistics().retained, 0);
        for i in 0..MAX_TABLES * 3 {
            cache
                .resolve(i.to_string().as_bytes(), 0, || {
                    Ok(Some(TableFacts::default()))
                })
                .unwrap();
        }
        assert_eq!(cache.statistics().retained, MAX_TABLES);
        cache.reset_statistics();
        assert_eq!(cache.statistics().loads, 0);
        cache
            .resolve(b"0", 0, || Ok(Some(TableFacts::default())))
            .unwrap();
        assert_eq!(cache.statistics().loads, 1);
        cache.clear();
        assert_eq!(cache.statistics().retained, 0);
        cache
            .resolve(b"0", 0, || Ok(Some(TableFacts::default())))
            .unwrap();
        assert_eq!(cache.statistics().loads, 1);
    }
}

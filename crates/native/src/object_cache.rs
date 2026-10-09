use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_TABLES: usize = 1024;
pub const MAX_FILENAME_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableFacts {
    pub floating_islands: bool,
    pub objects: u32,
    pub ground_grass: u32,
    pub glowing_grass: bool,
    pub glowing_flora: bool,
    pub glow_known: u8,
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
                .copied()
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
            .copied()
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
            facts.glow_known |= old.glow_known;
        }
        let retained = cache.entries.contains_key(filename);
        if !retained && cache.entries.len() == MAX_TABLES {
            let oldest = cache
                .order
                .pop_front()
                .ok_or("object-table cache order is invalid")?;
            cache.entries.remove(&oldest);
        }
        cache.entries.insert(filename.to_vec(), facts);
        if !retained {
            cache.order.push_back(filename.to_vec());
        }
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
            cache.resolve(b"LUSH", 1, || Ok(Some(upgraded))).unwrap(),
            Some(upgraded)
        );
        assert_eq!(
            cache.resolve(b"LUSH", 1, || unreachable!()).unwrap(),
            Some(upgraded)
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
            Some(flora)
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

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};

const MAX_TABLES: usize = 1024;
pub const MAX_FILENAME_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableFacts {
    pub floating_islands: bool,
    pub objects: u32,
    pub ground_grass: bool,
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
pub struct ObjectTableCache(Mutex<Cache>);

impl ObjectTableCache {
    pub fn resolve(
        &self,
        filename: &[u8],
        load: impl FnOnce() -> Result<Option<TableFacts>, &'static str>,
    ) -> Result<Option<TableFacts>, &'static str> {
        if filename.is_empty() || filename.len() > MAX_FILENAME_BYTES || filename.contains(&0) {
            return Err("invalid object-table filename");
        }
        let mut cache = self.0.lock().map_err(|_| "object-table cache poisoned")?;
        if let Some(facts) = cache.entries.get(filename).copied() {
            cache.statistics.hits = cache.statistics.hits.saturating_add(1);
            return Ok(Some(facts));
        }
        cache.statistics.loads = cache.statistics.loads.saturating_add(1);
        let Some(facts) = load()? else {
            return Ok(None);
        };
        if cache.entries.len() == MAX_TABLES {
            let oldest = cache
                .order
                .pop_front()
                .ok_or("object-table cache order is invalid")?;
            cache.entries.remove(&oldest);
        }
        cache.entries.insert(filename.to_vec(), facts);
        cache.order.push_back(filename.to_vec());
        cache.statistics.retained = cache.entries.len();
        Ok(Some(facts))
    }

    pub fn statistics(&self) -> Statistics {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .statistics
    }

    pub fn reset_statistics(&mut self) {
        let cache = self.0.get_mut().unwrap_or_else(|error| error.into_inner());
        cache.statistics = Statistics {
            retained: cache.entries.len(),
            ..Statistics::default()
        };
    }

    pub fn clear(&mut self) {
        *self.0.get_mut().unwrap_or_else(|error| error.into_inner()) = Cache::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn repeated_concurrent_reads_load_once_and_keep_only_owned_facts() {
        let cache = ObjectTableCache::default();
        let loads = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    assert_eq!(
                        cache
                            .resolve(b"TABLE.MBIN", || {
                                loads.fetch_add(1, Ordering::Relaxed);
                                Ok(Some(TableFacts {
                                    floating_islands: true,
                                    objects: 42,
                                    ground_grass: true,
                                }))
                            })
                            .unwrap()
                            .unwrap()
                            .objects,
                        42
                    );
                });
            }
        });
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        assert_eq!(cache.statistics().hits, 15);
    }

    #[test]
    fn failures_are_not_cached_and_keys_and_retention_are_bounded() {
        let mut cache = ObjectTableCache::default();
        assert!(cache.resolve(&[], || unreachable!()).is_err());
        assert!(
            cache
                .resolve(&[b'x'; MAX_FILENAME_BYTES + 1], || unreachable!())
                .is_err()
        );
        assert_eq!(cache.resolve(b"MISSING", || Ok(None)).unwrap(), None);
        assert!(cache.resolve(b"MISSING", || Err("unavailable")).is_err());
        assert_eq!(cache.statistics().retained, 0);
        for i in 0..MAX_TABLES * 3 {
            cache
                .resolve(i.to_string().as_bytes(), || Ok(Some(TableFacts::default())))
                .unwrap();
        }
        assert_eq!(cache.statistics().retained, MAX_TABLES);
        cache.reset_statistics();
        assert_eq!(cache.statistics().loads, 0);
        cache
            .resolve(b"0", || Ok(Some(TableFacts::default())))
            .unwrap();
        assert_eq!(cache.statistics().loads, 1);
        cache.clear();
        assert_eq!(cache.statistics().retained, 0);
        cache
            .resolve(b"0", || Ok(Some(TableFacts::default())))
            .unwrap();
        assert_eq!(cache.statistics().loads, 1);
    }
}

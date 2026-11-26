use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Condvar, Mutex, RwLock,
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

type LoadResult = Result<Option<TableFacts>, &'static str>;

#[derive(Default)]
struct InFlight {
    result: Mutex<Option<LoadResult>>,
    ready: Condvar,
}

#[derive(Default)]
pub struct ObjectTableCache {
    cache: RwLock<Cache>,
    in_flight: Mutex<BTreeMap<Vec<u8>, Arc<InFlight>>>,
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
        let (flight, leader) = {
            let mut flights = self
                .in_flight
                .lock()
                .map_err(|_| "object-table cache coordinator poisoned")?;
            if let Some(flight) = flights.get(filename) {
                (flight.clone(), false)
            } else {
                let flight = Arc::new(InFlight::default());
                flights.insert(filename.to_vec(), flight.clone());
                (flight, true)
            }
        };
        if !leader {
            let mut result = flight
                .result
                .lock()
                .map_err(|_| "object-table cache load poisoned")?;
            while result.is_none() {
                result = flight
                    .ready
                    .wait(result)
                    .map_err(|_| "object-table cache load poisoned")?;
            }
            let loaded = result
                .clone()
                .ok_or("object-table cache load did not finish")?;
            match &loaded {
                Ok(Some(facts)) if facts.glow_known & need_glow == need_glow => {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return loaded;
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => return loaded,
            }
            // The leading request may have asked for fewer optional facts. Resolve the upgrade
            // after it publishes instead of racing two native loads of the same table.
            drop(result);
            return self.resolve(filename, need_glow, load);
        }
        {
            let mut cache = self
                .cache
                .write()
                .map_err(|_| "object-table cache poisoned")?;
            cache.statistics.loads = cache.statistics.loads.saturating_add(1);
        }
        let loaded = load();
        let result = match loaded {
            Ok(Some(facts)) => self.insert(filename, facts).map(Some),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        };
        {
            let mut published = flight
                .result
                .lock()
                .map_err(|_| "object-table cache load poisoned")?;
            *published = Some(result.clone());
        }
        let mut flights = self
            .in_flight
            .lock()
            .map_err(|_| "object-table cache coordinator poisoned")?;
        flights.remove(filename);
        flight.ready.notify_all();
        result
    }

    fn insert(&self, filename: &[u8], mut facts: TableFacts) -> Result<TableFacts, &'static str> {
        let mut cache = self
            .cache
            .write()
            .map_err(|_| "object-table cache poisoned")?;
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
        Ok(facts)
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
        self.in_flight
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Barrier, atomic::AtomicUsize},
        thread,
        time::Duration,
    };

    fn facts(mask: u8) -> TableFacts {
        TableFacts {
            glow_known: mask,
            ..TableFacts::default()
        }
    }

    #[test]
    fn identical_misses_share_one_load() {
        let cache = Arc::new(ObjectTableCache::default());
        let loads = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(3));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let cache = cache.clone();
                let loads = loads.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    cache
                        .resolve(b"shared", 1, || {
                            loads.fetch_add(1, Ordering::SeqCst);
                            thread::sleep(Duration::from_millis(20));
                            Ok(Some(facts(1)))
                        })
                        .unwrap()
                        .unwrap()
                })
            })
            .collect();
        barrier.wait();
        for thread in threads {
            assert_eq!(thread.join().unwrap().glow_known, 1);
        }
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert_eq!(cache.statistics().hits, 1);
    }

    #[test]
    fn independent_misses_load_concurrently() {
        let cache = Arc::new(ObjectTableCache::default());
        let inside = Arc::new(Barrier::new(2));
        let threads: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .map(|name| {
                let cache = cache.clone();
                let inside = inside.clone();
                thread::spawn(move || {
                    cache
                        .resolve(name, 0, || {
                            inside.wait();
                            Ok(Some(facts(0)))
                        })
                        .unwrap()
                })
            })
            .collect();
        for thread in threads {
            assert!(thread.join().unwrap().is_some());
        }
        assert_eq!(cache.statistics().loads, 2);
    }
}

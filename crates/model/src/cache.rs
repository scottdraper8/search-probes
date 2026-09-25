use crate::{
    address::Address,
    criteria::Plan,
    facts::{Facts, Verdict},
    settings::MAX_RESULTS,
};
use std::collections::{BTreeMap, VecDeque};

pub const MAX_CACHED_SYSTEMS: usize = 16_384;
pub const MAX_NAME_BYTES: usize = 127;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SystemRecord {
    pub system: Facts,
    pub planets: [Facts; 8],
    names: [Option<Box<str>>; 8],
    count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedMatch {
    pub address: Address,
    pub name: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub matches: u8,
    pub unknown: u8,
}

impl SystemRecord {
    pub fn new(system: Facts, planets: [Facts; 8]) -> Result<Self, &'static str> {
        let count = usize::from(system.planet_count().ok_or("missing system planet count")?);
        if !(1..=8).contains(&count) {
            return Err("invalid system planet count");
        }
        Ok(Self {
            system,
            planets,
            names: Default::default(),
            count,
        })
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn name(&self, index: usize) -> Option<&str> {
        self.names.get(index)?.as_deref()
    }
    pub fn set_name(&mut self, index: usize, name: &str) -> Result<(), &'static str> {
        if index >= self.count || name.len() > MAX_NAME_BYTES {
            return Err("invalid cached planet name");
        }
        self.names[index] = Some(name.into());
        Ok(())
    }
    fn merge(&mut self, new: Self) -> Result<(), &'static str> {
        if new.count != self.count {
            return Err("system planet count changed within the session");
        }
        self.system.merge(&new.system);
        for (index, facts) in new.planets.iter().enumerate().take(self.count) {
            self.planets[index].merge(facts);
            if let Some(name) = &new.names[index] {
                self.names[index] = Some(name.clone());
            }
        }
        Ok(())
    }
    pub fn evaluate(&self, plan: &Plan) -> Evaluation {
        let mut result = Evaluation {
            matches: 0,
            unknown: 0,
        };
        let system = plan.system_match(&self.system);
        if system == Verdict::NoMatch {
            return result;
        }
        for (index, facts) in self.planets.iter().enumerate().take(self.count) {
            match plan.planet_match(facts) {
                Verdict::NoMatch => {}
                Verdict::Match if system == Verdict::Match => result.matches |= 1 << index,
                _ => result.unknown |= 1 << index,
            }
        }
        result
    }
}

pub struct Cache {
    records: BTreeMap<Address, Box<SystemRecord>>,
    order: VecDeque<Address>,
    capacity: usize,
}

impl Cache {
    pub fn new(capacity: usize) -> Result<Self, &'static str> {
        if capacity > MAX_CACHED_SYSTEMS {
            return Err("cache capacity exceeds memory bound");
        }
        Ok(Self {
            records: BTreeMap::new(),
            order: VecDeque::new(),
            capacity,
        })
    }
    pub fn records(&self) -> impl Iterator<Item = (&Address, &SystemRecord)> {
        self.records
            .iter()
            .map(|(address, record)| (address, record.as_ref()))
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn clear(&mut self) {
        self.records.clear();
        self.order.clear();
    }
    pub fn get(&self, address: Address) -> Option<&SystemRecord> {
        self.records.get(&address.system()).map(Box::as_ref)
    }
    pub fn insert(&mut self, address: Address, record: SystemRecord) -> Result<(), &'static str> {
        let address = address.system();
        if address.coordinates().system == 0 {
            return Err("cannot cache a region as a system");
        }
        if let Some(existing) = self.records.get_mut(&address) {
            return existing.merge(record);
        }
        if self.capacity == 0 {
            return Ok(());
        }
        if self.records.len() == self.capacity {
            let oldest = self.order.pop_front().expect("cache order tracks records");
            self.records.remove(&oldest);
        }
        self.records.insert(address, Box::new(record));
        self.order.push_back(address);
        Ok(())
    }
    pub fn matching(&self, plan: &Plan, galaxy: u8, limit: usize) -> Vec<CachedMatch> {
        let mut result = Vec::new();
        let limit = limit.min(MAX_RESULTS);
        if limit == 0 {
            return result;
        }
        for address in self.order.iter().rev() {
            if address.coordinates().galaxy != galaxy {
                continue;
            }
            let record = &self.records[address];
            let matches = record.evaluate(plan).matches;
            for index in 0..record.count {
                if matches & (1 << index) == 0 {
                    continue;
                }
                result.push(CachedMatch {
                    address: address.planet(index).expect("bounded cached planet"),
                    name: record.names[index].clone(),
                });
                if result.len() == limit {
                    return result;
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::criteria::{Criteria, Field, PlanetFlag};
    fn address(system: u64) -> Address {
        Address::from_raw(system << 40).unwrap()
    }
    fn record() -> SystemRecord {
        let mut system = Facts::default();
        system.set_planet_count(2);
        SystemRecord::new(system, [Facts::default(); 8]).unwrap()
    }
    #[test]
    fn facts_from_nonmatches_can_satisfy_a_later_search() {
        let mut cache = Cache::new(2).unwrap();
        let mut entry = record();
        entry.planets[0].set(Field::Sky, 5);
        entry.planets[0].set(Field::Biome, 1);
        entry.planets[1].set(Field::Sky, 0);
        let mut x = Criteria::default();
        x.choices.extend([(Field::Sky, 32), (Field::Biome, 1)]);
        assert_eq!(entry.evaluate(&x.compile().unwrap()).matches, 0);
        cache.insert(address(1), entry).unwrap();
        let mut y = Criteria::default();
        y.choices.insert(Field::Sky, 32);
        let result = cache.matching(&y.compile().unwrap(), 0, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].address, address(1).planet(0).unwrap());
        assert!(result[0].name.is_none());
    }
    #[test]
    fn unknowns_do_not_become_exclusions_or_false_matches() {
        let entry = record();
        let mut criteria = Criteria::default();
        criteria.planets.insert(PlanetFlag::FloatingIslands, false);
        assert_eq!(
            entry.evaluate(&criteria.compile().unwrap()),
            Evaluation {
                matches: 0,
                unknown: 3
            }
        );
    }
    #[test]
    fn eviction_and_updates_keep_storage_bounded() {
        let mut cache = Cache::new(2).unwrap();
        for i in 1..=1000 {
            cache.insert(address(i), record()).unwrap();
            assert!(cache.len() <= 2);
        }
        assert_eq!(cache.order.len(), 2);
        assert!(cache.get(address(998)).is_none());
        let mut newer = record();
        newer.set_name(0, "Retained").unwrap();
        cache.insert(address(999), newer).unwrap();
        assert_eq!(cache.get(address(999)).unwrap().name(0), Some("Retained"));
        assert_eq!(cache.order.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
        assert!(cache.order.is_empty());
        assert!(std::mem::size_of::<SystemRecord>() <= 2048);
        assert!(
            (std::mem::size_of::<SystemRecord>() + 8 * MAX_NAME_BYTES + 256) * MAX_CACHED_SYSTEMS
                < 64 * 1024 * 1024
        );
    }
    #[test]
    fn cache_results_are_limited_and_session_galaxy_specific() {
        let mut cache = Cache::new(100).unwrap();
        for i in 1..=100 {
            cache.insert(address(i), record()).unwrap();
        }
        let plan = Criteria::default().compile().unwrap();
        assert_eq!(cache.matching(&plan, 0, usize::MAX).len(), 100);
        assert!(cache.matching(&plan, 1, 100).is_empty());
        assert!(cache.matching(&plan, 0, 0).is_empty());
    }
}

use crate::{
    address::Address,
    criteria::Plan,
    facts::{Facts, Verdict},
    settings::MAX_RESULTS,
    target::{Target, TargetKind},
};
use std::collections::{BTreeMap, VecDeque};

pub const MAX_CACHED_SYSTEMS: usize = 16_384;
pub const MAX_NAME_BYTES: usize = 127;
pub const MAX_CACHED_FAUNA_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SystemRecord {
    pub system: Facts,
    pub planets: [Facts; 8],
    names: [Option<Box<str>>; 8],
    system_name: Option<Box<str>>,
    count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedMatch {
    pub address: Address,
    pub kind: TargetKind,
    pub name: Option<Box<str>>,
}

impl CachedMatch {
    pub fn target(&self) -> Target {
        Target {
            address: self.address,
            kind: self.kind,
        }
    }
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
            system_name: None,
            count,
        })
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn name(&self, index: usize) -> Option<&str> {
        self.names.get(index)?.as_deref()
    }
    pub fn set_system_name(&mut self, name: &str) -> Result<(), &'static str> {
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return Err("invalid cached system name");
        }
        self.system_name = Some(name.into());
        Ok(())
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
            return Err("system composition changed within the session");
        }
        self.system.merge(&new.system);
        if new.system_name.is_some() {
            self.system_name = new.system_name;
        }
        for (index, facts) in new.planets.iter().enumerate().take(self.count) {
            self.planets[index].merge(facts);
            if let Some(name) = &new.names[index] {
                self.names[index] = Some(name.clone());
            }
        }
        Ok(())
    }
    fn fauna_bytes(&self) -> Result<usize, &'static str> {
        self.planets
            .iter()
            .chain(std::iter::once(&self.system))
            .try_fold(0, |bytes, facts| {
                let bytes = if let Some(arena) = facts.arena() {
                    if arena.len() > crate::fauna::MAX_SPECIES {
                        return Err("cached Arena fauna exceeds species bound");
                    }
                    bytes + std::mem::size_of_val(arena) + 32
                } else {
                    bytes
                };
                let Some(species) = facts.fauna() else {
                    return Ok(bytes);
                };
                if species.len() > crate::fauna::MAX_SPECIES {
                    return Err("cached fauna exceeds species bound");
                }
                Ok(bytes + std::mem::size_of_val(species) + 32)
            })
    }
    pub fn evaluate(&self, plan: &Plan) -> Evaluation {
        let mut result = Evaluation {
            matches: 0,
            unknown: 0,
        };
        let system = plan.system_match(&self.system, &self.planets[..self.count]);
        if system == Verdict::NoMatch {
            return result;
        }
        if plan.planet.is_empty() {
            match system {
                Verdict::Match => result.matches = 1,
                Verdict::Unknown => result.unknown = 1,
                Verdict::NoMatch => {}
            }
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

    pub fn unresolved_arena_planets(&self, plan: &Plan) -> u8 {
        if plan.system_match(&self.system, &self.planets[..self.count]) != Verdict::Match {
            return 0;
        }
        self.planets
            .iter()
            .take(self.count)
            .enumerate()
            .fold(0, |mask, (index, facts)| {
                mask | if plan.unresolved_arena_ranks(facts) {
                    1 << index
                } else {
                    0
                }
            })
    }

    pub fn result(&self, address: Address, index: usize, plan: &Plan) -> CachedMatch {
        if plan.planet.is_empty() {
            CachedMatch {
                address: address.system(),
                kind: TargetKind::System,
                name: self.system_name.clone(),
            }
        } else {
            CachedMatch {
                address: address.planet(index).expect("bounded result planet"),
                kind: TargetKind::Planet,
                name: self.names[index].clone(),
            }
        }
    }
}

pub struct Cache {
    records: BTreeMap<Address, Box<SystemRecord>>,
    order: VecDeque<Address>,
    capacity: usize,
    fauna_bytes: usize,
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
            fauna_bytes: 0,
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
        self.fauna_bytes = 0;
    }
    pub fn get(&self, address: Address) -> Option<&SystemRecord> {
        self.records.get(&address.system()).map(Box::as_ref)
    }
    pub fn name_system(&mut self, address: Address, name: &str) -> Result<(), &'static str> {
        if let Some(record) = self.records.get_mut(&address.system()) {
            record.set_system_name(name)?;
        }
        Ok(())
    }
    pub fn insert(&mut self, address: Address, record: SystemRecord) -> Result<(), &'static str> {
        let address = address.system();
        if address.coordinates().system == 0 {
            return Err("cannot cache a region as a system");
        }
        let added = record.fauna_bytes()?;
        if self.capacity == 0 {
            return Ok(());
        }
        if let Some(existing) = self.records.get_mut(&address) {
            let previous = existing.fauna_bytes()?;
            existing.merge(record)?;
            self.fauna_bytes = self.fauna_bytes - previous + existing.fauna_bytes()?;
        } else {
            if self.records.len() == self.capacity {
                self.evict_oldest();
            }
            self.records.insert(address, Box::new(record));
            self.order.push_back(address);
            self.fauna_bytes += added;
        }
        while self.fauna_bytes > MAX_CACHED_FAUNA_BYTES {
            self.evict_oldest();
        }
        Ok(())
    }
    fn evict_oldest(&mut self) {
        let oldest = self.order.pop_front().expect("cache order tracks records");
        let removed = self
            .records
            .remove(&oldest)
            .expect("cache order tracks records");
        self.fauna_bytes -= removed.fauna_bytes().expect("validated cached fauna");
    }
    pub fn matching(
        &self,
        plan: &Plan,
        galaxy: u8,
        excluded_system: Address,
        limit: usize,
    ) -> Vec<CachedMatch> {
        let mut result = Vec::new();
        let limit = limit.min(MAX_RESULTS);
        if limit == 0 {
            return result;
        }
        for address in self.order.iter().rev() {
            if address.coordinates().galaxy != galaxy || *address == excluded_system.system() {
                continue;
            }
            let record = &self.records[address];
            let matches = record.evaluate(plan).matches;
            for index in 0..record.count {
                if matches & (1 << index) == 0 {
                    continue;
                }
                result.push(record.result(*address, index, plan));
                if result.len() == limit {
                    return result;
                }
            }
        }
        result
    }
}

use crate::{
    address::Address,
    criteria::Plan,
    facts::{Facts, Verdict},
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fauna_role: Option<u8>,
}

impl CachedMatch {
    pub fn target(&self) -> Target {
        Target {
            address: self.address,
            kind: self.kind,
            fauna_role: self.fauna_role,
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

    pub fn results(&self, address: Address, index: usize, plan: &Plan) -> Vec<CachedMatch> {
        if plan.planet.is_empty() {
            vec![CachedMatch {
                address: address.system(),
                kind: TargetKind::System,
                name: self.system_name.clone(),
                fauna_role: None,
            }]
        } else if let Some(facts) = self.planets.get(index) {
            let address = address.planet(index).expect("bounded result planet");
            if plan.targets_animals() {
                return plan
                    .animal_witnesses(facts)
                    .into_iter()
                    .map(|animal| CachedMatch {
                        address,
                        kind: TargetKind::Fauna,
                        // A generated role identifies the target but does not contain the
                        // species/genus identity from which NMS derives its exact name. That
                        // identity is captured from the spawned creature at the destination.
                        name: None,
                        fauna_role: Some(animal.role),
                    })
                    .collect();
            }
            vec![CachedMatch {
                address,
                kind: TargetKind::Planet,
                name: self.names[index].clone(),
                fauna_role: None,
            }]
        } else {
            Vec::new()
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        address::Coordinates,
        arena::{ArenaField, Creature, RankEvidence, Ranks, Selection as ArenaSelection},
        criteria::{Predicate, Requirements, ResourceMode, Section},
        fauna::{Domain, Family, FaunaField, HeightRange, Selection, Species},
    };

    fn address() -> Address {
        Address::new(Coordinates {
            x: 1,
            y: 2,
            z: 3,
            galaxy: 0,
            system: 4,
            planet: 0,
        })
        .unwrap()
    }

    fn plan(selection: Selection) -> Plan {
        Plan {
            system: Vec::new(),
            planet: vec![Predicate::Fauna(selection)],
            requirements: Requirements::default(),
            match_modes: [ResourceMode::All; Section::ALL.len()],
        }
    }

    fn record(species: Vec<Species>) -> SystemRecord {
        let mut system = Facts::default();
        system.set_planet_count(1);
        let mut planets: [Facts; 8] = std::array::from_fn(|_| Facts::default());
        planets[0].set_fauna(species);
        let mut record = SystemRecord::new(system, planets).unwrap();
        record.set_name(0, "Test Planet").unwrap();
        record
    }

    fn arena_plan(selection: ArenaSelection) -> Plan {
        Plan {
            system: Vec::new(),
            planet: vec![Predicate::Arena(selection)],
            requirements: Requirements::default(),
            match_modes: [ResourceMode::All; Section::ALL.len()],
        }
    }

    fn species(role: u8) -> Species {
        Species {
            role,
            family: Family::Cats,
            domain: Domain::Land,
            activity: 2,
            rarity: 0,
            cave: false,
            hemisphere: 0,
            heights_cm: [100, 120],
        }
    }

    #[test]
    fn positive_fauna_result_targets_the_matching_animal_without_inventing_a_name() {
        let mut masks = [0; FaunaField::ALL.len()];
        masks[FaunaField::Family as usize] = 1 << Family::Cats as usize;
        let selection = Selection {
            domain: Domain::Land,
            presence: None,
            masks,
            height: HeightRange::default(),
            mode: ResourceMode::All,
        };
        let results = record(vec![Species {
            role: 5,
            family: Family::Cats,
            domain: Domain::Land,
            activity: 2,
            rarity: 0,
            cave: false,
            hemisphere: 0,
            heights_cm: [100, 120],
        }])
        .results(address(), 0, &plan(selection));

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, TargetKind::Fauna);
        assert_eq!(results[0].fauna_role, Some(5));
        assert_eq!(results[0].name, None);
        assert_eq!(results[0].address, address().planet(0).unwrap());
    }

    #[test]
    fn negative_only_fauna_result_remains_a_planet_target() {
        let selection = Selection {
            domain: Domain::Airborne,
            presence: Some(false),
            masks: [0; FaunaField::ALL.len()],
            height: HeightRange::default(),
            mode: ResourceMode::All,
        };
        let results = record(Vec::new()).results(address(), 0, &plan(selection));

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, TargetKind::Planet);
        assert_eq!(results[0].fauna_role, None);
        assert_eq!(results[0].name.as_deref(), Some("Test Planet"));
    }

    #[test]
    fn arena_result_targets_the_qualifying_animal() {
        let mut masks = [0; ArenaField::ALL.len()];
        masks[ArenaField::Affinity as usize] = 1 << 7;
        masks[ArenaField::OverallRank as usize] = 1 << 3;
        masks[ArenaField::Combat as usize] = 1 << 3;
        masks[ArenaField::Agility as usize] = 1 << 3;
        masks[ArenaField::Health as usize] = 1 << 3;
        let plan = arena_plan(ArenaSelection {
            presence: None,
            masks,
            mode: ResourceMode::All,
        });
        let mut record = record(vec![species(5)]);
        record.planets[0].set_arena(vec![Creature {
            role: 5,
            affinity: 7,
            ranks: RankEvidence::exact(Ranks {
                combat: 3,
                agility: 3,
                health: 3,
            })
            .unwrap(),
        }]);

        let results = record.results(address(), 0, &plan);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].kind, TargetKind::Fauna);
        assert_eq!(results[0].fauna_role, Some(5));
        assert_eq!(results[0].name, None);
    }

    #[test]
    fn arena_result_does_not_fall_back_to_a_planet_without_an_animal_witness() {
        let mut masks = [0; ArenaField::ALL.len()];
        masks[ArenaField::Affinity as usize] = 1 << 7;
        let plan = arena_plan(ArenaSelection {
            presence: None,
            masks,
            mode: ResourceMode::All,
        });
        let mut record = record(vec![species(4)]);
        record.planets[0].set_arena(vec![Creature {
            role: 5,
            affinity: 7,
            ranks: RankEvidence::exact(Ranks {
                combat: 0,
                agility: 0,
                health: 0,
            })
            .unwrap(),
        }]);

        assert!(record.results(address(), 0, &plan).is_empty());
    }
}

use crate::{address::Address, criteria::Plan, regions::Regions};
use std::collections::{BTreeSet, VecDeque};

pub const MAX_FRONTIERS: usize = 128;
pub const MAX_REGION_SYSTEMS: usize = 4095;

#[derive(Debug, Clone)]
pub struct Frontier {
    regions: Regions,
    pending_region: Option<Address>,
    systems: Vec<Address>,
    cursor: usize,
    searched: u64,
}

impl Frontier {
    pub fn new(origin: Address) -> Self {
        Self {
            regions: Regions::new(origin),
            pending_region: None,
            systems: Vec::new(),
            cursor: 0,
            searched: 0,
        }
    }
    pub fn searched(&self) -> u64 {
        self.searched
    }
    pub fn next_region(&mut self) -> Option<Address> {
        if self.cursor < self.systems.len() {
            return None;
        }
        if self.pending_region.is_none() {
            self.pending_region = self.regions.next();
        }
        self.pending_region
    }
    pub fn load_region(
        &mut self,
        region: Address,
        systems: Vec<Address>,
    ) -> Result<(), &'static str> {
        if self.pending_region != Some(region) || systems.len() > MAX_REGION_SYSTEMS {
            return Err("region does not belong to the pending frontier");
        }
        let mut unique = BTreeSet::new();
        if systems.iter().any(|address| {
            address.region() != region
                || address.system() != *address
                || address.coordinates().system == 0
                || !unique.insert(*address)
        }) {
            return Err("invalid frontier system list");
        }
        self.systems = systems;
        self.cursor = 0;
        self.pending_region = None;
        Ok(())
    }
    pub fn pending(&self, limit: usize) -> &[Address] {
        &self.systems[self.cursor..self.systems.len().min(self.cursor.saturating_add(limit))]
    }
    pub fn commit(&mut self, completed: usize) -> Result<(), &'static str> {
        if completed > self.systems.len() - self.cursor {
            return Err("completed batch exceeds pending systems");
        }
        self.searched = self
            .searched
            .checked_add(completed as u64)
            .ok_or("search count overflow")?;
        self.cursor += completed;
        Ok(())
    }
}

struct Remembered {
    plan: Plan,
    galaxy: u8,
    frontier: Frontier,
}

#[derive(Default)]
pub struct History {
    recent: VecDeque<Remembered>,
}

impl History {
    pub fn take(&mut self, plan: &Plan, origin: Address, restart: bool) -> Frontier {
        let existing = self
            .recent
            .iter()
            .position(|entry| entry.plan == *plan && entry.galaxy == origin.coordinates().galaxy)
            .and_then(|index| self.recent.remove(index));
        if !restart {
            if let Some(entry) = existing {
                return entry.frontier;
            }
        }
        Frontier::new(origin)
    }
    pub fn remember(&mut self, plan: Plan, galaxy: u8, frontier: Frontier) -> bool {
        self.recent
            .retain(|entry| entry.plan != plan || entry.galaxy != galaxy);
        let evicted = self.recent.len() == MAX_FRONTIERS;
        if evicted {
            self.recent.pop_front();
        }
        self.recent.push_back(Remembered {
            plan,
            galaxy,
            frontier,
        });
        evicted
    }
    pub fn clear(&mut self) {
        self.recent.clear();
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::criteria::{Criteria, Field};
    fn origin() -> Address {
        Address::from_raw(0).unwrap()
    }
    fn systems(region: Address) -> Vec<Address> {
        (1..=100)
            .map(|i| Address::from_raw(region.raw() | (i << 40)).unwrap())
            .collect()
    }
    #[test]
    fn cancelled_batch_remains_available_until_committed() {
        let mut frontier = Frontier::new(origin());
        let region = frontier.next_region().unwrap();
        assert_eq!(frontier.next_region(), Some(region));
        frontier.load_region(region, systems(region)).unwrap();
        let batch = frontier.pending(16).to_vec();
        assert_eq!(batch, frontier.pending(16));
        assert!(frontier.commit(101).is_err());
        frontier.commit(5).unwrap();
        assert_eq!(frontier.pending(11), &batch[5..]);
        assert_eq!(frontier.searched(), 5);
        assert_eq!(frontier.next_region(), None);
        frontier.commit(95).unwrap();
        assert_ne!(frontier.next_region(), Some(region));
    }
    #[test]
    fn identical_filters_continue_when_origin_moves_within_the_galaxy() {
        let plan = Criteria::default().compile().unwrap();
        let mut history = History::default();
        let mut frontier = history.take(&plan, origin(), false);
        let region = frontier.next_region().unwrap();
        frontier.load_region(region, systems(region)).unwrap();
        frontier.commit(40).unwrap();
        let next = frontier.pending(1)[0];
        assert!(!history.remember(plan.clone(), 0, frontier));
        let moved = Address::from_raw(123).unwrap();
        let resumed = history.take(&plan, moved, false);
        assert_eq!(resumed.searched(), 40);
        assert_eq!(resumed.pending(1), &[next]);
        history.remember(plan.clone(), 0, resumed);
        let mut restarted = history.take(&plan, moved, true);
        assert_eq!(restarted.next_region(), Some(moved));
    }
    #[test]
    fn history_eviction_is_bounded_and_reported() {
        let mut history = History::default();
        for mask in 1..=MAX_FRONTIERS + 1 {
            let mut criteria = Criteria::default();
            criteria.choices.insert(Field::Grass, mask as u64);
            let plan = criteria.compile().unwrap();
            assert_eq!(
                history.remember(plan, 0, Frontier::new(origin())),
                mask > MAX_FRONTIERS
            );
        }
        assert_eq!(history.recent.len(), MAX_FRONTIERS);
        history.clear();
        assert!(history.recent.is_empty());
    }
    #[test]
    fn invalid_native_regions_cannot_advance_the_frontier() {
        let mut frontier = Frontier::new(origin());
        let region = frontier.next_region().unwrap();
        let duplicate = Address::from_raw(1 << 40).unwrap();
        assert!(
            frontier
                .load_region(region, vec![duplicate, duplicate])
                .is_err()
        );
        assert_eq!(frontier.next_region(), Some(region));
        frontier.load_region(region, Vec::new()).unwrap();
        assert_ne!(frontier.next_region(), Some(region));
    }
}

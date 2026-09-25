use crate::{
    address::Address,
    cache::{Cache, CachedMatch, MAX_CACHED_SYSTEMS, SystemRecord},
    criteria::Plan,
    frontier::{Frontier, History},
    settings::{MAX_RESULTS, SearchOptions},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Cached,
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    Results,
    Budget,
    Exhausted,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub phase: Phase,
    pub finish: Option<Finish>,
    pub options: SearchOptions,
    pub origin: Address,
    pub systems: u64,
    pub cached_systems: u64,
    pub previous_systems: u64,
    pub results: Vec<CachedMatch>,
    pub cached_results: usize,
    pub history_evicted: bool,
    pub error: Option<String>,
}

struct Search {
    progress: Progress,
    plan: Plan,
    frontier: Option<Frontier>,
    previous_page: Vec<Address>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Work {
    Idle,
    Region(Address),
    Systems(Vec<Address>),
}

pub struct Session {
    cache: Cache,
    history: History,
    search: Option<Search>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            cache: Cache::new(MAX_CACHED_SYSTEMS).expect("bounded default cache"),
            history: History::default(),
            search: None,
        }
    }
}

impl Session {
    pub fn reset(&mut self) {
        self.cache.clear();
        self.history.clear();
        self.search = None;
    }
    pub fn cached_records(&self) -> impl Iterator<Item = (&Address, &SystemRecord)> {
        self.cache.records()
    }
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
    pub fn record(&self, address: Address) -> Option<&SystemRecord> {
        self.cache.get(address)
    }
    pub fn progress(&self) -> Option<&Progress> {
        self.search.as_ref().map(|search| &search.progress)
    }
    pub fn plan(&self) -> Option<&Plan> {
        self.search.as_ref().map(|search| &search.plan)
    }
    pub fn start(
        &mut self,
        options: SearchOptions,
        origin: Address,
        restart: bool,
        use_cache: bool,
    ) -> Result<(), &'static str> {
        options.validate()?;
        if self
            .search
            .as_ref()
            .is_some_and(|search| matches!(search.progress.phase, Phase::Running | Phase::Cached))
        {
            return Err("cancel the current search before starting another");
        }
        let plan = options.criteria.compile()?;
        let frontier = self.history.take(&plan, origin, restart);
        let results = if use_cache {
            self.cache
                .matching(&plan, origin.coordinates().galaxy, options.results)
        } else {
            Vec::new()
        };
        let progress = Progress {
            phase: if results.is_empty() {
                Phase::Running
            } else {
                Phase::Cached
            },
            finish: None,
            options,
            origin,
            systems: 0,
            cached_systems: 0,
            previous_systems: frontier.searched(),
            cached_results: results.len(),
            results,
            history_evicted: false,
            error: None,
        };
        self.search = Some(Search {
            progress,
            plan,
            frontier: Some(frontier),
            previous_page: Vec::new(),
        });
        Ok(())
    }
    pub fn continue_live(&mut self, result_limit: usize) -> Result<(), &'static str> {
        let search = self.search.as_mut().ok_or("no search is pending")?;
        if search.progress.phase != Phase::Cached {
            return Err("search is not waiting for live continuation");
        }
        if result_limit == MAX_RESULTS && search.progress.results.len() == MAX_RESULTS {
            search.previous_page = search
                .progress
                .results
                .drain(..)
                .map(|result| result.address)
                .collect();
            search.progress.cached_results = 0;
        } else if result_limit <= search.progress.results.len() || result_limit > MAX_RESULTS {
            return Err("live continuation needs room for more results, up to 100");
        }
        search.progress.options.results = result_limit;
        search.progress.phase = Phase::Running;
        Ok(())
    }
    fn finish(&mut self, reason: Finish) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        let Some(frontier) = search.frontier.take() else {
            return;
        };
        search.progress.history_evicted = self.history.remember(
            search.plan.clone(),
            search.progress.origin.coordinates().galaxy,
            frontier,
        );
        search.progress.finish = Some(reason);
        search.progress.phase = match reason {
            Finish::Cancelled => Phase::Cancelled,
            Finish::Failed => Phase::Failed,
            _ => Phase::Completed,
        };
    }
    pub fn cancel(&mut self) {
        self.finish(Finish::Cancelled);
    }
    pub fn fail(&mut self, error: &str) {
        if let Some(search) = self.search.as_mut() {
            search.progress.error = Some(error.chars().take(512).collect());
        }
        self.finish(Finish::Failed);
    }
    pub fn next(&mut self, limit: usize) -> Result<Work, &'static str> {
        if limit == 0 {
            return Ok(Work::Idle);
        }
        for _ in 0..limit.min(256) {
            let Some(search) = self
                .search
                .as_mut()
                .filter(|search| search.progress.phase == Phase::Running)
            else {
                return Ok(Work::Idle);
            };
            let progress = &search.progress;
            if progress.results.len() >= progress.options.results {
                self.finish(Finish::Results);
                return Ok(Work::Idle);
            }
            if progress.systems >= progress.options.systems {
                self.finish(Finish::Budget);
                return Ok(Work::Idle);
            }
            let frontier = search
                .frontier
                .as_mut()
                .ok_or("running search has no frontier")?;
            let available = limit
                .min(256)
                .min((progress.options.systems - progress.systems) as usize);
            let pending = frontier.pending(available);
            if pending.is_empty() {
                if let Some(region) = frontier.next_region() {
                    return Ok(Work::Region(region));
                }
                self.finish(Finish::Exhausted);
                return Ok(Work::Idle);
            }
            let address = pending[0];
            if let Some(record) = self
                .cache
                .get(address)
                .filter(|record| record.evaluate(&search.plan).unknown == 0)
                .cloned()
            {
                self.apply(address, record, true)?;
            } else {
                return Ok(Work::Systems(pending.to_vec()));
            }
        }
        Ok(Work::Idle)
    }
    pub fn load_region(
        &mut self,
        region: Address,
        systems: Vec<Address>,
    ) -> Result<(), &'static str> {
        self.search
            .as_mut()
            .filter(|search| search.progress.phase == Phase::Running)
            .and_then(|search| search.frontier.as_mut())
            .ok_or("no live frontier")?
            .load_region(region, systems)
    }
    pub fn observe(&mut self, address: Address, record: SystemRecord) -> Result<(), &'static str> {
        self.cache.insert(address, record)
    }
    pub fn apply(
        &mut self,
        address: Address,
        record: SystemRecord,
        from_cache: bool,
    ) -> Result<(), &'static str> {
        let search = self
            .search
            .as_mut()
            .filter(|search| search.progress.phase == Phase::Running)
            .ok_or("search is not running")?;
        let frontier = search
            .frontier
            .as_mut()
            .ok_or("running search has no frontier")?;
        if frontier.pending(1) != [address] {
            return Err("completed system is out of frontier order");
        }
        self.cache.insert(address, record)?;
        let record = self
            .cache
            .get(address)
            .ok_or("completed system was not cached")?;
        let evaluation = record.evaluate(&search.plan);
        if evaluation.unknown != 0 {
            return Err("completed system still has unknown selected facts");
        }
        for index in 0..record.count() {
            if evaluation.matches & (1 << index) == 0 {
                continue;
            }
            let planet = address.planet(index)?;
            if search.progress.results.len() == search.progress.options.results {
                break;
            }
            if search.previous_page.contains(&planet)
                || search
                    .progress
                    .results
                    .iter()
                    .any(|result| result.address == planet)
            {
                continue;
            }
            search.progress.results.push(CachedMatch {
                address: planet,
                name: record.name(index).map(Into::into),
            });
        }
        frontier.commit(1)?;
        search.progress.systems += 1;
        search.progress.cached_systems += u64::from(from_cache);
        if search.progress.results.len() == search.progress.options.results {
            self.finish(Finish::Results);
        } else if search.progress.systems == search.progress.options.systems {
            self.finish(Finish::Budget);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{criteria::Field, facts::Facts};
    fn origin() -> Address {
        Address::from_raw(0).unwrap()
    }
    fn record(sky: u32) -> SystemRecord {
        let mut system = Facts::default();
        system.set_planet_count(1);
        let mut planets = [Facts::default(); 8];
        planets[0].set(Field::Sky, sky);
        SystemRecord::new(system, planets).unwrap()
    }
    fn load(session: &mut Session) -> Vec<Address> {
        let Work::Region(region) = session.next(16).unwrap() else {
            panic!("expected region");
        };
        let addresses: Vec<_> = (1..=100)
            .map(|index| Address::from_raw(region.raw() | (index << 40)).unwrap())
            .collect();
        session.load_region(region, addresses.clone()).unwrap();
        addresses
    }
    #[test]
    fn search_resume_advances_after_completed_systems() {
        let mut session = Session::default();
        let mut options = SearchOptions::default();
        options.criteria.choices.insert(Field::Sky, 32);
        session
            .start(options.clone(), origin(), false, false)
            .unwrap();
        let addresses = load(&mut session);
        session.apply(addresses[0], record(0), false).unwrap();
        session.apply(addresses[1], record(0), false).unwrap();
        session.cancel();
        session.start(options, origin(), false, false).unwrap();
        assert_eq!(session.progress().unwrap().previous_systems, 2);
        assert_eq!(session.next(1).unwrap(), Work::Systems(vec![addresses[2]]));
    }
    #[test]
    fn cached_matches_pause_before_any_live_generation() {
        let mut session = Session::default();
        let address = Address::from_raw(1 << 40).unwrap();
        session.observe(address, record(5)).unwrap();
        let mut options = SearchOptions::default();
        options.criteria.choices.insert(Field::Sky, 32);
        session.start(options, origin(), false, true).unwrap();
        assert_eq!(session.progress().unwrap().phase, Phase::Cached);
        assert_eq!(session.next(16).unwrap(), Work::Idle);
        assert_eq!(session.progress().unwrap().results.len(), 1);
        assert!(session.continue_live(1).is_err());
        session.continue_live(100).unwrap();
        let addresses = load(&mut session);
        assert_eq!(
            session.next(2).unwrap(),
            Work::Systems(addresses[1..3].to_vec())
        );
        assert_eq!(session.progress().unwrap().results.len(), 1);
        assert_eq!(session.progress().unwrap().cached_systems, 1);
    }
    #[test]
    fn full_cached_page_continues_without_repeating_displayed_matches() {
        let mut session = Session::default();
        for index in 1..=100 {
            session
                .observe(Address::from_raw(index << 40).unwrap(), record(5))
                .unwrap();
        }
        let mut options = SearchOptions::default();
        options.results = MAX_RESULTS;
        session.start(options, origin(), false, true).unwrap();
        let previous: Vec<_> = session
            .progress()
            .unwrap()
            .results
            .iter()
            .map(|result| result.address)
            .collect();
        assert_eq!(previous.len(), MAX_RESULTS);
        assert!(session.continue_live(MAX_RESULTS + 1).is_err());
        assert_eq!(session.progress().unwrap().results.len(), MAX_RESULTS);
        session.continue_live(MAX_RESULTS).unwrap();
        assert_eq!(session.cache_len(), 100);
        load(&mut session);
        for _ in 0..100 {
            session.next(1).unwrap();
        }
        let progress = session.progress().unwrap();
        assert_eq!(progress.systems, 100);
        assert_eq!(progress.cached_systems, 100);
        assert!(progress.results.is_empty());
        assert_eq!(progress.cached_results, 0);
        let addresses = load(&mut session);
        session.apply(addresses[0], record(5), false).unwrap();
        let progress = session.progress().unwrap();
        assert_eq!(progress.results.len(), 1);
        assert!(!previous.contains(&progress.results[0].address));
    }
    #[test]
    fn incomplete_cancelled_system_stays_at_frontier_and_facts_survive() {
        let mut session = Session::default();
        let mut options = SearchOptions::default();
        options.criteria.choices.insert(Field::Sky, 32);
        session
            .start(options.clone(), origin(), false, false)
            .unwrap();
        let addresses = load(&mut session);
        let mut incomplete = record(0);
        incomplete.planets = [Facts::default(); 8];
        session.observe(addresses[0], incomplete).unwrap();
        session.cancel();
        session.start(options, origin(), false, false).unwrap();
        assert_eq!(session.next(1).unwrap(), Work::Systems(vec![addresses[0]]));
        session.reset();
        assert!(session.progress().is_none());
        assert_eq!(session.cache_len(), 0);
    }
    #[test]
    fn result_limit_and_order_reject_uncommitted_work() {
        let mut session = Session::default();
        let mut options = SearchOptions::default();
        options.results = 1;
        session.start(options, origin(), false, false).unwrap();
        let addresses = load(&mut session);
        assert!(session.apply(addresses[1], record(5), false).is_err());
        session.apply(addresses[0], record(5), false).unwrap();
        assert_eq!(session.progress().unwrap().finish, Some(Finish::Results));
        assert_eq!(session.progress().unwrap().systems, 1);
        assert_eq!(session.next(16).unwrap(), Work::Idle);
    }
}

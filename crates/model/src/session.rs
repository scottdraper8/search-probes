use crate::{
    address::Address,
    cache::{Cache, CachedMatch, MAX_CACHED_SYSTEMS, SystemRecord},
    criteria::Plan,
    frontier::Frontier,
    settings::SearchOptions,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
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
    pub unresolved_arena_planets: u64,
    pub unresolved_arena_examples: Vec<Address>,
    pub results: Vec<CachedMatch>,
    pub error: Option<String>,
}

struct Search {
    progress: Progress,
    plan: Plan,
    frontier: Frontier,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Work {
    Idle,
    Region(Address),
    Systems(Vec<Address>),
}

pub struct Session {
    cache: Cache,
    search: Option<Search>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            cache: Cache::new(MAX_CACHED_SYSTEMS).expect("bounded default cache"),
            search: None,
        }
    }
}

impl Session {
    pub fn reset(&mut self) {
        self.cache.clear();
        self.search = None;
    }
    pub fn cached_records(&self) -> impl Iterator<Item = (&Address, &SystemRecord)> {
        self.cache.records()
    }
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
    pub fn clear_cached_facts(&mut self) {
        self.cache.clear();
    }
    pub fn is_in_search_galaxy(&self, player: Address) -> bool {
        self.search.as_ref().is_none_or(|search| {
            search.progress.origin.coordinates().galaxy == player.coordinates().galaxy
        })
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
    pub fn start(&mut self, options: SearchOptions, origin: Address) -> Result<(), &'static str> {
        options.validate()?;
        if self
            .search
            .as_ref()
            .is_some_and(|search| search.progress.phase == Phase::Running)
        {
            return Err("cancel the current search before starting another");
        }
        let plan = options.criteria.compile()?;
        let progress = Progress {
            phase: Phase::Running,
            finish: None,
            options,
            origin,
            systems: 0,
            cached_systems: 0,
            unresolved_arena_planets: 0,
            unresolved_arena_examples: Vec::new(),
            results: Vec::new(),
            error: None,
        };
        self.search = Some(Search {
            progress,
            plan,
            frontier: Frontier::new(origin),
        });
        Ok(())
    }
    fn finish(&mut self, reason: Finish) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
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
            let frontier = &mut search.frontier;
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
                .filter(|record| {
                    record.evaluate(&search.plan).unknown
                        & !record.unresolved_arena_planets(&search.plan)
                        == 0
                })
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
            .map(|search| &mut search.frontier)
            .ok_or("no live search")?
            .load_region(region, systems)
    }
    pub fn observe(&mut self, address: Address, record: SystemRecord) -> Result<(), &'static str> {
        self.cache.insert(address, record)
    }
    pub fn unnamed_system_results(&self) -> Vec<Address> {
        self.progress()
            .into_iter()
            .flat_map(|progress| &progress.results)
            .filter(|result| {
                result.kind == crate::target::TargetKind::System && result.name.is_none()
            })
            .map(|result| result.address)
            .collect()
    }
    pub fn name_system(&mut self, address: Address, name: &str) -> Result<(), &'static str> {
        if name.is_empty() || name.len() > crate::cache::MAX_NAME_BYTES {
            return Err("invalid result system name");
        }
        self.cache.name_system(address, name)?;
        if let Some(search) = self.search.as_mut() {
            for result in &mut search.progress.results {
                if result.kind == crate::target::TargetKind::System
                    && result.address == address.system()
                {
                    result.name = Some(name.into());
                }
            }
        }
        Ok(())
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
        let frontier = &mut search.frontier;
        if frontier.pending(1) != [address] {
            return Err("completed system is out of frontier order");
        }
        self.cache.insert(address, record)?;
        let record = self
            .cache
            .get(address)
            .ok_or("completed system was not cached")?;
        let evaluation = record.evaluate(&search.plan);
        let unresolved = record.unresolved_arena_planets(&search.plan);
        if evaluation.unknown & !unresolved != 0 {
            return Err("completed system still has unknown selected facts");
        }
        search.progress.unresolved_arena_planets += u64::from(unresolved.count_ones());
        for index in 0..record.count() {
            if unresolved & (1 << index) != 0 && search.progress.unresolved_arena_examples.len() < 8
            {
                search
                    .progress
                    .unresolved_arena_examples
                    .push(address.planet(index)?);
            }
        }
        for index in 0..record.count() {
            if address.system() == search.progress.origin.system() {
                break;
            }
            if evaluation.matches & (1 << index) == 0 {
                continue;
            }
            let result = record.result(address, index, &search.plan);
            if search.progress.results.len() == search.progress.options.results {
                break;
            }
            if search
                .progress
                .results
                .iter()
                .any(|previous| previous.address == result.address)
            {
                continue;
            }
            search.progress.results.push(result);
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

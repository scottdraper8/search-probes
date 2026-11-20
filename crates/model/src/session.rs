use crate::{
    address::Address,
    cache::{Cache, CachedMatch, MAX_CACHED_SYSTEMS, SystemRecord},
    criteria::Plan,
    frontier::Frontier,
    settings::{ExecutionSettings, SearchOptions},
};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Includes the currently displayed/live search.
pub const MAX_SEARCH_HISTORY: usize = 20;

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
    TimeLimit,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub id: u64,
    pub elapsed_ms: u64,
    pub user_paused: bool,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRecord {
    pub execution: ExecutionSettings,
    pub started_unix_ms: u64,
    pub finished_unix_ms: Option<u64>,
    pub progress: Arc<Progress>,
}

#[derive(Default)]
struct SearchClock {
    elapsed: Duration,
    running_since: Option<Instant>,
}

impl SearchClock {
    fn elapsed(&self, now: Instant) -> Duration {
        self.elapsed
            + self
                .running_since
                .map_or(Duration::ZERO, |since| now.saturating_duration_since(since))
    }
    fn observe(&mut self, now: Instant, running: bool) -> Duration {
        self.elapsed = self.elapsed(now);
        self.running_since = running.then_some(now);
        self.elapsed
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

struct Search {
    progress: Progress,
    plan: Plan,
    frontier: Frontier,
    paused: bool,
    available: bool,
    clock: SearchClock,
    started_unix_ms: u64,
    finished_unix_ms: Option<u64>,
    execution: ExecutionSettings,
}

impl Search {
    fn record(&self) -> SearchRecord {
        self.record_with(Arc::new(self.progress.clone()))
    }
    fn record_with(&self, progress: Arc<Progress>) -> SearchRecord {
        SearchRecord {
            execution: self.execution,
            started_unix_ms: self.started_unix_ms,
            finished_unix_ms: self.finished_unix_ms,
            progress,
        }
    }
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
    history: VecDeque<Arc<SearchRecord>>,
    next_id: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            cache: Cache::new(MAX_CACHED_SYSTEMS).expect("bounded default cache"),
            search: None,
            history: VecDeque::new(),
            next_id: 0,
        }
    }
}

impl Session {
    pub fn reset(&mut self) {
        self.cache.clear();
        self.search = None;
        self.history.clear();
    }
    /// Oldest first. Browsing these snapshots never mutates the live search.
    pub fn history(&self) -> Vec<Arc<SearchRecord>> {
        self.publication().1
    }
    /// Build the live progress once and share it with the current history record.
    pub fn publication(&self) -> (Option<Arc<Progress>>, Vec<Arc<SearchRecord>>) {
        let progress = self
            .search
            .as_ref()
            .map(|search| Arc::new(search.progress.clone()));
        let history = self
            .history
            .iter()
            .cloned()
            .chain(
                self.search
                    .iter()
                    .zip(progress.iter())
                    .map(|(search, progress)| Arc::new(search.record_with(progress.clone()))),
            )
            .collect();
        (progress, history)
    }
    pub fn search_result(&self, id: u64, target: crate::target::Target) -> Option<&CachedMatch> {
        let progress = self
            .search
            .as_ref()
            .filter(|search| search.progress.id == id)
            .map(|search| &search.progress)
            .or_else(|| {
                self.history
                    .iter()
                    .find(|record| record.progress.id == id)
                    .map(|record| record.progress.as_ref())
            })?;
        progress
            .results
            .iter()
            .find(|result| result.target() == target)
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
    pub fn start(
        &mut self,
        options: SearchOptions,
        origin: Address,
        execution: ExecutionSettings,
    ) -> Result<(), &'static str> {
        self.start_at(options, origin, execution, Instant::now())
    }
    fn start_at(
        &mut self,
        options: SearchOptions,
        origin: Address,
        execution: ExecutionSettings,
        now: Instant,
    ) -> Result<(), &'static str> {
        options.validate_search()?;
        execution.validate()?;
        if self
            .search
            .as_ref()
            .is_some_and(|search| search.progress.phase == Phase::Running)
        {
            return Err("cancel the current search before starting another");
        }
        let plan = options.criteria.compile()?;
        let results = self.retained_results(&plan, origin, options.results);
        let id = self
            .next_id
            .checked_add(1)
            .ok_or("search identity exhausted")?;
        self.next_id = id;
        if let Some(previous) = self.search.take() {
            self.history.push_back(Arc::new(previous.record()));
        }
        while self.history.len() >= MAX_SEARCH_HISTORY {
            self.history.pop_front();
        }
        let progress = Progress {
            id,
            elapsed_ms: 0,
            user_paused: false,
            phase: Phase::Running,
            finish: None,
            options,
            origin,
            systems: 0,
            cached_systems: 0,
            unresolved_arena_planets: 0,
            unresolved_arena_examples: Vec::new(),
            results,
            error: None,
        };
        self.search = Some(Search {
            progress,
            plan,
            frontier: Frontier::new(origin),
            paused: false,
            available: true,
            clock: SearchClock {
                elapsed: Duration::ZERO,
                running_since: Some(now),
            },
            started_unix_ms: unix_ms(),
            finished_unix_ms: None,
            execution,
        });
        // Previously retained matches can already satisfy the new result limit.
        if self
            .progress()
            .is_some_and(|progress| progress.results.len() >= progress.options.results)
        {
            self.finish_at(Finish::Results, now);
        }
        Ok(())
    }
    pub fn is_paused(&self) -> bool {
        self.search
            .as_ref()
            .is_some_and(|search| search.progress.phase == Phase::Running && search.paused)
    }
    pub fn configure_execution(&mut self, execution: ExecutionSettings) {
        if let Some(search) = self
            .search
            .as_mut()
            .filter(|search| search.progress.phase == Phase::Running)
        {
            search.execution = execution;
        }
    }
    pub fn set_paused(&mut self, paused: bool) -> Result<(), &'static str> {
        self.set_paused_at(paused, Instant::now())
    }
    fn set_paused_at(&mut self, paused: bool, now: Instant) -> Result<(), &'static str> {
        let search = self
            .search
            .as_mut()
            .filter(|search| search.progress.phase == Phase::Running)
            .ok_or("search is not running")?;
        search.paused = paused;
        search.progress.user_paused = paused;
        self.observe_time_at(now);
        Ok(())
    }
    /// Gameplay availability is independent of a user's pause choice.
    pub fn set_available(&mut self, available: bool) {
        self.set_available_at(available, Instant::now());
    }
    fn set_available_at(&mut self, available: bool, now: Instant) {
        if let Some(search) = &mut self.search {
            search.available = available;
        }
        self.observe_time_at(now);
    }
    pub fn observe_time(&mut self) {
        self.observe_time_at(Instant::now());
    }
    fn observe_time_at(&mut self, now: Instant) {
        let Some(search) = self
            .search
            .as_mut()
            .filter(|search| search.progress.phase == Phase::Running)
        else {
            return;
        };
        let elapsed = search
            .clock
            .observe(now, search.available && !search.paused);
        search.progress.elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        if search
            .progress
            .options
            .time_limit
            .is_some_and(|limit| elapsed >= limit.duration())
        {
            self.finish_at(Finish::TimeLimit, now);
        }
    }
    /// Monotonic deadline for the current uninterrupted active interval.
    pub fn deadline(&self) -> Option<Instant> {
        let search = self
            .search
            .as_ref()
            .filter(|search| search.progress.phase == Phase::Running)?;
        let limit = search.progress.options.time_limit?;
        let since = search.clock.running_since?;
        since.checked_add(limit.duration().saturating_sub(search.clock.elapsed))
    }
    fn retained_results(&self, plan: &Plan, origin: Address, limit: usize) -> Vec<CachedMatch> {
        let Some(previous) = self.search.as_ref().map(|search| &search.progress.results) else {
            return Vec::new();
        };
        previous
            .iter()
            .filter(|result| {
                let address = result.address.system();
                if address == origin.system()
                    || address.coordinates().galaxy != origin.coordinates().galaxy
                {
                    return false;
                }
                let Some(record) = self.cache.get(address) else {
                    return false;
                };
                let evaluation = record.evaluate(plan);
                (0..record.count()).any(|index| {
                    evaluation.matches & (1 << index) != 0
                        && record
                            .results(address, index, plan)
                            .iter()
                            .any(|candidate| candidate.target() == result.target())
                })
            })
            .take(limit)
            .cloned()
            .collect()
    }
    fn finish(&mut self, reason: Finish) {
        self.finish_at(reason, Instant::now());
    }
    fn finish_at(&mut self, reason: Finish, now: Instant) {
        let Some(search) = self
            .search
            .as_mut()
            .filter(|search| search.progress.phase == Phase::Running)
        else {
            return;
        };
        let mut elapsed = search.clock.observe(now, false);
        if reason == Finish::TimeLimit
            && let Some(limit) = search.progress.options.time_limit
        {
            elapsed = elapsed.min(limit.duration());
            search.clock.elapsed = elapsed;
        }
        search.progress.elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        search.finished_unix_ms = Some(unix_ms());
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
        if let Some(search) = self
            .search
            .as_mut()
            .filter(|search| search.progress.phase == Phase::Running)
        {
            search.progress.error = Some(error.chars().take(512).collect());
        }
        self.finish(Finish::Failed);
    }
    pub fn next(&mut self, limit: usize) -> Result<Work, &'static str> {
        self.observe_time();
        if limit == 0
            || self.is_paused()
            || self.search.as_ref().is_some_and(|search| !search.available)
        {
            return Ok(Work::Idle);
        }
        for _ in 0..limit.min(256) {
            self.observe_time();
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
        self.observe_time();
        if !self
            .progress()
            .is_some_and(|progress| progress.phase == Phase::Running)
        {
            return Ok(());
        }
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
            .filter(|result| result.system_name.is_none())
            .map(|result| result.address.system())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn unnamed_fauna_results(&self) -> Vec<crate::fauna::NameIdentity> {
        self.progress()
            .into_iter()
            .flat_map(|progress| &progress.results)
            .filter_map(|result| result.fauna.as_ref())
            .filter(|fauna| fauna.name.is_none())
            .map(|fauna| fauna.name_identity)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn name_system(&mut self, address: Address, name: &str) -> Result<(), &'static str> {
        if name.is_empty() || name.len() > crate::cache::MAX_NAME_BYTES {
            return Err("invalid result system name");
        }
        self.cache.name_system(address, name)?;
        if let Some(search) = self.search.as_mut() {
            for result in &mut search.progress.results {
                if result.address.system() == address.system() {
                    result.system_name = Some(name.into());
                }
            }
        }
        Ok(())
    }
    pub fn name_fauna(
        &mut self,
        identity: crate::fauna::NameIdentity,
        name: &str,
    ) -> Result<(), &'static str> {
        if name.is_empty() || name.len() > crate::cache::MAX_NAME_BYTES {
            return Err("invalid result fauna name");
        }
        if let Some(search) = self.search.as_mut() {
            for fauna in search
                .progress
                .results
                .iter_mut()
                .filter_map(|result| result.fauna.as_mut())
                .filter(|fauna| fauna.name_identity == identity)
            {
                fauna.name = Some(name.into());
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
        self.observe_time();
        if !self
            .progress()
            .is_some_and(|progress| progress.phase == Phase::Running)
        {
            // The copied record may outlive its deadline; it is safe cache data, not a result.
            return self.observe(address, record);
        }
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
            for result in record.results(address, index, &search.plan) {
                if search.progress.results.len() == search.progress.options.results {
                    break;
                }
                if search
                    .progress
                    .results
                    .iter()
                    .any(|previous| previous.target() == result.target())
                {
                    continue;
                }
                search.progress.results.push(result);
            }
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
    use crate::{address::Coordinates, criteria::PlanetFlag, facts::Facts, target::TargetKind};

    fn execution() -> ExecutionSettings {
        crate::settings::Settings::default().execution()
    }

    #[test]
    fn active_time_excludes_manual_and_automatic_pauses_and_preserves_manual_intent() {
        let now = Instant::now();
        let mut session = Session::default();
        session
            .start_at(SearchOptions::default(), address(1), execution(), now)
            .unwrap();
        session
            .set_paused_at(true, now + Duration::from_secs(5))
            .unwrap();
        session.set_available_at(false, now + Duration::from_secs(10));
        session.set_available_at(true, now + Duration::from_secs(20));
        assert!(session.is_paused());
        assert!(session.progress().unwrap().user_paused);
        assert_eq!(session.progress().unwrap().elapsed_ms, 5_000);
        session
            .set_paused_at(false, now + Duration::from_secs(30))
            .unwrap();
        session.observe_time_at(now + Duration::from_secs(35));
        assert_eq!(session.progress().unwrap().elapsed_ms, 10_000);
        session.set_available_at(false, now + Duration::from_secs(40));
        session.set_available_at(true, now + Duration::from_secs(80));
        session.observe_time_at(now + Duration::from_secs(85));
        assert_eq!(session.progress().unwrap().elapsed_ms, 20_000);
        session.finish_at(Finish::Cancelled, now + Duration::from_secs(90));
        session.observe_time_at(now + Duration::from_secs(200));
        session.finish_at(Finish::Failed, now + Duration::from_secs(250));
        assert_eq!(session.progress().unwrap().elapsed_ms, 25_000);
        assert_eq!(session.progress().unwrap().finish, Some(Finish::Cancelled));
    }

    #[test]
    fn deadline_moves_with_active_intervals_and_freezes_before_late_native_results() {
        let now = Instant::now();
        let mut session = Session::default();
        let options = SearchOptions {
            time_limit: Some(crate::settings::TimeLimit {
                hours: 0,
                minutes: 1,
            }),
            ..SearchOptions::default()
        };
        session
            .start_at(options, address(1), execution(), now)
            .unwrap();
        assert_eq!(session.deadline(), Some(now + Duration::from_secs(60)));
        session.set_available_at(false, now + Duration::from_secs(15));
        assert_eq!(session.deadline(), None);
        session.observe_time_at(now + Duration::from_secs(100));
        assert_eq!(session.progress().unwrap().phase, Phase::Running);
        session.set_available_at(true, now + Duration::from_secs(100));
        assert_eq!(session.deadline(), Some(now + Duration::from_secs(145)));
        session.observe_time_at(now + Duration::from_secs(144));
        assert_eq!(session.progress().unwrap().phase, Phase::Running);
        session.observe_time_at(now + Duration::from_secs(146));
        assert_eq!(session.progress().unwrap().finish, Some(Finish::TimeLimit));
        assert_eq!(session.progress().unwrap().elapsed_ms, 60_000);
        session.apply(address(2), record(true), false).unwrap();
        assert!(session.record(address(2)).is_some());
        assert!(session.progress().unwrap().results.is_empty());
        assert_eq!(session.progress().unwrap().systems, 0);
        session.fail("late worker failure");
        assert_eq!(session.progress().unwrap().finish, Some(Finish::TimeLimit));
        assert!(session.progress().unwrap().error.is_none());
    }

    #[test]
    fn committed_completion_wins_over_a_later_deadline_observation() {
        let now = Instant::now();
        let mut session = Session::default();
        let options = SearchOptions {
            time_limit: Some(crate::settings::TimeLimit {
                hours: 0,
                minutes: 1,
            }),
            ..SearchOptions::default()
        };
        session
            .start_at(options, address(1), execution(), now)
            .unwrap();
        session.finish_at(Finish::Results, now + Duration::from_secs(59));
        session.observe_time_at(now + Duration::from_secs(61));
        assert_eq!(session.progress().unwrap().finish, Some(Finish::Results));
        assert_eq!(session.progress().unwrap().elapsed_ms, 59_000);
    }

    #[test]
    fn history_is_bounded_independent_of_cache_and_authorizes_only_retained_search_targets() {
        let mut session = Session::default();
        let target_result = planet_result(address(2), "Recorded planet");
        let target = target_result.target();
        session
            .start(SearchOptions::default(), address(1), execution())
            .unwrap();
        let first_id = session.progress().unwrap().id;
        session
            .search
            .as_mut()
            .unwrap()
            .progress
            .results
            .push(target_result.clone());
        assert_eq!(
            session.search_result(first_id, target),
            Some(&target_result)
        );
        session.cancel();
        session.clear_cached_facts();
        session
            .start(SearchOptions::default(), address(1), execution())
            .unwrap();
        let second_id = session.progress().unwrap().id;
        assert!(second_id > first_id);
        assert_eq!(
            session.search_result(first_id, target),
            Some(&target_result)
        );
        assert!(session.search_result(second_id, target).is_none());
        assert!(session.search_result(0, target).is_none());
        let selected_snapshot = session.history()[0].clone();
        for _ in 0..MAX_SEARCH_HISTORY {
            session.cancel();
            session
                .start(SearchOptions::default(), address(1), execution())
                .unwrap();
        }
        assert_eq!(session.history().len(), MAX_SEARCH_HISTORY);
        assert!(session.search_result(first_id, target).is_none());
        assert_eq!(selected_snapshot.progress.results, vec![target_result]);
        let last_id = session.progress().unwrap().id;
        session.reset();
        assert!(session.history().is_empty());
        session
            .start(SearchOptions::default(), address(1), execution())
            .unwrap();
        assert!(session.progress().unwrap().id > last_id);
    }

    #[test]
    fn publication_shares_the_live_progress_with_its_history_record() {
        let mut session = Session::default();
        session
            .start(SearchOptions::default(), address(1), execution())
            .unwrap();
        let (progress, history) = session.publication();
        let progress = progress.unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &progress,
            &history.last().unwrap().progress
        ));
    }

    fn address(system: u16) -> Address {
        Address::new(Coordinates {
            x: 1,
            y: 2,
            z: 3,
            galaxy: 0,
            system,
            planet: 0,
        })
        .unwrap()
    }

    fn record(has_moons: bool) -> SystemRecord {
        let mut system = Facts::default();
        system.set_planet_count(1);
        let mut planets: [Facts; 8] = std::array::from_fn(|_| Facts::default());
        planets[0].set_planet_flag(PlanetFlag::Rings, true);
        planets[0].set_planet_flag(PlanetFlag::Moons, has_moons);
        SystemRecord::new(system, planets).unwrap()
    }

    fn planet_result(address: Address, name: &str) -> CachedMatch {
        CachedMatch {
            address: address.planet(0).unwrap(),
            kind: TargetKind::Planet,
            system_name: None,
            planet_name: Some(name.into()),
            fauna: None,
        }
    }

    #[test]
    fn starting_search_immediately_keeps_only_prior_results_matching_the_new_plan() {
        let origin = address(1);
        let retained_address = address(2);
        let removed_address = address(3);
        let uncached_address = address(4);
        let retained = planet_result(retained_address, "Retained");
        let removed = planet_result(removed_address, "Removed");
        let uncached = planet_result(uncached_address, "Uncached");
        let mut session = Session::default();
        session.observe(retained_address, record(true)).unwrap();
        session.observe(removed_address, record(false)).unwrap();

        let mut previous = SearchOptions::default();
        previous.criteria.planets.insert(PlanetFlag::Rings, true);
        session
            .start(
                previous,
                origin,
                crate::settings::Settings::default().execution(),
            )
            .unwrap();
        let progress = &mut session.search.as_mut().unwrap().progress;
        progress.phase = Phase::Completed;
        progress.results = vec![retained.clone(), removed, uncached];

        let mut next = SearchOptions::default();
        next.criteria.planets.insert(PlanetFlag::Rings, true);
        next.criteria.planets.insert(PlanetFlag::Moons, true);
        session
            .start(
                next,
                origin,
                crate::settings::Settings::default().execution(),
            )
            .unwrap();

        assert_eq!(session.progress().unwrap().results, vec![retained]);
        assert_eq!(session.progress().unwrap().systems, 0);
    }

    #[test]
    fn manual_pause_retains_and_resumes_the_active_frontier() {
        let mut session = Session::default();
        session
            .start(
                SearchOptions::default(),
                address(1),
                crate::settings::Settings::default().execution(),
            )
            .expect("start search");

        session.set_paused(true).expect("pause active search");
        assert!(session.is_paused());
        assert_eq!(session.next(256).expect("paused work"), Work::Idle);
        assert_eq!(session.progress().unwrap().systems, 0);

        session.set_paused(false).expect("resume active search");
        assert!(!session.is_paused());
        assert_ne!(session.next(256).expect("resumed work"), Work::Idle);
    }
}

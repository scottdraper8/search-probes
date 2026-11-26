use crate::{
    batch::{self, Job},
    region::RegionGenerator,
    search::Work,
    workers::Workers,
};
use search_probes_model::{
    address::Address,
    criteria::Plan,
    session::{Phase, Session, Work as Pending},
    settings::recommended_workers,
};
use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const CURRENT_WORLD_INSPECTION_LIMIT: Duration = Duration::from_secs(1);
const INITIAL_SYSTEM_COST_NS: [f64; 4] = [250_000.0, 1_000_000.0, 4_000_000.0, 12_000_000.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CostClass {
    System,
    Planet,
    Appearance,
    Fauna,
}

impl CostClass {
    fn for_plan(plan: &Plan) -> Self {
        let requirements = plan.requirements;
        if requirements.fauna {
            Self::Fauna
        } else if requirements.objects || requirements.colours != 0 {
            Self::Appearance
        } else if requirements.planet_query
            || requirements.hints
            || requirements.metadata
            || requirements.weather
            || requirements.terrain
            || requirements.screen_filter
            || !plan.planet.is_empty()
        {
            Self::Planet
        } else {
            Self::System
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

pub struct TickRequest<'a> {
    pub pool: &'a Workers,
    pub base: usize,
    pub live: *const u8,
    pub difficulty: usize,
    pub workers: Option<usize>,
    pub budget: Duration,
    pub cancel: &'a AtomicBool,
}

pub struct InspectRequest<'a> {
    pub pool: &'a Workers,
    pub base: usize,
    pub live: *const u8,
    pub difficulty: usize,
    pub address: Address,
    pub plan: &'a Plan,
    pub cancel: &'a AtomicBool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Statistics {
    pub work: Work,
    pub batches: u64,
    pub active_ns: u64,
    pub longest_batch_ns: u64,
    pub regions: u64,
}

pub struct Scheduler {
    pub session: Session,
    object_summaries: crate::object_cache::ObjectTableCache,
    statistics: Statistics,
    per_system_ns: [f64; 4],
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            session: Session::default(),
            object_summaries: crate::object_cache::ObjectTableCache::default(),
            statistics: Statistics::default(),
            per_system_ns: INITIAL_SYSTEM_COST_NS,
        }
    }
}

impl Scheduler {
    /// Inspect one loaded system without mutating the active search session.
    ///
    /// # Safety
    /// The loaded gameplay image and live configuration graph must remain valid and quiescent for
    /// the duration of the native query.
    pub unsafe fn inspect_current(
        &self,
        request: InspectRequest<'_>,
    ) -> Result<crate::search::Outcome, &'static str> {
        let InspectRequest {
            pool,
            base,
            live,
            difficulty,
            address,
            plan,
            cancel,
        } = request;
        if live.is_null() {
            return Err("null live planet generator");
        }
        let result = OnceLock::new();
        let live = live as usize;
        let deadline = Instant::now() + CURRENT_WORLD_INSPECTION_LIMIT;
        let dispatched = pool.run_count(1, |_| {
            let stop = crate::search::Stop::new(cancel, Some(deadline));
            let outcome = unsafe {
                crate::search::Generator::new(base, live as *const u8, &self.object_summaries)
            }
            .and_then(|mut generator| unsafe {
                generator.inspect_current(address, plan, difficulty, &stop)
            });
            let _ = result.set(outcome);
        });
        if dispatched.is_err() {
            return Err("current-world inspection worker failed");
        }
        result
            .into_inner()
            .unwrap_or(Err("current-world inspection worker did not complete"))
    }

    #[cfg(windows)]
    /// # Safety
    /// The loaded gameplay image and fauna naming resources must remain valid for the duration of
    /// this call.
    pub unsafe fn name_results(&mut self, base: usize) -> Result<(), &'static str> {
        for address in self.session.unnamed_system_results() {
            let name = unsafe { crate::system_name::generate(base, address) }?;
            self.session.name_system(address, &name)?;
            self.statistics.work.names += 1;
        }
        let identities = self.session.unnamed_fauna_results();
        if !identities.is_empty() {
            let context = unsafe { crate::fauna::Context::from_verified_image(base) }?;
            for identity in identities {
                let name = unsafe { context.name(identity.species_seed, identity.genus_seed) }?;
                self.session.name_fauna(identity, &name)?;
                self.statistics.work.names += 1;
            }
        }
        Ok(())
    }
    fn batch_limit(&self, workers: usize, budget: Duration, class: CostClass) -> usize {
        ((budget.as_nanos() as f64 / self.per_system_ns[class.index()]) as usize)
            .clamp(workers.clamp(1, batch::MAX_BATCH), batch::MAX_BATCH)
    }
    pub fn object_statistics(&self) -> crate::object_cache::Statistics {
        self.object_summaries.statistics()
    }
    pub fn reset(&mut self) {
        self.session.reset();
        self.object_summaries.clear();
        self.reset_statistics();
    }
    pub fn refresh_resources(&mut self) {
        self.session.clear_cached_facts();
        self.object_summaries.clear();
    }
    pub fn statistics(&self) -> Statistics {
        self.statistics
    }
    pub fn reset_statistics(&mut self) {
        self.statistics = Statistics::default();
        self.object_summaries.reset_statistics();
        self.per_system_ns = INITIAL_SYSTEM_COST_NS;
    }
    /// # Safety
    /// The gameplay callback must wait for the batch and preserve shared native configuration and
    /// resource lifetimes throughout it.
    pub unsafe fn tick(&mut self, request: TickRequest<'_>) -> Result<(), &'static str> {
        let TickRequest {
            pool,
            base,
            live,
            difficulty,
            workers,
            budget,
            cancel,
        } = request;
        self.session.observe_time();
        if cancel.load(Ordering::Acquire) {
            self.session.cancel();
            return Ok(());
        }
        let workers = workers
            .unwrap_or_else(|| recommended_workers(pool.count()))
            .min(pool.count());
        let started = Instant::now();
        let cost_class = self
            .session
            .plan()
            .map(CostClass::for_plan)
            .unwrap_or(CostClass::System);
        let limit = self.batch_limit(workers, budget, cost_class);
        let pending = self.session.next(limit)?;
        match pending {
            Pending::Idle => return Ok(()),
            Pending::Region(region) => {
                self.session.observe_time();
                if !self
                    .session
                    .progress()
                    .is_some_and(|progress| progress.phase == Phase::Running)
                {
                    return Ok(());
                }
                let generator = unsafe { RegionGenerator::from_verified_image(base) }?;
                let systems = unsafe { generator.generate(region) }?;
                self.session.load_region(
                    region,
                    systems
                        .into_iter()
                        .map(|row| search_probes_model::address::Address::from_raw(row.address))
                        .collect::<Result<Vec<_>, _>>()?,
                )?;
                self.statistics.regions += 1;
            }
            Pending::Systems(addresses) => {
                let jobs: Vec<_> = addresses
                    .iter()
                    .map(|&address| Job {
                        address,
                        cached: self.session.record(address),
                    })
                    .collect();
                let plan = self.session.plan().ok_or("search batch has no criteria")?;
                let completed = unsafe {
                    batch::evaluate(
                        pool,
                        batch::Evaluation {
                            object_summaries: &self.object_summaries,
                            base,
                            live,
                            jobs: &jobs,
                            plan,
                            difficulty,
                            workers,
                            cancel,
                            deadline: self.session.deadline(),
                        },
                    )
                }?;
                self.session.observe_time();
                let mut native_systems = 0;
                let mut failure = None;
                for result in completed {
                    match result {
                        Ok(row) => {
                            native_systems += row.work.systems;
                            self.statistics.work += row.work;
                            if row.cancelled
                                || cancel.load(Ordering::Acquire)
                                || !self
                                    .session
                                    .progress()
                                    .is_some_and(|progress| progress.phase == Phase::Running)
                                || failure.is_some()
                            {
                                self.session.observe(row.address, row.record)?;
                            } else if let Err(error) =
                                self.session.apply(row.address, row.record, row.from_cache)
                            {
                                failure = Some(error);
                            }
                        }
                        Err(error) => {
                            failure.get_or_insert(error);
                        }
                    }
                }
                let elapsed = started.elapsed().as_nanos() as f64;
                if native_systems > 0 {
                    let estimate = &mut self.per_system_ns[cost_class.index()];
                    *estimate = *estimate * 0.5 + elapsed / native_systems as f64 * 0.5;
                }
                if cancel.load(Ordering::Acquire) {
                    self.session.cancel();
                } else if let Some(error) = failure
                    && self
                        .session
                        .progress()
                        .is_some_and(|progress| progress.phase == Phase::Running)
                {
                    self.session.fail(error);
                    return Err(error);
                }
            }
        }
        let elapsed = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.statistics.batches += 1;
        self.statistics.active_ns = self.statistics.active_ns.saturating_add(elapsed);
        self.statistics.longest_batch_ns = self.statistics.longest_batch_ns.max(elapsed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_probes_model::criteria::Requirements;

    fn plan(requirements: Requirements) -> Plan {
        Plan {
            system: Vec::new(),
            planet: Vec::new(),
            requirements,
        }
    }

    #[test]
    fn scheduler_keeps_independent_cost_estimates_for_search_shapes() {
        assert_eq!(
            CostClass::for_plan(&plan(Requirements::default())),
            CostClass::System
        );
        assert_eq!(
            CostClass::for_plan(&plan(Requirements {
                planet_query: true,
                ..Requirements::default()
            })),
            CostClass::Planet
        );
        assert_eq!(
            CostClass::for_plan(&plan(Requirements {
                colours: 1,
                ..Requirements::default()
            })),
            CostClass::Appearance
        );
        assert_eq!(
            CostClass::for_plan(&plan(Requirements {
                fauna: true,
                ..Requirements::default()
            })),
            CostClass::Fauna
        );
    }
}

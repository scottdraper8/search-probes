use crate::{
    batch::{self, Job},
    region::RegionGenerator,
    search::Work,
    workers::Workers,
};
use search_probes_model::session::{Phase, Session, Work as Pending};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

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
    per_system_ns: f64,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            session: Session::default(),
            object_summaries: crate::object_cache::ObjectTableCache::default(),
            statistics: Statistics::default(),
            per_system_ns: 1_000_000.0,
        }
    }
}

impl Scheduler {
    #[cfg(windows)]
    pub unsafe fn name_system_results(&mut self, base: usize) -> Result<(), &'static str> {
        for address in self.session.unnamed_system_results() {
            let name = unsafe { crate::system_name::generate(base, address) }?;
            self.session.name_system(address, &name)?;
            self.statistics.work.names += 1;
        }
        Ok(())
    }
    fn batch_limit(&self, workers: usize, budget: Duration) -> usize {
        ((budget.as_nanos() as f64 / self.per_system_ns) as usize)
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
        self.per_system_ns = 1_000_000.0;
    }
    /// # Safety
    /// This runs inside the verified gameplay callback. That callback must wait for the batch and
    /// preserve shared native configuration/resource lifetimes throughout it.
    pub unsafe fn tick(
        &mut self,
        pool: &Workers,
        base: usize,
        live: *const u8,
        difficulty: usize,
        workers: usize,
        budget: Duration,
        cancel: &AtomicBool,
    ) -> Result<(), &'static str> {
        if cancel.load(Ordering::Acquire) {
            self.session.cancel();
            return Ok(());
        }
        let started = Instant::now();
        let limit = self.batch_limit(workers, budget);
        let pending = self.session.next(limit)?;
        match pending {
            Pending::Idle => return Ok(()),
            Pending::Region(region) => {
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
                        &self.object_summaries,
                        base,
                        live,
                        &jobs,
                        plan,
                        difficulty,
                        workers,
                        cancel,
                    )
                }?;
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
                    self.per_system_ns =
                        self.per_system_ns * 0.5 + elapsed / native_systems as f64 * 0.5;
                }
                if cancel.load(Ordering::Acquire) {
                    self.session.cancel();
                } else if let Some(error) = failure {
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

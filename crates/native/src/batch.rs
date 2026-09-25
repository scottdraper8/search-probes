use crate::{
    search::{Generator, Work},
    workers::Workers,
};
use search_probes_model::{address::Address, cache::SystemRecord, criteria::Plan};
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub const MAX_BATCH: usize = 256;

pub struct Job<'a> {
    pub address: Address,
    pub cached: Option<&'a SystemRecord>,
}

pub struct Completed {
    pub address: Address,
    pub record: SystemRecord,
    pub work: Work,
    pub cancelled: bool,
    pub from_cache: bool,
}

/// # Safety
/// The caller holds loaded gameplay state and resources fixed until every worker has joined.
/// Live is the verified planet generator and query hooks remain active throughout the batch.
pub unsafe fn evaluate(
    pool: &Workers,
    object_summaries: &crate::object_cache::ObjectTableCache,
    base: usize,
    live: *const u8,
    jobs: &[Job<'_>],
    plan: &Plan,
    difficulty: usize,
    workers: usize,
    cancel: &AtomicBool,
) -> Result<Vec<Result<Completed, &'static str>>, &'static str> {
    if jobs.len() > MAX_BATCH
        || live.is_null()
        || !(1..=pool.count()).contains(&workers)
        || difficulty > 3
    {
        return Err("invalid native search batch");
    }
    let slots: Vec<OnceLock<Result<Completed, &'static str>>> =
        (0..jobs.len()).map(|_| OnceLock::new()).collect();
    let next = AtomicUsize::new(0);
    let live = live as usize;
    let result = pool.run_count(workers, |_| {
        let mut generator = None;
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(job) = jobs.get(index) else {
                break;
            };
            let outcome = if cancel.load(Ordering::Acquire) {
                Err("search cancelled before system generation")
            } else if let Some(cached) = job
                .cached
                .filter(|record| record.evaluate(plan).unknown == 0)
            {
                Ok(Completed {
                    address: job.address,
                    record: cached.clone(),
                    work: Work::default(),
                    cancelled: false,
                    from_cache: true,
                })
            } else {
                let generator = generator.get_or_insert_with(|| unsafe {
                    Generator::new(base, live as *const u8, object_summaries)
                });
                match generator {
                    Ok(generator) => unsafe {
                        generator.search(job.address, plan, difficulty, cancel, job.cached)
                    }
                    .and_then(|outcome| {
                        let (work, cancelled) = (outcome.work, outcome.cancelled);
                        Ok(Completed {
                            address: job.address,
                            record: outcome.into_record()?,
                            work,
                            cancelled,
                            from_cache: false,
                        })
                    }),
                    Err(error) => Err(*error),
                }
            };
            let _ = slots[index].set(outcome);
        }
    });
    if result.is_err() {
        return Err("native worker batch failed");
    }
    Ok(slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .unwrap_or(Err("worker did not complete its system"))
        })
        .collect())
}

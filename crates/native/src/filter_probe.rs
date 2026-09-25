use crate::{search::Generator, workers::Workers};
use search_probes_model::{address::Address, criteria::Criteria};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

static POOL: Mutex<Option<Workers>> = Mutex::new(None);

#[repr(C)]
pub struct FilterRow {
    pub mask: u32,
    pub status: i32,
    pub work: [u64; 6],
    pub error: [u8; 128],
}

impl FilterRow {
    fn failure(error: &str) -> Self {
        let mut row = Self {
            mask: 0,
            status: -1,
            work: [0; 6],
            error: [0; 128],
        };
        let length = error.len().min(127);
        row.error[..length].copy_from_slice(&error.as_bytes()[..length]);
        row
    }
}

/// # Safety
/// Requires enabled query hooks and a waiting gameplay callback holding game state fixed. Input
/// buffers are readable for their lengths; output is disjoint and writable for count rows.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_filter_batch(
    base: usize,
    live: *const u8,
    addresses: *const u64,
    count: usize,
    criteria: *const u8,
    criteria_len: usize,
    difficulty: usize,
    workers: usize,
    output: *mut FilterRow,
) -> i32 {
    if live.is_null()
        || addresses.is_null()
        || criteria.is_null()
        || output.is_null()
        || !(1..=64).contains(&count)
        || criteria_len > 65536
        || !(1..=2).contains(&workers)
        || difficulty > 3
    {
        return -1;
    }
    let Ok(criteria) = serde_json::from_slice::<Criteria>(unsafe {
        std::slice::from_raw_parts(criteria, criteria_len)
    }) else {
        return -2;
    };
    let Ok(plan) = criteria.compile() else {
        return -3;
    };
    let Ok(mut pool) = POOL.lock() else {
        return -4;
    };
    if pool.is_none() {
        let Ok(new) = Workers::new(2) else {
            return -5;
        };
        *pool = Some(new);
    }
    let Some(pool) = pool.as_ref() else {
        return -5;
    };
    let addresses = unsafe { std::slice::from_raw_parts(addresses, count) }.to_vec();
    for index in 0..count {
        unsafe {
            output
                .add(index)
                .write(FilterRow::failure("worker not completed"));
        }
    }
    let (live, output) = (live as usize, output as usize);
    let next = AtomicUsize::new(0);
    let cancel = AtomicBool::new(false);
    let summaries = crate::object_cache::ObjectTableCache::default();
    let result = pool.run(|worker| {
        if worker >= workers {
            return;
        }
        let mut generator = unsafe { Generator::new(base, live as *const u8, &summaries) };
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(address) = addresses.get(index) else {
                break;
            };
            let value = match &mut generator {
                Ok(generator) => unsafe {
                    Address::from_raw(*address).and_then(|address| {
                        generator.search(address, &plan, difficulty, &cancel, None)
                    })
                },
                Err(error) => Err(*error),
            };
            let row = match value {
                Ok(outcome) => {
                    let work = outcome.work;
                    FilterRow {
                        mask: outcome
                            .matches
                            .iter()
                            .fold(0, |mask, result| mask | (1u32 << result.planet)),
                        status: 0,
                        work: [
                            work.systems,
                            work.planets,
                            work.metadata_captures,
                            work.colour_captures,
                            work.object_scans,
                            work.names,
                        ],
                        error: [0; 128],
                    }
                }
                Err(error) => FilterRow::failure(error),
            };
            unsafe {
                (output as *mut FilterRow).add(index).write(row);
            }
        }
    });
    if result.is_ok() { 0 } else { -6 }
}

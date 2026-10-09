use crate::{
    capture, capture_session,
    object_flags::{ObjectFlags, ObjectTables},
    planet_query_context::PlanetQueryContext,
    query::{Bindings, QUERY_SIZE, Query},
    solar::SolarContext,
    workers::Workers,
};
use search_probes_model::address::Address;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

static POOL: Mutex<Option<Workers>> = Mutex::new(None);

/// # Safety
/// The waiting gameplay callback holds all borrowed game state fixed. Outputs are disjoint writable
/// arrays of count queries, count * 8 Facts/ObjectFlags, count statuses and thread IDs. Rust hook only.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_persistent_queries(
    base: usize,
    addresses: *const u64,
    count: usize,
    output: *mut u8,
    facts: *mut capture::Facts,
    flags: *mut ObjectFlags,
    statuses: *mut i32,
    threads: *mut u32,
    live: *const u8,
    workers: usize,
) -> i32 {
    if !(1..=64).contains(&count)
        || !(1..=2).contains(&workers)
        || addresses.is_null()
        || output.is_null()
        || facts.is_null()
        || flags.is_null()
        || statuses.is_null()
        || threads.is_null()
        || live.is_null()
    {
        return -1;
    }
    let Ok(mut pool) = POOL.lock() else {
        return -2;
    };
    if pool.is_none() {
        let Ok(new) = Workers::new(2) else {
            return -3;
        };
        *pool = Some(new);
    }
    let Some(pool) = pool.as_ref() else {
        return -3;
    };
    let addresses = unsafe { std::slice::from_raw_parts(addresses, count) }.to_vec();
    for i in 0..count {
        unsafe {
            statuses.add(i).write(-99);
        }
    }
    let (output, facts, flags, statuses, threads, live) = (
        output as usize,
        facts as usize,
        flags as usize,
        statuses as usize,
        threads as usize,
        live as usize,
    );
    let next = AtomicUsize::new(0);
    let result = pool.run(|worker| {
        if worker >= workers {
            return;
        }
        let setup = unsafe {
            (|| {
                Ok::<_, &'static str>((
                    Bindings::from_verified_image(base)?,
                    SolarContext::new(base)?,
                    PlanetQueryContext::borrow_query_configuration(base, live as *const u8)?,
                    ObjectTables::from_verified_image(base)?,
                ))
            })()
        };
        let Ok((bindings, mut solar, mut planet, objects)) = setup else {
            return;
        };
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(address) = addresses.get(index) else {
                break;
            };
            let query = unsafe {
                (|| {
                    let address = Address::from_raw(*address)?;
                    let mut query = Query::begin(&bindings, solar.as_mut_ptr(), address)?;
                    let generator = planet.as_mut_ptr();
                    let mut captured = 0;
                    capture_session::visit(
                        |live_planet| {
                            let copied = capture::capture(live_planet, generator, capture::ALL)?;
                            let object_flags = objects.flags(
                                live_planet,
                                &crate::object_cache::ObjectTableCache::default(),
                                0,
                            )?;
                            (facts as *mut capture::Facts)
                                .add(index * 8 + captured)
                                .write(copied);
                            (flags as *mut ObjectFlags)
                                .add(index * 8 + captured)
                                .write(object_flags);
                            captured += 1;
                            Ok(())
                        },
                        || {
                            for i in 0..query.planet_count() {
                                query.generate_planet(generator, i)?;
                            }
                            Ok::<_, &'static str>(())
                        },
                    )??;
                    if captured != query.planet_count() {
                        return Err("capture count mismatch");
                    }
                    if !planet.borrowed_configuration_unchanged() {
                        return Err("private configuration changed");
                    }
                    std::ptr::copy_nonoverlapping(
                        query.bytes().as_ptr(),
                        (output as *mut u8).add(index * QUERY_SIZE),
                        QUERY_SIZE,
                    );
                    Ok(captured as i32)
                })()
            };
            unsafe {
                (statuses as *mut i32).add(index).write(query.unwrap_or(-4));
                (threads as *mut u32).add(index).write(GetCurrentThreadId());
            }
        }
    });
    if result.is_ok() { 0 } else { -5 }
}

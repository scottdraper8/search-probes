use crate::query::{Bindings, QUERY_SIZE, Query};
use search_probes_model::address::Address;

/// # Safety
/// Requires a verified image, loaded gameplay callback and an output writable for capacity entries.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_region(
    base: usize,
    address: u64,
    output: *mut crate::region::RegionSystem,
    capacity: usize,
) -> i32 {
    if output.is_null() || capacity < 4095 {
        return -1;
    }
    let result = unsafe {
        Address::from_raw(address).and_then(|address| {
            crate::region::RegionGenerator::from_verified_image(base)?.generate(address)
        })
    };
    match result {
        Ok(rows) => {
            unsafe {
                std::ptr::copy_nonoverlapping(rows.as_ptr(), output, rows.len());
            }
            rows.len() as i32
        }
        Err(_) => -2,
    }
}

/// # Safety
/// The planet remains live and output is writable. Requires a verified image and gameplay lease.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_object_flags(
    base: usize,
    planet: *const u8,
    output: *mut crate::object_flags::ObjectFlags,
) -> i32 {
    if output.is_null() {
        return -1;
    }
    let result = unsafe {
        crate::object_flags::ObjectTables::from_verified_image(base).and_then(|tables| {
            tables.flags(planet, &crate::object_cache::ObjectTableCache::default())
        })
    };
    match result {
        Ok(flags) => {
            unsafe {
                output.write(flags);
            }
            0
        }
        Err(_) => -2,
    }
}

/// # Safety
/// The waiting gameplay callback must keep all shared query state fixed. Buffers must be disjoint
/// and writable for count queries, count * 8 Facts, and count statuses/thread IDs respectively.
/// The native Rust planet hook must be active; Python capture callbacks are excluded.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_parallel_collected_queries(
    base: usize,
    addresses: *const u64,
    count: usize,
    outputs: *mut u8,
    facts: *mut crate::capture::Facts,
    statuses: *mut i32,
    thread_ids: *mut u32,
    live_planet: *const u8,
    workers: usize,
) -> i32 {
    if !(1..=64).contains(&count)
        || !(1..=2).contains(&workers)
        || addresses.is_null()
        || outputs.is_null()
        || facts.is_null()
        || statuses.is_null()
        || thread_ids.is_null()
        || live_planet.is_null()
    {
        return -1;
    }
    let addresses = unsafe { std::slice::from_raw_parts(addresses, count) }.to_vec();
    let outputs = outputs as usize;
    let facts = facts as usize;
    let statuses = statuses as usize;
    let thread_ids = thread_ids as usize;
    let live_planet = live_planet as usize;
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        let mut result = 0;
        for worker in 0..workers {
            let next = &next;
            let addresses = &addresses;
            let handle = std::thread::Builder::new()
                .name(format!("sqn-batch-audit-{worker}"))
                .spawn_scoped(scope, move || {
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(address) = addresses.get(index) else {
                            break;
                        };
                        unsafe {
                            let status = sqn_collected_query(
                                base,
                                *address,
                                live_planet as *const u8,
                                (outputs as *mut u8).add(index * QUERY_SIZE),
                                QUERY_SIZE,
                                (facts as *mut crate::capture::Facts).add(index * 8),
                                8 * std::mem::size_of::<crate::capture::Facts>(),
                            );
                            (statuses as *mut i32).add(index).write_unaligned(status);
                            (thread_ids as *mut u32)
                                .add(index)
                                .write_unaligned(GetCurrentThreadId());
                        }
                    }
                });
            match handle {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    result = -2;
                    break;
                }
            }
        }
        for handle in handles {
            if handle.join().is_err() {
                result = -3;
            }
        }
        result
    })
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}

/// # Safety
/// Requires planet_hook::enable's image and lifecycle contracts, or joined workers when disabling.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_planet_hook(base: usize, enable: bool) -> i32 {
    let result = if enable {
        unsafe { crate::query_hooks::enable(base) }
    } else {
        unsafe { crate::query_hooks::disable() }
    };
    if result.is_ok() { 0 } else { -1 }
}

/// # Safety
/// Same hook lifecycle requirements as sqn_planet_hook; error must be writable for capacity bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_planet_hook_detailed(
    base: usize,
    enable: bool,
    error: *mut u8,
    capacity: usize,
) -> i32 {
    if error.is_null() || capacity == 0 {
        return -2;
    }
    unsafe {
        error.write(0);
    }
    let result = if enable {
        unsafe { crate::query_hooks::enable(base) }
    } else {
        unsafe { crate::query_hooks::disable() }
    };
    match result {
        Ok(()) => 0,
        Err(value) => {
            let message = format!("{value:?}");
            let len = message.len().min(capacity - 1);
            unsafe {
                std::ptr::copy_nonoverlapping(message.as_ptr(), error, len);
                error.add(len).write(0);
            }
            -1
        }
    }
}

/// # Safety
/// Requires the collected-query buffer contracts and a waiting gameplay callback that keeps the
/// query's game state unchanged. This is a bounded worker experiment, not a production backend.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_worker_collected_query(
    base: usize,
    address: u64,
    live_planet: *const u8,
    output: *mut u8,
    output_size: usize,
    facts: *mut crate::capture::Facts,
    facts_size: usize,
) -> i32 {
    let live_planet = live_planet as usize;
    let output = output as usize;
    let facts = facts as usize;
    let Ok(worker) = std::thread::Builder::new()
        .name("sqn-query-audit".into())
        .spawn(move || unsafe {
            sqn_collected_query(
                base,
                address,
                live_planet as *const u8,
                output as *mut u8,
                output_size,
                facts as *mut crate::capture::Facts,
                facts_size,
            )
        })
    else {
        return -12;
    };
    worker.join().unwrap_or(-13)
}

unsafe fn thread_runtime_state(base: usize) -> [u64; 8] {
    let vector: usize;
    unsafe {
        core::arch::asm!("mov {}, gs:[0x58]", out(reg) vector, options(nostack, readonly, preserves_flags));
        let index = ((base + 0x6e10c68) as *const u32).read() as usize;
        if vector == 0 || index >= 64 {
            return [index as u64, 0, 0, 0, 0, 0, 0, 0];
        }
        let module = (vector as *const usize).add(index).read();
        let crt = (vector as *const usize).read();
        if module == 0 || crt == 0 {
            return [index as u64, module as u64, 0, 0, 0, 0, 0, 0];
        }
        [
            index as u64,
            module as u64,
            ((module + 0x1940) as *const u8).read() as u64,
            ((module + 0x1944) as *const u32).read() as u64,
            ((module + 0x1948) as *const u32).read() as u64,
            ((crt + 0x218) as *const u32).read() as u64,
            ((base + 0x5292041) as *const u8).read() as u64,
            ((base + 0x500a050 + 0x38) as *const u32).read() as u64,
        ]
    }
}

/// # Safety
/// Caller verifies the mapped 7.06 image and provides sixteen writable u64 values. This probe
/// reads thread/allocator metadata only; it does not call native generation on its new thread.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_thread_runtime_probe(base: usize, output: *mut u64) -> i32 {
    if output.is_null() {
        return -1;
    }
    let main = unsafe { thread_runtime_state(base) };
    let Ok(worker) = std::thread::Builder::new()
        .name("sqn-thread-audit".into())
        .spawn(move || unsafe { thread_runtime_state(base) })
    else {
        return -2;
    };
    let Ok(worker) = worker.join() else {
        return -3;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(main.as_ptr(), output, 8);
        std::ptr::copy_nonoverlapping(worker.as_ptr(), output.add(8), 8);
    }
    0
}

/// # Safety
/// The current capture's live planet contract applies; otherwise the pointer is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_capture_current_planet(planet: *const u8) -> i32 {
    match unsafe { crate::capture_session::observe(planet) } {
        Ok(active) => i32::from(active),
        Err(_) => -1,
    }
}

/// # Safety
/// Same gameplay-lease and lifetime requirements as sqn_private_context_query. `facts` must be
/// writable storage for eight Facts. The planet destructor hook must call sqn_capture_current_planet.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_collected_query(
    base: usize,
    address: u64,
    live_planet: *const u8,
    output: *mut u8,
    output_size: usize,
    facts: *mut crate::capture::Facts,
    facts_size: usize,
) -> i32 {
    if facts.is_null() || facts_size != 8 * std::mem::size_of::<crate::capture::Facts>() {
        return -9;
    }
    let Ok(mut planet) = (unsafe {
        crate::planet_query_context::PlanetQueryContext::borrow_query_configuration(
            base,
            live_planet,
        )
    }) else {
        return -7;
    };
    let generator = planet.as_mut_ptr();
    let Ok((result, captured)) = (unsafe {
        crate::capture_session::collect(generator, crate::capture::ALL, || {
            sqn_private_solar_query(base, address, generator, output, output_size)
        })
    }) else {
        return -10;
    };
    if !planet.borrowed_configuration_unchanged() {
        return -8;
    }
    if result < 1 {
        return result;
    }
    if captured.len() != result as usize {
        return -11;
    }
    for (index, value) in captured.into_iter().enumerate() {
        unsafe {
            facts.add(index).write_unaligned(value);
        }
    }
    result
}

/// # Safety
/// All sqn_private_context_query requirements apply. Audit output must be a writable 0x1820-byte
/// buffer; its copied pointer bytes are diagnostic values only and must not be dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_private_context_query_audit(
    base: usize,
    address: u64,
    live_planet: *const u8,
    output: *mut u8,
    output_size: usize,
    audit: *mut u8,
    audit_size: usize,
) -> i32 {
    if audit.is_null() || audit_size != 0x1820 {
        return -9;
    }
    let Ok(mut planet) = (unsafe {
        crate::planet_query_context::PlanetQueryContext::borrow_query_configuration(
            base,
            live_planet,
        )
    }) else {
        return -7;
    };
    let result =
        unsafe { sqn_private_solar_query(base, address, planet.as_mut_ptr(), output, output_size) };
    let (before, after) = planet.snapshots();
    unsafe {
        std::ptr::copy_nonoverlapping(before.as_ptr(), audit, before.len());
        std::ptr::copy_nonoverlapping(after.as_ptr(), audit.add(before.len()), after.len());
    }
    if !planet.borrowed_configuration_unchanged() {
        return -8;
    }
    result
}

/// # Safety
/// Requires the verified image, gameplay-lease and buffer contracts of sqn_explicit_query. Live
/// planet tables must remain valid and immutable until return. No object-resolution callback may
/// use the borrowed private context. Worker callers require a separately audited thread contract.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_private_context_query(
    base: usize,
    address: u64,
    live_planet: *const u8,
    output: *mut u8,
    output_size: usize,
) -> i32 {
    let Ok(mut planet) = (unsafe {
        crate::planet_query_context::PlanetQueryContext::borrow_query_configuration(
            base,
            live_planet,
        )
    }) else {
        return -7;
    };
    let result =
        unsafe { sqn_private_solar_query(base, address, planet.as_mut_ptr(), output, output_size) };
    if !planet.borrowed_configuration_unchanged() {
        return -8;
    }
    result
}

/// # Safety
/// Requires the same verified image, gameplay lease, live planet generator and writable output
/// as sqn_explicit_query. The private solar context is destroyed before this function returns.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_private_solar_query(
    base: usize,
    address: u64,
    planet: *mut u8,
    output: *mut u8,
    output_size: usize,
) -> i32 {
    let Ok(mut solar) = (unsafe { crate::solar::SolarContext::new(base) }) else {
        return -6;
    };
    unsafe {
        sqn_explicit_query(
            base,
            address,
            solar.as_mut_ptr(),
            planet,
            output,
            output_size,
        )
    }
}

/// # Safety
/// Same live planet/resource requirements as capture::capture. Output is writable, suitably
/// aligned storage for Facts. Investigation callers must check the exact output size.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_capture_planet(
    planet: *const u8,
    generator: *const u8,
    wanted: u32,
    output: *mut crate::capture::Facts,
    output_size: usize,
) -> i32 {
    if output.is_null() || output_size != std::mem::size_of::<crate::capture::Facts>() {
        return -1;
    }
    match unsafe { crate::capture::capture(planet, generator, wanted) } {
        Ok(value) => {
            unsafe {
                output.write_unaligned(value);
            }
            0
        }
        Err(_) => -2,
    }
}

/// # Safety
/// Caller verifies the executable hash and holds a gameplay lease with valid exclusive generators
/// and a writable QUERY_SIZE output buffer. Output contains raw diagnostic pointer bytes and may
/// only be decoded through fixed owned fields; none of its pointers may be dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn sqn_explicit_query(
    base: usize,
    address: u64,
    solar: *mut u8,
    planet: *mut u8,
    output: *mut u8,
    output_size: usize,
) -> i32 {
    if output.is_null() || output_size != QUERY_SIZE {
        return -1;
    }
    let Ok(address) = Address::from_raw(address) else {
        return -2;
    };
    let Ok(bindings) = (unsafe { Bindings::from_verified_image(base) }) else {
        return -3;
    };
    let Ok(mut query) = (unsafe { Query::begin(&bindings, solar, address) }) else {
        return -4;
    };
    for index in 0..query.planet_count() {
        if unsafe { query.generate_planet(planet, index) }.is_err() {
            return -5;
        }
    }
    unsafe {
        std::ptr::copy_nonoverlapping(query.bytes().as_ptr(), output, QUERY_SIZE);
    }
    query.planet_count() as i32
}

#[unsafe(no_mangle)]
pub extern "system" fn sqn_skipped_registrations() -> u64 {
    crate::registration_hook::skipped()
}

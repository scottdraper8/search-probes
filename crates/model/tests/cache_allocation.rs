use search_probes_model::{
    address::{Address, Coordinates},
    cache::{Cache, MAX_CACHED_SYSTEMS, MAX_NAME_BYTES, SystemRecord},
    facts::Facts,
    fauna::{Domain, Family, MAX_SPECIES, Species},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

struct MeasuredAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// Allocation and deallocation delegate unchanged to System; counters never allocate.
unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

#[test]
fn full_cache_and_repeated_eviction_stay_below_memory_budget() {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let mut cache = Cache::new(MAX_CACHED_SYSTEMS).unwrap();
    let name = "x".repeat(MAX_NAME_BYTES);
    let mut retained = Vec::new();
    for pass in 0..4 {
        for index in 0..MAX_CACHED_SYSTEMS {
            let unique = pass * MAX_CACHED_SYSTEMS + index;
            let address = Address::new(Coordinates {
                galaxy: 0,
                x: (unique / 4095) as i16,
                y: 0,
                z: 0,
                system: (unique % 4095 + 1) as u16,
                planet: 0,
            })
            .unwrap();
            let mut facts = Facts::default();
            facts.set_planet_count(8);
            let mut record =
                SystemRecord::new(facts, std::array::from_fn(|_| Facts::default())).unwrap();
            record.set_system_name(&name).unwrap();
            for planet in 0..8 {
                record.set_name(planet, &name).unwrap();
            }
            cache.insert(address, record).unwrap();
        }
        assert_eq!(cache.len(), MAX_CACHED_SYSTEMS);
        retained.push(LIVE.load(Ordering::Relaxed).saturating_sub(baseline));
    }
    let species = Species {
        family: Family::Cats,
        domain: Domain::Land,
        activity: 2,
        rarity: 1,
        cave: false,
        heights_cm: [125, 150],
    };
    for _ in 0..2 {
        for index in 0..MAX_CACHED_SYSTEMS {
            let address = Address::new(Coordinates {
                galaxy: 0,
                x: (index / 4095) as i16,
                y: 0,
                z: 0,
                system: (index % 4095 + 1) as u16,
                planet: 0,
            })
            .unwrap();
            let mut facts = Facts::default();
            facts.set_planet_count(8);
            let planets = std::array::from_fn(|_| {
                let mut planet = Facts::default();
                planet.set_fauna(vec![species; MAX_SPECIES]);
                planet
            });
            let record = SystemRecord::new(facts, planets).unwrap();
            cache.insert(address, record).unwrap();
        }
        assert!(
            cache.len() < MAX_CACHED_SYSTEMS,
            "species budget must evict populated records"
        );
        let last = cache.records().last().unwrap().1;
        assert_eq!(last.planets[0].fauna().unwrap().len(), MAX_SPECIES);
        retained.push(LIVE.load(Ordering::Relaxed).saturating_sub(baseline));
    }
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    assert!(peak < 64 * 1024 * 1024, "peak requested allocation: {peak}");
    drop(cache);
    let after_drop = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
    assert!(
        after_drop < 64 * 1024,
        "cache allocation retained: {after_drop}"
    );
    println!("retained_bytes={retained:?} peak_bytes={peak} after_drop_bytes={after_drop}");
}

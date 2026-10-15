//! M4 query budgets (Tester, written before M4): plan M4 DoD "Bounded work … budgets
//! recorded: Brașov query p95", Builder checklist §11 "each milestone states its time and
//! memory budget and tests a hostile input against it". The plan states no M4 query
//! budget, so this reuses M3's (≤ 1 s and ≤ 64 MiB of query heap per query, release);
//! Brașov keeps the DoD's ≤ 100 ms per query. Own test binary: counting global allocator.
//! `cargo test --locked --release -p rustline-cli --test m4_budget -- --include-ignored --nocapture --test-threads 1`

mod m4_common;

use m4_common::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

struct Counting;
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator and only updates two counters.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            let now = NOW.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        NOW.fetch_sub(l.size(), Relaxed);
    }
}

#[global_allocator]
static A: Counting = Counting;

const QUERY_HEAP: usize = 64 << 20;

/// Runs 1 000 queries (every 5th with garbage coordinates); returns (times, heap peaks).
fn run(e: &Engine, seed: u64) -> (Vec<Duration>, Vec<usize>) {
    let mut rng = Rng(seed);
    let (mut times, mut peaks) = (Vec::new(), Vec::new());
    for i in 0..1000 {
        let mut a = near_random_stop(e, &mut rng, 400.0);
        let b = near_random_stop(e, &mut rng, 400.0);
        if i % 5 == 4 {
            a = (f64::from_bits(rng.next()), (rng.unit() - 0.5) * 400.0);
        }
        let r = req(a, b, random_departure(e, &mut rng));
        let base = NOW.load(Relaxed);
        PEAK.store(base, Relaxed);
        let t0 = Instant::now();
        let res = e.plan(&r);
        times.push(t0.elapsed());
        peaks.push(PEAK.load(Relaxed).saturating_sub(base));
        drop(res);
    }
    times.sort();
    peaks.sort();
    (times, peaks)
}

fn report(name: &str, times: &[Duration], peaks: &[usize]) {
    let m = |b: usize| b as f64 / f64::from(1 << 20);
    let n = times.len();
    println!(
        "{name}: time p50 {:?} p95 {:?} max {:?}; query heap p50 {:.1} MiB p95 {:.1} MiB max {:.1} MiB",
        times[n / 2],
        times[n * 95 / 100],
        times[n - 1],
        m(peaks[n / 2]),
        m(peaks[n * 95 / 100]),
        m(peaks[n - 1])
    );
}

fn check(name: &str, osm: bool, seed: u64, max_time: Duration) {
    let d = TempDir::new("budget");
    let Some(p) = real_pack(name, osm, &d.0) else {
        return;
    };
    let e = engine(&p.pack);
    let (times, peaks) = run(&e, seed);
    report(name, &times, &peaks);
    assert!(
        *peaks.last().unwrap() <= QUERY_HEAP,
        "{name}: query heap {}",
        peaks.last().unwrap()
    );
    if !cfg!(debug_assertions) {
        assert!(
            *times.last().unwrap() <= max_time,
            "{name}: {:?}",
            times.last().unwrap()
        );
    }
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn brasov_door_to_door_budget() {
    check(
        "brasov-latest",
        true,
        0x4D34_B0D6_0001,
        Duration::from_millis(100),
    );
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn bucuresti_door_to_door_budget() {
    check("bucuresti", true, 0x4D34_B0D6_0002, Duration::from_secs(1));
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn mbta_without_osm_uses_the_fallback_within_budget() {
    // No OSM extract for Boston in the fixtures: straight-line fallback on 3 M+ stop_times.
    check("mbta", false, 0x4D34_B0D6_0003, Duration::from_secs(1));
}

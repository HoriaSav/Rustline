//! M3 budget test on MBTA (Builder checklist §11, DoD "Budgets measured in release").
//!
//! Own test binary because it installs a counting global allocator: the peak
//! heap growth *during* each query (above the bytes already held before the
//! call, i.e. excluding the loaded pack) is the "query state" memory. Also
//! prints the process peak RSS (VmHWM) for the report.
//!
//! `cargo test --locked --release -p rustline-core --test m3_budget -- --ignored --nocapture`

mod m3_common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use m3_common::*;
use rustline_core::raptor_stop_to_stop;
use rustline_model::{Seconds, StopIdx};

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

fn vm_hwm_kib() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[test]
#[ignore = "slow: builds the MBTA pack and runs 1 000 queries; run in release, see README"]
fn mbta_1000_queries_under_1s_and_64_mib_each() {
    let Some(feed) = shared_feed("mbta") else {
        return;
    };
    let tt = pack(&feed);
    let from_date = valid_from(&feed);
    let n = tt.stop_count() as u64;
    let mut rng = Rng(0x3B7A_0000_2026_1014);
    let (mut times, mut peaks) = (Vec::new(), Vec::new());
    for _ in 0..1000 {
        let (a, b) = (StopIdx(rng.below(n) as u32), StopIdx(rng.below(n) as u32));
        let dep = Seconds(5 * 3600 + rng.below(18 * 3600) as u32);
        let date = from_date + chrono::Days::new(rng.below(30));
        let base = NOW.load(Relaxed);
        PEAK.store(base, Relaxed);
        let t0 = Instant::now();
        let js = raptor_stop_to_stop(&tt, a, b, date, dep, 8).expect("valid query");
        times.push(t0.elapsed());
        peaks.push(PEAK.load(Relaxed) - base);
        drop(js);
    }
    times.sort();
    peaks.sort();
    let mib = |b: usize| b as f64 / (1 << 20) as f64;
    println!(
        "mbta: {} stops; time median {:?} p95 {:?} max {:?}; query heap peak median {:.1} MiB p95 {:.1} MiB max {:.1} MiB; process VmHWM {:?} KiB",
        n, times[500], times[950], times[999], mib(peaks[500]), mib(peaks[950]), mib(peaks[999]), vm_hwm_kib()
    );
    assert!(
        peaks[999] <= 64 << 20,
        "a query used {:.1} MiB",
        mib(peaks[999])
    );
    if !cfg!(debug_assertions) {
        assert!(
            times[999] < Duration::from_secs(1),
            "slowest query {:?}",
            times[999]
        );
    }
}

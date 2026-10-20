//! M5 benchmark (plan M5 step 8, spec §12): 1 000 fixed door-to-door queries
//! (fixed seed) on a real feed, planned by `Engine::plan`.
//!
//! Feed: `RUSTLINE_BENCH_FEED` (path relative to the workspace), default
//! `fixtures/feeds/brasov-latest.zip`; the bench skips when the file is missing.
//! Run: `cargo bench --locked -p rustline-core --bench plan`
//!
//! Before the criterion run it prints p50/p95 of one pass over the 1 000 queries.
//! M8: the same queries once more as arrive-by (same instants as deadlines).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, Days, FixedOffset, TimeDelta};
use criterion::{criterion_group, criterion_main, Criterion};
use rustline_core::{Engine, PlanRequest, TimeSpec};
use rustline_model::LatLon;
use rustline_pack::Pack;

const SEED: u64 = 0x5255_534C_494E_4535;
const QUERIES: usize = 1000;

/// xorshift64*: small, fixed, no dependency.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
    }
}

fn feed() -> Option<PathBuf> {
    let rel = std::env::var("RUSTLINE_BENCH_FEED")
        .unwrap_or_else(|_| "fixtures/feeds/brasov-latest.zip".into());
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    p.exists().then_some(p)
}

fn pack_of(feed: &std::path::Path) -> Pack {
    let raw = rustline_gtfs::load(feed).expect("feed loads");
    let (tt, _) = rustline_gtfs::build_timetable(&raw).expect("feed builds");
    Pack::new(
        rustline_gtfs::pack_meta(&tt, [0; 32], "bench", 0),
        rustline_gtfs::try_timetable_data(&tt).expect("pack arrays"),
    )
    .expect("pack passes check()")
}

/// Random stop pairs at random instants of the feed's range, in its zone.
fn queries(e: &Engine, p: &Pack) -> Vec<PlanRequest> {
    let pts: Vec<LatLon> = (0..p.stop_count())
        .filter(|&i| p.stop_kind[i] != 1)
        .filter_map(|i| p.stop_pos(i))
        .map(|(lat, lon)| LatLon::new(lat, lon))
        .collect();
    let meta = p.meta();
    let days = ((meta.valid_to - meta.valid_from).num_days().max(1)) as u64;
    let mut rng = Rng(SEED);
    (0..QUERIES)
        .map(|_| {
            let d = meta.valid_from + Days::new(rng.below(days));
            let naive =
                d.and_hms_opt(0, 0, 0).unwrap() + TimeDelta::seconds(rng.below(86_400) as i64);
            let t: DateTime<FixedOffset> = rustline_core::local_instant(e.timezone(), naive);
            PlanRequest {
                from: pts[rng.below(pts.len() as u64) as usize],
                to: pts[rng.below(pts.len() as u64) as usize],
                time: TimeSpec::DepartAt(t),
                max_access_m: 100,
                ..PlanRequest::default()
            }
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let Some(path) = feed() else {
        println!("skip: feed not found; set RUSTLINE_BENCH_FEED (see README)");
        return;
    };
    let p = pack_of(&path);
    let e = Engine::new(p.clone()).expect("engine");
    let qs = queries(&e, &p);
    let arrive: Vec<PlanRequest> = qs
        .iter()
        .map(|q| {
            let TimeSpec::DepartAt(t) = q.time else {
                return q.clone();
            };
            PlanRequest {
                time: TimeSpec::ArriveBy(t),
                ..q.clone()
            }
        })
        .collect();
    for (name, qs) in [("plan", &qs), ("plan_arrive_by", &arrive)] {
        let mut one: Vec<Duration> = qs
            .iter()
            .map(|q| {
                let t0 = Instant::now();
                let _ = e.plan(q);
                t0.elapsed()
            })
            .collect();
        one.sort_unstable();
        let at = |pct: usize| one[(one.len() * pct).div_ceil(100) - 1];
        println!(
            "{name}: {QUERIES} queries, p50 {:?}, p95 {:?}",
            at(50),
            at(95)
        );
    }
    c.bench_function("plan_1000_queries", |b| {
        b.iter(|| {
            for q in &qs {
                let _ = std::hint::black_box(e.plan(std::hint::black_box(q)));
            }
        })
    });
    c.bench_function("plan_arrive_by_1000_queries", |b| {
        b.iter(|| {
            for q in &arrive {
                let _ = std::hint::black_box(e.plan(std::hint::black_box(q)));
            }
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench
}
criterion_main!(benches);

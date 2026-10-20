//! M5 property and budget tests (Tester, written before M5). Plan M5 step 8 + spec §15:
//! proptest, 1 000 cases, fixed seed, random (from, to, instant) on the real feed;
//! invariants: arrival ≥ departure, legs continuous, no boarding before the query,
//! never dominated by walking only, every time carries the feed zone's offset. DoD
//! "p95 ≤ 20 ms Brașov" with midnight-spanning queries (release only).
//! RAPTOR vs CSA stays in oracle_feature.rs (the CSA is single-day; see the report).
//! Needs `RUSTLINE_M5_FEED` (skips when unset). The gate and ignores were removed in M5.
//! Run: `RUSTLINE_M5_FEED=fixtures/feeds/brasov-latest.zip \
//!   cargo test --locked --release -p rustline-core --test m5_properties -- --nocapture`

mod m5_common;

#[cfg(not(debug_assertions))]
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset};
use m5_common::api::*;
use m5_common::*;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};

const SEED: [u8; 32] = *b"rustline-m5-proptest-fixed-seed!";

/// Stop positions (no stations: kind 1).
fn points(p: &rustline_pack::Pack) -> Vec<(f64, f64)> {
    (0..p.stop_count())
        .filter(|&i| p.stop_kind[i] != 1)
        .filter_map(|i| p.stop_pos(i))
        .collect()
}

fn instant(p: &rustline_pack::Pack, day: u32, secs: u32, off_q: i32) -> DateTime<FixedOffset> {
    let d = p.meta().valid_from + chrono::Days::new(u64::from(day));
    let utc = d.and_hms_opt(0, 0, 0).unwrap().and_utc()
        + chrono::TimeDelta::seconds(i64::from(secs))
        - chrono::TimeDelta::hours(2);
    utc.with_timezone(&FixedOffset::east_opt(off_q * 900).unwrap())
}

fn invariants(zone: &str, q: DateTime<FixedOffset>, v: &[It]) -> Result<(), TestCaseError> {
    for it in v {
        prop_assert!(it.arr >= it.dep, "{it:?}");
        prop_assert!(
            it.dep >= q || it.rides == 0,
            "leaves before the query {q}: {it:?}"
        );
        let mut last = it.dep;
        for (a, b) in &it.spans {
            prop_assert!(*a >= last, "gap goes back in time: {it:?}");
            prop_assert!(b >= a, "{it:?}");
            last = *b;
        }
        prop_assert_eq!(it.arr, last);
        for l in &it.legs {
            prop_assert!(l.3 >= q, "boards before the query {q}: {it:?}");
        }
        for t in it
            .spans
            .iter()
            .flat_map(|s| [s.0, s.1])
            .chain([it.dep, it.arr])
        {
            prop_assert_eq!(t.offset().local_minus_utc(), offset_at(zone, t), "{:?}", it);
        }
    }
    if let Some(walk) = v.iter().find(|i| i.rides == 0) {
        for it in v.iter().filter(|i| i.rides > 0) {
            prop_assert!(
                it.arr < walk.arr || it.dep > walk.dep,
                "{it:?} dominated by walking only {walk:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn proptest_1000_random_queries_on_the_real_feed() {
    let Some(feed) = env_feed(FEED_VAR) else {
        return;
    };
    let p = pack_of(&feed);
    let zone = p.meta().timezone.clone();
    let pts = points(&p);
    let days = (p.meta().valid_to - p.meta().valid_from).num_days() as u32;
    let e = engine(p.clone());
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 1000,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &SEED),
    );
    let n = pts.len();
    // Any day of the feed; any second of a 26 h window; any query offset −12..+14 h.
    let strat = (0..n, 0..n, 0..=days, 0u32..26 * 3600, -48i32..=56);
    let (ok, none) = (std::cell::Cell::new(0u32), std::cell::Cell::new(0u32));
    runner
        .run(&strat, |(a, b, day, secs, off)| {
            let q = instant(&p, day, secs, off);
            let r = plan(&e, &req(pts[a], pts[b], q));
            if is_date_outside(&r) {
                none.set(none.get() + 1);
                let d = local_date(&zone, q);
                prop_assert!(d < p.meta().valid_from || d > p.meta().valid_to, "{q}");
                return Ok(());
            }
            match r {
                Ok(v) => {
                    ok.set(ok.get() + 1);
                    invariants(&zone, q, &v)
                }
                Err(PlanError::NoRoute) | Err(PlanError::OutsideCoverage) => {
                    none.set(none.get() + 1);
                    Ok(())
                }
                Err(e) => Err(TestCaseError::fail(format!("{q}: {e:?}"))),
            }
        })
        .unwrap();
    let (ok, none) = (ok.get(), none.get());
    println!("proptest: {ok} routes, {none} without");
    assert!(
        ok > 300,
        "too few routes ({ok}) for the invariants to mean much"
    );
}

#[cfg(not(debug_assertions))]
fn p95(mut v: Vec<Duration>) -> Duration {
    v.sort_unstable();
    v[(v.len() * 95).div_ceil(100) - 1]
}

#[cfg(not(debug_assertions))]
fn timings(e: &Engine, p: &rustline_pack::Pack, hours: &[u32], seed: u64) -> Vec<Duration> {
    let pts = points(p);
    let mut rng = Rng(seed);
    let days = (p.meta().valid_to - p.meta().valid_from).num_days() as u64 - 2;
    let mut out = Vec::new();
    for i in 0..300 {
        let (a, b) = (
            rng.below(pts.len() as u64) as usize,
            rng.below(pts.len() as u64) as usize,
        );
        let h = hours[i % hours.len()];
        let q = instant(
            p,
            1 + rng.below(days) as u32,
            h * 3600 + rng.below(3600) as u32,
            8,
        );
        let r = req(pts[a], pts[b], q);
        let t0 = Instant::now();
        let _ = e.plan(&r);
        out.push(t0.elapsed());
    }
    out
}

#[test]
#[cfg(not(debug_assertions))]
fn brasov_p95_under_20_ms_including_midnight_queries() {
    // Hours in UTC+2 terms (instant() subtracts 2 h); 23 and 0 search D-1, D and D+1.
    let Some(feed) = env_feed(FEED_VAR) else {
        return;
    };
    let p = pack_of(&feed);
    let e = engine(p.clone());
    let night = timings(&e, &p, &[23, 0, 1], 0x4D35_0001);
    let day = timings(&e, &p, &[11, 12, 13], 0x4D35_0002);
    let (pn, pd) = (p95(night.clone()), p95(day.clone()));
    println!("brasov p95: night {pn:?}, midday {pd:?} (M4 baseline p95 2.09 ms)");
    let mut all = night;
    all.extend(day);
    assert!(p95(all) <= Duration::from_millis(20));
    if let Some(mbta) = env_feed(MBTA_VAR) {
        let p = pack_of(&mbta);
        let e = engine(p.clone());
        let pn = p95(timings(&e, &p, &[23, 0], 0x4D35_0003));
        let pd = p95(timings(&e, &p, &[12], 0x4D35_0004));
        println!("mbta p95: night {pn:?}, midday {pd:?} (printed only)");
    }
}

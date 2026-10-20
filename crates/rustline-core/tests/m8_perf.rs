//! M8 tests-first (Tester, 2026-10-20): plan M8 DoD "PC p95 of arrive-by within 1.2×
//! of depart-at p95 on Brașov and MBTA (fixed seeds)". A measured test (the DoD also
//! names criterion; this is the Tester's cross-check): the same 400 door-to-door
//! queries (between random stops, 1 km access, all hours) both ways, after a warm-up,
//! p95 compared. Release only (prints in debug).
//! Ungated in M8 (Builder removed `cfg rustline_m8` and the "M8 not implemented" ignores).
//! Run: `cargo test --locked --release -p rustline-core --test m8_perf -- --nocapture --test-threads=1`
#![allow(unexpected_cfgs)]

mod m8_common;
use chrono::Duration;
use m8_common::api8::*;
use m8_common::*;
use rustline_core::TimeSpec;
use std::time::Instant;

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[(v.len() * 95 / 100).min(v.len() - 1)]
}

fn measure(feed: &str) {
    let path = workspace(&format!("fixtures/feeds/{feed}"));
    if !path.is_file() {
        println!("skip: {} missing", path.display());
        return;
    }
    let p = pack_of(&path);
    let zone = p.meta().timezone.clone();
    let e = engine(pack_of(&path));
    let mut r = Rng(0x6d38);
    let stops: Vec<usize> = (0..p.stop_count())
        .filter(|&s| p.stop_pos(s).is_some())
        .collect();
    let day = p.meta().valid_from + Duration::days(2);
    let mut qs = Vec::new();
    while qs.len() < 400 {
        let (a, b) = (
            stops[r.below(stops.len() as u64) as usize],
            stops[r.below(stops.len() as u64) as usize],
        );
        let Some(t) = local(
            &zone,
            day.and_hms_opt(0, 0, 0).unwrap() + Duration::seconds(r.below(86_400) as i64),
        ) else {
            continue;
        };
        qs.push((pos(&p, a), pos(&p, b), t));
    }
    let ok = |kind: u8| -> usize {
        qs.iter()
            .filter(|(a, b, t)| {
                let mut rq = req(*a, *b, *t);
                rq.max_access_m = 1000;
                rq.time = if kind == 0 {
                    TimeSpec::DepartAt(*t)
                } else {
                    arrive_by(*t)
                };
                e.plan(&rq).is_ok()
            })
            .count()
    };
    let (ok_dep, ok_arr) = (ok(0), ok(1));
    println!("{feed}: answered depart-at {ok_dep}/400, arrive-by {ok_arr}/400");
    let run = |kind: u8| -> Vec<f64> {
        qs.iter()
            .map(|(a, b, t)| {
                let mut rq = req(*a, *b, *t);
                rq.max_access_m = 1000;
                rq.time = if kind == 0 {
                    TimeSpec::DepartAt(*t)
                } else {
                    arrive_by(*t)
                };
                let t0 = Instant::now();
                let _ = e.plan(&rq);
                t0.elapsed().as_secs_f64() * 1e3
            })
            .collect()
    };
    let _ = (run(0), run(1)); // warm-up
    let (dep, arr) = (p95(run(0)), p95(run(1)));
    println!(
        "{feed}: depart-at p95 {dep:.3} ms, arrive-by p95 {arr:.3} ms, ratio {:.2}",
        arr / dep
    );
    assert!(
        ok_arr * 2 >= ok_dep,
        "arrive-by answers too few queries to time it"
    );
    if cfg!(debug_assertions) {
        println!("skip: ratio asserted in release only");
    } else {
        assert!(
            arr <= 1.2 * dep,
            "{feed}: arrive-by p95 {arr:.3} > 1.2 × {dep:.3}"
        );
    }
}

#[test]
fn arrive_by_p95_within_1_2x_brasov() {
    measure("brasov-latest.zip");
}

#[test]
fn arrive_by_p95_within_1_2x_mbta() {
    measure("mbta.zip");
}

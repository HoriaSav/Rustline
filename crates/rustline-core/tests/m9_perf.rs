//! M9 DoD perf budget + safety worst case (release only for the asserts).
//! Window 0 p95 ≤ 1.2× the M8 (acf46b7) depart-at p95, measured in the same session
//! with this test's 400-query harness (seed 0x6d39, release) at acf46b7 on 2026-10-23
//! (RUSTLINE_M9_BASE_MS_<FEED>; default Brașov 0.951 ms, MBTA 16.571 ms). The M8 depart-late cost is accepted
//! (Savin, 2026-10-23). 30 min window p95 (depart-at and arrive-by) has an absolute limit: Brașov 10 ms, MBTA 80 ms. Worst case (depart-at and arrive-by, each < 2 s): MBTA, window = cap, max_transfers = hard max, 20 results;
//! time and peak RSS (VmHWM) printed.
//! Run: `cargo test --locked --release -p rustline-core --test m9_perf -- --nocapture --test-threads=1`
#![allow(unexpected_cfgs)]
mod m9_common;

mod m9 {
    use super::m9_common::api9::*;
    use super::m9_common::*;
    use chrono::Duration;
    use rustline_core::TimeSpec;
    use std::hint::black_box;
    use std::time::Instant;

    fn p95(mut v: Vec<f64>) -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[(v.len() * 95 / 100).min(v.len() - 1)]
    }

    fn base(var: &str, default: f64) -> f64 {
        std::env::var(var)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    fn measure(feed: &str, base_ms: f64, w30_max_ms: f64) {
        let path = workspace(&format!("fixtures/feeds/{feed}"));
        if !path.is_file() {
            println!("skip: {} missing", path.display());
            return;
        }
        let p = pack_of(&path);
        let zone = p.meta().timezone.clone();
        let e = engine(pack_of(&path));
        let mut r = Rng(0x6d39);
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
        let run_as = |w: u32, arrive: bool| -> Vec<f64> {
            qs.iter()
                .map(|(a, b, t)| {
                    let mut rq = window(req(*a, *b, *t), w);
                    rq.max_access_m = 1000;
                    rq.time = if arrive {
                        TimeSpec::ArriveBy(*t)
                    } else {
                        TimeSpec::DepartAt(*t)
                    };
                    let t0 = Instant::now();
                    let _ = black_box(e.plan(black_box(&rq)));
                    t0.elapsed().as_secs_f64() * 1e3
                })
                .collect()
        };
        let run = |w: u32| run_as(w, false);
        let _ = (run(0), run(1800));
        let (w0, w30) = (p95(run(0)), p95(run(1800)));
        let _ = run_as(1800, true);
        let a30 = p95(run_as(1800, true));
        println!("{feed}: arrive-by 30 min p95 {a30:.3} ms (limit {w30_max_ms:.0})");
        println!("{feed}: window 0 p95 {w0:.3} ms (M8 base {base_ms:.3}, ×{:.2}); 30 min p95 {w30:.3} ms (limit {w30_max_ms:.0})", w0 / base_ms);
        if cfg!(debug_assertions) {
            println!("skip: budget asserted in release only");
            return;
        }
        assert!(
            w0 <= 1.2 * base_ms,
            "{feed}: window 0 p95 {w0:.3} > 1.2 × M8 {base_ms:.3}"
        );
        assert!(
            a30 < w30_max_ms,
            "{feed}: arrive-by 30 min p95 {a30:.3} ≥ {w30_max_ms:.0} ms"
        );
        assert!(
            w30 < w30_max_ms,
            "{feed}: 30 min p95 {w30:.3} ≥ {w30_max_ms:.0} ms"
        );
    }

    #[test]
    fn budget_brasov() {
        measure(
            "brasov-latest.zip",
            base("RUSTLINE_M9_BASE_MS_BRASOV", 0.951),
            10.0,
        );
    }

    #[test]
    fn budget_mbta() {
        measure("mbta.zip", base("RUSTLINE_M9_BASE_MS_MBTA", 16.571), 80.0);
    }

    fn vm_hwm_kb() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines().find(|l| l.starts_with("VmHWM")).map(|l| {
                    l.split_whitespace()
                        .nth(1)
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0)
                })
            })
            .unwrap_or(0)
    }

    /// Many ties: the busiest stop pair (most departures) on MBTA at the morning peak.
    #[test]
    fn worst_case_mbta_timed_with_peak_rss() {
        let path = workspace("fixtures/feeds/mbta.zip");
        if !path.is_file() {
            println!("skip: mbta.zip missing");
            return;
        }
        let p = pack_of(&path);
        let zone = p.meta().timezone.clone();
        let e = engine(pack_of(&path));
        let mut count = vec![0u32; p.stop_count()];
        for &s in &p.pattern_stops {
            count[s as usize] += 1;
        }
        let mut by: Vec<usize> = (0..p.stop_count())
            .filter(|&s| p.stop_pos(s).is_some())
            .collect();
        by.sort_by_key(|&s| std::cmp::Reverse(count[s]));
        let day = p.meta().valid_from + Duration::days(2);
        let t = local(&zone, day.and_hms_opt(7, 0, 0).unwrap()).unwrap();
        let mut worst = 0.0f64;
        let mut worst_ab = 0.0f64;
        for (&(a, b), arrive) in [(by[0], by[1]), (by[0], by[5]), (by[2], by[3])]
            .iter()
            .flat_map(|x| [(x, false), (x, true)])
        {
            let mut rq = window(req(pos(&p, a), pos(&p, b), t), MAX_WINDOW_SECS);
            if arrive {
                rq.time = TimeSpec::ArriveBy(t);
            }
            rq.max_access_m = 3000;
            rq.max_transfers = MAX_TRANSFERS;
            rq.max_results = MAX_RESULTS;
            let t0 = Instant::now();
            let v = black_box(plan(&e, black_box(&rq)));
            let ms = t0.elapsed().as_secs_f64() * 1e3;
            if arrive {
                worst_ab = worst_ab.max(ms);
            } else {
                worst = worst.max(ms);
            }
            if let Ok(v) = &v {
                assert!(v.len() <= MAX_RESULTS as usize);
            }
        }
        println!(
            "mbta worst case: depart-at {worst:.1} ms, arrive-by {worst_ab:.1} ms, VmHWM {} MB",
            vm_hwm_kb() / 1024
        );
        if !cfg!(debug_assertions) {
            assert!(
                worst < 2000.0,
                "worst case {worst:.1} ms (Tester bound 2 s; plan gives none)"
            );
            assert!(
                worst_ab < 2000.0,
                "arrive-by worst case {worst_ab:.1} ms (bound 2 s, decision 7)"
            );
        }
    }
}

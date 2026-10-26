//! M4 on real feeds + the dated Romania OSM extract (Tester, written before M4).
//! Plan M4 DoD: foot graph stats (nodes, edges, components, pack size delta) for Brașov
//! and Cluj; unlinked stop count; release timing of 1 000 Brașov door-to-door queries;
//! never worse than walking (second seed, "Tester will check"); garbage coordinates;
//! `build-pack --osm` peak RSS for Brașov and București; whole-country extract.
//! Needs `scripts/fetch-fixtures.sh` (skips with a message otherwise). Slow: run
//! `cargo test --locked --release -p rustline-cli --test m4_real -- --include-ignored --nocapture --test-threads 1`

mod m4_common;

use m4_common::*;
use std::time::{Duration, Instant};

fn mib(kib: Option<u64>) -> u64 {
    kib.unwrap_or(0) >> 10
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn brasov_and_cluj_foot_graph_stats() {
    let d = TempDir::new("real_stats");
    for feed in ["brasov-latest", "cluj"] {
        let (Some(with), Some(without)) =
            (real_pack(feed, true, &d.0), real_pack(feed, false, &d.0))
        else {
            return;
        };
        println!(
            "== {feed}: bbox {}",
            bbox_of(&fixture(&format!("{feed}.zip")).unwrap(), "2")
        );
        println!("{}", with.output);
        println!(
            "{feed}: build {:?}, peak {} MiB; .rlp {} → {} bytes (delta {})",
            with.elapsed,
            mib(with.peak_kib),
            without.rlp_bytes,
            with.rlp_bytes,
            with.rlp_bytes as i64 - without.rlp_bytes as i64
        );
        let low = with.output.to_lowercase();
        for w in ["nodes", "edges", "component"] {
            assert!(low.contains(w), "{feed}: no '{w}'");
        }
        assert!(
            low.contains("unlinked") || low.contains("not linked"),
            "{feed}: no unlinked count"
        );
        assert!(with.rlp_bytes > without.rlp_bytes);
        let e = engine(&with.pack);
        assert!(has_street_graph(&e.info()));
    }
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn brasov_1000_queries_timing_and_never_worse_than_walking() {
    let d = TempDir::new("real_q");
    let Some(p) = real_pack("brasov-latest", true, &d.0) else {
        return;
    };
    let e = engine(&p.pack);
    for seed in [0x4D34_B5A0_0001u64, 0x4D34_B5A0_0002] {
        let mut rng = Rng(seed);
        let (mut times, mut checked, mut outside) = (Vec::new(), 0, 0);
        for _ in 0..1000 {
            let a = near_random_stop(&e, &mut rng, 300.0);
            let b = near_random_stop(&e, &mut rng, 300.0);
            let when = random_departure(&e, &mut rng);
            let r = req(a, b, when);
            let t0 = Instant::now();
            let res = plan(&e, &r);
            times.push(t0.elapsed());
            match res {
                Ok(_) | Err(PlanError::OutsideCoverage | PlanError::NoRoute) => {}
                Err(err) => panic!("{a:?}->{b:?} {when}: {err:?}"),
            }
            // Never worse than walking: a second point within ~2.5 km of `a`
            // (walk-only is offered up to 3 km, Leader M4 decision 6).
            let (dx, dy) = (
                (rng.unit() * 2.0 - 1.0) * 1_750.0,
                (rng.unit() * 2.0 - 1.0) * 1_750.0,
            );
            let near = (a.0 + dlat(dy), a.1 + dlon(dx, a.0));
            let r = req(a, near, when);
            let its = match plan(&e, &r) {
                Ok(its) => its,
                Err(PlanError::OutsideCoverage) => {
                    outside += 1;
                    continue;
                }
                Err(PlanError::NoRoute) => continue,
                Err(err) => panic!("{a:?}->{near:?} {when}: {err:?}"),
            };
            // Walk-only reference: same request with no access walking allowed.
            let mut w = r.clone();
            w.max_access_m = 0;
            if let Ok(walk) = plan(&e, &w) {
                if let Some(wa) = walk.iter().filter(|i| i.rides == 0).map(|i| i.arr).min() {
                    let best = its.iter().map(|i| i.arr).min().unwrap();
                    assert!(
                        best <= wa + chrono::Duration::seconds(1),
                        "{a:?}->{near:?} {when}: {best} > walk {wa}"
                    );
                    checked += 1;
                }
            }
        }
        times.sort();
        let (p50, p95, max) = (
            times[times.len() / 2],
            times[times.len() * 95 / 100],
            *times.last().unwrap(),
        );
        println!("brasov seed {seed:#x}: p50 {p50:?} p95 {p95:?} max {max:?}; walk-checked {checked}, outside {outside}");
        assert!(checked >= 100, "{checked}");
        if !cfg!(debug_assertions) {
            assert!(max <= Duration::from_millis(100), "max {max:?}");
        }
    }
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn brasov_garbage_coordinates() {
    let d = TempDir::new("real_g");
    let Some(p) = real_pack("brasov-latest", true, &d.0) else {
        return;
    };
    let e = engine(&p.pack);
    let mut rng = Rng(0x4D34_6A4B_B5A0);
    let mut worst = Duration::ZERO;
    for i in 0..1000u32 {
        let mut pt = near_random_stop(&e, &mut rng, 2000.0);
        match i % 5 {
            0 => pt.0 = f64::NAN,
            1 => pt.1 = f64::from_bits(rng.next()),
            2 => pt = ((rng.unit() - 0.5) * 360.0, (rng.unit() - 0.5) * 720.0),
            _ => {}
        }
        let other = near_random_stop(&e, &mut rng, 2000.0);
        let (a, b) = if i.is_multiple_of(2) {
            (pt, other)
        } else {
            (other, pt)
        };
        let t0 = Instant::now();
        let _ = e.plan(&req(a, b, random_departure(&e, &mut rng)));
        worst = worst.max(t0.elapsed());
    }
    println!("brasov garbage: worst {worst:?}");
    if !cfg!(debug_assertions) {
        assert!(worst <= Duration::from_millis(100));
    }
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn bucuresti_build_peak_rss() {
    // DoD: record `build-pack --osm` peak RSS for a large extract (București).
    let d = TempDir::new("real_buc");
    let Some(p) = real_pack("bucuresti", true, &d.0) else {
        return;
    };
    println!("{}", p.output);
    println!(
        "bucuresti --osm: {:?}, peak {} MiB, .rlp {} bytes",
        p.elapsed,
        mib(p.peak_kib),
        p.rlp_bytes
    );
    assert!(has_street_graph(&engine(&p.pack).info()));
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn whole_country_extract_without_bbox() {
    // Edge-case block: huge bbox (whole country) → within the memory cap or a clear error.
    let (Some(gtfs), Some(pbf)) = (
        fixture("brasov-latest.zip"),
        fixture("romania-260101.osm.pbf"),
    ) else {
        return;
    };
    let d = TempDir::new("real_country");
    let out = d.0.join("out");
    let args = [
        "build-pack",
        "--gtfs",
        gtfs.to_str().unwrap(),
        "--osm",
        pbf.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--feed-id",
        "country",
        "--built-at",
        "0",
    ];
    let (o, peak, dt) = run_with_peak(&args);
    let t = text(&o);
    println!(
        "whole Romania: success {}, {dt:?}, peak {} MiB\n{t}",
        o.status.success(),
        mib(peak)
    );
    assert_ne!(o.status.code(), Some(101), "panicked: {t}");
    assert!(!t.contains("panicked"), "{t}");
    if !o.status.success() {
        let low = t.to_lowercase();
        assert!(
            low.contains("limit") || low.contains("too large") || low.contains("cap"),
            "unclear error: {t}"
        );
    }
}

/// Distance (m) from `p` to the nearest street piece, scanning every segment of the
/// pack (no grid, no scan cap), with the engine's local flat projection around `p`.
fn brute_snap_m(t: &rustline_pack::TimetableData, p: (f64, f64)) -> f64 {
    const R: f64 = 6_371_008.8;
    let k = p.0.to_radians().cos();
    let xy = |lat: i32, lon: i32| {
        let (lat, lon) = (f64::from(lat) * 1e-7, f64::from(lon) * 1e-7);
        (
            (lon - p.1).to_radians() * k * R,
            (lat - p.0).to_radians() * R,
        )
    };
    let mut best = f64::INFINITY;
    for g in 0..t.seg_a.len() {
        let (a, b) = (t.seg_a[g] as usize, t.seg_b[g] as usize);
        let mut pts = vec![xy(t.street_lat[a], t.street_lon[a])];
        for s in t.shape_start[g] as usize..t.shape_start[g + 1] as usize {
            pts.push(xy(t.shape_lat[s], t.shape_lon[s]));
        }
        pts.push(xy(t.street_lat[b], t.street_lon[b]));
        for w in pts.windows(2) {
            let ((ax, ay), (bx, by)) = (w[0], w[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let l2 = dx * dx + dy * dy;
            let u = if l2 > 0.0 {
                (-(ax * dx + ay * dy) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            best = best.min((ax + u * dx).hypot(ay + u * dy));
        }
    }
    best
}

fn hav_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((b.1 - a.1).to_radians() / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().asin()
}

#[test]
#[ignore = "slow: needs fixtures/feeds/ (feeds + Romania OSM extract); run in release, see README"]
fn capped_snap_matches_a_brute_force_nearest_edge_scan() {
    // Tester (M4 final check): MAX_SNAP_SCAN must not change the snap on real
    // cities. 1 000 seeded points per feed (500 uniform in the stops' area, 500
    // within 400 m of a random stop). The engine's snap is read from a walk-only
    // trip to a point 1 m east (its line starts [point, snapped point, ...]);
    // OutsideCoverage must agree with "nothing within 300 m" by brute force.
    let d = TempDir::new("real_snap");
    for (feed, seed) in [
        ("brasov-latest", 0x4D34_5A00_0001u64),
        ("bucuresti", 0x4D34_5A00_0002),
    ] {
        let Some(p) = real_pack(feed, true, &d.0) else {
            return;
        };
        let e = engine(&p.pack);
        let t = e.pack().timetable();
        assert!(!t.seg_a.is_empty(), "{feed}: no street graph");
        let (mut lo, mut hi) = ((90.0f64, 180.0f64), (-90.0f64, -180.0f64));
        for s in 0..t.stop_count() {
            if let Some((la, lo_)) = t.stop_pos(s) {
                lo = (lo.0.min(la), lo.1.min(lo_));
                hi = (hi.0.max(la), hi.1.max(lo_));
            }
        }
        let mut rng = Rng(seed);
        let when = random_departure(&e, &mut rng);
        let (mut same, mut diff, mut outside_ok, mut unobserved, mut max_delta) =
            (0u32, 0u32, 0u32, 0u32, 0.0f64);
        let mut worst = String::new();
        for i in 0..1000 {
            let a = if i < 500 {
                (
                    lo.0 + rng.unit() * (hi.0 - lo.0),
                    lo.1 + rng.unit() * (hi.1 - lo.1),
                )
            } else {
                near_random_stop(&e, &mut rng, 400.0)
            };
            let b = (a.0, a.1 + dlon(1.0, a.0));
            let brute = brute_snap_m(t, a);
            match e.plan(&req(a, b, when)) {
                Err(PlanError::OutsideCoverage) => {
                    let bb = brute_snap_m(t, b);
                    assert!(
                        brute > 299.9 || bb > 299.9,
                        "{feed} {a:?}: OutsideCoverage but a street {brute:.2} m away"
                    );
                    outside_ok += 1;
                }
                Ok(r) => {
                    let line = r
                        .itineraries
                        .iter()
                        .find_map(|it| match it.legs.as_slice() {
                            [rustline_core::Leg::Walk {
                                geometry: Some(g), ..
                            }] if g.points().len() >= 2 => Some(g.points()[1]),
                            _ => None,
                        });
                    let Some(s) = line else {
                        unobserved += 1;
                        continue;
                    };
                    let got = hav_m(a, (s.lat, s.lon));
                    let delta = (got - brute).abs();
                    if delta > 0.05 {
                        diff += 1;
                    } else {
                        same += 1;
                    }
                    if delta > max_delta {
                        max_delta = delta;
                        worst = format!("{a:?}: engine {got:.3} m, brute {brute:.3} m");
                    }
                    assert!(brute <= 300.0 + 0.5, "{feed} {a:?}: snapped, brute {brute}");
                }
                Err(err) => {
                    // NoRoute etc.: the snap succeeded but is not visible.
                    let _ = err;
                    unobserved += 1;
                }
            }
        }
        println!(
            "{feed}: {} segments; snap vs brute: {same} same, {diff} differ (> 5 cm), \
             {outside_ok} OutsideCoverage confirmed, {unobserved} not observable; \
             max delta {max_delta:.3} m ({worst})",
            t.seg_a.len()
        );
        assert_eq!(
            diff, 0,
            "{feed}: capped snap differs from brute force; worst {worst}"
        );
        assert!(same >= 500, "{feed}: only {same} snaps observed");
    }
}

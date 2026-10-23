//! M4 door-to-door tests on the synthetic "line city" (m4_common), written by Tester
//! before M4. Plan M4 DoD (PlanRequest validation, ≥ 8 door-to-door goldens, never
//! worse than walking, max walk distance), edge-case block "Snapping and queries",
//! spec §8.1/§9 (walk-only rule, 3 min bonus), plan M3 note MF2 (`max_results`).
//! Expected times are hand-derived from the map (1.3 m/s, steps ×1.5,
//! 111 195 m per degree of latitude) and the timetable in m4_common.
//! The `cfg rustline_m4` gate and the "M4 not implemented" ignores were removed in M4.
//! Run: `cargo test --locked -p rustline-cli --test m4_plan -- --include-ignored`

// The PlanRequest field types (f32/f64, u8/u32) are not fixed by the plan: casts stay generic.
#![allow(clippy::unnecessary_cast)]

mod m4_common;

use m4_common::*;
use std::time::{Duration, Instant};

fn city() -> (CityPack, Engine) {
    let c = city_pack(true);
    let e = engine(&c.pack);
    (c, e)
}

fn t(h: u32, m: u32, s: u32) -> f64 {
    f64::from(h * 3600 + m * 60 + s)
}

fn arr(it: &It) -> f64 {
    secs(wed(), it.arr)
}

/// The arrival `got` (seconds after midnight) is `dep + walk` within 0.3 % of the walk
/// and 1.5 s (the relative part applies to the walk, not to the time of day).
fn dur_close(got: f64, dep: f64, walk: f64) -> bool {
    close(got - dep, walk, 0.003, 1.5)
}

/// (rides, arrival seconds) of every itinerary, sorted by rides.
fn summary(its: &[It]) -> Vec<(usize, f64)> {
    let mut v: Vec<_> = its.iter().map(|i| (i.rides, arr(i))).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn assert_valid(its: &[It], dep: f64) {
    for it in its {
        assert!(
            secs(wed(), it.dep) >= dep - 1.0,
            "leaves before the query: {it:?}"
        );
        assert!(it.arr >= it.dep, "{it:?}");
        let mut t = it.dep;
        for l in &it.legs {
            let (s, e) = match l {
                LegV::Walk { start, end, .. } => (*start, *end),
                LegV::Transit { board, alight, .. } => (*board, *alight),
            };
            assert!(
                s >= t - chrono::Duration::seconds(1) && e >= s,
                "legs out of order: {it:?}"
            );
            t = e;
        }
        assert!(
            (it.arr - t).num_seconds().abs() <= 1,
            "arrival != last leg end: {it:?}"
        );
    }
    // Pareto on (arrival, rides): nobody is dominated by (or ties with) another.
    for (i, a) in its.iter().enumerate() {
        for (k, b) in its.iter().enumerate() {
            if i != k {
                assert!(
                    !(a.arr <= b.arr && a.rides <= b.rides),
                    "{k} dominated by {i}: {its:?}"
                );
            }
        }
    }
}

// ============================ golden door-to-door (≥ 8) ============================

#[test]
fn golden_1_normal_trip_one_ride() {
    // P0 → P3 at 08:05: L1_0810 arrives 08:17. Walking 3 892 m would arrive 08:54:54.
    let (_c, e) = city();
    let its = plan(&e, &req(city::P0, city::P3, at(wed(), 8, 5, 0))).unwrap();
    assert_valid(&its, t(8, 5, 0));
    let best = its.iter().min_by(|a, b| a.arr.cmp(&b.arr)).unwrap();
    assert!(close(arr(best), t(8, 17, 0), 0.0, 1.0), "{its:?}");
    assert_eq!(best.trips(), ["L1_0810"]);
    assert!(
        its.iter().all(|i| i.rides >= 1),
        "walk-only is > 3 min slower: {its:?}"
    );
}

#[test]
fn golden_2_walk_is_better_uc3() {
    // P0 → D (500.4 m north) at 08:05: walking arrives 08:11:24.9; the best ride
    // (L1_0810 to P0b 08:11 + 55.6 m) arrives 08:11:43, later with more rides → only walk.
    let (_c, e) = city();
    let its = plan(&e, &req(city::P0, city::D, at(wed(), 8, 5, 0))).unwrap();
    assert_valid(&its, t(8, 5, 0));
    let want = city::along(1.0, 1.0045) / WALK;
    assert_eq!(summary(&its).len(), 1, "{its:?}");
    assert_eq!(its[0].rides, 0);
    assert!(
        dur_close(arr(&its[0]), t(8, 5, 0), want),
        "{} vs {want}",
        arr(&its[0])
    );
    assert!(
        close(f64::from(its[0].walk_m), 500.4, 0.005, 2.0),
        "{}",
        its[0].walk_m
    );
}

#[test]
fn golden_3_walk_within_three_minutes_is_added() {
    // P0 → P1 at 08:00: walk 1 112 m arrives 08:14:15; L1_0810 arrives 08:12:00.
    // Spec §9: walk-only is added when walk_arrival ≤ best_transit + 3 min.
    let (_c, e) = city();
    let its = plan(&e, &req(city::P0, city::P1, at(wed(), 8, 0, 0))).unwrap();
    assert_valid(&its, t(8, 0, 0));
    let s = summary(&its);
    assert_eq!(s.len(), 2, "{its:?}");
    assert_eq!(s[0].0, 0);
    assert!(
        dur_close(s[0].1, t(8, 0, 0), city::along(1.0, 1.01) / WALK),
        "{s:?}"
    );
    assert_eq!(s[1], (1, t(8, 12, 0)), "{s:?}");
}

#[test]
fn golden_4_walk_more_than_three_minutes_slower_is_dropped() {
    // P0 → P1 at 08:05: walk would arrive 08:19:15, L1_0810 08:12:00 (7:15 earlier).
    let (_c, e) = city();
    let its = plan(&e, &req(city::P0, city::P1, at(wed(), 8, 5, 0))).unwrap();
    assert_valid(&its, t(8, 5, 0));
    assert_eq!(summary(&its), [(1, t(8, 12, 0))], "{its:?}");
}

#[test]
fn golden_5_two_rides_with_a_transfer() {
    // P0 → Q at 08:05: L1_0810 to P1 08:12, L2_0820 08:20 → Q 08:24.
    // Walking 3 336 m would arrive 08:47:46.
    let (_c, e) = city();
    let its = plan(&e, &req(city::P0, city::Q, at(wed(), 8, 5, 0))).unwrap();
    assert_valid(&its, t(8, 5, 0));
    let best = its.iter().min_by(|a, b| a.arr.cmp(&b.arr)).unwrap();
    assert!(close(arr(best), t(8, 24, 0), 0.0, 1.0), "{its:?}");
    assert_eq!(best.trips(), ["L1_0810", "L2_0820"]);
    assert_eq!(best.transfers, 1);
}

#[test]
fn golden_6_origin_equals_destination() {
    // Edge-case block: A == B → zero-length walk or empty itinerary, no error.
    let (_c, e) = city();
    let r = plan(&e, &req(city::P1, city::P1, at(wed(), 8, 0, 0))).expect("A == B is not an error");
    if let Some(it) = r.first() {
        assert!((it.arr - it.dep).num_seconds() <= 1, "{it:?}");
        assert_eq!(it.rides, 0);
    }
}

#[test]
fn golden_7_points_one_metre_apart() {
    let (_c, e) = city();
    let b = (city::P1.0 + dlat(1.0), city::P1.1);
    let its = plan(&e, &req(city::P1, b, at(wed(), 8, 0, 0))).unwrap();
    assert!(!its.is_empty());
    assert_eq!(its[0].rides, 0);
    assert!(arr(&its[0]) <= t(8, 0, 0) + 2.0, "{its:?}");
}

#[test]
fn golden_8_both_points_near_the_same_stop() {
    // 20 m north of P1 → 20 m south of P1: walk 40 m (30.8 s), no ride.
    let (_c, e) = city();
    let a = (city::P1.0 + dlat(20.0), city::P1.1);
    let b = (city::P1.0 - dlat(20.0), city::P1.1);
    let its = plan(&e, &req(a, b, at(wed(), 8, 11, 0))).unwrap();
    assert_eq!(summary(&its).len(), 1, "{its:?}");
    assert_eq!(its[0].rides, 0);
    assert!(dur_close(arr(&its[0]), t(8, 11, 0), 40.0 / WALK), "{its:?}");
}

#[test]
fn golden_9_point_far_from_any_street_is_outside_coverage() {
    // (1.0, 1.02) is 1.1 km from island A (dropped: no stop) and 2.2 km from the meridian.
    let (_c, e) = city();
    let far = (1.0, 1.02);
    for (a, b) in [(far, city::P1), (city::P1, far)] {
        let r = e.plan(&req(a, b, at(wed(), 8, 0, 0)));
        assert!(
            matches!(r, Err(PlanError::OutsideCoverage)),
            "{a:?}->{b:?}: {r:?}"
        );
    }
}

#[test]
fn golden_10_destination_on_the_stop_island() {
    // P0 → 55.6 m north of R at 08:35: L3_0840 to R 08:50, walk 55.6 m → 08:50:42.8.
    let (_c, e) = city();
    let d = (1.0015, 1.05);
    let its = plan(&e, &req(city::P0, d, at(wed(), 8, 35, 0))).unwrap();
    assert_valid(&its, t(8, 35, 0));
    assert_eq!(summary(&its).len(), 1, "{its:?}");
    assert_eq!(its[0].trips(), ["L3_0840"]);
    assert!(
        dur_close(arr(&its[0]), t(8, 50, 0), city::along(1.001, 1.0015) / WALK),
        "{its:?}"
    );
}

#[test]
fn golden_11_steps_cost_one_and_a_half() {
    // Spur at 1.005: 100 m residential + 100 m steps = 76.9 s + 115.4 s = 192.3 s.
    let (_c, e) = city();
    let its = plan(
        &e,
        &req(
            (city::SPUR, city::LON),
            city::spur_end(),
            at(wed(), 8, 0, 0),
        ),
    )
    .unwrap();
    assert_eq!(its[0].rides, 0);
    assert!(
        dur_close(arr(&its[0]), t(8, 0, 0), 100.0 / WALK + 150.0 / WALK),
        "{its:?}"
    );
}

// ============================ dates, errors ============================

#[test]
fn dates_outside_the_feed() {
    let (_c, e) = city();
    for d in [
        chrono::NaiveDate::from_ymd_opt(2029, 12, 31).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2031, 1, 1).unwrap(),
    ] {
        let r = e.plan(&req(city::P0, city::P3, at(d, 8, 0, 0)));
        assert!(matches!(r, Err(PlanError::DateOutsideFeed)), "{d}: {r:?}");
    }
}

#[test]
fn no_route_when_too_far_to_walk_and_no_service() {
    // (1.10, 1.0) → P0 at 23:00: 12.2 km on foot (over the 3 km walk-only limit of
    // spec §8.1 and the ~10 km A* cap of the DoD), no stop within 1 000 m of the origin.
    let (_c, e) = city();
    let r = e.plan(&req(city::on_m(1.10), city::P0, at(wed(), 23, 0, 0)));
    assert!(matches!(r, Err(PlanError::NoRoute)), "{r:?}");
}

// ============================ snapping ============================

#[test]
fn coverage_boundary_300_m() {
    // Plan step 4: > 300 m from any street → OutsideCoverage. Points due west of the
    // meridian at 1.06 (no other street nearby). 300.0 m itself: only no panic and a
    // stable answer (radius/metric differences decide it; see the report).
    let (_c, e) = city();
    let west = |m: f64| (1.06, city::LON - dlon(m, 1.06));
    let q = |p| e.plan(&req(p, city::P3, at(wed(), 8, 0, 0)));
    // 298 m still snaps (inside coverage), so OutsideCoverage would be wrong. NoRoute
    // is also accepted because nothing else reaches P3: walking only would be
    // 298 m to the street + 2 780 m along it = 3 078 m, over the 3 km walk-only cap
    // (WALK_ONLY_MAX_M; the walk to the street counts), and the stop near P3 is
    // beyond max_access_m (1 000 m default), so no ride can be used either.
    assert!(
        matches!(q(west(298.0)), Ok(_) | Err(PlanError::NoRoute)),
        "{:?}",
        q(west(298.0))
    );
    assert!(
        matches!(q(west(302.0)), Err(PlanError::OutsideCoverage)),
        "{:?}",
        q(west(302.0))
    );
    let a = format!("{:?}", q(west(300.0)));
    assert_eq!(a, format!("{:?}", q(west(300.0))));
}

#[test]
fn snapping_uses_the_nearest_edge_not_the_nearest_node() {
    // 250 m south of the middle of C1 (one 1.1 km segment): the nearest node is
    // ~610 m away, the edge 250 m. Must snap (no OutsideCoverage). Walk to Q:
    // 556 m along C1, plus the 250 m to the street if that is costed.
    let (_c, e) = city();
    let p = (city::C1 - dlat(250.0), 1.005);
    let its = plan(&e, &req(p, city::Q, at(wed(), 8, 0, 0))).expect("snaps to the edge");
    let a = arr(&its[0]) - t(8, 0, 0);
    let along = haversine_m((city::C1, 1.005), city::Q);
    assert!(
        a >= along / WALK - 1.5 && a <= (along + 250.0) / WALK * 1.01 + 1.5,
        "{a} s"
    );
}

#[test]
fn equidistant_point_gives_a_deterministic_answer() {
    // (1.022, 1.005) is 222.4 m from both C1 and C2.
    let (c, e) = city();
    let r = req((1.022, 1.005), city::Q, at(wed(), 8, 0, 0));
    let first = format!("{:?}", plan(&e, &r));
    for _ in 0..5 {
        assert_eq!(format!("{:?}", plan(&e, &r)), first);
    }
    assert_eq!(
        format!("{:?}", plan(&engine(&c.pack), &r)),
        first,
        "after reopening"
    );
}

#[test]
fn point_exactly_on_a_node() {
    let (_c, e) = city();
    let its = plan(&e, &req(city::P0B, city::D, at(wed(), 8, 0, 0))).unwrap();
    assert!(
        dur_close(arr(&its[0]), t(8, 0, 0), city::along(1.004, 1.0045) / WALK),
        "{its:?}"
    );
}

#[test]
fn stopless_island_is_dropped_and_outside_coverage() {
    // Edge-case block: small island kept only if it contains a stop.
    let (_c, e) = city();
    let r = e.plan(&req((1.001, 1.03), city::P1, at(wed(), 8, 0, 0)));
    assert!(matches!(r, Err(PlanError::OutsideCoverage)), "{r:?}");
}

#[test]
fn across_a_component_boundary_never_hangs() {
    // From island B (only R) to the main graph: no walk path exists; any answer is
    // fine (NoRoute, straight-line fallback) as long as it comes fast.
    let (_c, e) = city();
    for (a, b) in [((1.0015, 1.05), city::P3), (city::P3, (1.0015, 1.05))] {
        let t0 = Instant::now();
        let r = e.plan(&req(a, b, at(wed(), 8, 0, 0)));
        assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
        if let Err(err) = r {
            assert!(
                matches!(err, PlanError::NoRoute | PlanError::OutsideCoverage),
                "{err:?}"
            );
        }
    }
}

#[test]
fn unlinked_stop_is_reported_and_warned() {
    // U is 150 m from the meridian (> 100 m): listed in the build report; a plan
    // that uses it carries Warning::StopNotLinkedToStreets("U").
    let (c, e) = city();
    let token_u = c
        .output
        .split(|ch: char| !ch.is_alphanumeric())
        .any(|w| w == "U");
    assert!(
        token_u && c.output.to_lowercase().contains("link"),
        "{}",
        c.output
    );
    let r = req(city::u(), city::P3, at(wed(), 8, 0, 0));
    if let Ok(its) = plan(&e, &r) {
        let uses_u = its.iter().any(|i| {
            i.legs
                .iter()
                .any(|l| matches!(l, LegV::Transit { from_stop, .. } if from_stop == "U"))
        });
        if uses_u {
            assert!(warnings(&e, &r)
                .iter()
                .any(|w| w.contains("StopNotLinkedToStreets") && w.contains('U')));
        }
    }
}

// ============================ access limits ============================

#[test]
fn max_access_is_respected_at_the_boundary() {
    // Origin 500 m south of P0 on the meridian, to P3 at 08:00. P0 at 08:06:25,
    // L1_0810 → 08:17. 505 m allows it, 495 m doesn't (P0b is 945 m away).
    let (_c, e) = city();
    let mut r = req(city::south_500(), city::P3, at(wed(), 8, 0, 0));
    r.max_access_m = 505;
    let its = plan(&e, &r).unwrap();
    assert!(
        its.iter()
            .any(|i| i.rides == 1 && close(arr(i), t(8, 17, 0), 0.0, 1.0)),
        "{its:?}"
    );
    r.max_access_m = 495;
    match plan(&e, &r) {
        Ok(its) => assert!(its.iter().all(|i| i.rides == 0), "{its:?}"),
        Err(err) => assert!(matches!(err, PlanError::NoRoute), "{err:?}"),
    }
}

#[test]
fn max_access_zero_means_no_ride_from_a_point_off_the_stop() {
    let (_c, e) = city();
    let mut r = req(
        (city::P0.0 - dlat(100.0), city::LON),
        city::P3,
        at(wed(), 8, 0, 0),
    );
    r.max_access_m = 0;
    match plan(&e, &r) {
        Ok(its) => assert!(its.iter().all(|i| i.rides == 0), "{its:?}"),
        Err(err) => assert!(matches!(err, PlanError::NoRoute), "{err:?}"),
    }
}

#[test]
fn walking_only_trip_when_no_stop_is_within_reach() {
    // Edge-case block: walking-only trip (no stop within max_access_m).
    // (1.06) → (1.07) on the meridian: 1 112 m, nearest stop P3 2.8 km away.
    let (_c, e) = city();
    let its = plan(
        &e,
        &req(city::on_m(1.06), city::on_m(1.07), at(wed(), 8, 0, 0)),
    )
    .unwrap();
    assert_eq!(summary(&its).len(), 1);
    assert_eq!(its[0].rides, 0);
    assert!(dur_close(
        arr(&its[0]),
        t(8, 0, 0),
        city::along(1.06, 1.07) / WALK
    ));
}

// ============================ PlanRequest ============================

#[test]
fn plan_request_defaults_are_the_spec_defaults() {
    // Plan Q6 + spec §11.1: 1.3 m/s, 1 000 m access, 500 m transfer, 3 transfers, 5 results.
    let d = PlanRequest::default();
    assert!((d.walk_speed_mps as f64 - 1.3).abs() < 1e-6);
    assert_eq!(d.max_access_m, 1000);
    assert_eq!(d.max_transfer_m, 500);
    assert_eq!(d.max_transfers, 3);
    assert_eq!(d.max_results, 5);
}

#[test]
fn invalid_requests_are_rejected_without_panic() {
    // DoD: NaN/inf/out-of-range lat/lon, speed ≤ 0, absurd limits → InvalidRequest.
    let (_c, e) = city();
    let ok = req(city::P0, city::P3, at(wed(), 8, 0, 0));
    let mut cases: Vec<(&str, PlanRequest)> = Vec::new();
    for (name, p) in [
        ("lat NaN", (f64::NAN, 1.0)),
        ("lon NaN", (1.0, f64::NAN)),
        ("lat inf", (f64::INFINITY, 1.0)),
        ("lon -inf", (1.0, f64::NEG_INFINITY)),
        ("lat 91", (91.0, 1.0)),
        ("lat -90.0001", (-90.0001, 1.0)),
        ("lon 180.5", (1.0, 180.5)),
        ("lon -181", (1.0, -181.0)),
        ("1e308", (1e308, 1e308)),
    ] {
        let mut a = ok.clone();
        a.from.lat = p.0;
        a.from.lon = p.1;
        cases.push((name, a));
        let mut b = ok.clone();
        b.to.lat = p.0;
        b.to.lon = p.1;
        cases.push((name, b));
    }
    for (name, s) in [
        ("speed 0", 0.0f64),
        ("speed -1", -1.0),
        ("speed NaN", f64::NAN),
        ("speed inf", f64::INFINITY),
    ] {
        let mut a = ok.clone();
        a.walk_speed_mps = s as _;
        cases.push((name, a));
    }
    let mut a = ok.clone();
    a.max_access_m = u32::MAX;
    cases.push(("max_access u32::MAX", a));
    let mut a = ok.clone();
    a.max_transfer_m = u32::MAX;
    cases.push(("max_transfer u32::MAX", a));
    for (name, r) in cases {
        let res = e.plan(&r);
        assert!(
            matches!(res, Err(PlanError::InvalidRequest(_))),
            "{name}: {res:?}"
        );
    }
}

#[test]
fn poles_and_antimeridian_never_panic() {
    // Edge-case block: on the antimeridian/poles → InvalidRequest or OutsideCoverage.
    let (_c, e) = city();
    for p in [
        (90.0, 0.0),
        (-90.0, 0.0),
        (0.0, 180.0),
        (0.0, -180.0),
        (89.9999, 179.9999),
        (-89.9999, -179.9999),
    ] {
        for r in [
            req(p, city::P0, at(wed(), 8, 0, 0)),
            req(city::P0, p, at(wed(), 8, 0, 0)),
        ] {
            let res = e.plan(&r);
            assert!(
                matches!(
                    res,
                    Err(PlanError::InvalidRequest(_) | PlanError::OutsideCoverage)
                ),
                "{p:?}: {res:?}"
            );
        }
    }
}

#[test]
fn max_transfers_zero_and_255() {
    // max_transfers 0: only 1-ride (or walk) itineraries; 8 is the hard max, 255 invalid (M9).
    let (_c, e) = city();
    let mut r = req(city::P0, city::Q, at(wed(), 8, 5, 0));
    r.max_transfers = 0;
    if let Ok(its) = plan(&e, &r) {
        assert!(its.iter().all(|i| i.rides <= 1), "{its:?}");
    }
    r.max_transfers = 8;
    let its = plan(&e, &r).unwrap();
    assert!(its.iter().all(|i| i.rides <= 9), "{its:?}");
    // M9 decision 1: over the hard max is an error, no longer clamped.
    r.max_transfers = 255;
    let err = plan(&e, &r).unwrap_err();
    assert!(matches!(err, PlanError::InvalidRequest(_)), "{err:?}");
}

#[test]
fn max_results_caps_and_keeps_the_best_first() {
    // Plan M3 note MF2: `max_results` (including 0) is tested in M4.
    // golden_3's query has two itineraries.
    let (_c, e) = city();
    let mut r = req(city::P0, city::P1, at(wed(), 8, 0, 0));
    let all = plan(&e, &r).unwrap();
    assert_eq!(all.len(), 2);
    r.max_results = 1;
    let one = plan(&e, &r).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(
        one[0], all[0],
        "max_results keeps the first-ranked itinerary"
    );
    r.max_results = 0;
    match plan(&e, &r) {
        Ok(v) => assert!(v.is_empty(), "{v:?}"),
        Err(err) => assert!(matches!(err, PlanError::InvalidRequest(_)), "{err:?}"),
    }
    r.max_results = 20;
    assert_eq!(plan(&e, &r).unwrap(), all);
    // M9 decision 2: over the hard cap of 20 is an error.
    r.max_results = 255;
    let err = plan(&e, &r).unwrap_err();
    assert!(matches!(err, PlanError::InvalidRequest(_)), "{err:?}");
}

// ============================ properties ============================

/// Random OSM nodes of the main component (meridian, C1/C2 ends, spur).
fn main_nodes(osm: &Osm) -> Vec<(f64, f64)> {
    osm.nodes
        .iter()
        .map(|n| (n.lat, n.lon))
        .filter(|&(lat, lon)| lon < 1.02 && (0.99..=1.10).contains(&lat))
        .collect()
}

#[test]
fn never_worse_than_walking_1000_pairs() {
    // DoD: for 1 000 random pairs in coverage, best arrival ≤ walk-only arrival
    // (fixed seed). Walk oracle: Dijkstra on the reference graph; claims only when
    // the walk is ≤ 3 000 m (spec §8.1 walk-only limit).
    let (_c, e) = city();
    let osm = line_city_osm();
    let rg = RefGraph::from_osm(&osm);
    let nodes = main_nodes(&osm);
    let mut rng = Rng(0x4D34_0EA1);
    let mut checked = 0;
    for _ in 0..1000 {
        let a = nodes[rng.below(nodes.len() as u64) as usize];
        let b = nodes[rng.below(nodes.len() as u64) as usize];
        let dep = t(7, 30, 0) + rng.below(7200) as f64;
        let r = req(
            a,
            b,
            at(wed(), 0, 0, 0) + chrono::Duration::seconds(dep as i64),
        );
        let (Some(m), Some(w)) = (walk_metres(&rg, &osm, a, b), walk_secs(&rg, &osm, a, b)) else {
            continue;
        };
        let its = match plan(&e, &r) {
            Ok(its) => its,
            // Only legitimate when walking is over the walk-only limit and no ride helps.
            Err(PlanError::NoRoute) if m > 3000.0 => continue,
            Err(err) => panic!("{a:?}->{b:?} @{dep} (walk {m:.0} m): {err:?}"),
        };
        assert_valid(&its, dep);
        if m <= 3000.0 {
            let best = its.iter().map(arr).fold(f64::INFINITY, f64::min);
            assert!(
                best <= dep + w * 1.003 + 2.0,
                "{a:?}->{b:?} @{dep}: best {best}, walk {}",
                dep + w
            );
            checked += 1;
        }
    }
    assert!(checked > 300, "{checked}");
}

#[test]
fn one_ride_answers_match_a_brute_force_from_m3() {
    // Door-to-door cross-check: brute force = walk access (reference Dijkstra) × M3
    // `raptor_pareto` × walk egress, 1-ride journeys. Unless walking beats it, the
    // engine must return a 1-ride itinerary arriving exactly then (±2 s).
    let (_c, e) = city();
    let osm = line_city_osm();
    let rg = RefGraph::from_osm(&osm);
    let nodes = main_nodes(&osm);
    // Origins south of P1 and destinations north of P2 (plus everything else now and
    // then), so most pairs have a useful ride.
    let south: Vec<_> = nodes.iter().copied().filter(|p| p.0 < 1.012).collect();
    let north: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|p| p.0 > 1.025 && p.0 < 1.045)
        .collect();
    let mut rng = Rng(0x4D34_B417);
    let (mut compared, mut skipped) = (0, 0);
    for i in 0..600 {
        let pick = |v: &[(f64, f64)], rng: &mut Rng| v[rng.below(v.len() as u64) as usize];
        let (a, b) = match i % 4 {
            0 => (pick(&nodes, &mut rng), pick(&nodes, &mut rng)),
            1 => (pick(&north, &mut rng), pick(&south, &mut rng)),
            _ => (pick(&south, &mut rng), pick(&north, &mut rng)),
        };
        let dep = t(7, 50, 0) + rng.below(3000) as f64;
        let Ok(brute) = brute_one_ride(&e, &rg, &osm, a, b, wed(), dep, 1000.0) else {
            skipped += 1;
            continue;
        };
        let r = req(
            a,
            b,
            at(wed(), 0, 0, 0) + chrono::Duration::seconds(dep as i64),
        );
        let its = match (plan(&e, &r), brute) {
            (Ok(its), _) => its,
            (Err(PlanError::NoRoute), None) => continue,
            (Err(err), _) => panic!("{a:?}->{b:?} @{dep}: {err:?}, brute {brute:?}"),
        };
        let walk = walk_secs(&rg, &osm, a, b).map(|w| dep + w);
        // The oracle searches Wednesday's service only; since M5 the engine also
        // rides Thursday's trips (D+1), which arrive after midnight (no trip of
        // this feed runs past 24:00), so those are left out (Builder, M5).
        let one = its
            .iter()
            .filter(|i| i.rides == 1 && arr(i) < 86_400.0)
            .map(arr)
            .fold(f64::INFINITY, f64::min);
        if let Some(bt) = brute {
            if walk.is_none_or(|w| w > bt + 2.0) {
                assert!(
                    close(one, bt, 0.0, 2.0),
                    "{a:?}->{b:?} @{dep}: engine 1-ride {one}, brute {bt}, walk {walk:?}: {its:?}"
                );
                compared += 1;
            }
        } else {
            assert!(
                one.is_infinite(),
                "{a:?}->{b:?} @{dep}: engine found a 1-ride arriving {one}, brute force none"
            );
        }
    }
    println!("compared {compared}, skipped (rounding-sensitive) {skipped}");
    assert!(compared > 150, "{compared}");
}

#[test]
fn one_ride_answers_match_a_brute_force_over_days_d_minus_1_to_d_plus_1() {
    // Added by Builder (M5 review): the brute force above, run on the services of
    // Tuesday, Wednesday and Thursday shifted by one day (the line city is UTC, so
    // 86 400 s), for departures late on Wednesday, just after its midnight and in
    // the morning peak (where the test above finds Thursday rides).
    let (_c, e) = city();
    let osm = line_city_osm();
    let rg = RefGraph::from_osm(&osm);
    let nodes = main_nodes(&osm);
    let south: Vec<_> = nodes.iter().copied().filter(|p| p.0 < 1.012).collect();
    let north: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|p| p.0 > 1.025 && p.0 < 1.045)
        .collect();
    let mut rng = Rng(0x4D35_D3D3);
    let (mut compared, mut checked, mut next_day) = (0, 0, 0);
    'pairs: for i in 0..300 {
        let pick = |v: &[(f64, f64)], rng: &mut Rng| v[rng.below(v.len() as u64) as usize];
        let (a, b) = (pick(&south, &mut rng), pick(&north, &mut rng));
        let dep = match i % 3 {
            0 => t(22, 30, 0) + rng.below(5400) as f64,
            1 => rng.below(3600) as f64,
            _ => t(7, 50, 0) + rng.below(3000) as f64,
        };
        let mut brute: Option<f64> = None;
        for k in [-1i64, 0, 1] {
            let shift = 86_400.0 * k as f64;
            let day = wed() + chrono::Duration::days(k);
            match brute_one_ride(&e, &rg, &osm, a, b, day, dep - shift, 1000.0) {
                Err(()) => continue 'pairs,
                Ok(Some(x)) => brute = Some(brute.map_or(x + shift, |y: f64| y.min(x + shift))),
                Ok(None) => {}
            }
        }
        let r = req(
            a,
            b,
            at(wed(), 0, 0, 0) + chrono::Duration::seconds(dep as i64),
        );
        let its = match plan(&e, &r) {
            Ok(its) => its,
            Err(PlanError::NoRoute) if brute.is_none() => continue,
            Err(err) => panic!("{a:?}->{b:?} @{dep}: {err:?}, brute {brute:?}"),
        };
        let one = its
            .iter()
            .filter(|i| i.rides == 1)
            .map(arr)
            .fold(f64::INFINITY, f64::min);
        // Never better than the brute force over the three days …
        match brute {
            Some(bt) => assert!(one >= bt - 2.0, "{a:?}->{b:?} @{dep}: {one} < brute {bt}"),
            None => assert!(one.is_infinite(), "{a:?}->{b:?} @{dep}: {one}, brute none"),
        }
        checked += 1;
        // … and equal to it when the engine keeps a 1-ride or walking is not better.
        let walk = walk_secs(&rg, &osm, a, b).map(|w| dep + w);
        if let Some(bt) = brute {
            if one.is_finite() || walk.is_none_or(|w| w > bt + 2.0) {
                assert!(
                    close(one, bt, 0.0, 2.0),
                    "{a:?}->{b:?} @{dep}: engine 1-ride {one}, brute {bt}, walk {walk:?}"
                );
                compared += 1;
                next_day += usize::from(bt >= 86_400.0);
            }
        }
    }
    println!("checked {checked}, compared {compared} ({next_day} on Thursday)");
    assert!(
        checked > 150 && compared > 50 && next_day > 10,
        "{checked}, {compared}, {next_day}"
    );
}

#[test]
fn garbage_coordinates_never_panic_and_stay_fast() {
    // Edge-case block "Resources": 1 000 random pairs incl. NaN/inf/garbage.
    let (_c, e) = city();
    let mut rng = Rng(0x4D34_6A4B);
    let pick = |rng: &mut Rng| -> f64 {
        match rng.below(8) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => -f64::INFINITY,
            3 => f64::from_bits(rng.next()),
            4 => (rng.unit() - 0.5) * 400.0,
            _ => 0.99 + rng.unit() * 0.12,
        }
    };
    let mut worst = Duration::ZERO;
    for _ in 0..1000 {
        let a = (
            pick(&mut rng),
            if rng.below(2) == 0 {
                1.0
            } else {
                pick(&mut rng)
            },
        );
        let b = (
            pick(&mut rng),
            if rng.below(2) == 0 {
                1.0
            } else {
                pick(&mut rng)
            },
        );
        let dep = rng.below(30 * 3600) as i64;
        let t0 = Instant::now();
        let _ = e.plan(&req(
            a,
            b,
            at(wed(), 0, 0, 0) + chrono::Duration::seconds(dep),
        ));
        worst = worst.max(t0.elapsed());
    }
    println!("garbage: worst {worst:?}");
    if !cfg!(debug_assertions) {
        assert!(worst <= Duration::from_millis(100), "{worst:?}");
    }
}

// ============================ Tester (M4 verify) gap tests ============================

#[test]
fn coverage_298_m_point_snaps_and_its_walk_to_the_street_counts() {
    // Backs the 298 m NoRoute acceptance in coverage_boundary_300_m: the point
    // snaps (a destination 1 112 m up the meridian is reached on foot, costed as
    // 298 m straight to the street + 1 112 m along it), and the same trip from the
    // meridian itself to P3 (2 780 m ≤ 3 km) is a walk, so the 298 m → P3 NoRoute
    // comes from the snap leg pushing the walk over WALK_ONLY_MAX_M, not from snapping.
    let (_c, e) = city();
    let west = |m: f64| (1.06, city::LON - dlon(m, 1.06));
    let its = plan(&e, &req(west(298.0), city::on_m(1.07), at(wed(), 8, 0, 0))).unwrap();
    assert_eq!(its.len(), 1, "{its:?}");
    assert_eq!(its[0].rides, 0);
    let metres = 298.0 + city::along(1.06, 1.07);
    assert!(
        dur_close(arr(&its[0]), t(8, 0, 0), metres / WALK),
        "{its:?} vs {metres} m"
    );
    assert!(
        close(f64::from(its[0].walk_m), metres, 0.003, 2.0),
        "{its:?}"
    );
    let r = e.plan(&req(west(302.0), city::on_m(1.07), at(wed(), 8, 0, 0)));
    assert!(matches!(r, Err(PlanError::OutsideCoverage)), "{r:?}");
    let its = plan(&e, &req(city::on_m(1.06), city::P3, at(wed(), 8, 0, 0))).unwrap();
    assert_eq!(its[0].rides, 0, "{its:?}");
    assert!(dur_close(
        arr(&its[0]),
        t(8, 0, 0),
        city::along(1.035, 1.06) / WALK
    ));
}

#[test]
fn request_limits_are_inclusive_at_the_maximum() {
    // PlanRequest DoD: the documented maxima are accepted, one step over is
    // InvalidRequest (max_access_m 3 000, max_transfer_m 500, speed 3 m/s, results ≥ 1).
    let (_c, e) = city();
    let ok = req(city::P0, city::P3, at(wed(), 8, 0, 0));
    let invalid = |r: &PlanRequest| matches!(e.plan(r), Err(PlanError::InvalidRequest(_)));
    let mut a = ok.clone();
    a.max_access_m = 3000;
    assert!(!invalid(&a), "{:?}", e.plan(&a));
    a.max_access_m = 3001;
    assert!(invalid(&a));
    let mut a = ok.clone();
    a.max_transfer_m = 500;
    assert!(!invalid(&a));
    a.max_transfer_m = 501;
    assert!(invalid(&a));
    let mut a = ok.clone();
    a.walk_speed_mps = 3.0 as _;
    assert!(!invalid(&a));
    a.walk_speed_mps = 3.01 as _;
    assert!(invalid(&a));
    let mut a = ok.clone();
    a.max_results = 0;
    assert!(invalid(&a));
    a.max_results = 1;
    assert_eq!(plan(&e, &a).unwrap().len(), 1);
}

#[test]
fn walk_speed_scales_walking_time() {
    // 1 112 m walk-only trip at 2.6 m/s takes half as long as at 1.3 m/s.
    let (_c, e) = city();
    let mut r = req(city::on_m(1.06), city::on_m(1.07), at(wed(), 8, 0, 0));
    r.walk_speed_mps = 2.6 as _;
    let its = plan(&e, &r).unwrap();
    assert_eq!(its[0].rides, 0);
    assert!(
        dur_close(arr(&its[0]), t(8, 0, 0), city::along(1.06, 1.07) / 2.6),
        "{its:?}"
    );
}

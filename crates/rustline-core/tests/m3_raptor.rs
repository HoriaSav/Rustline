//! M3 (RAPTOR stop-to-stop) tests, written by Tester **before** M3 exists.
//!
//! Builder removed the M3 gating; changes made to follow Leader's M3
//! interface decisions are marked "M3 decision N".
//!
//! Run all of them:
//! `cargo test --locked --release -p rustline-core --test m3_raptor -- --include-ignored --nocapture`
//! (the RAPTOR vs CSA oracle tests are `#[ignore = "slow: ..."]`).
//!
//! Expected values come from data/fixtures/tiny (its stop_times.txt and README) or
//! from the small feeds written below; nothing is taken from an unknown source.

mod m3_common;

use std::time::{Duration, Instant};

use m3_common::*;
use rustline_core::{raptor_pareto, raptor_stop_to_stop, QueryError};
use rustline_model::{Journey, Leg, Seconds, StopIdx, TripIdx};

/// Weekday (Wednesday) inside the tiny feed: WK runs, WE doesn't.
fn wed() -> chrono::NaiveDate {
    date(2030, 1, 2)
}

fn plan(
    tt: &Tt,
    from: StopIdx,
    to: StopIdx,
    d: chrono::NaiveDate,
    dep: Seconds,
    k: u8,
) -> Vec<Journey> {
    // M3 decision 4: errors are `Err`; every query here is valid.
    let js = raptor_stop_to_stop(tt, from, to, d, dep, k).expect("valid query");
    assert_valid(tt, from, to, dep, &js);
    js
}

fn plan_ids(
    tt: &Tt,
    from: &str,
    to: &str,
    d: chrono::NaiveDate,
    dep: Seconds,
    k: u8,
) -> Vec<Journey> {
    plan(tt, stop(tt, from), stop(tt, to), d, dep, k)
}

fn transit_trips(tt: &Tt, j: &Journey) -> Vec<String> {
    j.legs
        .iter()
        .filter_map(|l| match l {
            Leg::Transit { trip, .. } => Some(trip_id(tt, *trip).to_string()),
            Leg::Walk { .. } => None,
        })
        .collect()
}

fn leg_ends(l: &Leg) -> (StopIdx, StopIdx) {
    match *l {
        Leg::Transit { from, to, .. } | Leg::Walk { from, to, .. } => (from, to),
    }
}

/// DoD "Every journey is valid" + "Pareto set" + rides/rounds bound, on every result.
fn assert_valid(tt: &Tt, from: StopIdx, to: StopIdx, dep: Seconds, js: &[Journey]) {
    for j in js {
        let q = format!("query {from:?}->{to:?} dep {} : {j:?}", dep.0);
        assert!(
            j.dep >= dep || j.legs.is_empty(),
            "leaves before the query time, {q}"
        );
        assert!(j.dep <= j.arr, "{q}");
        let rides = j
            .legs
            .iter()
            .filter(|l| matches!(l, Leg::Transit { .. }))
            .count();
        assert_eq!(usize::from(j.rides), rides, "rides != transit legs, {q}");
        assert!(
            j.rides <= 9,
            "more rides than the hard round cap allows, {q}"
        );
        // Leader (M3 answers): a parent station stands for its child platforms.
        let at = |s: StopIdx, q: StopIdx| s == q || tt.stop_parent[s.index()] == q.0;
        if from == to || (j.legs.is_empty() && (at(from, to) || at(to, from))) {
            continue;
        }
        assert!(!j.legs.is_empty(), "{q}");
        assert!(
            at(leg_ends(&j.legs[0]).0, from),
            "first leg not at origin, {q}"
        );
        assert!(
            at(leg_ends(j.legs.last().unwrap()).1, to),
            "last leg not at destination, {q}"
        );
        let mut t = j.dep;
        for (i, l) in j.legs.iter().enumerate() {
            if i > 0 {
                assert_eq!(
                    leg_ends(&j.legs[i - 1]).1,
                    leg_ends(l).0,
                    "legs not continuous, {q}"
                );
            }
            match *l {
                Leg::Transit {
                    route,
                    trip,
                    from: a,
                    to: b,
                    dep: d,
                    arr,
                } => {
                    assert!(d >= t && arr >= d, "times decrease, {q}");
                    assert_eq!(
                        tt.trip_route[trip.index()],
                        route.0,
                        "route != trip's route, {q}"
                    );
                    // Board at some position p with stop a and departure d, alight at a
                    // later position r with stop b and arrival arr (loop routes: any such pair).
                    let times = tt.trip_times(trip.index());
                    let ok = (0..times.len()).any(|p| {
                        times[p].0 == a.0
                            && times[p].2 == d.0
                            && (p + 1..times.len())
                                .any(|r| times[r].0 == b.0 && times[r].1 == arr.0)
                    });
                    assert!(ok, "transit leg times differ from the timetable, {q}");
                    t = arr;
                }
                Leg::Walk {
                    from: a,
                    to: b,
                    secs,
                } => {
                    // Tester (M3 fix): a walk is one transfers.txt row a → b (a ≠ b,
                    // type ≠ 3) with exactly its min_transfer_time, and never
                    // follows another walk (no chained hops, M3 decision 8).
                    let row = tt.transfers(a.index()).any(|e| {
                        tt.transfer_to[e] == b.0
                            && tt.transfer_type[e] != 3
                            && tt.transfer_secs[e] == secs
                    });
                    assert!(a != b && row, "walk leg is not a transfers.txt row, {q}");
                    assert!(
                        i == 0 || matches!(j.legs[i - 1], Leg::Transit { .. }),
                        "two walks in a row, {q}"
                    );
                    t = Seconds(t.0.saturating_add(secs));
                }
            }
        }
        assert_eq!(t, j.arr, "journey arr != last leg arrival, {q}");
    }
    for (i, a) in js.iter().enumerate() {
        for (k, b) in js.iter().enumerate() {
            if i != k {
                let dominated = a.arr <= b.arr && a.rides <= b.rides;
                assert!(
                    !dominated,
                    "journey {k} is dominated by (or ties with) journey {i}: {js:?}"
                );
            }
        }
    }
}

fn earliest(js: &[Journey]) -> u32 {
    js.iter().map(|j| j.arr.0).min().unwrap_or(u32::MAX)
}

// ============================ Acceptance: fixture tests ============================

#[test]
fn direct_ride_a_to_d() {
    let tt = tiny();
    let js = plan_ids(&tt, "A", "D", wed(), hms(8, 0, 0), 4);
    assert_eq!(js.len(), 1, "{js:?}");
    assert_eq!(js[0].arr, hms(8, 15, 0));
    assert_eq!(js[0].dep, hms(8, 0, 0));
    assert_eq!(js[0].rides, 1);
    assert_eq!(transit_trips(&tt, &js[0]), ["R1_0800"]);
}

#[test]
fn forced_transfer_e_to_d() {
    // R2_0700 E 07:00 → C 07:06 / B 07:12, then R1_0800 at C 08:10 or B 08:05 → D 08:15.
    let tt = tiny();
    let js = plan_ids(&tt, "E", "D", wed(), hms(7, 0, 0), 4);
    assert_eq!(js.len(), 1, "{js:?}");
    assert_eq!(js[0].arr, hms(8, 15, 0));
    assert_eq!(js[0].rides, 2);
    let t = transit_trips(&tt, &js[0]);
    assert_eq!(t[1], "R1_0800");
    assert!(t[0] == "R2_0700", "{t:?}");
}

#[test]
fn no_route_is_empty_not_error() {
    // D is only ever the last stop of R1: nothing leaves D.
    let tt = tiny();
    assert!(plan_ids(&tt, "D", "A", wed(), hms(6, 0, 0), 4).is_empty());
}

#[test]
fn origin_equals_destination_gives_zero_legs() {
    let tt = tiny();
    let js = plan_ids(&tt, "A", "A", wed(), hms(8, 0, 0), 4);
    assert_eq!(js.len(), 1, "expected one zero-leg journey: {js:?}");
    assert!(js[0].legs.is_empty());
    assert_eq!(
        (js[0].dep, js[0].arr, js[0].rides),
        (hms(8, 0, 0), hms(8, 0, 0), 0)
    );
}

#[test]
fn inactive_service_is_skipped() {
    let tt = tiny();
    // Saturday 2030-01-05: only R1_we_1000 (WE) runs A→D, 10:00 → 10:15.
    let sat = plan_ids(&tt, "A", "D", date(2030, 1, 5), hms(8, 0, 0), 4);
    assert_eq!(earliest(&sat), hms(10, 15, 0).0);
    assert_eq!(transit_trips(&tt, &sat[0]), ["R1_we_1000"]);
    // Holiday Tue 2030-01-01: WK removed, WE added (tiny README).
    let hol = plan_ids(&tt, "A", "D", date(2030, 1, 1), hms(8, 0, 0), 4);
    assert_eq!(earliest(&hol), hms(10, 15, 0).0);
}

/// Feed: trip T X 08:00 → Y 08:10 → Z 08:20, with pickup/drop-off types at Y.
fn pickup_feed(y_pickup: u8, y_dropoff: u8) -> TempFeed {
    let st = format!(
        "trip_id,arrival_time,departure_time,stop_id,stop_sequence,pickup_type,drop_off_type\n\
         T,08:00:00,08:00:00,X,1,0,1\nT,08:10:00,08:10:00,Y,2,{y_pickup},{y_dropoff}\nT,08:20:00,08:20:00,Z,3,1,0\n"
    );
    TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "T")])),
        ("stop_times.txt", &st),
    ])
}

#[test]
fn pickup_type_1_is_respected() {
    let f = pickup_feed(1, 0);
    let tt = f.tt();
    let d = date(2030, 3, 1);
    assert!(
        plan_ids(&tt, "Y", "Z", d, hms(7, 0, 0), 4).is_empty(),
        "boarded where pickup_type=1"
    );
    assert_eq!(
        earliest(&plan_ids(&tt, "X", "Y", d, hms(7, 0, 0), 4)),
        hms(8, 10, 0).0
    );
    assert_eq!(
        earliest(&plan_ids(&tt, "X", "Z", d, hms(7, 0, 0), 4)),
        hms(8, 20, 0).0
    );
}

#[test]
fn drop_off_type_1_is_respected() {
    let f = pickup_feed(0, 1);
    let tt = f.tt();
    let d = date(2030, 3, 1);
    assert!(
        plan_ids(&tt, "X", "Y", d, hms(7, 0, 0), 4).is_empty(),
        "alighted where drop_off_type=1"
    );
    assert_eq!(
        earliest(&plan_ids(&tt, "Y", "Z", d, hms(7, 0, 0), 4)),
        hms(8, 20, 0).0
    );
}

// ============================ Acceptance: Pareto / ranking ============================

/// Direct R: A 08:00 → C 09:00. Via B: R2a A 08:00 → B 08:10, R2b B 08:15 → C `via_arr`.
fn tradeoff_feed(via_arr: &str) -> TempFeed {
    let st = stop_times(&[
        ("direct", "08:00:00", "A"),
        ("direct", "09:00:00", "C"),
        ("leg1", "08:00:00", "A"),
        ("leg1", "08:10:00", "B"),
        ("leg2", "08:15:00", "B"),
        ("leg2", via_arr, "C"),
    ]);
    TempFeed::new(&[
        (
            "trips.txt",
            &trips(&[("R", "direct"), ("Q", "leg1"), ("Q", "leg2")]),
        ),
        ("stop_times.txt", &st),
    ])
}

#[test]
fn more_rides_kept_when_at_least_5_min_earlier() {
    let f = tradeoff_feed("08:50:00"); // 10 min earlier with 2 rides
    let tt = f.tt();
    let mut js = plan_ids(&tt, "A", "C", date(2030, 3, 1), hms(7, 0, 0), 4);
    js.sort_by_key(|j| j.rides);
    assert_eq!(js.len(), 2, "{js:?}");
    assert_eq!((js[0].rides, js[0].arr), (1, hms(9, 0, 0)));
    assert_eq!((js[1].rides, js[1].arr), (2, hms(8, 50, 0)));
}

#[test]
fn more_rides_dropped_when_under_5_min_earlier() {
    let f = tradeoff_feed("08:57:00"); // only 3 min earlier
    let tt = f.tt();
    let js = plan_ids(&tt, "A", "C", date(2030, 3, 1), hms(7, 0, 0), 4);
    assert_eq!(js.len(), 1, "{js:?}");
    assert_eq!((js[0].rides, js[0].arr), (1, hms(9, 0, 0)));
}

#[test]
fn pareto_tie_keeps_one_journey_with_lowest_trip_index() {
    // Two trips, same pattern, identical times. Trips are indexed by sorted id: "t1" < "t2".
    let st = stop_times(&[
        ("t2", "08:00:00", "A"),
        ("t2", "08:10:00", "B"),
        ("t1", "08:00:00", "A"),
        ("t1", "08:10:00", "B"),
    ]);
    let f = TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "t2"), ("R", "t1")])),
        ("stop_times.txt", &st),
    ]);
    let tt = f.tt();
    for _ in 0..2 {
        let js = plan_ids(&tt, "A", "B", date(2030, 3, 1), hms(7, 0, 0), 4);
        assert_eq!(js.len(), 1, "{js:?}");
        assert_eq!(transit_trips(&tt, &js[0]), ["t1"]);
    }
}

#[test]
fn equal_departures_at_a_stop_pick_the_earliest_arrival() {
    // At A 08:00 both R1_0800 (C 08:10) and R3_slow (C 08:20) leave; R3_express leaves 08:05 (C 08:15).
    let tt = tiny();
    let js = plan_ids(&tt, "A", "C", wed(), hms(8, 0, 0), 4);
    assert_eq!(earliest(&js), hms(8, 10, 0).0);
    assert_eq!(js.len(), 1, "{js:?}");
    assert_eq!(transit_trips(&tt, &js[0]), ["R1_0800"]);
}

// ============================ Edge cases: query input ============================

#[test]
fn out_of_range_stop_idx_is_empty_not_panic() {
    let tt = tiny();
    let n = tt.stop_count() as u32;
    for (a, b) in [
        (n, 0),
        (0, n),
        (u32::MAX, 0),
        (0, u32::MAX),
        (u32::MAX, u32::MAX),
    ] {
        // M3 decision 4: an unknown stop is an error, not an empty result.
        let js = raptor_stop_to_stop(&tt, StopIdx(a), StopIdx(b), wed(), hms(8, 0, 0), 4);
        assert!(
            matches!(js, Err(QueryError::UnknownStop { .. })),
            "{a}->{b}: {js:?}"
        );
    }
}

/// Service words covering the hand-made packs' window 1970-01-01 … 2030-12-31.
const WORDS: u32 = 22_280 / 64 + 1;

/// Pack with no stops at all, built by hand (passes the M2 `check()`).
/// Reviewer MF1: a `Pack` can't be changed after it is made, so tests edit
/// this `TimetableData` and then `seal()` it.
fn empty_pack() -> rustline_pack::TimetableData {
    let timetable = rustline_pack::TimetableData {
        pattern_stops_start: vec![0],
        pattern_trips_start: vec![0],
        stop_patterns_start: vec![0],
        service_words: WORDS,
        ..Default::default()
    };
    timetable
        .check()
        .expect("hand-made empty pack is consistent");
    timetable
}

/// M3 decision 2: a `Pack` needs a header; valid 1970-01-01 … 2030-12-31
/// so the dates used below are inside it. `Pack::new` runs `check()`.
fn seal(timetable: rustline_pack::TimetableData) -> Tt {
    let meta = rustline_pack::PackMeta {
        format_version: rustline_pack::FORMAT_VERSION,
        feed_sha256: [0; 32],
        osm_sha256: [0; 32],
        built_at_unix: 0,
        valid_from: date(1970, 1, 1),
        valid_to: date(2030, 12, 31),
        timezone: "UTC".into(),
        feed_id: "hand-made".into(),
    };
    Tt::new(meta, timetable).expect("hand-made pack passes check()")
}

#[test]
fn empty_pack_and_one_stop_pack() {
    let e = seal(empty_pack());
    // M3 decision 4: stop 0 does not exist in an empty pack → error.
    assert!(raptor_stop_to_stop(&e, StopIdx(0), StopIdx(0), wed(), hms(8, 0, 0), 4).is_err());
    assert!(raptor_stop_to_stop(&e, StopIdx(0), StopIdx(1), wed(), hms(8, 0, 0), 4).is_err());

    let mut one = empty_pack();
    one.stop_ids = vec!["only".into()];
    one.stop_names = vec!["Only".into()];
    one.stop_lat = vec![1f64.to_bits()];
    one.stop_lon = vec![1f64.to_bits()];
    one.stop_kind = vec![0];
    one.stop_parent = vec![u32::MAX];
    one.stop_patterns_start = vec![0, 0];
    one.check().expect("hand-made one-stop pack is consistent");
    let one = seal(one);
    let js = raptor_stop_to_stop(&one, StopIdx(0), StopIdx(0), wed(), hms(8, 0, 0), 4).unwrap();
    assert_eq!(
        js.len(),
        1,
        "origin == destination on a 1-stop pack: {js:?}"
    );
    assert!(js[0].legs.is_empty());
}

#[test]
fn extreme_departure_times_do_not_overflow() {
    let tt = tiny();
    let (e, b) = (stop(&tt, "E"), stop(&tt, "B"));
    for dep in [0, 86_399, 86_400, 108_000, 172_800, u32::MAX - 1, u32::MAX] {
        let js = plan(&tt, e, b, wed(), Seconds(dep), 4);
        if dep >= 86_400 {
            assert!(
                js.is_empty(),
                "dep {dep}: nothing leaves E after 23:50: {js:?}"
            );
        }
    }
    // dep 0: first R2 trip, E 07:00 → B 07:12.
    assert_eq!(
        earliest(&plan(&tt, e, b, wed(), Seconds(0), 4)),
        hms(7, 12, 0).0
    );
    // 23:50 night trip arrives after midnight on the same service day.
    assert_eq!(
        earliest(&plan(&tt, e, b, wed(), hms(23, 50, 0), 4)),
        hms(24, 10, 0).0
    );
    assert!(plan(&tt, e, b, wed(), hms(23, 59, 59), 4).is_empty());
}

#[test]
fn departure_after_last_trip_is_empty() {
    let tt = tiny();
    assert!(plan_ids(&tt, "A", "D", wed(), hms(9, 0, 1), 4).is_empty()); // R1_0900 left at 09:00
    assert!(plan_ids(&tt, "A", "D", date(2030, 1, 5), hms(10, 0, 1), 4).is_empty());
}

#[test]
fn max_transfers_0_1_255() {
    let tt = tiny();
    // E→D needs 2 rides (forced_transfer_e_to_d).
    assert!(plan_ids(&tt, "E", "D", wed(), hms(7, 0, 0), 0).is_empty());
    assert_eq!(
        earliest(&plan_ids(&tt, "E", "D", wed(), hms(7, 0, 0), 1)),
        hms(8, 15, 0).0
    );
    assert_eq!(
        earliest(&plan_ids(&tt, "E", "D", wed(), hms(7, 0, 0), 255)),
        hms(8, 15, 0).0
    );
    // Direct rides work with 0 transfers.
    assert_eq!(
        earliest(&plan_ids(&tt, "A", "D", wed(), hms(8, 0, 0), 0)),
        hms(8, 15, 0).0
    );
}

#[test]
fn max_transfers_255_is_clamped_to_the_hard_cap() {
    // Chain of 20 one-hop trips S00→S01→…→S20, each 10 min after the previous.
    // The plan's hard cap is "e.g. 8" transfers (exact value is Builder's choice):
    // the 20-ride journey must not be returned, and the query stays fast.
    let mut rows = Vec::new();
    let names: Vec<String> = (0..=20).map(|i| format!("S{i:02}")).collect();
    let tids: Vec<String> = (0..20).map(|i| format!("h{i:02}")).collect();
    let times: Vec<String> = (0..=40)
        .map(|i| format!("{:02}:{:02}:00", 8 + i * 5 / 60, (i * 5) % 60))
        .collect();
    for i in 0..20 {
        rows.push((tids[i].as_str(), times[2 * i].as_str(), names[i].as_str()));
        rows.push((
            tids[i].as_str(),
            times[2 * i + 1].as_str(),
            names[i + 1].as_str(),
        ));
    }
    let tr: Vec<(&str, &str)> = tids.iter().map(|t| ("R", t.as_str())).collect();
    let f = TempFeed::new(&[
        ("trips.txt", &trips(&tr)),
        ("stop_times.txt", &stop_times(&rows)),
    ]);
    let tt = f.tt();
    let t0 = Instant::now();
    let js = plan_ids(&tt, "S00", "S20", date(2030, 3, 1), hms(7, 0, 0), 255);
    assert!(t0.elapsed() < Duration::from_secs(1));
    assert!(
        js.is_empty(),
        "20 rides exceed any cap ≤ 19 transfers: {js:?}"
    );
    // 9 rides (8 transfers) must still be found: S00 → S09.
    let js = plan_ids(&tt, "S00", "S09", date(2030, 3, 1), hms(7, 0, 0), 255);
    assert_eq!(js.iter().map(|j| j.rides).min(), Some(9));
}

#[test]
fn dates_outside_the_feed_or_without_service() {
    let tt = tiny(); // valid 2030-01-01 … 2030-12-31
    for d in [
        date(2029, 12, 31),
        date(2031, 1, 1),
        date(1970, 1, 1),
        date(9999, 12, 31),
    ] {
        // M3 decision 4: a date outside the pack is an error.
        let r = raptor_stop_to_stop(&tt, stop(&tt, "A"), stop(&tt, "D"), d, hms(8, 0, 0), 4);
        assert!(
            matches!(r, Err(QueryError::DateOutsideFeed { .. })),
            "{d}: {r:?}"
        );
    }
    // Weekday-only feed on a Saturday: no service active.
    let cal = "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nS,1,1,1,1,1,0,0,20300101,20301231\n";
    let f = TempFeed::new(&[
        ("calendar.txt", cal),
        ("trips.txt", &trips(&[("R", "T")])),
        (
            "stop_times.txt",
            &stop_times(&[("T", "08:00:00", "X"), ("T", "08:10:00", "Y")]),
        ),
    ]);
    let tt = f.tt();
    assert!(plan_ids(&tt, "X", "Y", date(2030, 1, 5), hms(7, 0, 0), 4).is_empty());
    assert_eq!(
        earliest(&plan_ids(&tt, "X", "Y", date(2030, 1, 7), hms(7, 0, 0), 4)),
        hms(8, 10, 0).0
    );
}

#[test]
fn calendar_dates_only_and_empty_calendar_feeds() {
    let st = stop_times(&[("T", "08:00:00", "X"), ("T", "08:10:00", "Y")]);
    let tr = trips(&[("R", "T")]);
    let dates = "service_id,date,exception_type\nS,20300305,1\n";
    let empty_cal =
        "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n";
    // (a) calendar.txt present but empty; (b) no calendar.txt file at all.
    let a = TempFeed::new(&[
        ("calendar.txt", empty_cal),
        ("calendar_dates.txt", dates),
        ("trips.txt", &tr),
        ("stop_times.txt", &st),
    ]);
    let b = TempFeed::new(&[
        ("calendar_dates.txt", dates),
        ("trips.txt", &tr),
        ("stop_times.txt", &st),
    ]);
    let _ = std::fs::remove_file(b.0.join("calendar.txt"));
    for f in [&a, &b] {
        let tt = f.tt();
        assert_eq!(
            earliest(&plan_ids(&tt, "X", "Y", date(2030, 3, 5), hms(7, 0, 0), 4)),
            hms(8, 10, 0).0
        );
        // M3 decision 4: the feed is valid only on 2030-03-05, so the next day
        // is outside it (an error) rather than an empty result.
        let r = raptor_stop_to_stop(
            &tt,
            stop(&tt, "X"),
            stop(&tt, "Y"),
            date(2030, 3, 6),
            hms(7, 0, 0),
            4,
        );
        assert!(
            matches!(r, Err(QueryError::DateOutsideFeed { .. })),
            "{r:?}"
        );
    }
}

// ============================ Edge cases: timetable shape ============================

#[test]
fn stop_with_only_arrivals_or_no_patterns() {
    let tt = tiny();
    // D: only arrivals.
    assert!(plan_ids(&tt, "D", "B", wed(), hms(0, 0, 0), 4).is_empty());
    // F: in stops.txt, served by nothing.
    let stops = "stop_id,stop_name,stop_lat,stop_lon\nX,X,1.0,1.0\nY,Y,2.0,1.0\nF,F,3.0,1.0\n";
    let f = TempFeed::new(&[
        ("stops.txt", stops),
        ("trips.txt", &trips(&[("R", "T")])),
        (
            "stop_times.txt",
            &stop_times(&[("T", "08:00:00", "X"), ("T", "08:10:00", "Y")]),
        ),
    ]);
    let tt = f.tt();
    assert!(plan_ids(&tt, "F", "Y", date(2030, 3, 1), hms(7, 0, 0), 4).is_empty());
    assert!(plan_ids(&tt, "X", "F", date(2030, 3, 1), hms(7, 0, 0), 4).is_empty());
}

#[test]
fn stop_served_only_by_no_pickup_or_no_drop_off() {
    // pickup_feed: X has drop_off_type=1 (only boarding), Z has pickup_type=1 (only alighting).
    let f = pickup_feed(0, 0);
    let tt = f.tt();
    let d = date(2030, 3, 1);
    assert!(plan_ids(&tt, "Z", "X", d, hms(7, 0, 0), 4).is_empty());
    assert_eq!(
        earliest(&plan_ids(&tt, "X", "Z", d, hms(7, 0, 0), 4)),
        hms(8, 20, 0).0
    );
}

#[test]
fn separate_network_component_is_unreachable() {
    let st = stop_times(&[
        ("a", "08:00:00", "P1"),
        ("a", "08:10:00", "P2"),
        ("b", "08:00:00", "Q1"),
        ("b", "08:10:00", "Q2"),
    ]);
    let f = TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "a"), ("Q", "b")])),
        ("stop_times.txt", &st),
    ]);
    let tt = f.tt();
    assert!(plan_ids(&tt, "P1", "Q2", date(2030, 3, 1), hms(7, 0, 0), 8).is_empty());
}

#[test]
fn loop_route_visiting_a_stop_twice() {
    // L: P 08:00 → Q 08:05 → R 08:10 → P 08:15 → S 08:20.
    let st = stop_times(&[
        ("L", "08:00:00", "P"),
        ("L", "08:05:00", "Q"),
        ("L", "08:10:00", "R"),
        ("L", "08:15:00", "P"),
        ("L", "08:20:00", "S"),
    ]);
    let f = TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "L")])),
        ("stop_times.txt", &st),
    ]);
    let tt = f.tt();
    let d = date(2030, 3, 1);
    // Board at the second visit of P.
    let js = plan_ids(&tt, "P", "S", d, hms(8, 10, 0), 4);
    assert_eq!(js.len(), 1);
    assert_eq!(
        (js[0].dep, js[0].arr, js[0].rides),
        (hms(8, 15, 0), hms(8, 20, 0), 1)
    );
    // From Q, ride through P to S.
    assert_eq!(
        earliest(&plan_ids(&tt, "Q", "S", d, hms(8, 0, 0), 4)),
        hms(8, 20, 0).0
    );
    // R → P: arrive at the second visit, 08:15.
    assert_eq!(
        earliest(&plan_ids(&tt, "R", "P", d, hms(8, 0, 0), 4)),
        hms(8, 15, 0).0
    );
    // R → Q: Q is only before R on the trip.
    assert!(plan_ids(&tt, "R", "Q", d, hms(8, 0, 0), 4).is_empty());
}

#[test]
fn very_long_trip_and_zero_duration_hops() {
    // 250 stops, 6 min apart: 00:00 … 24:54 (≥ 24 h span).
    let names: Vec<String> = (0..250).map(|i| format!("L{i:03}")).collect();
    let times: Vec<String> = (0..250u32)
        .map(|i| {
            let s = i * 360;
            format!("{:02}:{:02}:00", s / 3600, (s % 3600) / 60)
        })
        .collect();
    let rows: Vec<(&str, &str, &str)> = (0..250)
        .map(|i| ("long", times[i].as_str(), names[i].as_str()))
        .collect();
    // Plus a trip whose stop times are all the same second.
    let mut rows = rows;
    for s in ["Z1", "Z2", "Z3"] {
        rows.push(("zero", "09:00:00", s));
    }
    let f = TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "long"), ("Q", "zero")])),
        ("stop_times.txt", &stop_times(&rows)),
    ]);
    let tt = f.tt();
    let d = date(2030, 3, 1);
    assert_eq!(
        earliest(&plan_ids(&tt, "L000", "L249", d, Seconds(0), 4)),
        249 * 360
    );
    let js = plan_ids(&tt, "Z1", "Z3", d, hms(8, 0, 0), 4);
    assert_eq!(js.len(), 1);
    assert_eq!((js[0].dep, js[0].arr), (hms(9, 0, 0), hms(9, 0, 0)));
}

#[test]
fn boarding_where_you_just_alighted_counts_as_a_new_ride() {
    // a: X 08:00 → Y 08:10; b: Y 08:10 → Z 08:20 (same second at Y).
    // With max_transfers 0 the Y transfer must NOT be taken in the same round.
    let st = stop_times(&[
        ("a", "08:00:00", "X"),
        ("a", "08:10:00", "Y"),
        ("b", "08:10:00", "Y"),
        ("b", "08:20:00", "Z"),
    ]);
    let f = TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "a"), ("Q", "b")])),
        ("stop_times.txt", &st),
    ]);
    let tt = f.tt();
    let d = date(2030, 3, 1);
    assert!(plan_ids(&tt, "X", "Z", d, hms(7, 0, 0), 0).is_empty());
    let js = plan_ids(&tt, "X", "Z", d, hms(7, 0, 0), 1);
    assert_eq!(js.len(), 1);
    assert_eq!((js[0].rides, js[0].arr), (2, hms(8, 20, 0)));
}

#[test]
fn parent_station_queries() {
    // Station ST with platforms P1, P2. a: W 08:00 → P1 08:10; b: P2 08:20 → V 08:30.
    let stops = "stop_id,stop_name,stop_lat,stop_lon,location_type,parent_station\n\
                 ST,Station,1.0,1.0,1,\nP1,Platform 1,1.0,1.0001,0,ST\nP2,Platform 2,1.0,1.0002,0,ST\n\
                 W,W,2.0,1.0,0,\nV,V,3.0,1.0,0,\n";
    let st = stop_times(&[
        ("a", "08:00:00", "W"),
        ("a", "08:10:00", "P1"),
        ("b", "08:20:00", "P2"),
        ("b", "08:30:00", "V"),
    ]);
    let f = TempFeed::new(&[
        ("stops.txt", stops),
        ("trips.txt", &trips(&[("R", "a"), ("Q", "b")])),
        ("stop_times.txt", &st),
    ]);
    let tt = f.tt();
    let d = date(2030, 3, 1);
    // From the station: either resolved to P2 (arrive V 08:30) or empty (the "clear
    // error" option, since the signature has no Result). Never a panic or a bogus leg.
    let from_station = plan_ids(&tt, "ST", "V", d, hms(8, 0, 0), 4);
    assert!(
        from_station.is_empty() || earliest(&from_station) == hms(8, 30, 0).0,
        "{from_station:?}"
    );
    // Platform → sibling platform via the parent: W → V needs P1 → P2.
    // M3 decision 8: the spec says nothing about siblings, so only a
    // transfers.txt row links P1 to P2 (footpaths come in M4).
    let js = plan_ids(&tt, "W", "V", d, hms(7, 0, 0), 4);
    assert!(js.is_empty(), "{js:?}");
    let f = TempFeed::new(&[
        ("stops.txt", stops),
        ("trips.txt", &trips(&[("R", "a"), ("Q", "b")])),
        ("stop_times.txt", &st),
        (
            "transfers.txt",
            "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nP1,P2,2,120\n",
        ),
    ]);
    let tt = f.tt();
    let js = plan_ids(&tt, "W", "V", d, hms(7, 0, 0), 4);
    assert_eq!(earliest(&js), hms(8, 30, 0).0, "{js:?}");
}

/// a: W 08:00 → X1 08:00:00 arrival; b: X2 08:03 → Z; c: X2 08:10 → Z;
/// d: X1 08:05 → Z; e: X1 08:15 → Z. Z arrivals: b 08:30, c 08:40, d 08:35, e 08:45.
fn transfer_feed(transfers: Option<&str>) -> TempFeed {
    let st = stop_times(&[
        ("a", "07:50:00", "W"),
        ("a", "08:00:00", "X1"),
        ("b", "08:03:00", "X2"),
        ("b", "08:30:00", "Z"),
        ("c", "08:10:00", "X2"),
        ("c", "08:40:00", "Z"),
        ("d", "08:05:00", "X1"),
        ("d", "08:35:00", "Z"),
        ("e", "08:15:00", "X1"),
        ("e", "08:45:00", "Z"),
    ]);
    let tr = trips(&[("R", "a"), ("Q", "b"), ("Q", "c"), ("Q", "d"), ("Q", "e")]);
    let mut files: Vec<(&str, &str)> =
        vec![("trips.txt", tr.as_str()), ("stop_times.txt", st.as_str())];
    if let Some(t) = transfers {
        files.push(("transfers.txt", t));
    }
    TempFeed::new(&files)
}

#[test]
fn transfers_txt_absent_zero_large_and_self() {
    let d = date(2030, 3, 1);
    let h = "from_stop_id,to_stop_id,transfer_type,min_transfer_time\n";
    // Absent: no X1→X2 link; at X1 itself, transfer time 0 → d (08:05) → Z 08:35.
    let tt = transfer_feed(None).tt();
    assert_eq!(
        earliest(&plan_ids(&tt, "W", "Z", d, hms(7, 0, 0), 4)),
        hms(8, 35, 0).0
    );
    assert!(plan_ids(&tt, "W", "X2", d, hms(7, 0, 0), 4).is_empty());
    // X1→X2 type 2, 0 s: catch b at 08:03 → Z 08:30.
    let t0 = format!("{h}X1,X2,2,0\n");
    let tt = transfer_feed(Some(&t0)).tt();
    assert_eq!(
        earliest(&plan_ids(&tt, "W", "Z", d, hms(7, 0, 0), 4)),
        hms(8, 30, 0).0
    );
    // X1→X2 type 2, 300 s: b (08:03) missed, c (08:10) → 08:40; but d at X1 (08:05) → 08:35 is better.
    let t300 = format!("{h}X1,X2,2,300\n");
    let tt = transfer_feed(Some(&t300)).tt();
    assert_eq!(
        earliest(&plan_ids(&tt, "W", "Z", d, hms(7, 0, 0), 4)),
        hms(8, 35, 0).0
    );
    // Self transfer X1→X1 600 s and very large X1→X2: d (08:05) missed, e (08:15) → 08:45.
    let tself = format!("{h}X1,X1,2,600\nX1,X2,2,86400\n");
    let tt = transfer_feed(Some(&tself)).tt();
    assert_eq!(
        earliest(&plan_ids(&tt, "W", "Z", d, hms(7, 0, 0), 4)),
        hms(8, 45, 0).0
    );
}

#[test]
fn block_id_is_treated_as_a_normal_transfer() {
    let tr = "route_id,service_id,trip_id,block_id\nR,S,a,B1\nQ,S,b,B1\n";
    let st = stop_times(&[
        ("a", "08:00:00", "X"),
        ("a", "08:10:00", "Y"),
        ("b", "08:10:00", "Y"),
        ("b", "08:20:00", "Z"),
    ]);
    let f = TempFeed::new(&[("trips.txt", tr), ("stop_times.txt", &st)]);
    let tt = f.tt();
    let js = plan_ids(&tt, "X", "Z", date(2030, 3, 1), hms(7, 0, 0), 4);
    assert_eq!(js.len(), 1);
    assert_eq!(
        js[0].rides, 2,
        "in-seat transfer is Known gap (M3); counts as 2 rides"
    );
    assert!(plan_ids(&tt, "X", "Z", date(2030, 3, 1), hms(7, 0, 0), 0).is_empty());
}

// ============================ Results: determinism ============================

fn digest(js: &[Journey]) -> u64 {
    // FNV-1a over the Debug text: stable across runs and builds of the same code.
    format!("{js:?}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        })
}

#[test]
fn results_identical_across_runs_and_profiles() {
    let Some(feed) = shared_feed("brasov-manifest") else {
        return;
    };
    let tt = pack(&feed);
    let day = valid_from(&feed) + chrono::Days::new(30);
    let n = tt.stop_count() as u64;
    let mut rng = Rng(0xD37E_2026_1014_0003);
    let mut h = 0u64;
    for _ in 0..200 {
        let (a, b) = (StopIdx(rng.below(n) as u32), StopIdx(rng.below(n) as u32));
        let dep = Seconds(18_000 + rng.below(64_800) as u32);
        let one = raptor_stop_to_stop(&tt, a, b, day, dep, 8).unwrap();
        let two = raptor_stop_to_stop(&tt, a, b, day, dep, 8).unwrap();
        assert_eq!(
            format!("{one:?}"),
            format!("{two:?}"),
            "{a:?}->{b:?} dep {}",
            dep.0
        );
        h ^= digest(&one).rotate_left(7);
    }
    // Debug vs release: each profile records its digest; the second one compares.
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let other = if cfg!(debug_assertions) {
        "release"
    } else {
        "debug"
    };
    let dir = std::env::temp_dir().join("rustline_m3_digest");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(profile), h.to_string()).unwrap();
    println!("results digest ({profile}): {h:016x}");
    if let Ok(o) = std::fs::read_to_string(dir.join(other)) {
        assert_eq!(
            o,
            h.to_string(),
            "debug and release give different journeys"
        );
    }
}

// ============================ Oracle: CSA ============================

/// Earliest arrival with at most `max_rides` rides, by a connection scan
/// (independent of RAPTOR). Tester (M3 verify): extended with the pack's
/// transfers.txt rules as documented in raptor.rs (M3 decision 8), since
/// Bucureşti and MBTA have transfers.txt:
/// - changing trips at one stop: 0 s, or that stop's own `s → s` row's time
///   (type 3 = not allowed);
/// - one transfers.txt hop `s → w` (w ≠ s, type ≠ 3) after a ride or from the
///   origin, taking `min_transfer_time`; no chained hops, no other walking.
///
/// Without transfers.txt this is the plain 0 s same-stop oracle.
fn csa(tt: &Tt, from: u32, to: u32, day: u32, dep: u32, max_rides: usize, conns: &[Conn]) -> u32 {
    if from == to {
        return dep;
    }
    let n = tt.stop_count();
    let edges = |s: usize| {
        let (a, b) = (
            tt.transfer_start[s] as usize,
            tt.transfer_start[s + 1] as usize,
        );
        (a..b).map(|e| {
            (
                tt.transfer_to[e] as usize,
                tt.transfer_type[e],
                tt.transfer_secs[e],
            )
        })
    };
    let own = |s: usize| -> Option<u32> {
        match edges(s).find(|&(w, _, _)| w == s) {
            Some((_, 3, _)) => None,
            Some((_, _, secs)) => Some(secs),
            None => Some(0),
        }
    };
    let has_transfers = !tt.transfer_start.is_empty();
    // ride[k][s]: earliest arrival at s by a ride, ≤ k rides; walk[k][s]: by the
    // origin or a transfers.txt hop (can board at once).
    let mut ride = vec![vec![u32::MAX; n]; max_rides + 1];
    let mut walk = vec![vec![u32::MAX; n]; max_rides + 1];
    for row in &mut walk {
        row[from as usize] = dep;
    }
    if has_transfers {
        for (w, ty, secs) in edges(from as usize) {
            if w != from as usize && ty != 3 {
                for row in &mut walk {
                    row[w] = row[w].min(dep.saturating_add(secs));
                }
            }
        }
    }
    let board_at = |ride: &Vec<Vec<u32>>, walk: &Vec<Vec<u32>>, k: usize, u: usize| {
        let r = if has_transfers {
            own(u).map_or(u32::MAX, |x| ride[k][u].saturating_add(x))
        } else {
            ride[k][u]
        };
        r.min(walk[k][u])
    };
    // pos[t * max_rides + k]: first position (connection index within trip t)
    // where t was boarded with ≤ k rides before. Position-aware, so equal-time
    // connections scanned out of order can't ride a trip backwards.
    let mut pos = vec![u32::MAX; tt.trip_count() * max_rides];
    let mut target = walk[max_rides][to as usize];
    // Connections with the same departure time are scanned until nothing
    // changes: with a zero-duration connection (arrival == departure) an
    // arrival can enable a boarding listed before it in the sort order.
    let start = conns.partition_point(|c| c.0 < dep);
    let mut g = start;
    while g < conns.len() {
        let d = conns[g].0;
        if d >= target {
            break;
        }
        let end = g + conns[g..].partition_point(|c| c.0 == d);
        loop {
            let mut changed = false;
            for &(_, a, u, v, t, ok, i) in &conns[g..end] {
                if !tt.service_runs(tt.trip_service[t as usize] as usize, day) {
                    continue;
                }
                let ti = t as usize;
                let found = if ok & 1 != 0 {
                    (0..max_rides).find(|&k| board_at(&ride, &walk, k, u as usize) <= d)
                } else {
                    None // pickup_type 1: no boarding here
                };
                if let Some(k) = found {
                    for p in &mut pos[ti * max_rides + k..(ti + 1) * max_rides] {
                        if i < *p {
                            *p = i;
                            changed = true;
                        }
                    }
                }
                let on = (0..max_rides).find(|&k| pos[ti * max_rides + k] <= i);
                if let (Some(k0), true) = (on, ok & 2 != 0) {
                    let k = k0 + 1;
                    for row in &mut ride[k..] {
                        if a < row[v as usize] {
                            row[v as usize] = a;
                            changed = true;
                        }
                    }
                    if has_transfers {
                        for (w, ty, secs) in edges(v as usize) {
                            if w != v as usize && ty != 3 {
                                let x = a.saturating_add(secs);
                                for row in &mut walk[k..] {
                                    if x < row[w] {
                                        row[w] = x;
                                        changed = true;
                                    }
                                }
                                if w == to as usize {
                                    target = target.min(x);
                                }
                            }
                        }
                    }
                    if v == to {
                        target = target.min(a);
                    }
                }
            }
            if !changed {
                break;
            }
        }
        g = end;
    }
    ride[max_rides][to as usize].min(walk[max_rides][to as usize])
}

/// (dep, arr, from_stop, to_stop, trip, ok), sorted by departure. `ok` bit 0:
/// boarding allowed at from_stop (pickup_type ≠ 1); bit 1: getting off allowed
/// at to_stop (drop_off_type ≠ 1), the rule documented in raptor.rs. Last:
/// the connection's index within its trip.
type Conn = (u32, u32, u32, u32, u32, u8, u32);

fn connections(tt: &Tt) -> Vec<Conn> {
    let mut c = Vec::new();
    for t in 0..tt.trip_count() {
        let st = tt.trip_times(t);
        let base = tt.trip_times_start[t] as usize / 2; // stop_times holds (arr, dep) pairs
        for (i, w) in st.windows(2).enumerate() {
            let pick = tt.pickup_drop_off(base + i).0 != 1;
            let drop = tt.pickup_drop_off(base + i + 1).1 != 1;
            let ok = u8::from(pick) | (u8::from(drop) << 1);
            c.push((w[0].2, w[1].1, w[0].0, w[1].0, t as u32, ok, i as u32));
        }
    }
    c.sort_unstable();
    c
}

/// Seed differs from anything Builder uses; printed so a run can be replayed.
const ORACLE_SEED: u64 = 0x07E5_7E12_0003_1014;

fn oracle(name: &str, queries: usize) {
    let Some(feed) = shared_feed(name) else {
        return;
    };
    let tt = pack(&feed);
    let from_date = valid_from(&feed);
    let conns = connections(&tt);
    let served: Vec<u32> = (0..tt.stop_count())
        .filter(|&s| tt.stop_patterns_start[s] < tt.stop_patterns_start[s + 1])
        .map(|s| s as u32)
        .collect();
    // Tester (M3 verify): RUSTLINE_ORACLE_SEED / RUSTLINE_ORACLE_QUERIES override
    // the defaults, so other query sets can be checked without editing the file.
    let seed = std::env::var("RUSTLINE_ORACLE_SEED")
        .ok()
        .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .unwrap_or(ORACLE_SEED);
    let queries = std::env::var("RUSTLINE_ORACLE_QUERIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(queries);
    let mut rng = Rng(seed);
    println!("{name}: seed {seed:#x}, {queries} queries, max_transfers 8");
    let (mut mismatches, mut times) = (Vec::new(), Vec::new());
    for q in 0..queries {
        let a = served[rng.below(served.len() as u64) as usize];
        let b = served[rng.below(served.len() as u64) as usize];
        let day = rng.below(60) as u32;
        let dep = 4 * 3600 + rng.below(20 * 3600) as u32;
        let date = from_date + chrono::Days::new(u64::from(day));
        let t0 = Instant::now();
        // M3 decision 5: raptor_stop_to_stop drops journeys under the 5-minute
        // rule, so its earliest arrival can be later than CSA's. Compare the
        // full (arrival, rides) set instead.
        let js = raptor_pareto(&tt, StopIdx(a), StopIdx(b), date, Seconds(dep), 8).unwrap();
        times.push(t0.elapsed());
        assert_valid(&tt, StopIdx(a), StopIdx(b), Seconds(dep), &js);
        let want = csa(&tt, a, b, day, dep, 9, &conns);
        let got = if a == b { dep } else { earliest(&js) };
        if want != got {
            mismatches.push(format!(
                "#{q}: from {} ({a}) to {} ({b}) on {date} dep {dep}: RAPTOR {got}, CSA {want}",
                tt.stop_ids[a as usize], tt.stop_ids[b as usize]
            ));
        }
    }
    times.sort();
    println!(
        "{name}: median {:?}, p95 {:?}, max {:?}, {} mismatches",
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() - 1],
        mismatches.len()
    );
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
#[ignore = "slow: 1 000 RAPTOR vs CSA queries; run in release, see README"]
fn raptor_equals_csa_on_1000_brasov_queries() {
    oracle("brasov-manifest", 1000);
}

#[test]
#[ignore = "slow: 1 000 RAPTOR vs CSA queries; run in release, see README"]
fn raptor_equals_csa_on_1000_cluj_queries() {
    oracle("cluj", 1000);
}

#[test]
#[ignore = "slow: 1 000 RAPTOR vs CSA queries; run in release, see README"]
fn raptor_equals_csa_on_1000_timisoara_queries() {
    oracle("timisoara", 1000);
}

#[test]
#[ignore = "slow: 1 000 RAPTOR vs CSA queries; run in release, see README"]
fn raptor_equals_csa_on_1000_bucuresti_queries() {
    oracle("bucuresti", 1000);
}

#[test]
#[ignore = "slow: 1 000 RAPTOR vs CSA queries; run in release, see README"]
fn raptor_equals_csa_on_1000_brasov_latest_queries() {
    oracle("brasov-latest", 1000);
}

#[test]
#[ignore = "slow: builds the MBTA pack; 1 000 RAPTOR vs CSA queries; run in release"]
fn raptor_equals_csa_on_1000_mbta_queries() {
    oracle("mbta", 1000);
}

// ============================ Safety and bounded work ============================

#[test]
fn fuzz_query_parameters_never_panic() {
    // Random (incl. out-of-range) stops, times incl. 0 and 30:00:00, dates in and out of range.
    let Some(feed) = shared_feed("brasov-manifest") else {
        return;
    };
    let tt = pack(&feed);
    let from_date = valid_from(&feed);
    let n = tt.stop_count() as u64;
    let mut rng = Rng(0xF022_2026_1014_0001);
    for _ in 0..2000 {
        let a = StopIdx(rng.below(n + 3) as u32);
        let b = StopIdx(rng.below(n + 3) as u32);
        let dep = Seconds(match rng.below(6) {
            0 => 0,
            1 => 30 * 3600,
            2 => u32::MAX - rng.below(2) as u32,
            _ => rng.below(48 * 3600) as u32,
        });
        let date = from_date + chrono::Days::new(rng.below(800));
        let k = [0u8, 1, 8, 255][rng.below(4) as usize];
        // M3 decision 4: bad stops and dates are errors.
        match raptor_stop_to_stop(&tt, a, b, date, dep, k) {
            Ok(js) => assert_valid(&tt, a, b, dep, &js),
            Err(QueryError::UnknownStop { .. }) => assert!((a.0 as u64) >= n || (b.0 as u64) >= n),
            Err(QueryError::DateOutsideFeed { .. }) => assert!(date > tt.meta().valid_to),
            Err(e) => panic!("{e}"),
        }
    }
}

#[test]
fn crafted_pattern_with_a_long_stop_cycle_terminates() {
    // Hand-made pack: one pattern visiting A,B,A,B,… 5 000 times, one trip.
    // Exercises bounded loops; must return quickly, valid or empty, never hang.
    let mut t = empty_pack();
    t.stop_ids = vec!["A".into(), "B".into()];
    t.stop_names = t.stop_ids.clone();
    t.stop_lat = vec![1f64.to_bits(); 2];
    t.stop_lon = vec![1f64.to_bits(); 2];
    t.stop_kind = vec![0; 2];
    t.stop_parent = vec![u32::MAX; 2];
    t.route_ids = vec!["R".into()];
    t.route_short_names = vec!["R".into()];
    t.route_long_names = vec!["R".into()];
    t.route_types = vec![3];
    t.trip_ids = vec!["T".into()];
    t.trip_route = vec![0];
    t.trip_service = vec![0];
    t.trip_headsigns = vec![String::new()];
    t.trip_approximate = vec![0];
    t.trip_pattern = vec![0];
    t.trip_times_start = vec![0];
    let n = 5000u32;
    t.pattern_stops = (0..n).map(|i| i % 2).collect();
    t.pattern_stops_start = vec![0, n];
    t.pattern_trips = vec![0];
    t.pattern_trips_start = vec![0, 1];
    t.stop_times = (0..n).flat_map(|i| [i * 10, i * 10]).collect();
    t.stop_patterns = vec![0, 0];
    t.stop_patterns_start = vec![0, 1, 2];
    t.service_ids = vec!["S".into()];
    t.service_words = WORDS;
    t.service_bits = vec![u64::MAX; WORDS as usize];
    t.check().expect("crafted pack passes the M2 check");
    let t = seal(t);
    let t0 = Instant::now();
    let js = raptor_stop_to_stop(
        &t,
        StopIdx(0),
        StopIdx(1),
        date(1970, 1, 1),
        Seconds(0),
        255,
    )
    .unwrap();
    assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    assert_valid(&t, StopIdx(0), StopIdx(1), Seconds(0), &js);
    // A at 0 s → B at 10 s on the same trip.
    assert_eq!(earliest(&js), 10);
}

#[test]
fn ten_thousand_brasov_queries_none_over_100_ms() {
    let Some(feed) = shared_feed("brasov-manifest") else {
        return;
    };
    let tt = pack(&feed);
    let from_date = valid_from(&feed);
    let n = tt.stop_count() as u64;
    let mut rng = Rng(0xB2A5_0000_1000_0001);
    let mut worst = Duration::ZERO;
    for _ in 0..10_000 {
        let (a, b) = (StopIdx(rng.below(n) as u32), StopIdx(rng.below(n) as u32));
        let dep = Seconds(rng.below(30 * 3600) as u32);
        let date = from_date + chrono::Days::new(rng.below(60));
        let t0 = Instant::now();
        let js = raptor_stop_to_stop(&tt, a, b, date, dep, 8).unwrap();
        worst = worst.max(t0.elapsed());
        assert_valid(&tt, a, b, dep, &js);
    }
    println!("brasov 10 000 queries: worst {worst:?}");
    if !cfg!(debug_assertions) {
        assert!(worst < Duration::from_millis(100), "worst query {worst:?}");
    }
}

#[test]
fn overtaking_is_impossible_within_a_pattern() {
    // Also checked (passing today) in m3_prereq.rs; repeated here on the router's input.
    let tt = tiny();
    for p in 0..tt.pattern_count() {
        let (a, b) = (
            tt.pattern_trips_start[p] as usize,
            tt.pattern_trips_start[p + 1] as usize,
        );
        for w in tt.pattern_trips[a..b].windows(2) {
            let (x, y) = (tt.trip_times(w[0] as usize), tt.trip_times(w[1] as usize));
            assert!(x.iter().zip(&y).all(|(s, t)| s.1 <= t.1 && s.2 <= t.2));
        }
    }
    let _ = TripIdx(0);
}

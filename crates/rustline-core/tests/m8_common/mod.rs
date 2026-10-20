//! M8 tests-first (Tester, 2026-10-20): helpers and the one adapter (`api`) for the
//! arrive-by tests (`m8_*.rs`). Reuses the M5 feed builder (`m5_common`).
//! Builder: adapt only `api` when names differ (the M8 gate and ignores are gone).
//!
//! Assumed (plan M8 step 1): `rustline_core::TimeSpec::ArriveBy(DateTime<FixedOffset>)`;
//! `Itinerary.departure` = when you leave the origin (no waiting there), and for a
//! request whose points sit on stops the first ride boards at `departure`.
#![allow(dead_code, unexpected_cfgs)]

#[path = "../m5_common/mod.rs"]
pub mod m5;

pub use m5::api::{engine, local, local_date, plan, req, It};
pub use m5::*;
pub use rustline_pack::Pack;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime};
use rustline_core::{Engine, PlanError, PlanRequest, TimeSpec};
use rustline_model::LatLon;

/// The instant `secs` after "noon minus 12 h" of service day `d` in `zone`.
pub fn service_instant(zone: &str, d: NaiveDate, secs: u32) -> DateTime<FixedOffset> {
    let noon = local(zone, d.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap())).unwrap();
    noon - Duration::hours(12) + Duration::seconds(secs as i64)
}

/// Every departure event (instant, trip index) at stops `at` on service days `days`.
pub fn departures(
    p: &Pack,
    zone: &str,
    at: &[usize],
    days: &[NaiveDate],
) -> Vec<(DateTime<FixedOffset>, usize)> {
    let mut out = Vec::new();
    for t in 0..p.trip_ids.len() {
        let pat = p.trip_pattern[t] as usize;
        let (s0, s1) = (
            p.pattern_stops_start[pat] as usize,
            p.pattern_stops_start[pat + 1] as usize,
        );
        let base = p.trip_times_start[t] as usize;
        let sid = p.service_ids[p.trip_service[t] as usize].clone();
        for (i, &stop) in p.pattern_stops[s0..s1].iter().enumerate() {
            // The last stop of a pattern has no onward ride.
            if i + 1 == s1 - s0 || !at.contains(&(stop as usize)) {
                continue;
            }
            let dep = p.stop_times[base + 2 * i + 1];
            for &d in days {
                if runs(p, &sid, d) {
                    out.push((service_instant(zone, d, dep), t));
                }
            }
        }
    }
    out.sort();
    out
}

/// Stops within 1 m of stop `s` (itself included: same coordinates).
pub fn colocated(p: &Pack, s: usize) -> Vec<usize> {
    let Some(a) = p.stop_pos(s) else {
        return vec![s];
    };
    (0..p.stop_count())
        .filter(|&j| {
            p.stop_pos(j)
                .is_some_and(|b| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5)
        })
        .collect()
}

/// Exact point of stop `s`.
pub fn pos(p: &Pack, s: usize) -> (f64, f64) {
    p.stop_pos(s).expect("stop with coordinates")
}

/// Stop to stop (points on the stops, 1 m access, so only co-located stops).
pub fn stop_req(from: (f64, f64), to: (f64, f64), time: TimeSpec) -> PlanRequest {
    PlanRequest {
        from: LatLon::new(from.0, from.1),
        to: LatLon::new(to.0, to.1),
        time,
        max_access_m: 1,
        ..PlanRequest::default()
    }
}

/// Boarding instant of the first ride, or `dep` for a walk-only itinerary.
pub fn first_board(it: &It) -> DateTime<FixedOffset> {
    it.legs.first().map(|l| l.3).unwrap_or(it.dep)
}

/// Earliest arrival of a depart-at query (M5 engine: the oracle's building block).
pub fn depart_at_arrival(
    e: &Engine,
    from: (f64, f64),
    to: (f64, f64),
    t: DateTime<FixedOffset>,
) -> Option<DateTime<FixedOffset>> {
    plan(e, &stop_req(from, to, TimeSpec::DepartAt(t)))
        .ok()?
        .iter()
        .map(|i| i.arr)
        .min()
}

/// Brute-force arrive-by: the latest departure event at the origin (on D-1, D, D+1 of
/// the deadline's local date) from which the depart-at engine arrives by `deadline`.
/// `None` = no such departure. Gives up (Err) after `max_tries` depart-at calls.
pub fn oracle_latest_departure(
    p: &Pack,
    e: &Engine,
    zone: &str,
    from: usize,
    to: usize,
    deadline: DateTime<FixedOffset>,
    max_tries: usize,
) -> Result<Option<DateTime<FixedOffset>>, ()> {
    let d = local_date(zone, deadline);
    let days = [d - Duration::days(1), d, d + Duration::days(1)];
    let evs = departures(p, zone, &colocated(p, from), &days);
    let mut last = None;
    let mut tries = 0;
    for (t, _) in evs.iter().rev() {
        if *t > deadline || Some(*t) == last {
            continue;
        }
        last = Some(*t);
        tries += 1;
        if tries > max_tries {
            return Err(());
        }
        if depart_at_arrival(e, pos(p, from), pos(p, to), *t).is_some_and(|a| a <= deadline) {
            return Ok(Some(*t));
        }
    }
    Ok(None)
}

/// Door to door between two points with the default request (1 km access walks,
/// walk-only offered as in the engine): what the oracle and the arrive-by call share.
pub fn door_req(from: (f64, f64), to: (f64, f64), time: TimeSpec) -> PlanRequest {
    PlanRequest {
        from: LatLon::new(from.0, from.1),
        to: LatLon::new(to.0, to.1),
        time,
        ..PlanRequest::default()
    }
}

/// Earliest arrival of the depart-at engine for `door_req(.., DepartAt(t))`, with the
/// itineraries (None = no answer).
pub fn depart_at_door(
    e: &Engine,
    from: (f64, f64),
    to: (f64, f64),
    t: DateTime<FixedOffset>,
) -> Option<(DateTime<FixedOffset>, Vec<It>)> {
    let its = plan(e, &door_req(from, to, TimeSpec::DepartAt(t))).ok()?;
    let a = its.iter().map(|i| i.arr).min()?;
    Some((a, its))
}

/// Brute-force arrive-by on top of the depart-at engine only (never the arrive-by
/// code): earliest arrival EA(t) is non-decreasing in t (FIFO), so the latest leave
/// time t* with EA(t*) ≤ deadline is found by bisection, to the second, over the
/// window [max(start of service day D − 1, deadline − 24 h), deadline] (plan M8
/// decision 7). Walks to nearby
/// stops and walk-only journeys come with the depart-at engine. Returns t* and the
/// depart-at itineraries at t* that arrive by the deadline; None = no such t.
pub fn oracle_latest_leave(
    e: &Engine,
    zone: &str,
    from: (f64, f64),
    to: (f64, f64),
    deadline: DateTime<FixedOffset>,
) -> Option<(DateTime<FixedOffset>, Vec<It>)> {
    let d = local_date(zone, deadline);
    // Not before deadline − 24 h (decision 7): a depart-at search sees D − 1 … D + 1
    // of its own date, so from there on it still sees the deadline's day.
    let lo_t = service_instant(zone, d - Duration::days(1), 0).max(deadline - Duration::hours(24));
    let ok =
        |t: DateTime<FixedOffset>| depart_at_door(e, from, to, t).filter(|(a, _)| *a <= deadline);
    ok(lo_t)?;
    let (mut lo, mut hi) = (0i64, (deadline - lo_t).num_seconds() + 1); // ok(lo), !ok(hi)
    if ok(lo_t + Duration::seconds(hi - 1)).is_some() {
        lo = hi - 1;
    } else {
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if ok(lo_t + Duration::seconds(mid)).is_some() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }
    let t = lo_t + Duration::seconds(lo);
    let (_, its) = ok(t)?;
    Some((t, its.into_iter().filter(|i| i.arr <= deadline).collect()))
}

/// Service day of a ride (trip id, boarding stop id, boarding instant), read from the
/// pack's timetable: the day d (local date − 2 … local date) with
/// `service_instant(d, dep) == board`.
pub fn ride_service_day(
    p: &Pack,
    zone: &str,
    trip: &str,
    from_stop: &str,
    board: DateTime<FixedOffset>,
) -> Option<NaiveDate> {
    let t = p.trip_ids.iter().position(|x| x == trip)?;
    let pat = p.trip_pattern[t] as usize;
    let (s0, s1) = (
        p.pattern_stops_start[pat] as usize,
        p.pattern_stops_start[pat + 1] as usize,
    );
    let base = p.trip_times_start[t] as usize;
    let ld = local_date(zone, board);
    for (i, &stop) in p.pattern_stops[s0..s1].iter().enumerate() {
        if p.stop_ids[stop as usize] != from_stop {
            continue;
        }
        let dep = p.stop_times[base + 2 * i + 1];
        for k in 0..=2 {
            let d = ld - Duration::days(k);
            if service_instant(zone, d, dep) == board {
                return Some(d);
            }
        }
    }
    None
}

/// A ride checked against the pack's timetable alone: the trip runs on the
/// service day found for its boarding, boards at `from` at that stop's departure
/// time and gets off later in the same trip at `to` at that stop's arrival time.
pub fn ride_is_real(
    p: &Pack,
    zone: &str,
    leg: &(
        String,
        String,
        String,
        DateTime<FixedOffset>,
        DateTime<FixedOffset>,
    ),
) -> bool {
    let (trip, from, to, board, alight) = leg;
    let Some(d) = ride_service_day(p, zone, trip, from, *board) else {
        return false;
    };
    let Some(t) = p.trip_ids.iter().position(|x| x == trip) else {
        return false;
    };
    if !runs(p, &p.service_ids[p.trip_service[t] as usize].clone(), d) {
        return false;
    }
    let pat = p.trip_pattern[t] as usize;
    let (s0, s1) = (
        p.pattern_stops_start[pat] as usize,
        p.pattern_stops_start[pat + 1] as usize,
    );
    let base = p.trip_times_start[t] as usize;
    let stops = &p.pattern_stops[s0..s1];
    let Some(i) = (0..stops.len()).find(|&i| {
        p.stop_ids[stops[i] as usize] == *from
            && service_instant(zone, d, p.stop_times[base + 2 * i + 1]) == *board
    }) else {
        return false;
    };
    (i + 1..stops.len()).any(|j| {
        p.stop_ids[stops[j] as usize] == *to
            && service_instant(zone, d, p.stop_times[base + 2 * j]) == *alight
    })
}

/// Every ride of `it` is real, and the legs (walks included) follow each other in time.
pub fn journey_is_real(p: &Pack, zone: &str, it: &It) -> bool {
    it.legs.iter().all(|l| ride_is_real(p, zone, l))
        && it.spans.windows(2).all(|w| w[0].1 <= w[1].0)
        && it.spans.iter().all(|(a, b)| a <= b)
        && it.spans.first().is_none_or(|s| s.0 >= it.dep)
        && it.spans.last().is_none_or(|s| s.1 <= it.arr)
}

/// True when a ride of `it` runs on a service day before D − 1 of `deadline`'s local
/// date (outside the arrive-by window, plan M8 decision 7). Unknown → false.
pub fn uses_day_before_window(
    p: &Pack,
    zone: &str,
    it: &It,
    deadline: DateTime<FixedOffset>,
) -> bool {
    let first = local_date(zone, deadline) - Duration::days(1);
    it.legs
        .iter()
        .any(|l| ride_service_day(p, zone, &l.0, &l.1, l.3).is_some_and(|d| d < first))
}

pub fn is_no_route(r: &Result<Vec<It>, PlanError>) -> bool {
    matches!(r, Err(PlanError::NoRoute))
}
pub fn is_date_outside(r: &Result<Vec<It>, PlanError>) -> bool {
    matches!(r, Err(PlanError::DateOutsideFeed))
}

/// The planned M8 names. Builder: adapt only this module.
pub mod api8 {
    use super::*;

    pub fn arrive_by(t: DateTime<FixedOffset>) -> TimeSpec {
        TimeSpec::ArriveBy(t)
    }

    /// Arrive-by door to door with the M5 test request (100 m access).
    pub fn plan_arrive(
        e: &Engine,
        from: (f64, f64),
        to: (f64, f64),
        t: DateTime<FixedOffset>,
    ) -> Result<Vec<It>, PlanError> {
        let mut r = req(from, to, t);
        r.time = arrive_by(t);
        plan(e, &r)
    }

    /// Stop-to-stop arrive-by (1 m access).
    pub fn plan_arrive_stops(
        e: &Engine,
        from: (f64, f64),
        to: (f64, f64),
        t: DateTime<FixedOffset>,
    ) -> Result<Vec<It>, PlanError> {
        plan(e, &stop_req(from, to, arrive_by(t)))
    }

    /// The answer the planner ranks first.
    pub fn best(r: &Result<Vec<It>, PlanError>) -> Option<&It> {
        r.as_ref().ok()?.first()
    }
}

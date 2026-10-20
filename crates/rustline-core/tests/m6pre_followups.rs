//! M6 tests-first (Tester, 2026-10-20): the M5 review follow-ups that are not FFI.
//! Not gated: they run on the M5 API that is in the tree.
//! - D-1 brute force: trips that run past 24:00, so the previous service day really
//!   gives the answer (the m4_plan brute force uses a feed with no trip past 24:00).
//! - A trip whose frequency rows are all invalid: dropped with warnings, or a build
//!   error; never run at its template times.
//!
//! Run: `cargo test --locked -p rustline-core --test m6pre_followups`

mod m5_common;

use chrono::{Duration, NaiveDate, NaiveTime};
use m5_common::api::*;
use m5_common::*;
use rustline_gtfs::IssueKind;

fn gtfs_time(secs: u32) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Line A→B→C every 37 min from 21:00 to 27:40 on service "S" (Oct 2031, DST end on
/// the 26th), with two days removed so D-1 and D differ.
fn late_line() -> Feed {
    let mut f = Feed::daily(BUCHAREST, "20311001", "20311031")
        .exception("S", "20311014", 2)
        .exception("S", "20311025", 2);
    let mut dep = hms(21, 0, 0);
    let mut k = 0;
    while dep <= hms(27, 40, 0) {
        let (a, b, c) = (gtfs_time(dep), gtfs_time(dep + 600), gtfs_time(dep + 1500));
        f = f.trip("S", &format!("t{k}"), &[(&a, "A"), (&b, "B"), (&c, "C")]);
        dep += 37 * 60;
        k += 1;
    }
    f
}

/// The instant of `secs` after "noon minus 12 h" of service day `d` (GTFS rule).
fn service_instant(d: NaiveDate, secs: u32) -> chrono::DateTime<chrono::FixedOffset> {
    let noon = local(
        BUCHAREST,
        d.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap()),
    )
    .unwrap();
    noon - Duration::hours(12) + Duration::seconds(secs as i64)
}

#[test]
fn d_minus_1_brute_force_with_trips_past_midnight() {
    let p = late_line().pack();
    let e = engine(late_line().pack());
    let mut rng = Rng(0x6d36);
    let (mut checked, mut from_d_minus_1) = (0, 0);
    for _ in 0..400 {
        let day = date(2031, 10, 2 + rng.below(28) as u32);
        // 22:00 .. 04:00 local, wall clock (skips the hour that does not exist).
        let wall = day.and_time(NaiveTime::from_hms_opt(22, 0, 0).unwrap())
            + Duration::seconds(rng.below(6 * 3600) as i64);
        let Some(t) = local(BUCHAREST, wall) else {
            continue;
        };
        if local_date(BUCHAREST, t) > date(2031, 10, 30) {
            continue;
        }
        let (from, to) = [("A", "C"), ("A", "B"), ("B", "C")][rng.below(3) as usize];
        // Brute force: every service day the instant maps to (D-1, D, D+1 when it fits).
        let mut best: Option<(chrono::DateTime<chrono::FixedOffset>, NaiveDate)> = None;
        for (d, s) in service_times(BUCHAREST, t) {
            if d < date(2031, 10, 1) || d > date(2031, 10, 31) {
                continue;
            }
            if let Some(arr) = earliest(&p, from, to, d, s) {
                let i = service_instant(d, arr);
                if best.is_none_or(|b| i < b.0) {
                    best = Some((i, d));
                }
            }
        }
        let got = plan_stops(&e, from, to, t);
        let got_arr = got
            .as_ref()
            .ok()
            .and_then(|v| v.iter().map(|i| i.arr).min());
        // The planner also looks ahead to later running days: add the next two
        // service days from their start (those rides are after t anyway).
        for n in 1..=2 {
            let d = local_date(BUCHAREST, t) + Duration::days(n);
            if d > date(2031, 10, 31) {
                continue;
            }
            if let Some(arr) = earliest(&p, from, to, d, 0) {
                let i = service_instant(d, arr);
                if i > t && best.is_none_or(|b| i < b.0) {
                    best = Some((i, d));
                }
            }
        }
        assert_eq!(
            got_arr,
            best.map(|b| b.0),
            "{from}->{to} at {t}: plan {got:?}, brute {best:?}"
        );
        if let Some((_, d)) = best {
            if d < local_date(BUCHAREST, t) {
                from_d_minus_1 += 1;
            }
        }
        checked += 1;
    }
    println!("D-1 brute force: {checked} queries, {from_d_minus_1} answered by D-1");
    assert!(checked > 300);
    assert!(from_d_minus_1 > 50, "too few D-1 answers: {from_d_minus_1}");
}

#[test]
fn trip_whose_frequency_rows_are_all_invalid_is_dropped_or_an_error() {
    let f = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "keep", &[("06:00:00", "A"), ("06:10:00", "B")])
        .trip("S", "f", &[("08:00:00", "A"), ("08:10:00", "B")])
        .freq("f", "08:00:00", "09:00:00", "0", "1")
        .freq("f", "10:00:00", "09:00:00", "600", "1");
    match f.write().build() {
        Err(e) => println!("all-invalid frequency trip is a build error: {e}"),
        Ok((tt, report)) => {
            assert_eq!(report.count(IssueKind::InvalidFrequency), 2);
            assert_eq!(
                tt.trips.len(),
                1,
                "the template trip must not run at its template times"
            );
            let p = f.pack();
            assert_eq!(
                earliest(&p, "A", "B", date(2031, 1, 15), hms(7, 0, 0)),
                None,
                "a ride after 07:00 exists, so trip f ran at its template time"
            );
        }
    }
}

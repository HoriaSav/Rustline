//! M8 review MF1 (Builder): arrive-by keeps a 2-ride option that meets the
//! deadline when the 1-ride option from the same departure arrives too late
//! (the 5-minute rule must not run before the deadline check), and the
//! ranking: latest departure, then fewer rides, then earliest arrival.
//! Run: `cargo test --locked -p rustline-core --test m8_arrive_pick`
#![allow(dead_code)]

mod m5_common;
use m5_common::api::*;
use m5_common::*;
use rustline_core::{PlanRequest, TimeSpec};
use rustline_model::LatLon;

#[test]
fn two_rides_by_08_59_beat_one_ride_at_09_03_for_a_09_00_deadline() {
    let f = Feed::daily(BUCHAREST, "20310101", "20310131")
        // 1 ride, early: arrives well before the deadline.
        .trip("S", "early", &[("08:00:00", "A"), ("08:30:00", "C")])
        // 2 rides via B, leaving 08:20: arrive 08:59.
        .trip("S", "ab", &[("08:20:00", "A"), ("08:40:00", "B")])
        .trip("S", "bc", &[("08:45:00", "B"), ("08:59:00", "C")])
        // 1 ride after 08:20, but at 09:03: too late (and < 5 min later).
        .trip("S", "late", &[("08:40:00", "A"), ("09:03:00", "C")]);
    let e = engine(f.pack());
    let deadline = at(2031, 1, 15, 9, 0, 0, 2);
    let r = PlanRequest {
        from: LatLon::new(1.0, 1.0),
        to: LatLon::new(3.0, 1.0),
        time: TimeSpec::ArriveBy(deadline),
        max_access_m: 1,
        ..PlanRequest::default()
    };
    let its = plan(&e, &r).expect("a route");
    for it in &its {
        assert!(it.arr <= deadline, "{it:?}");
    }
    let best = &its[0];
    assert_eq!(best.trips(), vec!["ab", "bc"], "{its:?}");
    assert!(same(best.dep, at(2031, 1, 15, 8, 20, 0, 2)), "{}", best.dep);
}

#[test]
fn same_departure_fewer_rides_beats_an_earlier_arrival() {
    // Leaving F at 04:21:30: 1 ride arriving 05:27:05, or 2 rides (via B)
    // arriving 05:18:06. Same departure, so fewer rides wins (plan M8 step 5).
    let f = Feed::daily(BUCHAREST, "20310301", "20310430")
        .trip("S", "one", &[("04:21:30", "F"), ("05:27:05", "J")])
        .trip("S", "fb", &[("04:21:30", "F"), ("04:29:19", "B")])
        .trip("S", "bj", &[("04:50:57", "B"), ("05:18:06", "J")]);
    let e = engine(f.pack());
    let deadline = at(2031, 3, 31, 5, 30, 0, 3);
    let r = PlanRequest {
        from: LatLon::new(6.0, 1.0),
        to: LatLon::new(10.0, 1.0),
        time: TimeSpec::ArriveBy(deadline),
        max_access_m: 1,
        max_transfers: 3,
        ..PlanRequest::default()
    };
    let its = plan(&e, &r).expect("a route");
    let best = &its[0];
    assert_eq!(best.trips(), vec!["one"], "{its:?}");
    assert!(same(best.arr, at(2031, 3, 31, 5, 27, 5, 3)), "{}", best.arr);
}

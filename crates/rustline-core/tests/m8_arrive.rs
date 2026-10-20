//! M8 tests-first (Tester, 2026-10-20): arrive-by edge cases on small feeds (plan M8
//! edge-case list): DST (Bucharest 2031-03-30 / 2031-10-26), after midnight and
//! D-1, no route, tie-breaks, walking only, frequencies, transfers, max_transfers 0,
//! a non-Romanian zone, and the safety bounds.
//! Ungated in M8 (Builder removed `cfg rustline_m8` and the "M8 not implemented" ignores).
//! Run: `cargo test --locked -p rustline-core --test m8_arrive`
#![allow(unexpected_cfgs)]

mod m8_common;
use chrono::NaiveDate;
use m8_common::api8::*;
use m8_common::*;

const A: (f64, f64) = (1.0, 1.0);
const B: (f64, f64) = (2.0, 1.0);
const C: (f64, f64) = (3.0, 1.0);

fn wall(d: NaiveDate, h: u32, m: u32) -> chrono::NaiveDateTime {
    d.and_hms_opt(h, m, 0).unwrap()
}

fn line(start: &str, end: &str, trips: &[(&str, &str, &str)]) -> rustline_core::Engine {
    let mut f = Feed::daily(BUCHAREST, start, end);
    for (id, a, b) in trips {
        f = f.trip("S", id, &[(a, "A"), (b, "B")]);
    }
    engine(f.pack())
}

fn latest(
    r: &Result<Vec<It>, rustline_core::PlanError>,
) -> Option<(chrono::DateTime<chrono::FixedOffset>, Vec<&str>)> {
    let its = r.as_ref().ok()?;
    let it = its.iter().max_by_key(|i| first_board(i))?;
    Some((first_board(it), it.trips()))
}

// ---- basics -------------------------------------------------------------------

#[test]
fn takes_the_latest_trip_that_arrives_by_the_deadline() {
    let e = line(
        "20310101",
        "20310131",
        &[
            ("t1", "08:00:00", "08:30:00"),
            ("t2", "08:20:00", "08:50:00"),
            ("t3", "08:40:00", "09:10:00"),
        ],
    );
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 15, 9, 0, 0, 2));
    assert_eq!(latest(&r).unwrap().1, vec!["t2"]);
    // A deadline exactly at an arrival counts (≤).
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 15, 9, 10, 0, 2));
    assert_eq!(latest(&r).unwrap().1, vec!["t3"]);
    for it in r.unwrap() {
        assert!(it.arr <= at(2031, 1, 15, 9, 10, 0, 2));
    }
}

#[test]
fn answer_offsets_are_the_feed_offset() {
    let e = line("20310101", "20310131", &[("t1", "08:00:00", "08:30:00")]);
    // Deadline given in UTC+13: same instant rules, answer in +02:00.
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 15, 19, 0, 0, 13)).unwrap();
    assert!(r
        .iter()
        .all(|i| i.arr.offset().local_minus_utc() == 7200
            && i.dep.offset().local_minus_utc() == 7200));
}

// ---- DST ------------------------------------------------------------------------

#[test]
fn deadline_in_the_spring_forward_gap_moves_forward() {
    // 2031-03-30 03:30 does not exist in Bucharest: same rule as --at (forward, 04:30 +03).
    let e = line(
        "20310301",
        "20310430",
        &[
            ("early", "02:00:00", "02:20:00"),
            ("late", "04:00:00", "04:20:00"),
        ],
    );
    let t =
        rustline_core::local_instant(BUCHAREST.parse().unwrap(), wall(date(2031, 3, 30), 3, 30));
    assert!(same(t, at(2031, 3, 30, 4, 30, 0, 3)), "{t}");
    let r = plan_arrive_stops(&e, A, B, t);
    assert_eq!(
        latest(&r).unwrap().1,
        vec!["late"],
        "04:00 (+03) -> 04:20 arrives by 04:30"
    );
}

#[test]
fn deadline_in_the_fall_back_overlap_takes_the_earlier_instant() {
    // 2031-10-26 03:30 happens twice (+03 then +02); the earlier is 00:30 UTC.
    let e = line(
        "20311001",
        "20311130",
        &[("a", "02:50:00", "03:10:00"), ("b", "03:40:00", "03:59:00")],
    );
    let t =
        rustline_core::local_instant(BUCHAREST.parse().unwrap(), wall(date(2031, 10, 26), 3, 30));
    assert!(same(t, at(2031, 10, 26, 3, 30, 0, 3)), "{t}");
    let r = plan_arrive_stops(&e, A, B, t);
    let (dep, trips) = latest(&r).unwrap();
    assert!(dep <= t);
    println!("overlap: {trips:?} at {dep}");
}

#[test]
fn journey_across_the_dst_change_and_23_h_25_h_service_days() {
    // Service-day times past 24:00 on the day before each change (D-1 of 23 h / 25 h).
    let e = line("20310301", "20311031", &[("n", "25:30:00", "26:30:00")]);
    // Spring: Saturday 03-29 service; service time counts from noon − 12 h (00:00 +02
    // on 03-29), so 26:30 = 03-30 00:30 UTC, before the 01:00 UTC change: the wall
    // clock shows 02:30 +02 (EET). The deadline is that same instant.
    let spring = plan_arrive_stops(&e, A, B, at(2031, 3, 30, 2, 30, 0, 2));
    assert_eq!(latest(&spring).unwrap().1, vec!["n"]);
    let spring = spring.unwrap();
    assert!(
        same(spring[0].arr, at(2031, 3, 30, 2, 30, 0, 2)),
        "{:?}",
        spring[0]
    );
    assert!(
        same(spring[0].dep, at(2031, 3, 30, 1, 30, 0, 2)),
        "{:?}",
        spring[0]
    );
    // One second earlier the trip no longer makes it (and nothing else runs): no route.
    assert!(is_no_route(&plan_arrive_stops(
        &e,
        A,
        B,
        at(2031, 3, 30, 2, 29, 59, 2)
    )));
    // Autumn: Saturday 10-25 service, 26:30 = 01:30 +02 + ... on a 25 h day.
    let t = service_instant(BUCHAREST, date(2031, 10, 25), hms(26, 30, 0));
    let autumn = plan_arrive_stops(&e, A, B, t);
    assert_eq!(latest(&autumn).unwrap().1, vec!["n"]);
    assert_eq!(autumn.unwrap()[0].arr, t);
}

// ---- after midnight ---------------------------------------------------------------

#[test]
fn arrive_by_00_30_served_by_a_d_minus_1_trip() {
    let e = line(
        "20310101",
        "20310131",
        &[
            ("night", "24:05:00", "24:25:00"),
            ("day", "08:00:00", "08:30:00"),
        ],
    );
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 16, 0, 30, 0, 2));
    let (dep, trips) = latest(&r).unwrap();
    assert_eq!(trips, vec!["night"]);
    assert!(same(dep, at(2031, 1, 16, 0, 5, 0, 2)), "{dep}");
}

#[test]
fn arrive_by_23_50_where_the_only_trip_is_on_d() {
    let e = line("20310101", "20310131", &[("eve", "23:00:00", "23:40:00")]);
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 16, 23, 50, 0, 2));
    let (dep, _) = latest(&r).unwrap();
    assert!(same(dep, at(2031, 1, 16, 23, 0, 0, 2)));
}

#[test]
fn trips_up_to_the_48_h_cap() {
    // M7: times stop below 48:00. A 47:10 → 47:50 trip is day D+1 23:10 → 23:50.
    let e = line("20310101", "20310131", &[("late", "47:10:00", "47:50:00")]);
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 16, 23, 55, 0, 2));
    let (dep, trips) = latest(&r).unwrap();
    assert_eq!(trips, vec!["late"]);
    assert!(same(dep, at(2031, 1, 16, 23, 10, 0, 2)), "{dep}");
}

// ---- no route ---------------------------------------------------------------------

#[test]
fn no_route_cases() {
    let e = line("20310101", "20310131", &[("t", "08:00:00", "08:30:00")]);
    // Before the first trip of the day, D-1's trip arrives by the deadline (valid,
    // Leader's ruling; checked: 01-14 08:00 → 08:30).
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 15, 7, 0, 0, 2)).expect("D-1 trip");
    assert_eq!(r[0].trips(), vec!["t"]);
    assert!(same(r[0].dep, at(2031, 1, 14, 8, 0, 0, 2)), "{:?}", r[0]);
    // On the first feed day D-1 is outside the feed: truly unreachable.
    assert!(is_no_route(&plan_arrive_stops(
        &e,
        A,
        B,
        at(2031, 1, 1, 7, 0, 0, 2)
    )));
    // Unreachable destination (stop C has no trips; 111 km away, no walk).
    let f = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "t", &[("08:00:00", "A"), ("08:30:00", "B")])
        .trip("S", "u", &[("09:00:00", "C"), ("09:30:00", "D")]);
    let e2 = engine(f.pack());
    assert!(is_no_route(&plan_arrive_stops(
        &e2,
        A,
        C,
        at(2031, 1, 15, 12, 0, 0, 2)
    )));
}

#[test]
fn date_with_no_service_is_no_route() {
    let f = Feed::daily(BUCHAREST, "20310101", "20310131")
        .service("WK", "1111100", "20310101", "20310131")
        .trip("WK", "t", &[("08:00:00", "A"), ("08:30:00", "B")]);
    let e = engine(f.pack());
    // Sunday 2031-01-19 08:45: Saturday and Sunday have no trip; Friday's is > 48 h back.
    assert!(is_no_route(&plan_arrive_stops(
        &e,
        A,
        B,
        at(2031, 1, 19, 8, 45, 0, 2)
    )));
}

#[test]
fn date_outside_the_feed_is_decided_on_the_local_date() {
    let e = line("20310110", "20310131", &[("t", "08:00:00", "08:30:00")]);
    // One day before valid_from (local date 2031-01-09).
    assert!(is_date_outside(&plan_arrive_stops(
        &e,
        A,
        B,
        at(2031, 1, 9, 9, 0, 0, 2)
    )));
    // 2031-01-09 23:30 UTC is 2031-01-10 01:30 local: inside (but no route).
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 9, 23, 30, 0, 0));
    assert!(!is_date_outside(&r), "{r:?}");
    assert!(is_date_outside(&plan_arrive_stops(
        &e,
        A,
        B,
        at(2031, 2, 2, 9, 0, 0, 2)
    )));
}

// ---- tie-breaks ---------------------------------------------------------------------

fn ties() -> Feed {
    // x and y: same departure and arrival A→C directly; z: same departure, via B
    // (two rides) arriving at the same time.
    Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "y", &[("08:00:00", "A"), ("08:40:00", "C")])
        .trip("S", "x", &[("08:00:00", "A"), ("08:40:00", "C")])
        .trip("S", "z1", &[("08:00:00", "A"), ("08:15:00", "B")])
        .trip("S", "z2", &[("08:20:00", "B"), ("08:40:00", "C")])
}

#[test]
fn tie_breaks_fewer_rides_then_lowest_index_and_stable() {
    let p = ties().pack();
    let e = engine(ties().pack());
    let t = at(2031, 1, 15, 9, 0, 0, 2);
    let r = plan_arrive_stops(&e, A, C, t).unwrap();
    let first = &r[0];
    assert_eq!(
        first.rides, 1,
        "fewer rides wins on equal departure and arrival: {r:?}"
    );
    // Equal everything: lowest trip index in the pack.
    let idx = |id: &str| p.trip_ids.iter().position(|x| x == id).unwrap();
    let want = if idx("x") < idx("y") { "x" } else { "y" };
    assert_eq!(first.trips(), vec![want]);
    // Stable across runs and across a rebuild of the pack.
    for _ in 0..5 {
        assert_eq!(plan_arrive_stops(&e, A, C, t).unwrap(), r);
    }
    let e2 = engine(ties().pack());
    assert_eq!(plan_arrive_stops(&e2, A, C, t).unwrap(), r);
}

// ---- walking only -----------------------------------------------------------------------

#[test]
fn walking_only_cases() {
    let e = line("20310101", "20310131", &[("t", "08:00:00", "08:30:00")]);
    let t = at(2031, 1, 15, 9, 0, 0, 2);
    // Origin == destination: a zero walk (or one empty itinerary), never NoRoute.
    let r = plan_arrive(&e, A, A, t).expect("origin == destination");
    assert!(r[0].rides == 0 && r[0].arr <= t);
    // Origin within walking distance of the destination, no stop near: walk only.
    let near = (5.0, 5.0);
    let near2 = (5.0, 5.003); // ~330 m
    let r = plan_arrive(&e, near, near2, t).expect("walk");
    assert_eq!(r[0].rides, 0);
    assert!(r[0].arr <= t);
    // No stop within access distance and too far to walk: no route.
    assert!(is_no_route(&plan_arrive(&e, (7.0, 7.0), (8.0, 8.0), t)));
}

#[test]
fn walk_vs_transit_only_when_competitive() {
    // A at (1,1), stop AA 0.5° north is far; build a short hop: stops are 111 km
    // apart in this builder, so "walk wins" is checked with points between which
    // walking is fast and the only trip leaves too early (transit loses).
    let e = line("20310101", "20310131", &[("t", "06:00:00", "06:10:00")]);
    let t = at(2031, 1, 15, 9, 0, 0, 2);
    let r = plan_arrive(&e, (1.0, 1.0), (1.0, 1.002), t).unwrap();
    assert_eq!(
        r[0].rides, 0,
        "a 220 m walk beats a 06:00 trip for a 09:00 deadline"
    );
}

// ---- frequencies, transfers, limits --------------------------------------------------------

#[test]
fn frequency_trips_exact_and_approximate() {
    let f = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "fx", &[("06:00:00", "A"), ("06:20:00", "B")])
        .freq("fx", "06:00:00", "10:00:00", "600", "1");
    let e = engine(f.pack());
    let r = plan_arrive_stops(&e, A, B, at(2031, 1, 15, 9, 5, 0, 2));
    let (dep, _) = latest(&r).unwrap();
    assert!(same(dep, at(2031, 1, 15, 8, 40, 0, 2)), "exact: {dep}");
    let g = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "fa", &[("06:00:00", "A"), ("06:20:00", "B")])
        .freq("fa", "06:00:00", "10:00:00", "600", "0");
    let e = engine(g.pack());
    let mut rq = req(A, B, at(2031, 1, 15, 9, 5, 0, 2));
    rq.time = arrive_by(at(2031, 1, 15, 9, 5, 0, 2));
    let resp = e.plan(&rq).unwrap();
    assert!(resp
        .warnings
        .iter()
        .any(|w| matches!(w, rustline_core::Warning::ApproximateFrequencyTrip)));
}

fn transfer_feed(transfers: &str) -> rustline_core::Engine {
    let f = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "r1", &[("08:00:00", "A"), ("08:20:00", "B")])
        .trip("S", "r2", &[("08:21:00", "B"), ("08:40:00", "C")])
        .trip("S", "r3", &[("08:30:00", "B"), ("08:50:00", "C")])
        .transfers(transfers);
    engine(f.pack())
}

#[test]
fn transfer_type_2_without_min_time_and_type_3_forbidden() {
    let t = at(2031, 1, 15, 9, 0, 0, 2);
    // Type 2 with no min_transfer_time: M5 rule applies (same as depart-at).
    let e = transfer_feed("from_stop_id,to_stop_id,transfer_type,min_transfer_time\nB,B,2,\n");
    let depart = plan(
        &e,
        &stop_req(
            A,
            C,
            rustline_core::TimeSpec::DepartAt(at(2031, 1, 15, 7, 59, 0, 2)),
        ),
    )
    .unwrap();
    let back = plan_arrive_stops(&e, A, C, t).unwrap();
    assert!(
        back.iter().any(|i| i.trips() == depart[0].trips()),
        "arrive-by and depart-at agree on transfer rules"
    );
    // Type 3 (not possible) on the only transfer: no route.
    let f = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "r1", &[("08:00:00", "A"), ("08:20:00", "B")])
        .trip("S", "r3", &[("08:30:00", "B"), ("08:50:00", "C")])
        .transfers("from_stop_id,to_stop_id,transfer_type,min_transfer_time\nB,B,3,\n");
    assert!(is_no_route(&plan_arrive_stops(&engine(f.pack()), A, C, t)));
}

#[test]
fn max_transfers_zero_and_the_round_cap() {
    let e = transfer_feed("from_stop_id,to_stop_id,transfer_type,min_transfer_time\n");
    let t = at(2031, 1, 15, 9, 0, 0, 2);
    let mut r0 = stop_req(A, C, arrive_by(t));
    r0.max_transfers = 0;
    assert!(is_no_route(&plan(&e, &r0)), "A→C needs one transfer");
    // A huge limit is capped (no panic, rides bounded by cap + 1).
    let mut big = stop_req(A, C, arrive_by(t));
    big.max_transfers = u8::MAX;
    match plan(&e, &big) {
        Ok(its) => assert!(its.iter().all(|i| i.rides <= 9)),
        Err(x) => assert!(
            matches!(x, rustline_core::PlanError::InvalidRequest(_)),
            "{x:?}"
        ),
    }
}

#[test]
fn deadline_in_the_past_is_allowed() {
    // The device clock plays no part: a deadline years ago inside the feed plans.
    let e = line("20200101", "20200131", &[("t", "08:00:00", "08:30:00")]);
    assert!(plan_arrive_stops(&e, A, B, at(2020, 1, 15, 9, 0, 0, 2)).is_ok());
}

#[test]
fn non_romanian_zone() {
    let f = Feed::daily("America/New_York", "20310301", "20310331").trip(
        "S",
        "t",
        &[("01:30:00", "A"), ("03:15:00", "B")],
    );
    let e = engine(f.pack());
    // 2031-03-09 is the US spring-forward day: 03:15 EDT (-04).
    let r = plan_arrive_stops(&e, A, B, at(2031, 3, 9, 3, 20, 0, -4));
    let (dep, _) = latest(&r).unwrap();
    assert_eq!(r.unwrap()[0].arr.offset().local_minus_utc(), -4 * 3600);
    println!("NY spring day: leaves {dep}");
}

#[test]
fn long_chain_reconstruction_is_bounded_and_fast() {
    // 60 single-hop trips in a chain A→…; with max_transfers 3 at most 4 rides.
    let names: Vec<String> = (0..26).map(|i| ((b'A' + i) as char).to_string()).collect();
    let mut f = Feed::daily(BUCHAREST, "20310101", "20310131");
    for (i, w) in names.windows(2).enumerate() {
        let (a, b) = (
            format!("{:02}:{:02}:00", 6 + i / 6, (i % 6) * 10),
            format!("{:02}:{:02}:00", 6 + i / 6, (i % 6) * 10 + 5),
        );
        f = f.trip("S", &format!("h{i}"), &[(&a, &w[0]), (&b, &w[1])]);
    }
    let e = engine(f.pack());
    let t0 = std::time::Instant::now();
    let r = plan_arrive_stops(
        &e,
        stop_pos("A"),
        stop_pos("Z"),
        at(2031, 1, 15, 23, 0, 0, 2),
    );
    assert!(t0.elapsed() < std::time::Duration::from_secs(2));
    assert!(is_no_route(&r) || r.unwrap().iter().all(|i| i.rides <= 4));
}

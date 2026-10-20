//! M8 tests-first (Tester, 2026-10-20): arrive-by through the FFI (plan M8 "FFI +
//! Dart") on the tiny fixture: same answer as rustline-core, arrives by the deadline,
//! the default kind is still depart-at, and arrive-by errors map to the M6 codes.
//! Ungated in M8 (Builder removed `cfg rustline_m8` and the "M8 not implemented" ignores).
//! Run: `cargo test --locked -p rustline-ffi --test m8_arrive_ffi`
#![allow(unexpected_cfgs)]

mod ffi_common;
mod m8_ffi;
use ffi_common::*;
use m8_ffi::*;

#[test]
fn arrive_by_matches_core_and_meets_the_deadline() {
    let d = scratch("m8ffi");
    let e = open(&tiny_pack_file(&d)).unwrap();
    let fwd = plan(&e, tiny_req()).unwrap();
    let first = &fwd.itineraries[0];
    // Deadline = the depart-at arrival, given in the arrival's offset.
    let r = arrive_by(req(
        TINY_A,
        TINY_D,
        (first.arrival_ms, first.arrival_offset_secs),
    ));
    let back = plan(&e, r.clone()).expect("arrive-by at a reachable arrival");
    assert!(!back.itineraries.is_empty());
    for i in &back.itineraries {
        assert!(i.arrival_ms <= first.arrival_ms, "{i:?}");
    }
    assert!(back
        .itineraries
        .iter()
        .any(|i| i.departure_ms >= first.departure_ms));
    let ffi: Vec<_> = summary(&back)
        .into_iter()
        .map(|(a, b, c, d, _)| (a, b, c, d))
        .collect();
    assert_eq!(ffi, core_arrive_by(&tiny_pack_bytes(), &r));
}

#[test]
fn default_kind_is_depart_at() {
    assert!(is_depart_at(&tiny_req()));
}

#[test]
fn arrive_by_errors_use_the_m6_codes() {
    let d = scratch("m8ffierr");
    let e = open(&tiny_pack_file(&d)).unwrap();
    // 00:10 on 2030-01-02 is answered (a 33 min walk-only A → D, ≤ 3 km, mirrored
    // from depart-at): not a no-route case.
    let early = arrive_by(req(TINY_A, TINY_D, when(2030, 1, 2, 0, 10, -5)));
    let ok = plan(&e, early).expect("walk-only answer");
    assert!(ok
        .itineraries
        .iter()
        .all(|i| i.arrival_ms <= when(2030, 1, 2, 0, 10, -5).0));
    // Truly unreachable: stop F is 11 km from A (no walk-only), served by one weekday
    // trip at 08:00; 2030-01-02 07:00 is before it and D-1 (2030-01-01) has no weekday
    // service (holiday exception) → no_route.
    let far = far_pack_file(&d);
    let ef = open(&far).unwrap();
    let r = arrive_by(req(TINY_A, (10.1, 20.0), when(2030, 1, 2, 7, 0, -5)));
    assert_eq!(err_code(plan(&ef, r)), "no_route");
    // Control: the same query by 09:00 finds that trip.
    let r = arrive_by(req(TINY_A, (10.1, 20.0), when(2030, 1, 2, 9, 0, -5)));
    let got = plan(&ef, r).expect("the 08:00 trip");
    assert!(got.itineraries[0].departure_ms <= when(2030, 1, 2, 8, 0, -5).0);
    // Far outside the feed: date_outside_feed.
    let out = arrive_by(req(TINY_A, TINY_D, when(2001, 1, 2, 9, 0, -5)));
    assert_eq!(err_code(plan(&e, out)), "date_outside_feed");
    // Bad offset is still invalid_request with arrive-by.
    let mut bad = arrive_by(tiny_req());
    bad.time_offset_secs = 99 * 3600;
    assert_eq!(err_code(plan(&e, bad)), "invalid_request");
}

/// The tiny fixture plus stop F (10.1, 20.0), 11 km north of A, with one weekday
/// trip A 08:00 → F 08:40.
fn far_pack_file(dir: &std::path::Path) -> String {
    let src = workspace("data/fixtures/tiny");
    let g = dir.join("far_gtfs");
    std::fs::create_dir_all(&g).unwrap();
    for f in std::fs::read_dir(&src).unwrap() {
        let f = f.unwrap().path();
        std::fs::copy(&f, g.join(f.file_name().unwrap())).unwrap();
    }
    let add = |name: &str, line: &str| {
        let p = g.join(name);
        let mut s = std::fs::read_to_string(&p).unwrap();
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s.push_str(line);
        std::fs::write(p, s).unwrap();
    };
    add("stops.txt", "F,Foxtrot,10.1000,20.0000\n");
    let route = std::fs::read_to_string(g.join("trips.txt")).unwrap();
    let route_id = route
        .lines()
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap()
        .to_string();
    let header: Vec<String> = route
        .lines()
        .next()
        .unwrap()
        .split(',')
        .map(str::to_string)
        .collect();
    let row: Vec<&str> = header
        .iter()
        .map(|h| match h.as_str() {
            "route_id" => route_id.as_str(),
            "service_id" => "WK",
            "trip_id" => "FAR",
            _ => "",
        })
        .collect();
    add("trips.txt", &format!("{}\n", row.join(",")));
    add(
        "stop_times.txt",
        "FAR,08:00:00,08:00:00,A,1\nFAR,08:40:00,08:40:00,F,2\n",
    );
    let raw = rustline_gtfs::load(&g).expect("far loads");
    let (tt, _) = rustline_gtfs::build_timetable(&raw).expect("far builds");
    let meta = rustline_gtfs::pack_meta(&tt, [0; 32], "far", 0);
    let data = rustline_gtfs::try_timetable_data(&tt).expect("pack arrays");
    let mut out = Vec::new();
    rustline_pack::write_pack(&data, &meta, &mut out).expect("write pack");
    put(dir, "far.rlp", &out)
}

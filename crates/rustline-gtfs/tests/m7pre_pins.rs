//! M7 tests-first (Tester, 2026-10-20): behaviour the M7 own reader must keep (plan M7
//! edge cases). Not gated: these pass on the M6 reader and must still pass after M7.
//! Run: `cargo test --locked -p rustline-gtfs --test m7pre_pins`

mod m7_common;
use m7_common::*;

fn clean_two_stops(f: &Feed) {
    let (tt, rep) = ingest(f).expect("feed ingests");
    assert!(rep.is_clean(), "{rep:?}");
    assert_eq!(
        times(&tt, "T"),
        vec![(hms(8, 0, 0), hms(8, 0, 0)), (hms(8, 5, 0), hms(8, 5, 0))]
    );
}

#[test]
fn bom_in_header() {
    let mut b = vec![0xEF, 0xBB, 0xBF];
    b.extend_from_slice(
        st(&[
            ("08:00:00", "08:00:00", "A", 1),
            ("08:05:00", "08:05:00", "B", 2),
        ])
        .as_bytes(),
    );
    let mut t = vec![0xEF, 0xBB, 0xBF];
    t.extend_from_slice(TRIPS.as_bytes());
    clean_two_stops(&Feed::new(&[("stop_times.txt", &b), ("trips.txt", &t)]));
}

#[test]
fn columns_in_any_order_and_extra_unknown_columns() {
    let body = "stop_sequence,stop_id,zz_unknown,departure_time,arrival_time,trip_id,shape_dist_traveled\n\
                1,A,x,08:00:00,08:00:00,T,0\n2,B,y,08:05:00,08:05:00,T,10\n";
    let trips = "zz,trip_id,service_id,route_id\nq,T,S,R\n";
    clean_two_stops(&Feed::new(&[
        ("stop_times.txt", body.as_bytes()),
        ("trips.txt", trips.as_bytes()),
    ]));
}

#[test]
fn crlf_line_endings() {
    let body = st(&[
        ("08:00:00", "08:00:00", "A", 1),
        ("08:05:00", "08:05:00", "B", 2),
    ])
    .replace('\n', "\r\n");
    let trips = TRIPS.replace('\n', "\r\n");
    clean_two_stops(&Feed::new(&[
        ("stop_times.txt", body.as_bytes()),
        ("trips.txt", trips.as_bytes()),
    ]));
}

#[test]
fn quoted_fields_with_commas_and_newlines() {
    let trips =
        "route_id,service_id,trip_id,trip_headsign\nR,S,T,\"Gara, Peron 1\nvia \"\"Centru\"\"\"\n";
    let body = "trip_id,arrival_time,departure_time,stop_id,stop_sequence,stop_headsign\n\
                \"T\",08:00:00,08:00:00,A,1,\"a, b\"\nT,\"08:05:00\",08:05:00,B,2,\"x\ny\"\n";
    let f = Feed::new(&[
        ("stop_times.txt", body.as_bytes()),
        ("trips.txt", trips.as_bytes()),
    ]);
    clean_two_stops(&f);
}

#[test]
fn stop_sequence_gaps_are_allowed() {
    let f = Feed::stop_times(&st(&[
        ("08:00:00", "08:00:00", "A", 3),
        ("08:05:00", "08:05:00", "B", 70),
        ("08:09:00", "08:09:00", "C", 1000),
    ]));
    let (tt, rep) = ingest(&f).unwrap();
    assert!(rep.is_clean(), "{rep:?}");
    assert_eq!(times(&tt, "T").len(), 3);
}

#[test]
fn times_only_on_first_and_last_stop_interpolate_as_in_m6() {
    // M6: even by stop index, truncated to the second, reported as InterpolatedTimes.
    let f = Feed::stop_times(&st(&[
        ("08:00:00", "08:00:00", "A", 1),
        ("", "", "B", 2),
        ("", "", "C", 3),
        ("08:10:00", "08:10:00", "D", 4),
    ]));
    let (tt, rep) = ingest(&f).unwrap();
    let t: Vec<u32> = times(&tt, "T").iter().map(|x| x.0).collect();
    assert_eq!(
        t,
        vec![hms(8, 0, 0), hms(8, 3, 20), hms(8, 6, 40), hms(8, 10, 0)]
    );
    assert!(rep.count(IssueKind::InterpolatedTimes) >= 1);
}

#[test]
fn header_only_files_give_no_trips_without_panicking() {
    let f = Feed::new(&[
        ("stop_times.txt", ST_HEADER.as_bytes()),
        ("trips.txt", b"route_id,service_id,trip_id\n"),
    ]);
    match ingest_no_panic(&f) {
        Ok((tt, _)) => assert!(tt.trips.is_empty()),
        Err(e) => println!("header-only feed is an error: {e}"),
    }
}

#[test]
fn row_with_fewer_fields_than_header_does_not_panic() {
    // Gated m7_reader::short_rows_load_and_are_reported wants Ok; today it may fail.
    let body = "trip_id,arrival_time,departure_time,stop_id,stop_sequence,pickup_type\nT,08:00:00,08:00:00,A,1,\nT,08:05:00,08:05:00,B,2\n";
    let _ = ingest_no_panic(&Feed::stop_times(body));
}

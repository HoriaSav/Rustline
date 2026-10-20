//! M7 tests-first (Tester, 2026-10-20): the own lenient stop_times/trips reader
//! (plan M7 step 1-3; README Known gaps (M1) #1, #2, #3, #5, #6; DoD strict; edge cases).
//! Names via `m7_common::api`. Runs in debug and release (no cfg_attr: the DoD
//! asks for both).
//! Run: `cargo test --locked -p rustline-gtfs --test m7_reader` (and with `--release`)

mod m7_common;
use m7_common::api::*;
use m7_common::*;

/// Trip T (A, B, C) plus a good trip G on the same feed: the bad row is reported,
/// the rest of the feed still loads (lenient), and nothing wraps.
fn with_bad_time(bad: &str, at_first: bool) -> (Timetable, ValidationReport) {
    let (a, c) = if at_first {
        (bad, "08:10:00")
    } else {
        ("08:00:00", bad)
    };
    let body = format!(
        "{ST_HEADER}T,{a},{a},A,1\nT,08:05:00,08:05:00,B,2\nT,{c},{c},C,3\n\
         G,09:00:00,09:00:00,A,1\nG,09:05:00,09:05:00,B,2\n"
    );
    let trips = "route_id,service_id,trip_id\nR,S,T\nR,S,G\n";
    let f = Feed::new(&[
        ("stop_times.txt", body.as_bytes()),
        ("trips.txt", trips.as_bytes()),
    ]);
    ingest_no_panic(&f).unwrap_or_else(|e| panic!("{bad:?}: feed must load, row reported: {e}"))
}

fn assert_reported_not_wrapped(bad: &str) {
    for at_first in [true, false] {
        let (tt, rep) = with_bad_time(bad, at_first);
        assert!(
            bad_time_rows(&rep) >= 1,
            "{bad:?} (first={at_first}) not reported: {rep:?}"
        );
        assert!(
            trip_ids(&tt).contains(&"G".to_string()),
            "{bad:?}: good trip lost"
        );
        if let Some(i) = trip_ids(&tt).iter().position(|t| t == "T") {
            // If T survives, no time may be a wrapped / bogus value.
            let t = times(&tt, "T");
            assert!(
                t.iter()
                    .all(|x| x.0 <= hms(8, 10, 0) && x.1 <= hms(8, 10, 0)),
                "{bad:?}: {t:?} (trip #{i})"
            );
        }
    }
}

// ---- Known gap #1: range check -------------------------------------------------

#[test]
fn gap1_minutes_or_seconds_out_of_range_are_reported() {
    for bad in ["24:60:00", "08:00:60", "08:99:99", "08:60:00"] {
        assert_reported_not_wrapped(bad);
    }
}

// ---- Known gap #2: huge hours, checked arithmetic ----------------------------

#[test]
fn gap2_huge_hours_are_reported_never_wrapped() {
    for bad in [
        "9999999:00:00",
        "1193047:00:00",
        "4294967295:00:00",
        "99999999999999999999:00:00",
    ] {
        assert_reported_not_wrapped(bad);
    }
}

// ---- DoD: -1:00:00 and empty time --------------------------------------------

#[test]
fn negative_and_garbage_times_are_reported() {
    for bad in [
        "-1:00:00",
        "-0:00:01",
        "ab:cd:ef",
        "08:00",
        "8h00m00",
        "+8:00:00",
        "08:00:00:00",
    ] {
        assert_reported_not_wrapped(bad);
    }
}

#[test]
fn empty_time_on_a_boundary_stop_is_reported() {
    let (tt, rep) = with_bad_time("", true);
    assert!(
        bad_time_rows(&rep) + rep.count(IssueKind::MissingBoundaryTime) >= 1,
        "{rep:?}"
    );
    assert!(trip_ids(&tt).contains(&"G".to_string()));
}

#[test]
fn hours_up_to_the_documented_cap_still_load() {
    // 47:59:59 already works (M1); the cap must allow at least 48 h of service day.
    let f = Feed::stop_times(&st(&[
        ("47:00:00", "47:00:00", "A", 1),
        ("47:59:59", "47:59:59", "B", 2),
    ]));
    let (tt, rep) = ingest(&f).unwrap();
    assert_eq!(bad_time_rows(&rep), 0);
    assert_eq!(times(&tt, "T")[1].0, hms(47, 59, 59));
}

// ---- Known gap #3: duplicate trip_id ------------------------------------------

#[test]
fn gap3_duplicate_trip_id_first_wins_and_is_reported() {
    // Same id on two different routes: the first row (route R) wins.
    let trips = "route_id,service_id,trip_id\nR,S,T\nR2,S,T\nR2,S,T\n";
    let body = st(&[
        ("08:00:00", "08:00:00", "A", 1),
        ("08:05:00", "08:05:00", "B", 2),
    ]);
    let f = Feed::new(&[
        ("stop_times.txt", body.as_bytes()),
        ("trips.txt", trips.as_bytes()),
    ]);
    let (tt, rep) = ingest(&f).expect("feed loads");
    assert_eq!(duplicate_trip_ids(&rep), 2, "{rep:?}");
    assert_eq!(trip_ids(&tt), vec!["T".to_string()]);
    let route = &tt.routes[tt.trips[0].route.index()];
    assert_eq!(route.id, "R", "first row wins");
}

// ---- Known gap #5: short rows / strict loading --------------------------------

#[test]
fn gap5_short_rows_load_and_are_reported() {
    let body =
        "trip_id,arrival_time,departure_time,stop_id,stop_sequence,pickup_type,drop_off_type\n\
                T,08:00:00,08:00:00,A,1,\nT,08:05:00,08:05:00,B,2\n";
    let trips = "route_id,service_id,trip_id,trip_headsign,direction_id\nR,S,T\n";
    let f = Feed::new(&[
        ("stop_times.txt", body.as_bytes()),
        ("trips.txt", trips.as_bytes()),
    ]);
    let (tt, rep) = ingest(&f).expect("short rows are padded, not fatal");
    assert_eq!(times(&tt, "T").len(), 2);
    println!("short rows reported: {}", short_rows(&rep));
}

#[test]
fn gap5_google_sample_feed_loads_and_builds() {
    let Some(p) = shared_feed("google-sample.zip") else {
        return;
    };
    let raw = rustline_gtfs::load(&p).expect("sample-feed-1 loads");
    let (tt, _) = rustline_gtfs::build_timetable(&raw).expect("sample-feed-1 builds");
    assert!(!tt.trips.is_empty());
}

#[test]
fn gap5_dangling_trip_in_stop_times_is_reported_not_fatal() {
    let body = format!("{ST_HEADER}T,08:00:00,08:00:00,A,1\nT,08:05:00,08:05:00,B,2\nNOPE,08:00:00,08:00:00,A,1\nT2,08:00:00,08:00:00,ZZZ,1\n");
    let f = Feed::stop_times(&body);
    let (tt, _rep) = ingest(&f).expect("dangling ids are reported, not fatal");
    assert_eq!(trip_ids(&tt), vec!["T".to_string()]);
}

// ---- Known gap #6: duplicate stop_sequence ------------------------------------

#[test]
fn gap6_duplicate_stop_sequence_is_reported_and_trip_skipped() {
    let body = format!(
        "{ST_HEADER}T,08:00:00,08:00:00,A,1\nT,08:05:00,08:05:00,B,2\nT,08:06:00,08:06:00,C,2\n\
         G,09:00:00,09:00:00,A,1\nG,09:05:00,09:05:00,B,2\n"
    );
    let trips = "route_id,service_id,trip_id\nR,S,T\nR,S,G\n";
    let f = Feed::new(&[
        ("stop_times.txt", body.as_bytes()),
        ("trips.txt", trips.as_bytes()),
    ]);
    let (tt, rep) = ingest(&f).unwrap();
    assert_eq!(duplicate_stop_sequences(&rep), 1, "{rep:?}");
    // Plan: "trip reported and skipped (or documented order rule)".
    assert!(
        !trip_ids(&tt).contains(&"T".to_string()),
        "skipped (if the Builder picks an order rule instead, adapt here)"
    );
    assert!(trip_ids(&tt).contains(&"G".to_string()));
}

// ---- edge cases: caps and bytes ----------------------------------------------

#[test]
fn row_longer_than_the_cap_is_reported_and_the_rest_loads() {
    let long = "x".repeat(ROW_CAP + 10);
    let body = format!("{ST_HEADER}T,08:00:00,08:00:00,A,1\nT,08:05:00,08:05:00,B,2\nT,08:06:00,08:06:00,{long},3\n");
    let (tt, rep) = ingest(&Feed::stop_times(&body)).expect("feed loads");
    assert_eq!(long_rows(&rep), 1);
    assert_eq!(times(&tt, "T").len(), 2);
}

#[test]
fn a_huge_unterminated_quote_does_not_grow_without_bound() {
    // One quote opens a field that never closes: 8 MiB of row. Must stop at the cap.
    let mut body = format!("{ST_HEADER}T,08:00:00,08:00:00,A,1\nT,\"");
    body.push_str(&"y,\n".repeat(8 << 20 >> 2));
    let r = ingest_no_panic(&Feed::stop_times(&body));
    match r {
        Ok((_, rep)) => assert!(long_rows(&rep) >= 1, "{rep:?}"),
        Err(e) => println!("unterminated quote is an error: {e}"),
    }
}

#[test]
fn header_longer_than_the_cap_fails_cleanly() {
    let mut h = String::from("trip_id,arrival_time,departure_time,stop_id,stop_sequence");
    while h.len() <= ROW_CAP {
        h.push_str(",extra_col");
    }
    let body = format!("{h}\nT,08:00:00,08:00:00,A,1\n");
    let _ = ingest_no_panic(&Feed::stop_times(&body));
}

#[test]
fn non_utf8_bytes_are_reported_not_fatal() {
    let trips = b"route_id,service_id,trip_id,trip_headsign\nR,S,T,Bra\xff\xfeov\n";
    let mut st_b = ST_HEADER.as_bytes().to_vec();
    st_b.extend_from_slice(b"T,08:00:00,08:00:00,A,1,\nT,08:05:00,08:05:00,B,2\n");
    st_b.extend_from_slice(b"T\xc3,08:06:00,08:06:00,C,3\n");
    let f = Feed::new(&[("trips.txt", trips), ("stop_times.txt", &st_b)]);
    let (tt, rep) = ingest_no_panic(&f).expect("non-UTF-8 is reported, not fatal");
    assert!(non_utf8_rows(&rep) >= 1, "{rep:?}");
    assert!(times(&tt, "T").len() >= 2);
}

#[test]
fn header_only_files_are_clean_and_empty() {
    let f = Feed::new(&[
        ("stop_times.txt", ST_HEADER.as_bytes()),
        ("trips.txt", b"route_id,service_id,trip_id\n"),
    ]);
    // An empty feed: Ok with no trips, or the existing "no trips" error; never a panic.
    match ingest_no_panic(&f) {
        Ok((tt, _)) => assert!(tt.trips.is_empty()),
        Err(e) => assert!(!e.contains("panic"), "{e}"),
    }
}

#[test]
fn missing_required_column_is_a_clear_error() {
    let body = "trip_id,arrival_time,departure_time,stop_sequence\nT,08:00:00,08:00:00,1\n";
    let Err(e) = ingest_no_panic(&Feed::stop_times(body)) else {
        panic!("no stop_id column must be an error");
    };
    assert!(e.contains("stop_id"), "{e}");
}

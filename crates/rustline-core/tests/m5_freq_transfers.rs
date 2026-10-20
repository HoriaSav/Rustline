//! M5 frequencies.txt and transfers.txt tests (Tester, written before M5).
//! Plan M5 steps 5–6, DoD "transfers.txt types 0–3 implemented and tested; unknown
//! type → report warning, not failure", edge-case block "Frequencies and transfers".
//! Current APIs only (pack + stop-to-stop RAPTOR); the "M5 not implemented"
//! ignores were removed in M5. The warning-in-the-result tests are in m5_time.rs
//! (they need the M5 API). block_id: Known gap #7, pinned by M3's
//! `tester_m3::block_id_is_treated_as_a_normal_transfer`.
//! Run: `cargo test --locked -p rustline-core --test m5_freq_transfers -- --include-ignored`

mod m5_common;

use m5_common::*;

fn day() -> chrono::NaiveDate {
    date(2031, 1, 15)
}

/// Template trip F (A → B, 10 min) repeated by `rows` of frequencies.txt.
fn freq_feed(rows: &[(&str, &str, &str, &str)]) -> Feed {
    let mut f = Feed::daily(BUCHAREST, "20310101", "20310131").trip(
        "S",
        "F",
        &[("00:00:00", "A"), ("00:10:00", "B")],
    );
    for (a, b, h, e) in rows {
        f = f.freq("F", a, b, h, e);
    }
    f
}

/// Departure times (service seconds) of every trip in the pack, sorted.
fn starts(p: &rustline_pack::Pack) -> Vec<u32> {
    let mut v: Vec<u32> = (0..p.trip_count()).map(|t| p.trip_times(t)[0].2).collect();
    v.sort_unstable();
    v
}

#[test]
fn exact_times_1_trips_are_found_and_not_approximate() {
    let p = freq_feed(&[("06:00:00", "07:00:00", "1200", "1")]).pack();
    assert_eq!(starts(&p), vec![hms(6, 0, 0), hms(6, 20, 0), hms(6, 40, 0)]);
    assert!(p.trip_approximate.iter().all(|&a| a == 0));
    assert_eq!(
        earliest(&p, "A", "B", day(), hms(6, 1, 0)),
        Some(hms(6, 30, 0))
    );
    assert_eq!(earliest(&p, "A", "B", day(), hms(6, 41, 0)), None);
}

#[test]
fn exact_times_0_trips_are_found_and_marked_approximate() {
    let tf = freq_feed(&[("06:00:00", "07:00:00", "1200", "0")]).write();
    let (_, report) = tf.build().unwrap();
    assert_eq!(
        report.count(rustline_gtfs::IssueKind::ApproximateFrequencyTrips),
        3
    );
    let p = tf.pack();
    assert!(p.trip_approximate.iter().all(|&a| a == 1));
    assert_eq!(
        earliest(&p, "A", "B", day(), hms(6, 1, 0)),
        Some(hms(6, 30, 0))
    );
}

#[test]
fn headway_that_does_not_divide_the_window() {
    // 06:00–07:00 every 25 min: 06:00, 06:25, 06:50 (07:15 is past the window).
    let p = freq_feed(&[("06:00:00", "07:00:00", "1500", "1")]).pack();
    assert_eq!(starts(&p), vec![hms(6, 0, 0), hms(6, 25, 0), hms(6, 50, 0)]);
    assert_eq!(
        earliest(&p, "A", "B", day(), hms(6, 26, 0)),
        Some(hms(7, 0, 0))
    );
}

#[test]
fn overlapping_windows_give_one_answer_per_departure() {
    // 06:00–07:00 and 06:30–07:30 every 30 min: departures 06:00, 06:30 (twice), 07:00.
    let p = freq_feed(&[
        ("06:00:00", "07:00:00", "1800", "1"),
        ("06:30:00", "07:30:00", "1800", "1"),
    ])
    .pack();
    let st = starts(&p);
    assert!(
        st.contains(&hms(6, 30, 0)) && st.contains(&hms(7, 0, 0)),
        "{st:?}"
    );
    let r = rides(&p, "A", "B", day(), hms(6, 1, 0)).unwrap();
    assert_eq!(
        r.len(),
        1,
        "one Pareto answer, not one per duplicate trip: {r:?}"
    );
    assert_eq!(r[0].2, hms(6, 40, 0));
}

#[test]
fn window_crossing_midnight_on_the_same_service_day() {
    // 23:30–24:30 every 30 min: 23:30 and 24:00 belong to the service day.
    let p = freq_feed(&[("23:30:00", "24:30:00", "1800", "1")]).pack();
    assert_eq!(starts(&p), vec![hms(23, 30, 0), hms(24, 0, 0)]);
    assert_eq!(
        earliest(&p, "A", "B", day(), hms(23, 31, 0)),
        Some(hms(24, 10, 0))
    );
}

#[test]
fn headway_zero_is_rejected_without_hanging() {
    // The row is rejected (InvalidFrequency); the other row still expands.
    let tf = freq_feed(&[
        ("06:00:00", "07:00:00", "0", "1"),
        ("08:00:00", "08:30:00", "900", "1"),
    ])
    .write();
    let t0 = std::time::Instant::now();
    let (_, report) = tf.build().unwrap();
    assert!(t0.elapsed().as_secs() < 5);
    assert_eq!(report.count(rustline_gtfs::IssueKind::InvalidFrequency), 1);
    assert_eq!(starts(&tf.pack()), vec![hms(8, 0, 0), hms(8, 15, 0)]);
}

#[test]
fn a_realistic_dense_row_is_not_rejected_by_the_cap() {
    // 05:00–24:00 every minute = 1 140 trips: real feeds do this; the M5 cap (plan
    // §11) must stay above it.
    let tf = freq_feed(&[("05:00:00", "24:00:00", "60", "1")]).write();
    let (tt, _) = tf.build().unwrap();
    assert_eq!(tt.trips.len(), 1140);
}

// ---- transfers.txt ----------------------------------------------------------------

/// R1 A 08:00 → X 08:10; from X: Q1 08:10, Q2 08:12, Q3 08:20, Q4 09:30 to Z.
/// Y (111 km from X) has its own trips Y1 08:15, Y2 08:25 to Z; only transfers.txt
/// links X and Y (no street graph, no footpath).
fn transfer_feed(transfers: Option<&str>) -> Feed {
    let mut f = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "R1", &[("08:00:00", "A"), ("08:10:00", "X")])
        .trip("S", "Q1", &[("08:10:00", "X"), ("08:30:00", "Z")])
        .trip("S", "Q2", &[("08:12:00", "X"), ("08:32:00", "Z")])
        .trip("S", "Q3", &[("08:20:00", "X"), ("08:40:00", "Z")])
        .trip("S", "Q4", &[("09:30:00", "X"), ("09:50:00", "Z")])
        .trip("S", "Y1", &[("08:15:00", "Y"), ("08:45:00", "Z")])
        .trip("S", "Y2", &[("08:25:00", "Y"), ("08:55:00", "Z")]);
    if let Some(t) = transfers {
        f = f.transfers(t);
    }
    f
}

const TH: &str = "from_stop_id,to_stop_id,transfer_type,min_transfer_time\n";

fn arrive(transfers: Option<&str>) -> Option<u32> {
    earliest(
        &transfer_feed(transfers).pack(),
        "A",
        "Z",
        day(),
        hms(7, 0, 0),
    )
}

#[test]
fn no_transfers_row_changes_at_the_same_stop_in_0_s() {
    assert_eq!(arrive(None), Some(hms(8, 30, 0)));
}

#[test]
fn type_0_recommended_changes_like_no_row() {
    assert_eq!(arrive(Some(&format!("{TH}X,X,0,\n"))), Some(hms(8, 30, 0)));
}

#[test]
fn type_1_timed_transfer_without_time_is_0_min() {
    assert_eq!(arrive(Some(&format!("{TH}X,X,1,\n"))), Some(hms(8, 30, 0)));
}

#[test]
fn type_1_timed_transfer_is_0_min_even_with_a_min_transfer_time() {
    // Plan M5 step 6: "type 1 = timed transfer (treat as 0 min)". Today the row's
    // min_transfer_time (10 min) is used, giving Q3 (08:40).
    assert_eq!(
        arrive(Some(&format!("{TH}X,X,1,600\n"))),
        Some(hms(8, 30, 0))
    );
}

#[test]
fn type_2_min_transfer_time_0_missing_and_set() {
    assert_eq!(arrive(Some(&format!("{TH}X,X,2,0\n"))), Some(hms(8, 30, 0)));
    // Missing min_transfer_time on type 2 (required by GTFS): falls back to the
    // footpath time (0 s at the same stop) and warns (Leader, M5 decisions;
    // changed by Builder in M5, was "read as 0" without a warning).
    assert_eq!(arrive(Some(&format!("{TH}X,X,2,\n"))), Some(hms(8, 30, 0)));
    let (_, report) = transfer_feed(Some(&format!("{TH}X,X,2,\n")))
        .write()
        .build()
        .unwrap();
    assert_eq!(
        report.count(rustline_gtfs::IssueKind::MissingMinTransferTime),
        1
    );
    assert_eq!(
        arrive(Some(&format!("{TH}X,X,2,120\n"))),
        Some(hms(8, 32, 0))
    );
    assert_eq!(
        arrive(Some(&format!("{TH}X,X,2,600\n"))),
        Some(hms(8, 40, 0))
    );
}

#[test]
fn type_2_very_large_min_transfer_time_does_not_overflow() {
    // 2 days and u32::MAX: no change possible today, no panic in debug.
    assert_eq!(arrive(Some(&format!("{TH}X,X,2,172800\n"))), None);
    assert_eq!(arrive(Some(&format!("{TH}X,X,2,{}\n", u32::MAX))), None);
}

#[test]
fn type_2_between_two_stops_overrides_the_missing_footpath() {
    // X → Y in 5 min: Y1 at 08:15 is caught (arrive 08:45 is later than Q1's 08:30,
    // so ask for Z via Y only by forbidding the change at X).
    let t = format!("{TH}X,X,3,\nX,Y,2,300\n");
    assert_eq!(arrive(Some(&t)), Some(hms(8, 45, 0)));
    let t = format!("{TH}X,X,3,\nX,Y,2,360\n");
    assert_eq!(arrive(Some(&t)), Some(hms(8, 55, 0)));
}

#[test]
fn type_3_forbids_the_change_including_stop_to_itself() {
    assert_eq!(arrive(Some(&format!("{TH}X,X,3,\n"))), None);
    let t = format!("{TH}X,X,3,\nX,Y,3,\n");
    assert_eq!(arrive(Some(&t)), None);
}

#[test]
fn unknown_transfer_types_4_and_5_do_not_fail_the_feed() {
    // DoD: unknown type → report warning, not failure. (The warning count is checked in
    // m5_time.rs via the adapter.) Types 4/5 (in-seat) are not supported: the change
    // at X still works as without the row.
    for kind in ["4", "5", "9"] {
        let tf = transfer_feed(Some(&format!("{TH}X,X,{kind},\n"))).write();
        let r = tf.build();
        assert!(r.is_ok(), "type {kind}: {:?}", r.err());
        assert_eq!(
            earliest(&tf.pack(), "A", "Z", day(), hms(7, 0, 0)),
            Some(hms(8, 30, 0)),
            "type {kind}"
        );
    }
}

#[test]
fn trip_to_trip_rows_do_not_act_as_stop_to_stop_rows() {
    // Edge-case block: trip-to-trip transfers (GTFS extension) → used, or ignored with
    // a warning. A "R1 → Q1 not possible" row must not forbid every change at X
    // (today the trip columns are dropped and it does: Known gap #8).
    let t = "from_stop_id,to_stop_id,transfer_type,min_transfer_time,from_trip_id,to_trip_id\n\
             X,X,3,,R1,Q1\n";
    let got = arrive(Some(t));
    assert!(
        got == Some(hms(8, 30, 0)) || got == Some(hms(8, 32, 0)),
        "{got:?} (08:30 = row ignored, 08:32 = row applied to R1→Q1 only)"
    );
}

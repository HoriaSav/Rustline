//! Edge cases for GTFS ingest, each on a small feed written to a temp dir:
//! time parsing, overtaking splits, interpolation, validation and timezone.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustline_gtfs::{
    build_timetable, load, GtfsError, IssueKind, Severity, Timetable, ValidationReport,
};
use rustline_model::{Seconds, TripIdx};

const AGENCY: &str = "agency_id,agency_name,agency_url,agency_timezone\nAG,Edge Transit,https://example.invalid/,Asia/Tokyo\n";
const STOPS: &str = "stop_id,stop_name,stop_lat,stop_lon\nA,A,1.0,1.0\nB,B,1.001,1.0\nC,C,1.002,1.0\nD,D,1.003,1.0\nE,E,1.004,1.0\n";
const ROUTES: &str =
    "route_id,agency_id,route_short_name,route_long_name,route_type\nR,AG,1,One,3\n";
const CALENDAR: &str = "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nS,1,1,1,1,1,1,1,20400101,20401231\n";

/// A feed directory under the system temp dir, removed on drop.
struct TempFeed(PathBuf);

impl TempFeed {
    fn new(files: &[(&str, &str)]) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustline_edge_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let defaults = [
            ("agency.txt", AGENCY),
            ("stops.txt", STOPS),
            ("routes.txt", ROUTES),
            ("calendar.txt", CALENDAR),
        ];
        for (name, body) in defaults {
            if !files.iter().any(|(n, _)| *n == name) {
                fs::write(dir.join(name), body).unwrap();
            }
        }
        for (name, body) in files {
            if !body.is_empty() {
                fs::write(dir.join(name), body).unwrap();
            }
        }
        TempFeed(dir)
    }
}

impl Drop for TempFeed {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Trips on route R / service S, one per id.
fn trips(ids: &[&str]) -> String {
    let mut s = String::from("route_id,service_id,trip_id\n");
    for id in ids {
        s.push_str(&format!("R,S,{id}\n"));
    }
    s
}

/// stop_times from `(trip, arrival, departure, stop, sequence)` rows.
fn stop_times(rows: &[(&str, &str, &str, &str, u32)]) -> String {
    let mut s = String::from("trip_id,arrival_time,departure_time,stop_id,stop_sequence\n");
    for (t, a, d, st, q) in rows {
        s.push_str(&format!("{t},{a},{d},{st},{q}\n"));
    }
    s
}

/// One trip `T` A→B with the given departure at A and arrival at B.
fn one_trip(dep: &str, arr: &str) -> TempFeed {
    TempFeed::new(&[
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[("T", dep, dep, "A", 1), ("T", arr, arr, "B", 2)]),
        ),
    ])
}

fn build(feed: &TempFeed) -> (Timetable, ValidationReport) {
    build_timetable(&load(&feed.0).expect("feed loads")).expect("feed builds")
}

fn trip(tt: &Timetable, id: &str) -> TripIdx {
    TripIdx::from_usize(tt.trips.iter().position(|t| t.id == id).expect("trip"))
}

fn times(tt: &Timetable, id: &str) -> Vec<(Seconds, Seconds)> {
    tt.trip_stop_times(trip(tt, id))
        .iter()
        .map(|s| (s.arrival, s.departure))
        .collect()
}

// ---- 4. time parsing -------------------------------------------------------

#[test]
fn times_far_past_midnight_and_single_digit_hours() {
    let feed = one_trip("5:03:00", "47:59:59");
    let (tt, report) = build(&feed);
    assert!(report.is_clean());
    let t = times(&tt, "T");
    assert_eq!(t[0].1, Seconds::hms(5, 3, 0));
    assert_eq!(t[1].0, Seconds::hms(47, 59, 59));
}

#[test]
fn time_25_30_keeps_the_service_day() {
    let feed = one_trip("25:30:00", "26:00:00");
    let (tt, _) = build(&feed);
    assert_eq!(times(&tt, "T")[0].0, Seconds(25 * 3600 + 30 * 60));
}

#[test]
fn time_with_surrounding_spaces_is_accepted() {
    let feed = one_trip(" 08:00:00", "08:10:00 ");
    let (tt, _) = build(&feed);
    let t = times(&tt, "T");
    assert_eq!(
        (t[0].0, t[1].0),
        (Seconds::hms(8, 0, 0), Seconds::hms(8, 10, 0))
    );
}

/// Lenient loading (Leader, M7): the feed loads, the row with the bad time is
/// dropped as `InvalidTime`, and trip T (left with one stop time) is dropped.
fn dropped(dep: &str) -> bool {
    let (tt, report) = build(&one_trip(dep, "09:00:00"));
    report.count(IssueKind::InvalidTime) == 1 && tt.trips.is_empty()
}

#[test]
fn garbage_times_drop_the_row() {
    for bad in ["-1", "ab:cd:ef", "08:00", "8h00m00", "08:0a:00"] {
        assert!(dropped(bad), "{bad:?} should be dropped");
    }
}

#[test]
fn out_of_range_minutes_or_seconds_drop_the_row() {
    for bad in ["24:60:00", "08:00:60", "08:99:99"] {
        assert!(dropped(bad), "{bad:?} should be dropped");
    }
}

#[test]
fn overflowing_hours_drop_the_row_without_panicking() {
    // 48:00:00 is the first hour over the cap (Leader, M7); 9999999 used to overflow.
    for bad in ["48:00:00", "9999999:00:00"] {
        let r = std::panic::catch_unwind(|| dropped(bad));
        assert_eq!(r.ok(), Some(true), "{bad}");
    }
}

// ---- 5/6. patterns and overtaking ------------------------------------------

fn three_stop_trips(rows: &[(&str, [&str; 3])]) -> TempFeed {
    let ids: Vec<&str> = rows.iter().map(|(id, _)| *id).collect();
    let mut st = Vec::new();
    for (id, t) in rows {
        for (i, (time, stop)) in t.iter().zip(["A", "B", "C"]).enumerate() {
            st.push((*id, *time, *time, stop, i as u32 + 1));
        }
    }
    TempFeed::new(&[
        ("trips.txt", &trips(&ids)),
        ("stop_times.txt", &stop_times(&st)),
    ])
}

#[test]
fn equal_times_do_not_split_and_ties_sort_by_id() {
    let feed = three_stop_trips(&[
        ("t2", ["08:00:00", "08:05:00", "08:10:00"]),
        ("t1", ["08:00:00", "08:05:00", "08:10:00"]),
        ("t0", ["07:00:00", "07:05:00", "07:10:00"]),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(tt.patterns.len(), 1);
    assert_eq!(report.count(IssueKind::OvertakingSplit), 0);
    let ids: Vec<&str> = tt.patterns[0]
        .trips
        .iter()
        .map(|&t| tt.trips[t.index()].id.as_str())
        .collect();
    assert_eq!(ids, ["t0", "t1", "t2"]);
}

#[test]
fn overtaking_only_at_the_last_stop_splits() {
    let feed = three_stop_trips(&[
        ("slow", ["08:00:00", "08:05:00", "08:30:00"]),
        ("fast", ["08:01:00", "08:06:00", "08:20:00"]),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(tt.patterns.len(), 2);
    assert_eq!(report.count(IssueKind::OvertakingSplit), 1);
}

#[test]
fn three_trips_each_overtaking_need_three_lanes() {
    let feed = three_stop_trips(&[
        ("a", ["08:00:00", "08:20:00", "08:40:00"]),
        ("b", ["08:01:00", "08:15:00", "08:30:00"]),
        ("c", ["08:02:00", "08:10:00", "08:20:00"]),
        ("d", ["09:00:00", "09:10:00", "09:20:00"]),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(tt.patterns.len(), 3);
    assert_eq!(report.count(IssueKind::OvertakingSplit), 2);
    // The late, non-overtaking trip joins the first lane, after `a`.
    let p = tt.trips[trip(&tt, "a").index()].pattern;
    assert_eq!(tt.trips[trip(&tt, "d").index()].pattern, p);
}

#[test]
fn different_stop_sequences_give_different_patterns() {
    let feed = TempFeed::new(&[
        ("trips.txt", &trips(&["x", "y"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("x", "08:00:00", "08:00:00", "A", 1),
                ("x", "08:05:00", "08:05:00", "B", 2),
                ("y", "08:00:00", "08:00:00", "B", 1),
                ("y", "08:05:00", "08:05:00", "A", 2),
            ]),
        ),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(tt.patterns.len(), 2);
    assert_eq!(report.count(IssueKind::OvertakingSplit), 0);
}

// ---- 7. interpolation ------------------------------------------------------

#[test]
fn interpolation_is_even_by_stop_index_and_truncates() {
    let feed = TempFeed::new(&[
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "", "", "B", 2),
                ("T", "", "", "C", 3),
                ("T", "08:00:10", "08:00:10", "D", 4),
            ]),
        ),
    ]);
    let (tt, report) = build(&feed);
    let t = times(&tt, "T");
    assert_eq!(t[1].0, Seconds::hms(8, 0, 3));
    assert_eq!(t[2].0, Seconds::hms(8, 0, 6));
    assert_eq!(t[1].0, t[1].1);
    // Counted per interpolated stop time, not per trip.
    assert_eq!(report.count(IssueKind::InterpolatedTimes), 2);
}

#[test]
fn stop_sequence_order_not_file_order_drives_the_trip() {
    let feed = TempFeed::new(&[
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:10:00", "08:10:00", "C", 30),
                ("T", "08:00:00", "08:00:00", "A", 10),
                ("T", "08:05:00", "08:05:00", "B", 20),
            ]),
        ),
    ]);
    let (tt, report) = build(&feed);
    assert!(report.is_clean());
    let names: Vec<&str> = tt.patterns[0]
        .stops
        .iter()
        .map(|s| tt.stops[s.index()].id.as_str())
        .collect();
    assert_eq!(names, ["A", "B", "C"]);
}

#[test]
fn missing_last_time_is_an_error() {
    let feed = one_trip("08:00:00", "");
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::MissingBoundaryTime), 1);
    assert!(tt.trips.is_empty());
}

// ---- 8. validation ---------------------------------------------------------

#[test]
fn missing_required_file_is_a_hard_error() {
    let feed = TempFeed::new(&[("trips.txt", &trips(&["T"])), ("stop_times.txt", "")]);
    assert!(load(&feed.0).is_err());
}

#[test]
fn dangling_stop_id_in_stop_times_drops_the_trip() {
    let feed = TempFeed::new(&[
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "ZZ", 2),
            ]),
        ),
    ]);
    // Lenient loading (Leader, M7): the trip goes, the feed stays.
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::UnknownStop), 1);
    assert!(tt.trips.is_empty());
}

#[test]
fn stop_time_of_a_trip_not_in_trips_txt_is_a_warning() {
    // M7 (Leader decision 4): the row is dropped and reported, the feed stays.
    let feed = TempFeed::new(&[
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
                ("GHOST", "09:00:00", "09:00:00", "A", 1),
            ]),
        ),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::UnknownTrip), 1);
    assert_eq!(IssueKind::UnknownTrip.severity(), Severity::Warning);
    assert_eq!(tt.trips.len(), 1);
}

#[test]
fn a_trip_with_one_stop_time_is_a_warning() {
    // M7 (Leader decision 2): the trip is dropped, as for an unknown stop.
    let feed = TempFeed::new(&[
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[("T", "08:00:00", "08:00:00", "A", 1)]),
        ),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::TooFewStopTimes), 1);
    assert_eq!(IssueKind::TooFewStopTimes.severity(), Severity::Warning);
    assert!(tt.trips.is_empty());
}

#[test]
fn decreasing_times_are_an_error() {
    let feed = one_trip("09:00:00", "08:00:00");
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::DecreasingTimes), 1);
    assert!(tt.trips.is_empty());
}

#[test]
fn duplicate_trip_ids_are_reported() {
    let feed = TempFeed::new(&[
        ("trips.txt", "route_id,service_id,trip_id\nR,S,T\nR,S,T\n"),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
            ]),
        ),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::DuplicateTripId), 1);
    assert_eq!(tt.trips.len(), 1);
}

#[test]
fn duplicate_stop_sequence_is_reported() {
    // M7: its own warning, and the trip is dropped.
    let feed = TempFeed::new(&[
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
                ("T", "08:06:00", "08:06:00", "C", 2),
            ]),
        ),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::DuplicateStopSequence), 1);
    assert!(tt.trips.is_empty());
}

#[test]
fn calendar_dates_only_service_is_known() {
    let feed = TempFeed::new(&[
        ("calendar.txt", "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n"),
        ("calendar_dates.txt", "service_id,date,exception_type\nS,20400105,1\nX,20400106,1\n"),
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[("T", "08:00:00", "08:00:00", "A", 1), ("T", "08:05:00", "08:05:00", "B", 2)]),
        ),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::UnknownService), 0);
    assert_eq!(tt.trips.len(), 1);
    assert_eq!(tt.valid_from.to_string(), "2040-01-05");
}

// ---- 11. timezone ----------------------------------------------------------

#[test]
fn timezone_comes_from_agency_txt() {
    let (tt, _) = build(&one_trip("08:00:00", "08:05:00"));
    assert_eq!(tt.timezone, "Asia/Tokyo");
}

#[test]
fn mixed_agency_timezones_are_rejected() {
    let agency = "agency_id,agency_name,agency_url,agency_timezone\nAG,A,https://a.invalid/,Asia/Tokyo\nAH,B,https://b.invalid/,Europe/Paris\n";
    let feed = TempFeed::new(&[
        ("agency.txt", agency),
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
            ]),
        ),
    ]);
    let feed = load(&feed.0).unwrap();
    assert!(matches!(
        feed.timezone(),
        Err(GtfsError::MixedTimezones { .. })
    ));
}

#[test]
fn same_timezone_on_two_agencies_is_fine() {
    let agency = "agency_id,agency_name,agency_url,agency_timezone\nAG,A,https://a.invalid/,Asia/Tokyo\nAH,B,https://b.invalid/,Asia/Tokyo\n";
    let feed = TempFeed::new(&[
        ("agency.txt", agency),
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
            ]),
        ),
    ]);
    assert_eq!(load(&feed.0).unwrap().timezone().unwrap(), "Asia/Tokyo");
}

#[test]
fn missing_agency_timezone_does_not_default() {
    let agency = "agency_id,agency_name,agency_url,agency_timezone\nAG,A,https://a.invalid/,\n";
    let feed = TempFeed::new(&[
        ("agency.txt", agency),
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
            ]),
        ),
    ]);
    let tz = load(&feed.0).ok().map(|f| f.timezone().ok());
    assert_ne!(tz, Some(Some("Europe/Bucharest".to_string())));
    assert!(
        !matches!(tz, Some(Some(ref s)) if !s.is_empty()),
        "got {tz:?}"
    );
}

#[test]
fn invalid_timezone_is_rejected() {
    // Was Known gap (M1) #4; fixed in M5 (chrono-tz).
    let agency =
        "agency_id,agency_name,agency_url,agency_timezone\nAG,A,https://a.invalid/,Mars/Olympus\n";
    let feed = TempFeed::new(&[
        ("agency.txt", agency),
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
            ]),
        ),
    ]);
    let ok = load(&feed.0).map(|f| f.timezone().is_ok()).unwrap_or(false);
    assert!(!ok);
}

// ---- 12. format quirks -----------------------------------------------------

#[test]
fn bom_crlf_quotes_and_extra_columns_load() {
    let stops = "\u{feff}stop_id,stop_name,stop_lat,stop_lon,extra\r\nA,\"Alpha, North\",1.0,1.0,x\r\nB,B,1.001,1.0,y\r\n";
    let feed = TempFeed::new(&[
        ("stops.txt", stops),
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
            ]),
        ),
    ]);
    let (tt, report) = build(&feed);
    assert!(report.is_clean());
    assert_eq!(
        tt.stops.iter().find(|s| s.id == "A").unwrap().name,
        "Alpha, North"
    );
}

#[test]
fn short_rows_load() {
    let st = "trip_id,arrival_time,departure_time,stop_id,stop_sequence,pickup_type\nT,08:00:00,08:00:00,A,1,\nT,08:05:00,08:05:00,B,2\n";
    let feed = TempFeed::new(&[("trips.txt", &trips(&["T"])), ("stop_times.txt", st)]);
    assert!(load(&feed.0).is_ok());
}

#[test]
fn duplicate_transfer_rows_are_counted_and_one_is_kept() {
    // A→B three times, A→C once: 2 duplicates. The kept A→B row has the longest
    // min_transfer_time (Leader, M4 fix round). D→E twice with the same time: the
    // higher transfer_type (3, not possible) is kept.
    let feed = TempFeed::new(&[
        ("trips.txt", &trips(&["T"])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("T", "08:00:00", "08:00:00", "A", 1),
                ("T", "08:05:00", "08:05:00", "B", 2),
            ]),
        ),
        (
            "transfers.txt",
            "from_stop_id,to_stop_id,transfer_type,min_transfer_time\n\
             A,B,2,120\nA,B,0,\nA,B,2,60\nA,C,2,90\nD,E,0,30\nD,E,3,30\n",
        ),
    ]);
    let (tt, report) = build(&feed);
    assert_eq!(report.count(IssueKind::DuplicateTransfer), 3);
    assert!(report.is_clean(), "a duplicate is a warning, not an error");
    assert_eq!(tt.transfers.len(), 3);
    let kept: Vec<(u8, u32)> = tt.transfers.iter().map(|t| (t.kind, t.min_secs)).collect();
    assert_eq!(kept, [(2, 120), (2, 90), (3, 30)]);
}

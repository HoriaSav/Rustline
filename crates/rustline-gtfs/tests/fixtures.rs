//! Loads the hand-written fixture feeds in `data/fixtures/` and checks the
//! timetable and validation report against what their READMEs describe.

use std::path::PathBuf;

use rustline_gtfs::{build_timetable, load, IssueKind, StopKind, Timetable, ValidationReport};
use rustline_model::{Seconds, TripIdx};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/fixtures")
        .join(name)
}

fn build(name: &str) -> (Timetable, ValidationReport) {
    let feed = load(&fixture(name)).expect("fixture loads");
    build_timetable(&feed).expect("fixture builds")
}

fn trip(tt: &Timetable, id: &str) -> TripIdx {
    TripIdx::from_usize(
        tt.trips
            .iter()
            .position(|t| t.id == id)
            .expect("trip exists"),
    )
}

#[test]
fn tiny_counts() {
    let feed = load(&fixture("tiny")).unwrap();
    let c = feed.counts();
    assert_eq!((c.stops, c.routes, c.trips, c.stop_times), (5, 3, 10, 34));
    assert_eq!((c.services, c.service_exceptions), (2, 2));
}

#[test]
fn tiny_timezone_and_validity_come_from_the_feed() {
    let (tt, _) = build("tiny");
    assert_eq!(tt.timezone, "America/Toronto");
    assert_eq!(tt.valid_from.to_string(), "2030-01-01");
    assert_eq!(tt.valid_to.to_string(), "2030-12-31");
    assert!(tt
        .stops
        .iter()
        .all(|s| s.kind == StopKind::Stop && s.pos.is_some()));
}

#[test]
fn tiny_after_midnight_trip_keeps_times_past_24h() {
    let (tt, _) = build("tiny");
    let night = tt.trip_stop_times(trip(&tt, "R2_night"));
    assert_eq!(night.last().unwrap().arrival, Seconds::hms(24, 10, 0));
}

#[test]
fn tiny_holiday_exception() {
    let (tt, _) = build("tiny");
    let wk = tt.services.iter().find(|s| s.id == "WK").unwrap();
    let we = tt.services.iter().find(|s| s.id == "WE").unwrap();
    assert_eq!(wk.removed.len(), 1);
    assert_eq!(we.added, wk.removed);
    assert_eq!(wk.weekdays, [true, true, true, true, true, false, false]);
}

#[test]
fn tiny_interpolates_missing_time() {
    let (tt, report) = build("tiny");
    let noon = tt.trip_stop_times(trip(&tt, "R3_noon"));
    assert_eq!(noon[1].arrival, Seconds::hms(12, 15, 0));
    assert_eq!(report.count(IssueKind::InterpolatedTimes), 1);
}

#[test]
fn tiny_patterns_sorted_and_overtaking_split() {
    let (tt, report) = build("tiny");
    assert!(report.is_clean());
    // R1, R2 and R3 sequences, with R3 split once because the express overtakes.
    assert_eq!(tt.patterns.len(), 4);
    assert_eq!(report.count(IssueKind::OvertakingSplit), 1);

    let slow = trip(&tt, "R3_slow");
    let express = trip(&tt, "R3_express");
    assert_ne!(
        tt.trips[slow.index()].pattern,
        tt.trips[express.index()].pattern
    );

    for p in &tt.patterns {
        // Every trip of a pattern visits exactly the pattern's stops …
        for &t in &p.trips {
            let stops: Vec<_> = tt.trip_stop_times(t).iter().map(|st| st.stop).collect();
            assert_eq!(stops, p.stops);
        }
        // … in order of first departure, and none overtakes the one before.
        for w in p.trips.windows(2) {
            let (a, b) = (tt.trip_stop_times(w[0]), tt.trip_stop_times(w[1]));
            assert!(a
                .iter()
                .zip(b)
                .all(|(x, y)| x.arrival <= y.arrival && x.departure <= y.departure));
        }
    }
}

#[test]
fn broken_feed_report() {
    let (tt, report) = build("broken");
    assert_eq!(tt.timezone, "Pacific/Auckland");
    for kind in [
        IssueKind::TooFewStopTimes,
        IssueKind::DecreasingTimes,
        IssueKind::UnknownService,
        IssueKind::UnknownRoute,
        IssueKind::MissingBoundaryTime,
        IssueKind::InvalidFrequency,
        IssueKind::StopWithoutCoordinates,
    ] {
        assert_eq!(report.count(kind), 1, "{kind:?}");
    }
    assert_eq!(report.errors.len(), 6);
}

#[test]
fn broken_feed_expands_frequencies() {
    let (tt, report) = build("broken");
    // 06:00, 06:20, 06:40 (headway only) + 07:00, 07:30 (exact); headway-0 row skipped.
    let ids: Vec<&str> = tt.trips.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "freq@06:00:00",
            "freq@06:20:00",
            "freq@06:40:00",
            "freq@07:00:00",
            "freq@07:30:00"
        ]
    );
    assert_eq!(report.count(IssueKind::ApproximateFrequencyTrips), 3);
    assert_eq!(tt.trips.iter().filter(|t| t.approximate).count(), 3);
    let first = tt.trip_stop_times(trip(&tt, "freq@07:30:00"));
    assert_eq!(
        (first[0].departure, first[1].arrival),
        (Seconds::hms(7, 30, 0), Seconds::hms(7, 40, 0))
    );
    assert_eq!(tt.patterns.len(), 1);
}

//! M5 review follow-up (Builder, 2026-10-18; Tester final check item 3): the exact
//! report kinds and error variants of the M5 calendar, transfers, timezone and
//! frequency rules (the other m5_* tests check behaviour, mostly `is_err`/`is_ok`).
//! Run: `cargo test --locked -p rustline-core --test m5_variants`

mod m5_common;

use m5_common::*;
use rustline_gtfs::{GtfsError, IssueKind, Severity};

fn base() -> Feed {
    Feed::daily(BUCHAREST, "20310101", "20310131").trip(
        "S",
        "t",
        &[("08:00:00", "A"), ("08:10:00", "B")],
    )
}

fn count(f: &Feed, kind: IssueKind) -> usize {
    let (_, report) = f.write().build().expect("feed builds");
    report.count(kind)
}

#[test]
fn type_1_and_type_2_on_one_day_is_conflicting_calendar_dates() {
    let f = base()
        .exception("S", "20310110", 1)
        .exception("S", "20310110", 2);
    assert_eq!(count(&f, IssueKind::ConflictingCalendarDates), 1);
    assert!(!runs(&f.pack(), "S", date(2031, 1, 10)), "removed wins");
}

#[test]
fn start_after_end_is_empty_date_range() {
    let f = base()
        .service("BAD", "1111111", "20310120", "20310110")
        .trip("BAD", "bad", &[("07:00:00", "A"), ("07:10:00", "B")]);
    assert_eq!(count(&f, IssueKind::EmptyDateRange), 1);
}

#[test]
fn transfer_to_an_unknown_stop_is_unknown_transfer_stop() {
    let f = base().transfers(
        "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,NOPE,2,60\nA,B,2,60\n",
    );
    assert_eq!(count(&f, IssueKind::UnknownTransferStop), 1);
}

#[test]
fn unparsable_type_2_time_is_treated_as_missing() {
    // "x", negative and > u32::MAX: footpath time plus MissingMinTransferTime.
    for bad in ["x", "-60", "4294967296"] {
        let f = base().transfers(&format!(
            "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,B,2,{bad}\n"
        ));
        assert_eq!(count(&f, IssueKind::MissingMinTransferTime), 1, "{bad}");
    }
}

#[test]
fn bad_agency_timezone_is_invalid_timezone() {
    let tf = Feed::daily("Mars/Olympus", "20310101", "20310131")
        .trip("S", "t", &[("08:00:00", "A"), ("08:10:00", "B")])
        .write();
    let r = tf
        .load()
        .and_then(|raw| rustline_gtfs::build_timetable(&raw));
    assert!(
        matches!(&r, Err(GtfsError::InvalidTimezone(z)) if z == "Mars/Olympus"),
        "{:?}",
        r.err()
    );
}

#[test]
fn frequency_row_over_the_cap_is_too_many_frequency_trips() {
    let f = base().freq("t", "00:00:00", "48:00:00", "1", "1");
    let r = f.write().build();
    assert!(
        matches!(r, Err(GtfsError::TooManyFrequencyTrips(_))),
        "{:?}",
        r.err()
    );
}

#[test]
fn headway_0_drops_the_row_with_a_warning() {
    // Tester's decision (M5 final check item 4): a warning, not an error.
    let f = base()
        .freq("t", "08:00:00", "09:00:00", "0", "1")
        .freq("t", "10:00:00", "10:20:00", "600", "1");
    let (tt, report) = f.write().build().expect("feed builds");
    assert_eq!(report.count(IssueKind::InvalidFrequency), 1);
    assert_eq!(IssueKind::InvalidFrequency.severity(), Severity::Warning);
    assert_eq!(tt.trips.len(), 2, "the other row still expands");
}

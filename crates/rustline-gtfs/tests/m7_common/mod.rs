//! M7 tests-first (Tester, 2026-10-20): helpers for the lenient stop_times/trips reader
//! tests (`m7_reader.rs`, gated) and the regression pins (`m7pre_pins.rs`, not gated).
//! Feeds are written as raw bytes, so BOM, CRLF, quoting and non-UTF-8 stay exact.
//! The M7 names (new IssueKind variants, the row cap) live only in `api` below.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub use rustline_gtfs::{GtfsError, IssueKind, Timetable, ValidationReport};

pub const AGENCY: &str = "agency_id,agency_name,agency_url,agency_timezone\nAG,M7,https://example.invalid/,Europe/Bucharest\n";
pub const STOPS: &str = "stop_id,stop_name,stop_lat,stop_lon\nA,A,1.0,1.0\nB,B,1.001,1.0\nC,C,1.002,1.0\nD,D,1.003,1.0\n";
pub const ROUTES: &str =
    "route_id,agency_id,route_short_name,route_long_name,route_type\nR,AG,1,One,3\nR2,AG,2,Two,3\n";
pub const CALENDAR: &str = "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nS,1,1,1,1,1,1,1,20400101,20401231\n";
pub const TRIPS: &str = "route_id,service_id,trip_id\nR,S,T\n";
pub const ST_HEADER: &str = "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n";

/// A feed dir under the temp dir (removed on drop). Files not given get defaults.
pub struct Feed(pub PathBuf);

impl Feed {
    pub fn new(files: &[(&str, &[u8])]) -> Feed {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustline_m7_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, body) in [
            ("agency.txt", AGENCY),
            ("stops.txt", STOPS),
            ("routes.txt", ROUTES),
            ("calendar.txt", CALENDAR),
            ("trips.txt", TRIPS),
        ] {
            if !files.iter().any(|(n, _)| *n == name) {
                fs::write(dir.join(name), body).unwrap();
            }
        }
        for (name, body) in files {
            fs::write(dir.join(name), body).unwrap();
        }
        Feed(dir)
    }

    /// Trip T with `stop_times` body (header included by the caller).
    pub fn stop_times(body: &str) -> Feed {
        Feed::new(&[("stop_times.txt", body.as_bytes())])
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// load + build.
pub fn ingest(f: &Feed) -> Result<(Timetable, ValidationReport), GtfsError> {
    rustline_gtfs::build_timetable(&rustline_gtfs::load(f.path())?)
}

/// Same, with any panic turned into Err (a panic is always a test failure).
pub fn ingest_no_panic(f: &Feed) -> Result<(Timetable, ValidationReport), String> {
    match std::panic::catch_unwind(|| ingest(f)) {
        Ok(r) => r.map_err(|e| format!("{e:?}")),
        Err(_) => panic!("ingest panicked on {}", f.path().display()),
    }
}

pub fn trip_ids(tt: &Timetable) -> Vec<String> {
    tt.trips.iter().map(|t| t.id.clone()).collect()
}

/// `(arrival, departure)` per stop of trip `id`.
pub fn times(tt: &Timetable, id: &str) -> Vec<(u32, u32)> {
    let i = tt.trips.iter().position(|t| t.id == id).expect("trip");
    tt.trip_stop_times(rustline_model::TripIdx::from_usize(i))
        .iter()
        .map(|s| (s.arrival.0, s.departure.0))
        .collect()
}

pub fn hms(h: u32, m: u32, s: u32) -> u32 {
    h * 3600 + m * 60 + s
}

/// stop_times body for trip T: `(arr, dep, stop, seq)`.
pub fn st(rows: &[(&str, &str, &str, u32)]) -> String {
    let mut s = String::from(ST_HEADER);
    for (a, d, stop, q) in rows {
        s.push_str(&format!("T,{a},{d},{stop},{q}\n"));
    }
    s
}

/// Workspace path (shared feeds live in fixtures/feeds).
pub fn workspace(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// A shared feed from fixtures/feeds, or None after printing `skip:`.
pub fn shared_feed(name: &str) -> Option<PathBuf> {
    let p = workspace(&format!("fixtures/feeds/{name}"));
    if p.is_file() {
        Some(p)
    } else {
        println!("skip: {} missing", p.display());
        None
    }
}

/// The planned M7 names. Builder: adapt only this module.
pub mod api {
    use super::*;

    /// Row length cap of the own reader (plan M7 DoD: "e.g. 64 KiB").
    pub const ROW_CAP: usize = rustline_gtfs::ROW_CAP;

    /// Rows whose time is out of range / unparsable / missing where required.
    pub fn bad_time_rows(r: &ValidationReport) -> usize {
        r.count(IssueKind::InvalidTime)
    }
    pub fn duplicate_trip_ids(r: &ValidationReport) -> usize {
        r.count(IssueKind::DuplicateTripId)
    }
    pub fn duplicate_stop_sequences(r: &ValidationReport) -> usize {
        r.count(IssueKind::DuplicateStopSequence)
    }
    pub fn short_rows(r: &ValidationReport) -> usize {
        r.count(IssueKind::ShortRow)
    }
    pub fn long_rows(r: &ValidationReport) -> usize {
        r.count(IssueKind::RowTooLong)
    }
    pub fn non_utf8_rows(r: &ValidationReport) -> usize {
        r.count(IssueKind::InvalidUtf8)
    }
}

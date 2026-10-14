//! Shared helpers for the M3 tests written before M3 (Tester).
//! Included with `mod m3_common;` from the m3_* test files; not a test itself.
#![allow(dead_code)] // Each test file uses a different subset of the helpers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::NaiveDate;
use rustline_pack::Pack;

/// The type `raptor_stop_to_stop` takes. The plan writes `&Timetable`;
/// rustline-core cannot depend on rustline-gtfs. Leader's decision 2: `&Pack`
/// (validity window from its header). `Pack` derefs to its `TimetableData`.
pub type Tt = Pack;

pub fn workspace(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// GTFS feed → M1 timetable → pack arrays (what the engine reads).
pub fn pack(feed: &Path) -> Tt {
    let raw = rustline_gtfs::load(feed).expect("feed loads");
    let (tt, _) = rustline_gtfs::build_timetable(&raw).expect("feed builds");
    // Reviewer MF1: a Pack is only made through the checking constructor.
    Pack::new(
        rustline_gtfs::pack_meta(&tt, [0; 32], "test", 0),
        rustline_gtfs::try_timetable_data(&tt).expect("pack arrays"),
    )
    .expect("pack passes check()")
}

pub fn valid_from(feed: &Path) -> NaiveDate {
    let raw = rustline_gtfs::load(feed).unwrap();
    rustline_gtfs::build_timetable(&raw).unwrap().0.valid_from
}

pub fn tiny() -> Tt {
    pack(&workspace("data/fixtures/tiny"))
}

/// A fixture feed from fixtures/feeds/, or None (test then skips with a note).
pub fn shared_feed(name: &str) -> Option<PathBuf> {
    let p = workspace(&format!("fixtures/feeds/{name}.zip"));
    if p.is_file() {
        Some(p)
    } else {
        println!(
            "{} missing, skipped (run scripts/fetch-fixtures.sh)",
            p.display()
        );
        None
    }
}

pub fn stop(tt: &Tt, gtfs_id: &str) -> rustline_model::StopIdx {
    let i = tt
        .stop_ids
        .iter()
        .position(|s| s == gtfs_id)
        .expect("stop id in pack");
    rustline_model::StopIdx::from_usize(i)
}

pub fn trip_id(tt: &Tt, trip: rustline_model::TripIdx) -> &str {
    &tt.trip_ids[trip.index()]
}

pub fn hms(h: u32, m: u32, s: u32) -> rustline_model::Seconds {
    rustline_model::Seconds::hms(h, m, s)
}

pub fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

// ---- small hand-written feeds -------------------------------------------------

pub const AGENCY: &str = "agency_id,agency_name,agency_url,agency_timezone\nAG,M3 Test,https://example.invalid/,Asia/Tokyo\n";
pub const ROUTES: &str =
    "route_id,agency_id,route_short_name,route_long_name,route_type\nR,AG,1,One,3\nQ,AG,2,Two,3\n";
/// Every day of 2030.
pub const CALENDAR: &str = "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nS,1,1,1,1,1,1,1,20300101,20301231\n";

/// A feed directory under the system temp dir, removed on drop. Files not given
/// get the defaults above; `stops.txt` defaults to stops named in stop_times.
pub struct TempFeed(pub PathBuf);

impl TempFeed {
    pub fn new(files: &[(&str, &str)]) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustline_m3_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let get = |n: &str| {
            files
                .iter()
                .find(|(f, _)| *f == n)
                .map(|(_, b)| b.to_string())
        };
        let st = get("stop_times.txt").unwrap_or_default();
        let stops = get("stops.txt").unwrap_or_else(|| {
            let mut ids: Vec<String> = st
                .lines()
                .skip(1)
                .filter_map(|l| l.split(',').nth(3).map(str::to_string))
                .collect();
            ids.sort();
            ids.dedup();
            let mut s = String::from("stop_id,stop_name,stop_lat,stop_lon\n");
            for (i, id) in ids.iter().enumerate() {
                s.push_str(&format!("{id},{id},{}.0,1.0\n", i + 1));
            }
            s
        });
        let defaults = [
            ("agency.txt", AGENCY.to_string()),
            ("routes.txt", ROUTES.to_string()),
            ("calendar.txt", CALENDAR.to_string()),
            ("stops.txt", stops),
        ];
        for (n, b) in &defaults {
            if get(n).is_none() || *n == "stops.txt" {
                fs::write(dir.join(n), b).unwrap();
            }
        }
        for (n, b) in files {
            if *n != "stops.txt" {
                fs::write(dir.join(n), b).unwrap();
            }
        }
        TempFeed(dir)
    }

    pub fn tt(&self) -> Tt {
        pack(&self.0)
    }
}

impl Drop for TempFeed {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// trips.txt from `(route, trip)` rows on service S.
pub fn trips(rows: &[(&str, &str)]) -> String {
    let mut s = String::from("route_id,service_id,trip_id\n");
    for (r, t) in rows {
        s.push_str(&format!("{r},S,{t}\n"));
    }
    s
}

/// stop_times.txt from `(trip, time, stop)`; arrival = departure = time,
/// sequence = row order within the trip.
pub fn stop_times(rows: &[(&str, &str, &str)]) -> String {
    let mut s = String::from("trip_id,arrival_time,departure_time,stop_id,stop_sequence\n");
    let mut seq = std::collections::BTreeMap::<&str, u32>::new();
    for (t, time, st) in rows {
        let q = seq.entry(t).or_insert(0);
        *q += 1;
        s.push_str(&format!("{t},{time},{time},{st},{q}\n"));
    }
    s
}

/// Fixed-seed xorshift64, so every run asks the same queries.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

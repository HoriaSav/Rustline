//! Shared helpers for the M5 tests written before M5 (Tester).
//! Included with `mod m5_common;` from the m5_* test files; not a test itself.
//!
//! - Feed builder for small hand-written GTFS feeds (any agency timezone).
//! - `api`: the only place that touches the M5 interfaces (plan M5 "Interfaces":
//!   `TimeSpec::DepartAt(DateTime<FixedOffset>)`, `Warning::{FeedExpiresSoon,
//!   ApproximateFrequencyTrip}`, `to_service_time`). The `cfg rustline_m5` gate
//!   was removed in M5 (the names matched the plan; nothing needed adapting).
//!
//! Nothing here reads the machine's local timezone: every instant is built with an
//! explicit `FixedOffset`, so `TZ=UTC` and `TZ=Pacific/Auckland` give the same results.
#![allow(dead_code)] // Each test file uses a different subset of the helpers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};
use rustline_pack::Pack;

pub fn workspace(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

pub fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// `y-m-d h:mi:s` at a fixed UTC offset of `off_h` hours (e.g. 3, -5).
pub fn at(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32, off_h: i32) -> DateTime<FixedOffset> {
    FixedOffset::east_opt(off_h * 3600)
        .unwrap()
        .with_ymd_and_hms(y, m, d, h, mi, s)
        .single()
        .unwrap()
}

/// Same instant check that also pins the offset (`+02:00` vs `+03:00`).
pub fn same(got: DateTime<FixedOffset>, want: DateTime<FixedOffset>) -> bool {
    got == want && got.offset().local_minus_utc() == want.offset().local_minus_utc()
}

pub fn hms(h: u32, m: u32, s: u32) -> u32 {
    h * 3600 + m * 60 + s
}

/// The feed-dependent tests read the feed path (relative to the workspace) from this
/// variable, e.g. `RUSTLINE_M5_FEED=fixtures/feeds/brasov-latest.zip`; unset → skip.
pub const FEED_VAR: &str = "RUSTLINE_M5_FEED";
/// Optional second feed for the bench reproduction (e.g. fixtures/feeds/mbta.zip).
pub const MBTA_VAR: &str = "RUSTLINE_M5_MBTA";

/// The feed named by `var`, or None after printing a `skip:` line.
pub fn env_feed(var: &str) -> Option<PathBuf> {
    let Some(rel) = std::env::var(var).ok().filter(|v| !v.trim().is_empty()) else {
        println!("skip: set {var}=fixtures/feeds/<feed>.zip (see README)");
        return None;
    };
    let p = workspace(&rel);
    if p.exists() {
        Some(p)
    } else {
        println!(
            "skip: {var}={rel} not found ({}); run scripts/fetch-fixtures.sh",
            p.display()
        );
        None
    }
}

// ---- small hand-written feeds -------------------------------------------------

pub const BUCHAREST: &str = "Europe/Bucharest";
pub const TORONTO: &str = "America/Toronto";

/// A hand-written feed. Stops are named in `stop_times` and placed 1° of latitude
/// apart (111 km), so no walk ever competes with a ride; a query point placed
/// exactly on a stop walks 0 m to it.
#[derive(Clone)]
pub struct Feed {
    pub tz: String,
    /// Extra agencies `(id, tz)` (for the mixed-timezone case).
    pub more_agencies: Vec<(String, String)>,
    /// calendar.txt body rows `service,mon..sun(7 digits),start,end`, e.g.
    /// `("S", "1111111", "20310101", "20310131")`. `None` → no calendar.txt at all.
    pub calendar: Option<Vec<(String, String, String, String)>>,
    /// calendar_dates.txt rows `(service, yyyymmdd, type)`.
    pub calendar_dates: Vec<(String, String, u8)>,
    /// trips.txt rows `(route, service, trip)`.
    pub trips: Vec<(String, String, String)>,
    /// stop_times rows `(trip, time, stop)`; arrival = departure = time.
    pub stop_times: Vec<(String, String, String)>,
    /// frequencies.txt rows `(trip, start, end, headway, exact_times)`.
    pub frequencies: Vec<(String, String, String, String, String)>,
    /// transfers.txt body (header included), raw.
    pub transfers: Option<String>,
}

pub fn s(x: &str) -> String {
    x.to_string()
}

impl Feed {
    /// Every day from `start` to `end` (yyyymmdd) on service "S".
    pub fn daily(tz: &str, start: &str, end: &str) -> Feed {
        Feed {
            tz: s(tz),
            more_agencies: Vec::new(),
            calendar: Some(vec![(s("S"), s("1111111"), s(start), s(end))]),
            calendar_dates: Vec::new(),
            trips: Vec::new(),
            stop_times: Vec::new(),
            frequencies: Vec::new(),
            transfers: None,
        }
    }

    pub fn service(mut self, id: &str, days: &str, start: &str, end: &str) -> Feed {
        self.calendar
            .get_or_insert_with(Vec::new)
            .push((s(id), s(days), s(start), s(end)));
        self
    }

    pub fn exception(mut self, service: &str, day: &str, kind: u8) -> Feed {
        self.calendar_dates.push((s(service), s(day), kind));
        self
    }

    /// A trip on `service` through `(time, stop)` in order (route "R").
    pub fn trip(mut self, service: &str, id: &str, calls: &[(&str, &str)]) -> Feed {
        self.trips.push((s("R"), s(service), s(id)));
        for (t, st) in calls {
            self.stop_times.push((s(id), s(t), s(st)));
        }
        self
    }

    pub fn freq(mut self, trip: &str, start: &str, end: &str, headway: &str, exact: &str) -> Feed {
        self.frequencies
            .push((s(trip), s(start), s(end), s(headway), s(exact)));
        self
    }

    pub fn transfers(mut self, body: &str) -> Feed {
        self.transfers = Some(s(body));
        self
    }

    /// Writes the feed into a fresh temp directory.
    pub fn write(&self) -> TempFeed {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustline_m5_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut agency = format!(
            "agency_id,agency_name,agency_url,agency_timezone\nAG,M5 Test,https://example.invalid/,{}\n",
            self.tz
        );
        for (id, tz) in &self.more_agencies {
            agency.push_str(&format!("{id},Other,https://example.invalid/,{tz}\n"));
        }
        fs::write(dir.join("agency.txt"), agency).unwrap();
        fs::write(
            dir.join("routes.txt"),
            "route_id,agency_id,route_short_name,route_long_name,route_type\nR,AG,1,One,3\n",
        )
        .unwrap();
        if let Some(cal) = &self.calendar {
            let mut c = s("service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n");
            for (id, days, a, b) in cal {
                let d: Vec<String> = days.chars().map(|ch| ch.to_string()).collect();
                c.push_str(&format!("{id},{},{a},{b}\n", d.join(",")));
            }
            fs::write(dir.join("calendar.txt"), c).unwrap();
        }
        if !self.calendar_dates.is_empty() {
            let mut c = s("service_id,date,exception_type\n");
            for (id, d, k) in &self.calendar_dates {
                c.push_str(&format!("{id},{d},{k}\n"));
            }
            fs::write(dir.join("calendar_dates.txt"), c).unwrap();
        }
        let mut t = s("route_id,service_id,trip_id\n");
        for (r, sv, id) in &self.trips {
            t.push_str(&format!("{r},{sv},{id}\n"));
        }
        fs::write(dir.join("trips.txt"), t).unwrap();
        let mut st = s("trip_id,arrival_time,departure_time,stop_id,stop_sequence\n");
        let mut seq = std::collections::BTreeMap::<&str, u32>::new();
        let mut stops: Vec<&str> = Vec::new();
        for (trip, time, stop) in &self.stop_times {
            let q = seq.entry(trip).or_insert(0);
            *q += 1;
            st.push_str(&format!("{trip},{time},{time},{stop},{q}\n"));
            if !stops.contains(&stop.as_str()) {
                stops.push(stop);
            }
        }
        fs::write(dir.join("stop_times.txt"), st).unwrap();
        stops.sort_unstable();
        let mut sp = s("stop_id,stop_name,stop_lat,stop_lon\n");
        for id in &stops {
            let (lat, lon) = stop_pos(id);
            sp.push_str(&format!("{id},{id},{lat},{lon}\n"));
        }
        fs::write(dir.join("stops.txt"), sp).unwrap();
        if !self.frequencies.is_empty() {
            let mut f = s("trip_id,start_time,end_time,headway_secs,exact_times\n");
            for (trip, a, b, h, e) in &self.frequencies {
                f.push_str(&format!("{trip},{a},{b},{h},{e}\n"));
            }
            fs::write(dir.join("frequencies.txt"), f).unwrap();
        }
        if let Some(tr) = &self.transfers {
            fs::write(dir.join("transfers.txt"), tr).unwrap();
        }
        TempFeed(dir)
    }

    pub fn pack(&self) -> Pack {
        self.write().pack()
    }
}

/// Stop positions: the stop's letter decides the latitude (A = 1°, B = 2°, …), so
/// every stop is ≥ 111 km from the others.
pub fn stop_pos(id: &str) -> (f64, f64) {
    let c = id.bytes().next().unwrap_or(b'A');
    let n = f64::from(c.saturating_sub(b'A')) + f64::from(id.len() as u8 - 1) * 0.5;
    (1.0 + n, 1.0)
}

/// A feed directory under the system temp dir, removed on drop.
pub struct TempFeed(pub PathBuf);

impl TempFeed {
    pub fn load(&self) -> Result<rustline_gtfs::RawFeed, rustline_gtfs::GtfsError> {
        rustline_gtfs::load(&self.0)
    }

    pub fn build(
        &self,
    ) -> Result<(rustline_gtfs::Timetable, rustline_gtfs::ValidationReport), rustline_gtfs::GtfsError>
    {
        rustline_gtfs::build_timetable(&self.load()?)
    }

    pub fn pack(&self) -> Pack {
        pack_of(&self.0)
    }
}

impl Drop for TempFeed {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// GTFS feed (dir or zip) → timetable → checked pack (no street graph).
pub fn pack_of(feed: &Path) -> Pack {
    let raw = rustline_gtfs::load(feed).expect("feed loads");
    let (tt, _) = rustline_gtfs::build_timetable(&raw).expect("feed builds");
    Pack::new(
        rustline_gtfs::pack_meta(&tt, [0; 32], "m5", 0),
        rustline_gtfs::try_timetable_data(&tt).expect("pack arrays"),
    )
    .expect("pack passes check()")
}

pub fn stop(p: &Pack, id: &str) -> rustline_model::StopIdx {
    let i = p
        .stop_ids
        .iter()
        .position(|x| x == id)
        .unwrap_or_else(|| panic!("stop {id} in pack"));
    rustline_model::StopIdx::from_usize(i)
}

pub fn service(p: &Pack, id: &str) -> usize {
    p.service_ids
        .iter()
        .position(|x| x == id)
        .unwrap_or_else(|| panic!("service {id} in pack"))
}

/// Does `service` run on `d` (false outside the pack's window)?
pub fn runs(p: &Pack, id: &str, d: NaiveDate) -> bool {
    let from = p.meta().valid_from;
    if d < from || d > p.meta().valid_to {
        return false;
    }
    p.service_runs(service(p, id), (d - from).num_days() as u32)
}

/// Stop-to-stop RAPTOR on one service day (the M3 API, unchanged in M5):
/// `(trip ids, dep, arr)` of every journey, in result order.
pub fn rides(
    p: &Pack,
    from: &str,
    to: &str,
    d: NaiveDate,
    dep: u32,
) -> Result<Vec<(Vec<String>, u32, u32)>, rustline_core::QueryError> {
    let js = rustline_core::raptor_stop_to_stop(
        p,
        stop(p, from),
        stop(p, to),
        d,
        rustline_model::Seconds(dep),
        3,
    )?;
    Ok(js
        .iter()
        .map(|j| {
            let trips = j
                .legs
                .iter()
                .filter_map(|l| match l {
                    rustline_model::Leg::Transit { trip, .. } => {
                        Some(p.trip_ids[trip.index()].clone())
                    }
                    _ => None,
                })
                .collect();
            (trips, j.dep.0, j.arr.0)
        })
        .collect())
}

/// Earliest arrival (seconds of the service day) A→B, if any.
pub fn earliest(p: &Pack, from: &str, to: &str, d: NaiveDate, dep: u32) -> Option<u32> {
    rides(p, from, to, d, dep)
        .unwrap()
        .iter()
        .map(|r| r.2)
        .min()
}

/// Fixed-seed xorshift64.
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

// ---- the M5 interfaces (gated) ---------------------------------------------------

/// Adapter over the M5 API. Builder: adapt only this module when names differ.
pub mod api {
    use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone};
    pub use rustline_core::{Engine, PlanError, PlanRequest, TimeSpec, Warning};
    use rustline_model::LatLon;

    /// `(trip_id, from stop id, to stop id, board, alight)`.
    pub type Ride = (
        String,
        String,
        String,
        DateTime<FixedOffset>,
        DateTime<FixedOffset>,
    );

    /// One itinerary in plain types.
    #[derive(Debug, Clone, PartialEq)]
    pub struct It {
        pub dep: DateTime<FixedOffset>,
        pub arr: DateTime<FixedOffset>,
        pub rides: usize,
        /// `(trip_id, from stop id, to stop id, board, alight)` of each ride.
        pub legs: Vec<Ride>,
        /// Start and end of every leg (walks included), in order.
        pub spans: Vec<(DateTime<FixedOffset>, DateTime<FixedOffset>)>,
    }

    impl It {
        pub fn trips(&self) -> Vec<&str> {
            self.legs.iter().map(|l| l.0.as_str()).collect()
        }
    }

    fn tz(name: &str) -> chrono_tz::Tz {
        name.parse().unwrap_or_else(|_| panic!("IANA zone {name}"))
    }

    /// Plan M5: `to_service_time(tz, t) -> Vec<(NaiveDate, Seconds)>` (D-1, D, and D+1).
    pub fn service_times(zone: &str, t: DateTime<FixedOffset>) -> Vec<(NaiveDate, u32)> {
        rustline_core::to_service_time(tz(zone), t)
            .into_iter()
            .map(|(d, s)| (d, s.0))
            .collect()
    }

    /// The instant of local wall time `t` in `zone` (earliest if ambiguous, None if it
    /// does not exist), as a DateTime with that zone's offset.
    pub fn local(zone: &str, t: NaiveDateTime) -> Option<DateTime<FixedOffset>> {
        tz(zone)
            .from_local_datetime(&t)
            .earliest()
            .map(|x| x.fixed_offset())
    }

    /// The zone's UTC offset (seconds) at instant `t`.
    pub fn offset_at(zone: &str, t: DateTime<FixedOffset>) -> i32 {
        t.with_timezone(&tz(zone))
            .fixed_offset()
            .offset()
            .local_minus_utc()
    }

    /// The local date of instant `t` in `zone`.
    pub fn local_date(zone: &str, t: DateTime<FixedOffset>) -> NaiveDate {
        t.with_timezone(&tz(zone)).date_naive()
    }

    pub fn engine(p: rustline_pack::Pack) -> Engine {
        Engine::new(p).expect("engine")
    }

    /// Door to door between two points at `t` (access walks up to 100 m, so a point on
    /// a stop uses only that stop).
    pub fn req(from: (f64, f64), to: (f64, f64), t: DateTime<FixedOffset>) -> PlanRequest {
        PlanRequest {
            from: LatLon::new(from.0, from.1),
            to: LatLon::new(to.0, to.1),
            time: TimeSpec::DepartAt(t),
            max_access_m: 100,
            ..PlanRequest::default()
        }
    }

    fn view(it: &rustline_core::Itinerary) -> It {
        let mut legs = Vec::new();
        let mut spans = Vec::new();
        for l in &it.legs {
            match l {
                rustline_core::Leg::Walk { start, end, .. } => spans.push((*start, *end)),
                rustline_core::Leg::Transit {
                    trip_id,
                    from_stop,
                    to_stop,
                    board,
                    alight,
                    ..
                } => {
                    spans.push((*board, *alight));
                    legs.push((
                        trip_id.clone(),
                        from_stop.id.clone(),
                        to_stop.id.clone(),
                        *board,
                        *alight,
                    ));
                }
            }
        }
        It {
            dep: it.departure,
            arr: it.arrival,
            rides: legs.len(),
            legs,
            spans,
        }
    }

    pub fn plan(e: &Engine, r: &PlanRequest) -> Result<Vec<It>, PlanError> {
        e.plan(r).map(|x| x.itineraries.iter().map(view).collect())
    }

    /// Stop to stop door to door: points exactly on the two stops.
    pub fn plan_stops(
        e: &Engine,
        from: &str,
        to: &str,
        t: DateTime<FixedOffset>,
    ) -> Result<Vec<It>, PlanError> {
        plan(e, &req(super::stop_pos(from), super::stop_pos(to), t))
    }

    pub fn is_date_outside(r: &Result<Vec<It>, PlanError>) -> bool {
        matches!(r, Err(PlanError::DateOutsideFeed))
    }

    pub fn is_no_route(r: &Result<Vec<It>, PlanError>) -> bool {
        matches!(r, Err(PlanError::NoRoute))
    }

    /// `Some(valid_to)` when the response carries `Warning::FeedExpiresSoon`.
    pub fn expires_soon(e: &Engine, r: &PlanRequest) -> Option<NaiveDate> {
        e.plan(r).ok()?.warnings.iter().find_map(|w| match w {
            Warning::FeedExpiresSoon { valid_to } => Some(*valid_to),
            _ => None,
        })
    }

    pub fn approximate_warning(e: &Engine, r: &PlanRequest) -> bool {
        e.plan(r).is_ok_and(|x| {
            x.warnings
                .iter()
                .any(|w| matches!(w, Warning::ApproximateFrequencyTrip))
        })
    }

    /// The validation-report count of transfers.txt rows that are skipped because
    /// their type (4, 5, unknown) or trip/route columns are not supported.
    pub fn unsupported_transfers(r: &rustline_gtfs::ValidationReport) -> usize {
        r.count(rustline_gtfs::IssueKind::UnsupportedTransfer)
    }

    /// That issue kind is a warning, not an error.
    pub fn unsupported_transfer_is_warning() -> bool {
        rustline_gtfs::IssueKind::UnsupportedTransfer.severity() != rustline_gtfs::Severity::Error
    }
}

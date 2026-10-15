//! Tester (M4, written before M4): shared helpers for the door-to-door tests.
//! - `api`: Tester's reading of the planned M4 interface (plan M4 "Definition of done
//!   (strict)" + spec §11.1/§11.2). If Builder's names differ, adapt only `api`.
//! - the synthetic "line city" (GTFS directory + OSM PBF) with hand-derivable answers;
//! - the CLI wrapper for `build-pack --osm`;
//! - the door-to-door brute force from M3's public API (walk access + RAPTOR + walk egress).
#![allow(dead_code)] // Each test file uses a different subset.

#[path = "../../../rustline-osm/tests/m4_pbf/mod.rs"]
pub mod m4_pbf;

pub use m4_pbf::*;

use chrono::{NaiveDate, NaiveDateTime};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// ============================ planned interface ============================

pub mod api {
    use chrono::NaiveDateTime;
    use rustline_model::LatLon;

    // The whole planned interface; not every test file names every type.
    #[allow(unused_imports)]
    pub use rustline_core::{
        Engine, PackInfo, PlanError, PlanRequest, PlanResponse, TimeSpec, Warning,
    };

    /// One itinerary in plain types.
    #[derive(Debug, Clone, PartialEq)]
    pub struct It {
        pub dep: NaiveDateTime,
        pub arr: NaiveDateTime,
        pub rides: usize,
        pub transfers: u8,
        pub walk_m: u32,
        pub legs: Vec<LegV>,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum LegV {
        Walk {
            start: NaiveDateTime,
            end: NaiveDateTime,
            distance_m: u32,
            geometry: Option<Vec<(f64, f64)>>,
        },
        Transit {
            trip_id: String,
            from_stop: String,
            to_stop: String,
            board: NaiveDateTime,
            alight: NaiveDateTime,
        },
    }

    impl It {
        pub fn trips(&self) -> Vec<String> {
            self.legs
                .iter()
                .filter_map(|l| match l {
                    LegV::Transit { trip_id, .. } => Some(trip_id.clone()),
                    LegV::Walk { .. } => None,
                })
                .collect()
        }
    }

    /// Spec §11.1 `PlanRequest` with the plan's field names (DoD), defaults otherwise.
    pub fn req(from: (f64, f64), to: (f64, f64), at: NaiveDateTime) -> PlanRequest {
        PlanRequest {
            from: LatLon::new(from.0, from.1),
            to: LatLon::new(to.0, to.1),
            time: TimeSpec::DepartAt(at),
            ..PlanRequest::default()
        }
    }

    pub fn view(it: &rustline_core::Itinerary) -> It {
        let legs = it
            .legs
            .iter()
            .map(|l| match l {
                rustline_core::Leg::Walk {
                    start,
                    end,
                    distance_m,
                    geometry,
                    ..
                } => LegV::Walk {
                    start: *start,
                    end: *end,
                    distance_m: *distance_m,
                    geometry: geometry
                        .as_ref()
                        .map(|g| g.points().iter().map(|p| (p.lat, p.lon)).collect()),
                },
                rustline_core::Leg::Transit {
                    trip_id,
                    from_stop,
                    to_stop,
                    board,
                    alight,
                    ..
                } => LegV::Transit {
                    trip_id: trip_id.clone(),
                    from_stop: from_stop.id.clone(),
                    to_stop: to_stop.id.clone(),
                    board: *board,
                    alight: *alight,
                },
            })
            .collect::<Vec<_>>();
        It {
            dep: it.departure,
            arr: it.arrival,
            rides: legs
                .iter()
                .filter(|l| matches!(l, LegV::Transit { .. }))
                .count(),
            transfers: it.transfers,
            walk_m: it.walk_m,
            legs,
        }
    }

    pub fn plan(e: &Engine, r: &PlanRequest) -> Result<Vec<It>, PlanError> {
        e.plan(r).map(|p| p.itineraries.iter().map(view).collect())
    }

    pub fn warnings(e: &Engine, r: &PlanRequest) -> Vec<String> {
        e.plan(r)
            .map(|p| p.warnings.iter().map(|w| format!("{w:?}")).collect())
            .unwrap_or_default()
    }

    /// DoD: "pack without OSM uses the straight-line fallback and says so in PackInfo".
    pub fn has_street_graph(i: &PackInfo) -> bool {
        i.street_graph
    }

    /// Precomputed footpaths (plan step 7) as (from stop_id, to stop_id, seconds).
    pub fn footpaths(e: &Engine) -> Vec<(String, String, u32)> {
        let t = e.pack().timetable();
        let mut out = Vec::new();
        for s in 0..t.stop_count() {
            for f in t.footpaths(s) {
                out.push((
                    t.stop_ids[s].clone(),
                    t.stop_ids[t.footpath_to[f] as usize].clone(),
                    t.footpath_secs[f],
                ));
            }
        }
        out
    }

    /// M3's public API on the engine's pack (for the brute force).
    pub fn m3_pareto(
        e: &Engine,
        from: &str,
        to: &str,
        date: chrono::NaiveDate,
        dep: u32,
    ) -> Vec<(u8, u32)> {
        let t = e.pack().timetable();
        let idx = |id: &str| {
            t.stop_ids
                .iter()
                .position(|s| s == id)
                .map(|i| rustline_model::StopIdx(i as u32))
        };
        let (Some(a), Some(b)) = (idx(from), idx(to)) else {
            return vec![];
        };
        rustline_core::raptor_pareto(e.pack(), a, b, date, rustline_model::Seconds(dep), 8)
            .map(|js| js.iter().map(|j| (j.rides, j.arr.0)).collect())
            .unwrap_or_default()
    }
}

pub use api::*;

// ============================ time helpers ============================

pub fn at(date: NaiveDate, h: u32, m: u32, s: u32) -> NaiveDateTime {
    date.and_hms_opt(h, m, s).unwrap()
}

/// Wednesday inside the line city's calendar (all of 2030, weekdays).
pub fn wed() -> NaiveDate {
    NaiveDate::from_ymd_opt(2030, 1, 2).unwrap()
}

/// Seconds after midnight of `d` (can exceed 86 400).
pub fn secs(d: NaiveDate, t: NaiveDateTime) -> f64 {
    (t - at(d, 0, 0, 0)).num_milliseconds() as f64 / 1000.0
}

pub const WALK: f64 = 1.3;

// ============================ the line city ============================

/// Coordinates of the line city (all hand-derivable, see `line_city_osm`).
pub mod city {
    use super::{dlat, dlon};
    pub const LON: f64 = 1.0;
    pub fn on_m(lat: f64) -> (f64, f64) {
        (lat, LON)
    }
    pub const P0: (f64, f64) = (1.000, LON);
    pub const P0B: (f64, f64) = (1.004, LON);
    pub const D: (f64, f64) = (1.0045, LON);
    pub const P1: (f64, f64) = (1.010, LON);
    pub const P2: (f64, f64) = (1.030, LON);
    pub const P3: (f64, f64) = (1.035, LON);
    pub const Q: (f64, f64) = (1.020, 1.010);
    pub const R: (f64, f64) = (1.001, 1.050);
    pub const C1: f64 = 1.020;
    pub const C2: f64 = 1.024;
    pub const SPUR: f64 = 1.005;
    pub fn spur_mid() -> (f64, f64) {
        (SPUR, LON + dlon(100.0, SPUR))
    }
    pub fn spur_end() -> (f64, f64) {
        (SPUR, LON + dlon(200.0, SPUR))
    }
    /// Stop U: 150 m east of the meridian at 1.008 (no street within 100 m).
    pub fn u() -> (f64, f64) {
        (1.008, LON + dlon(150.0, 1.008))
    }
    pub fn south_500() -> (f64, f64) {
        (1.0 - dlat(500.0), LON)
    }
    /// Metres between two points of the meridian.
    pub fn along(a: f64, b: f64) -> f64 {
        dlat(1.0).recip() * (a - b).abs()
    }
}

/// Streets (all `highway=residential` unless noted; nodes every ≤ 55 m):
/// - meridian lon 1.0 from 0.99 to 1.10, with nodes exactly at every stop latitude;
/// - C1 along lat 1.020 and C2 along 1.024, lon 1.0 → 1.01, each ONE 1.1 km segment
///   (snapping must use the edge, not its nodes);
/// - spur along lat 1.005: 100 m residential then 100 m `highway=steps`;
/// - island A (no stop): footway lon 1.03, lat 1.000 → 1.002 (2.2 km from anything);
/// - island B (stop R): footway lon 1.05, lat 1.000 → 1.001 → 1.002.
pub fn line_city_osm() -> Osm {
    let mut o = Osm::new();
    let res = [("highway", "residential")];
    let lats = [
        0.99,
        city::south_500().0,
        1.0,
        1.004,
        1.0045,
        1.005,
        1.008,
        1.01,
        1.02,
        1.024,
        1.03,
        1.035,
        1.06,
        1.10,
    ];
    for w in lats.windows(2) {
        o.street(city::on_m(w[0]), city::on_m(w[1]), 55.0, &res);
    }
    o.street((city::C1, 1.0), (city::C1, 1.01), 1e9, &res);
    o.street((city::C2, 1.0), (city::C2, 1.01), 1e9, &res);
    o.street((city::SPUR, 1.0), city::spur_mid(), 50.0, &res);
    o.street(
        city::spur_mid(),
        city::spur_end(),
        50.0,
        &[("highway", "steps")],
    );
    o.street(
        (1.000, 1.03),
        (1.002, 1.03),
        55.0,
        &[("highway", "footway")],
    );
    o.street(
        (1.000, 1.05),
        (1.001, 1.05),
        55.0,
        &[("highway", "footway")],
    );
    o.street(
        (1.001, 1.05),
        (1.002, 1.05),
        55.0,
        &[("highway", "footway")],
    );
    o
}

/// Stops: P0, P0b (445 m from P0: one footpath), P1 and P1a (same coordinates:
/// a 0 s footpath), P2, P3, Q (end of C1), R (island B), U (unlinked, unserved).
/// Trips (weekdays 2030, timezone UTC):
/// - L1_0810: P0 08:10, P0b 08:11, P1 08:12, P2 08:16, P3 08:17
/// - L1_0830: P0 08:30, P0b 08:31, P1 08:32, P2 08:36, P3 08:37
/// - L2_0820: P1 08:20 → Q 08:24
/// - L3_0840: P0 08:40 → R 08:50
pub fn line_city_gtfs(dir: &Path) {
    let u = city::u();
    let w = |name: &str, s: &str| std::fs::write(dir.join(name), s).unwrap();
    w("agency.txt", "agency_id,agency_name,agency_url,agency_timezone\nT,Tester Line City,https://example.invalid/,UTC\n");
    w(
        "stops.txt",
        &format!(
            "stop_id,stop_name,stop_lat,stop_lon\n\
             P0,P zero,1.000,1.0\nP0b,P zero b,1.004,1.0\nP1,P one,1.010,1.0\nP1a,P one a,1.010,1.0\n\
             P2,P two,1.030,1.0\nP3,P three,1.035,1.0\nQ,Q,1.020,1.010\nR,R island,1.001,1.050\n\
             U,U unlinked,{},{}\n",
            u.0, u.1
        ),
    );
    w("routes.txt", "route_id,agency_id,route_short_name,route_long_name,route_type\nL1,T,1,Line one,3\nL2,T,2,Line two,3\nL3,T,3,Line three,3\n");
    w("calendar.txt", "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nWK,1,1,1,1,1,0,0,20300101,20301231\n");
    w(
        "trips.txt",
        "route_id,service_id,trip_id\nL1,WK,L1_0810\nL1,WK,L1_0830\nL2,WK,L2_0820\nL3,WK,L3_0840\n",
    );
    let mut st = String::from("trip_id,arrival_time,departure_time,stop_id,stop_sequence\n");
    for (trip, rows) in [
        (
            "L1_0810",
            &[
                ("08:10", "P0"),
                ("08:11", "P0b"),
                ("08:12", "P1"),
                ("08:16", "P2"),
                ("08:17", "P3"),
            ][..],
        ),
        (
            "L1_0830",
            &[
                ("08:30", "P0"),
                ("08:31", "P0b"),
                ("08:32", "P1"),
                ("08:36", "P2"),
                ("08:37", "P3"),
            ][..],
        ),
        ("L2_0820", &[("08:20", "P1"), ("08:24", "Q")][..]),
        ("L3_0840", &[("08:40", "P0"), ("08:50", "R")][..]),
    ] {
        for (i, (t, s)) in rows.iter().enumerate() {
            st.push_str(&format!("{trip},{t}:00,{t}:00,{s},{}\n", i + 1));
        }
    }
    w("stop_times.txt", &st);
}

// ============================ CLI ============================

pub fn rustline(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustline"))
        .args(args)
        .output()
        .unwrap()
}

pub fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `rustline build-pack --gtfs <gtfs> [--osm <pbf>] --out <dir> --feed-id <id> --built-at 0 [extra]`.
/// Returns the CLI output and the `.rlp` it wrote (if any).
pub fn build_pack(
    gtfs: &Path,
    osm: Option<&Path>,
    out: &Path,
    extra: &[&str],
) -> (Output, Option<PathBuf>) {
    let mut args = vec![
        "build-pack".to_string(),
        "--gtfs".into(),
        gtfs.display().to_string(),
        "--out".into(),
        out.display().to_string(),
        "--feed-id".into(),
        "tester".into(),
        "--built-at".into(),
        "0".into(),
    ];
    if let Some(p) = osm {
        args.push("--osm".into());
        args.push(p.display().to_string());
    }
    args.extend(extra.iter().map(|s| s.to_string()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = rustline(&refs);
    let rlp = std::fs::read_dir(out).ok().and_then(|d| {
        d.filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "rlp"))
    });
    (o, rlp)
}

/// The line city pack in its own temp dir: (dir, gtfs dir, pbf path, pack path, build output).
pub struct CityPack {
    pub dir: TempDir,
    pub gtfs: PathBuf,
    pub pbf: PathBuf,
    pub pack: PathBuf,
    pub output: String,
}

pub fn city_pack(with_osm: bool) -> CityPack {
    let dir = TempDir::new("city");
    let gtfs = dir.0.join("gtfs");
    std::fs::create_dir_all(&gtfs).unwrap();
    line_city_gtfs(&gtfs);
    let pbf = dir.file(
        "city.osm.pbf",
        &to_pbf(&line_city_osm(), PbfOptions::default()),
    );
    let out = dir.0.join("out");
    let (o, pack) = build_pack(&gtfs, with_osm.then_some(pbf.as_path()), &out, &[]);
    assert!(o.status.success(), "build-pack failed: {}", text(&o));
    CityPack {
        output: text(&o),
        pack: pack.expect("build-pack wrote a .rlp"),
        dir,
        gtfs,
        pbf,
    }
}

pub fn engine(p: &Path) -> Engine {
    Engine::open(p).expect("pack opens")
}

/// Polls /proc/<pid>/status for VmHWM while the child runs (Linux; KiB).
pub fn run_with_peak(args: &[&str]) -> (Output, Option<u64>, std::time::Duration) {
    let before_children = children_max_rss_kib();
    let t0 = std::time::Instant::now();
    let child = Command::new(env!("CARGO_BIN_EXE_rustline"))
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let peak = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (p2, stop) = (
        peak.clone(),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let s2 = stop.clone();
    let h = std::thread::spawn(move || {
        while !s2.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
                if let Some(v) = s
                    .lines()
                    .find(|l| l.starts_with("VmHWM:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<u64>().ok())
                {
                    p2.fetch_max(v, std::sync::atomic::Ordering::Relaxed);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    let out = child.wait_with_output().unwrap();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    h.join().unwrap();
    let polled = peak.load(std::sync::atomic::Ordering::Relaxed);
    // Tester (M4 verify): a child that exits within the first 5 ms poll leaves no
    // real sample (the hostile-PBF cases printed "peak 0 MiB"). The kernel's max
    // RSS over all waited-for children is exact for this child when it rose during
    // this run, and an upper bound for it otherwise.
    let after = children_max_rss_kib();
    let p = if after > before_children || polled < 1024 {
        polled.max(after)
    } else {
        polled
    };
    (out, (p > 0).then_some(p), t0.elapsed())
}

/// `getrusage(RUSAGE_CHILDREN).ru_maxrss` in KiB (Linux), 0 elsewhere.
pub fn children_max_rss_kib() -> u64 {
    #[cfg(target_os = "linux")]
    {
        // struct rusage: two struct timeval, then 14 longs starting with ru_maxrss.
        #[repr(C)]
        struct Rusage([i64; 18]);
        extern "C" {
            fn getrusage(who: i32, usage: *mut Rusage) -> i32;
        }
        let mut r = Rusage([0; 18]);
        // SAFETY: getrusage writes one struct rusage (18 × 8 bytes on 64-bit Linux).
        let rc = unsafe { getrusage(-1, &mut r) };
        if rc == 0 {
            return u64::try_from(r.0[4]).unwrap_or(0);
        }
    }
    0
}

pub fn fixture(name: &str) -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/feeds")
        .join(name);
    if p.exists() {
        Some(p)
    } else {
        eprintln!("skip: {} missing (scripts/fetch-fixtures.sh)", p.display());
        None
    }
}

// ============================ oracles ============================

/// Reference walking seconds between two OSM nodes of the line city (steps ×1.5).
pub fn walk_secs(r: &RefGraph, osm: &Osm, a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let id = |c: (f64, f64)| {
        osm.nodes
            .iter()
            .find(|n| haversine_m((n.lat, n.lon), c) < 0.01)
            .map(|n| n.id)
    };
    let (ia, ib) = (id(a)?, id(b)?);
    r.dijkstra(ia, WALK).get(&ib).copied()
}

pub fn walk_metres(r: &RefGraph, osm: &Osm, a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let id = |c: (f64, f64)| {
        osm.nodes
            .iter()
            .find(|n| haversine_m((n.lat, n.lon), c) < 0.01)
            .map(|n| n.id)
    };
    let (ia, ib) = (id(a)?, id(b)?);
    r.metres(ia).get(&ib).copied()
}

/// Linked stops of the line city with coordinates (U is not linked, so not here).
pub const LINKED_STOPS: &[(&str, (f64, f64))] = &[
    ("P0", city::P0),
    ("P0b", city::P0B),
    ("P1", city::P1),
    ("P1a", city::P1),
    ("P2", city::P2),
    ("P3", city::P3),
    ("Q", city::Q),
    ("R", city::R),
];

/// Earliest 1-ride door-to-door arrival (seconds after midnight) by brute force:
/// every access stop s (walk ≤ max_access_m) × every egress stop e (walk ≤ max_access_m),
/// M3 `raptor_pareto` from s at dep + walk(o, s), 1-ride journeys, + walk(e, d).
/// `None` when nothing; `Err(())` when a walk lies within 1 % of the limit or a boarding
/// within 2 s of a departure (rounding-sensitive: the caller skips the pair).
// Plain inputs keep the oracle independent of the engine's types.
#[allow(clippy::too_many_arguments)]
pub fn brute_one_ride(
    e: &Engine,
    r: &RefGraph,
    osm: &Osm,
    o: (f64, f64),
    d: (f64, f64),
    date: NaiveDate,
    dep: f64,
    max_access_m: f64,
) -> Result<Option<f64>, ()> {
    let mut best: Option<f64> = None;
    for (s, sc) in LINKED_STOPS {
        let Some(ms) = walk_metres(r, osm, o, *sc) else {
            continue;
        };
        if (ms - max_access_m).abs() <= 0.01 * max_access_m {
            return Err(());
        }
        if ms > max_access_m {
            continue;
        }
        let t_s = dep + walk_secs(r, osm, o, *sc).unwrap();
        for (x, xc) in LINKED_STOPS {
            if x == s {
                continue;
            }
            let Some(mx) = walk_metres(r, osm, *xc, d) else {
                continue;
            };
            if (mx - max_access_m).abs() <= 0.01 * max_access_m {
                return Err(());
            }
            if mx > max_access_m {
                continue;
            }
            let egress = walk_secs(r, osm, *xc, d).unwrap();
            for (rides, arr) in m3_pareto(e, s, x, date, t_s.ceil() as u32) {
                if rides == 1 {
                    let a = f64::from(arr) + egress;
                    best = Some(best.map_or(a, |b: f64| b.min(a)));
                }
            }
            // Rounding check: a departure of s within 2 s after t_s.
            let near = m3_pareto(e, s, x, date, (t_s - 2.0).max(0.0) as u32);
            let far = m3_pareto(e, s, x, date, (t_s + 2.0).ceil() as u32);
            if near != far {
                return Err(());
            }
        }
    }
    Ok(best)
}

// ============================ real feeds ============================

/// `rustline bbox --gtfs <feed> --margin-km <km>` → the bbox line it prints (trimmed).
pub fn bbox_of(gtfs: &Path, margin_km: &str) -> String {
    let o = rustline(&[
        "bbox",
        "--gtfs",
        gtfs.to_str().unwrap(),
        "--margin-km",
        margin_km,
    ]);
    assert!(o.status.success(), "bbox: {}", text(&o));
    String::from_utf8_lossy(&o.stdout)
        .trim()
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// A real pack built by the CLI (release binary when tests run with --release).
pub struct RealPack {
    pub pack: PathBuf,
    pub output: String,
    pub peak_kib: Option<u64>,
    pub elapsed: std::time::Duration,
    pub rlp_bytes: u64,
}

/// `build-pack` for a shared feed, optionally with the dated Romania extract cropped to
/// `rustline bbox --margin-km 2` (`--bbox` flag assumed, see the report).
/// `None` when a fixture is missing.
pub fn real_pack(feed: &str, osm: bool, dir: &Path) -> Option<RealPack> {
    let gtfs = fixture(&format!("{feed}.zip"))?;
    let pbf = if osm {
        Some(fixture("romania-260101.osm.pbf")?)
    } else {
        None
    };
    let out = dir.join(format!("{feed}-{}", if osm { "osm" } else { "plain" }));
    let mut args: Vec<String> = vec![
        "build-pack".into(),
        "--gtfs".into(),
        gtfs.display().to_string(),
        "--out".into(),
        out.display().to_string(),
        "--feed-id".into(),
        feed.into(),
        "--built-at".into(),
        "0".into(),
    ];
    if let Some(p) = &pbf {
        args.extend([
            "--osm".into(),
            p.display().to_string(),
            "--bbox".into(),
            bbox_of(&gtfs, "2"),
        ]);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (o, peak_kib, elapsed) = run_with_peak(&refs);
    assert!(o.status.success(), "{feed}: {}", text(&o));
    let pack = std::fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|x| x == "rlp"))
        .expect(".rlp written");
    Some(RealPack {
        rlp_bytes: std::fs::metadata(&pack).unwrap().len(),
        pack,
        output: text(&o),
        peak_kib,
        elapsed,
    })
}

/// Random coordinate within `radius_m` of a random located stop.
pub fn near_random_stop(e: &Engine, rng: &mut Rng, radius_m: f64) -> (f64, f64) {
    let t = e.pack().timetable();
    loop {
        let s = rng.below(t.stop_count() as u64) as usize;
        if let Some((lat, lon)) = t.stop_pos(s) {
            let (dx, dy) = (
                (rng.unit() * 2.0 - 1.0) * radius_m,
                (rng.unit() * 2.0 - 1.0) * radius_m,
            );
            return (lat + dlat(dy), lon + dlon(dx, lat));
        }
    }
}

/// A random departure on a random day of the pack's first 30 days, 05:00–23:00.
pub fn random_departure(e: &Engine, rng: &mut Rng) -> NaiveDateTime {
    let i = e.info();
    let days = ((i.valid_to - i.valid_from).num_days().clamp(0, 29) + 1) as u64;
    let d = i.valid_from + chrono::Days::new(rng.below(days));
    at(d, 5, 0, 0) + chrono::Duration::seconds(rng.below(18 * 3600) as i64)
}

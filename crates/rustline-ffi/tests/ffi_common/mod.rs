//! M6 tests-first (Tester, 2026-10-20): the one adapter between the M6 FFI tests and
//! `rustline_ffi::api`. The `cfg rustline_m6` gate and the "M6 not implemented"
//! ignores were removed in M6 (Builder).
//!
//! Real names (Builder, M6): the API returns `Result<_, RustlineException>` (FRB maps
//! that type to a typed Dart exception, Leader's M6 decision); this module wraps it in
//! `anyhow::Error` so the tests below stay as written, and `FfiError` is an alias of
//! `RustlineException`.
//!
//! Planned interface (plan M6 step 2, spec §11.3), as assumed here:
//! - `RustlineEngine::open(path: String) -> anyhow::Result<RustlineEngine>`
//! - `RustlineEngine::open_with_limit(path: String, max_body_bytes: u64) -> anyhow::Result<_>`
//!   (the "lower cap on a phone" of the low-memory edge case)
//! - `RustlineEngine::plan(&self, FfiPlanRequest) -> anyhow::Result<FfiPlanResponse>`
//! - `RustlineEngine::info(&self) -> anyhow::Result<FfiPackInfo>` (plan says no Result; DoD
//!   says every public fn returns Result; see the note)
//! - `install_pack(src_zst: String, dest: String) -> anyhow::Result<FfiPackInfo>`:
//!   free-space check, `dest.tmp`, fsync, atomic rename (plan M6 "open-pack safety")
//! - debug only: `install_pack_with_free_space(src, dest, free_bytes: u64)` and
//!   `debug_inject_panic() -> anyhow::Result<()>` (runs a panic through the same guard
//!   as `plan`).
//! - errors: `anyhow::Error` wrapping `FfiError { code: String, message: String }`,
//!   codes `invalid_request | outside_coverage | no_route | date_outside_feed |
//!   pack_corrupt | pack_version | internal` (+ whatever the Builder adds for a missing
//!   file / no space; listed in `KNOWN_CODES`).
//! - times: epoch milliseconds (UTC) + offset seconds, everywhere.
//!
//! Dev-dependencies these tests need: rustline-core, rustline-gtfs, rustline-pack,
//! chrono, zstd, anyhow.
//!
//! Env: `RUSTLINE_M6_PACK` = a Brașov pack with streets (`.rlp`), for the tests that
//! need real streets; they print `skip:` when it is unset or missing.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub use rustline_ffi::api::{
    FfiPlanRequest, FfiPlanResponse, FfiTimeKind, RustlineEngine, RustlineException as FfiError,
};

pub const KNOWN_CODES: &[&str] = &[
    "invalid_request",
    "outside_coverage",
    "no_route",
    "date_outside_feed",
    "pack_corrupt",
    "pack_version",
    "internal",
    // Not named in the plan; the Builder picks the final names.
    "pack_missing",
    "pack_io",
    "no_space",
    "pack_too_large",
];

pub fn workspace(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// A fresh scratch dir under the system temp dir.
pub fn scratch(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "rustline_m6_{tag}_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The bytes of a pack of `data/fixtures/tiny` (America/Toronto, 2030, no streets).
pub fn tiny_pack_bytes() -> Vec<u8> {
    let raw = rustline_gtfs::load(&workspace("data/fixtures/tiny")).expect("tiny loads");
    let (tt, _) = rustline_gtfs::build_timetable(&raw).expect("tiny builds");
    let meta = rustline_gtfs::pack_meta(&tt, [0; 32], "tiny", 0);
    let data = rustline_gtfs::try_timetable_data(&tt).expect("pack arrays");
    let mut out = Vec::new();
    rustline_pack::write_pack(&data, &meta, &mut out).expect("write pack");
    out
}

/// Writes `bytes` to `dir/name` and returns the path as a String (what Dart passes).
pub fn put(dir: &Path, name: &str, bytes: &[u8]) -> String {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p.to_string_lossy().into_owned()
}

pub fn tiny_pack_file(dir: &Path) -> String {
    put(dir, "tiny.rlp", &tiny_pack_bytes())
}

/// `RUSTLINE_M6_PACK`, or None after printing `skip:`.
pub fn env_pack() -> Option<String> {
    match std::env::var("RUSTLINE_M6_PACK") {
        Ok(p) if !p.is_empty() && Path::new(&p).is_file() => Some(p),
        _ => {
            println!("skip: RUSTLINE_M6_PACK unset or not a file");
            None
        }
    }
}

// ---- the planned API -------------------------------------------------------------

pub fn open(path: &str) -> anyhow::Result<RustlineEngine> {
    Ok(RustlineEngine::open(path.to_string())?)
}

pub fn open_with_limit(path: &str, max_body_bytes: u64) -> anyhow::Result<RustlineEngine> {
    Ok(RustlineEngine::open_with_limit(
        path.to_string(),
        max_body_bytes,
    )?)
}

pub fn plan(e: &RustlineEngine, r: FfiPlanRequest) -> anyhow::Result<FfiPlanResponse> {
    Ok(e.plan(r)?)
}

pub fn install(src: &str, dest: &str) -> anyhow::Result<()> {
    rustline_ffi::api::install_pack(src.to_string(), dest.to_string())?;
    Ok(())
}

#[cfg(debug_assertions)]
pub fn install_with_free(src: &str, dest: &str, free: u64) -> anyhow::Result<()> {
    rustline_ffi::api::install_pack_with_free_space(src.to_string(), dest.to_string(), free)?;
    Ok(())
}

#[cfg(debug_assertions)]
pub fn inject_panic() -> anyhow::Result<()> {
    Ok(rustline_ffi::api::debug_inject_panic()?)
}

/// The stable code of an FFI error (panics when the error carries none).
pub fn code(e: &anyhow::Error) -> String {
    match e.downcast_ref::<FfiError>() {
        Some(f) => f.code.clone(),
        None => panic!("error without a stable code: {e:#}"),
    }
}

pub fn err_code<T>(r: anyhow::Result<T>) -> String {
    match r {
        Ok(_) => panic!("expected an error, got Ok"),
        Err(e) => {
            let c = code(&e);
            assert!(KNOWN_CODES.contains(&c.as_str()), "unknown code {c}: {e:#}");
            c
        }
    }
}

/// Epoch ms + offset of a local wall time at a fixed offset (hours).
pub fn when(y: i32, m: u32, d: u32, h: u32, mi: u32, off_h: i32) -> (i64, i32) {
    use chrono::TimeZone;
    let t = chrono::FixedOffset::east_opt(off_h * 3600)
        .unwrap()
        .with_ymd_and_hms(y, m, d, h, mi, 0)
        .single()
        .unwrap();
    (t.timestamp_millis(), off_h * 3600)
}

/// A request between two points at `(epoch ms, offset s)`, walk 400 m max.
pub fn req(from: (f64, f64), to: (f64, f64), t: (i64, i32)) -> FfiPlanRequest {
    FfiPlanRequest {
        from_lat: from.0,
        from_lon: from.1,
        to_lat: to.0,
        to_lon: to.1,
        time_kind: FfiTimeKind::DepartAt,
        time_ms: t.0,
        time_offset_secs: t.1,
        max_results: 3,
        max_access_m: Some(400),
        walk_speed_mps: None,
        max_transfers: None,
    }
}

/// Tiny fixture stops A and D, Wednesday 2030-01-02 07:55 Toronto (-05:00).
pub const TINY_A: (f64, f64) = (10.0, 20.0);
pub const TINY_D: (f64, f64) = (10.015, 20.01);
pub fn tiny_req() -> FfiPlanRequest {
    req(TINY_A, TINY_D, when(2030, 1, 2, 7, 55, -5))
}

/// Plain summary of a response: per itinerary `(dep ms, dep off, arr ms, arr off, rides)`.
pub fn summary(r: &FfiPlanResponse) -> Vec<(i64, i32, i64, i32, usize)> {
    r.itineraries
        .iter()
        .map(|i| {
            let rides = i
                .legs
                .iter()
                .filter(|l| matches!(l, rustline_ffi::api::FfiLeg::Transit { .. }))
                .count();
            (
                i.departure_ms,
                i.departure_offset_secs,
                i.arrival_ms,
                i.arrival_offset_secs,
                rides,
            )
        })
        .collect()
}

/// Every `(ms, offset)` pair of an itinerary and its legs, in order.
pub fn times(r: &FfiPlanResponse) -> Vec<(i64, i32)> {
    use rustline_ffi::api::FfiLeg;
    let mut v = Vec::new();
    for i in &r.itineraries {
        v.push((i.departure_ms, i.departure_offset_secs));
        for l in &i.legs {
            match l {
                FfiLeg::Walk {
                    start_ms,
                    start_offset_secs,
                    end_ms,
                    end_offset_secs,
                    ..
                } => v.extend([(*start_ms, *start_offset_secs), (*end_ms, *end_offset_secs)]),
                FfiLeg::Transit {
                    board_ms,
                    board_offset_secs,
                    alight_ms,
                    alight_offset_secs,
                    ..
                } => v.extend([
                    (*board_ms, *board_offset_secs),
                    (*alight_ms, *alight_offset_secs),
                ]),
            }
        }
        v.push((i.arrival_ms, i.arrival_offset_secs));
    }
    v
}

/// The same request on rustline-core directly, as `(dep, arr)` instants.
pub fn core_summary(pack_bytes: &[u8], r: &FfiPlanRequest) -> Vec<(i64, i32, i64, i32)> {
    use chrono::TimeZone;
    let e = rustline_core::Engine::from_bytes(pack_bytes).unwrap();
    let off = chrono::FixedOffset::east_opt(r.time_offset_secs).unwrap();
    let t = off.timestamp_millis_opt(r.time_ms).single().unwrap();
    let cr = rustline_core::PlanRequest {
        from: rustline_model::LatLon::new(r.from_lat, r.from_lon),
        to: rustline_model::LatLon::new(r.to_lat, r.to_lon),
        time: rustline_core::TimeSpec::DepartAt(t),
        max_access_m: r.max_access_m.unwrap_or(400),
        max_results: r.max_results,
        ..rustline_core::PlanRequest::default()
    };
    e.plan(&cr)
        .unwrap()
        .itineraries
        .iter()
        .map(|i| {
            (
                i.departure.timestamp_millis(),
                i.departure.offset().local_minus_utc(),
                i.arrival.timestamp_millis(),
                i.arrival.offset().local_minus_utc(),
            )
        })
        .collect()
}

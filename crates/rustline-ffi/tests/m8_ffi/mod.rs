//! M8 tests-first (Tester, 2026-10-20): the one adapter between the M8 FFI tests and
//! `rustline_ffi::api`. Planned interface (plan M8 "FFI + Dart", assumed here; rename
//! here only):
//! - `FfiPlanRequest.time_kind: FfiTimeKind` with `FfiTimeKind::{DepartAt, ArriveBy}`;
//!   `time_ms` / `time_offset_secs` then carry the deadline for `ArriveBy`.
//! - `ffi_common::req` (M6) builds a request with the default kind (`DepartAt`), so
//!   the M6/M7 tests keep their meaning. When the field is added, `ffi_common::req`
//!   needs `time_kind: FfiTimeKind::DepartAt` (the Builder's one-line change).
#![allow(dead_code, unexpected_cfgs)]

pub use rustline_ffi::api::FfiTimeKind;

pub fn arrive_by(mut r: rustline_ffi::api::FfiPlanRequest) -> rustline_ffi::api::FfiPlanRequest {
    r.time_kind = FfiTimeKind::ArriveBy;
    r
}

pub fn is_depart_at(r: &rustline_ffi::api::FfiPlanRequest) -> bool {
    matches!(r.time_kind, FfiTimeKind::DepartAt)
}

/// Core arrive-by on the same request: `(dep ms, dep off, arr ms, arr off)`.
pub fn core_arrive_by(
    pack_bytes: &[u8],
    r: &rustline_ffi::api::FfiPlanRequest,
) -> Vec<(i64, i32, i64, i32)> {
    use chrono::TimeZone;
    let e = rustline_core::Engine::from_bytes(pack_bytes).unwrap();
    let off = chrono::FixedOffset::east_opt(r.time_offset_secs).unwrap();
    let t = off.timestamp_millis_opt(r.time_ms).single().unwrap();
    let cr = rustline_core::PlanRequest {
        from: rustline_model::LatLon::new(r.from_lat, r.from_lon),
        to: rustline_model::LatLon::new(r.to_lat, r.to_lon),
        time: rustline_core::TimeSpec::ArriveBy(t),
        max_access_m: r.max_access_m.unwrap_or(400),
        max_results: r.max_results,
        ..rustline_core::PlanRequest::default()
    };
    match e.plan(&cr) {
        Ok(resp) => resp
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
            .collect(),
        Err(_) => Vec::new(),
    }
}

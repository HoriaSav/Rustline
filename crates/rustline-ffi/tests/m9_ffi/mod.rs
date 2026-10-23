//! M9 tests-first (Tester, 2026-10-23): the one adapter between the M9 FFI tests and
//! `rustline_ffi::api`. Assumed (plan M9 step 4; rename here only):
//! - `FfiPlanRequest.window_secs: Option<u32>` (None = 0 = M8 behaviour); then
//!   `ffi_common::req` needs `window_secs: None` (Builder's one-line change).
//! - `FfiItinerary.tags: Vec<FfiTag>`, read here as `fastest`, `fewest_transfers`, `leaves_latest`.
#![allow(dead_code, unexpected_cfgs)]

pub fn window(
    mut r: rustline_ffi::api::FfiPlanRequest,
    secs: u32,
) -> rustline_ffi::api::FfiPlanRequest {
    r.window_secs = Some(secs);
    r
}

pub fn tags(i: &rustline_ffi::api::FfiItinerary) -> Vec<String> {
    use rustline_ffi::api::FfiTag;
    i.tags
        .iter()
        .map(|t| match t {
            FfiTag::Fastest => "fastest",
            FfiTag::FewestTransfers => "fewest_transfers",
            FfiTag::LeavesLatest => "leaves_latest",
        })
        .map(String::from)
        .collect()
}

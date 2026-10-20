//! M6 tests-first (Tester, 2026-10-20): open + plan through `rustline_ffi::api`, times as
//! epoch ms + offset, input checks and the stable plan error codes.
//! The `cfg rustline_m6` gate and the "M6 not implemented" ignores were removed in M6.
//! Run: `cargo test --locked -p rustline-ffi --test ffi_api`

mod ffi_common;
use ffi_common::*;

#[test]
fn smoke_open_and_plan_tiny() {
    let d = scratch("smoke");
    let e = open(&tiny_pack_file(&d)).expect("tiny pack opens");
    let r = plan(&e, tiny_req()).expect("A->D plans");
    assert!(!r.itineraries.is_empty());
    assert!(summary(&r).iter().all(|s| s.4 >= 1), "{:?}", summary(&r));
}

#[test]
fn info_matches_the_pack() {
    let d = scratch("info");
    let e = open(&tiny_pack_file(&d)).unwrap();
    let i = e.info().expect("info");
    assert_eq!(i.feed_id, "tiny");
    assert_eq!(i.timezone, "America/Toronto");
    assert_eq!(i.format_version, rustline_pack::FORMAT_VERSION);
    assert_eq!(i.feed_sha256.len(), 64);
    // Dates cross as plain data too (days since 1970-01-01 or "yyyy-mm-dd").
    assert!(i.valid_from_days < i.valid_to_days);
}

#[test]
fn times_are_epoch_ms_and_feed_offset_and_match_core() {
    let d = scratch("times");
    let bytes = tiny_pack_bytes();
    let e = open(&put(&d, "tiny.rlp", &bytes)).unwrap();
    let r = plan(&e, tiny_req()).unwrap();
    // Every time of the response is whole seconds and carries Toronto's -05:00 in January.
    for (ms, off) in times(&r) {
        assert_eq!(ms % 1000, 0, "{ms}");
        assert_eq!(off, -5 * 3600, "{ms}");
    }
    let ffi: Vec<_> = summary(&r).iter().map(|s| (s.0, s.1, s.2, s.3)).collect();
    assert_eq!(
        ffi,
        core_summary(&bytes, &tiny_req()),
        "FFI differs from core"
    );
}

#[test]
fn device_offset_other_than_the_feed_gives_the_same_answer() {
    // Device in Bucharest (+02:00), feed in Toronto: same instant, same itineraries,
    // and the answer is in the feed's offset.
    let d = scratch("devtz");
    let e = open(&tiny_pack_file(&d)).unwrap();
    let base = tiny_req();
    for off_h in [2, 13, -11, 0] {
        let mut r = tiny_req();
        r.time_offset_secs = off_h * 3600;
        assert_eq!(
            summary(&plan(&e, r).unwrap()),
            summary(&plan(&e, base.clone()).unwrap()),
            "offset {off_h} h"
        );
    }
}

#[test]
fn clock_off_by_a_day_still_plans_or_says_date_outside_feed() {
    let d = scratch("clock");
    let e = open(&tiny_pack_file(&d)).unwrap();
    let mut r = tiny_req();
    r.time_ms += 86_400_000; // Thursday: still in the feed.
    assert!(plan(&e, r).is_ok());
    let mut r = tiny_req();
    r.time_ms -= 3 * 365 * 86_400_000; // 2027: before the feed.
    assert_eq!(err_code(plan(&e, r)), "date_outside_feed");
}

type Mutate = Box<dyn Fn(&mut FfiPlanRequest)>;

#[test]
fn nan_infinite_and_out_of_range_input_is_invalid_request() {
    let d = scratch("nan");
    let e = open(&tiny_pack_file(&d)).unwrap();
    let bad: Vec<(&str, Mutate)> = vec![
        ("from_lat NaN", Box::new(|r| r.from_lat = f64::NAN)),
        ("from_lon NaN", Box::new(|r| r.from_lon = f64::NAN)),
        ("to_lat NaN", Box::new(|r| r.to_lat = f64::NAN)),
        ("to_lon NaN", Box::new(|r| r.to_lon = f64::NAN)),
        ("to_lat inf", Box::new(|r| r.to_lat = f64::INFINITY)),
        ("from_lat 91", Box::new(|r| r.from_lat = 91.0)),
        ("from_lon -181", Box::new(|r| r.from_lon = -181.0)),
        (
            "walk speed NaN",
            Box::new(|r| r.walk_speed_mps = Some(f64::NAN)),
        ),
        ("walk speed 0", Box::new(|r| r.walk_speed_mps = Some(0.0))),
        ("walk speed -1", Box::new(|r| r.walk_speed_mps = Some(-1.0))),
        ("offset 20 h", Box::new(|r| r.time_offset_secs = 20 * 3600)),
        ("ms i64::MAX", Box::new(|r| r.time_ms = i64::MAX)),
        ("ms i64::MIN", Box::new(|r| r.time_ms = i64::MIN)),
    ];
    for (name, f) in bad {
        let mut r = tiny_req();
        f(&mut r);
        assert_eq!(err_code(plan(&e, r)), "invalid_request", "{name}");
    }
}

#[test]
fn far_from_every_stop_without_streets_is_no_route() {
    let d = scratch("noroute");
    let e = open(&tiny_pack_file(&d)).unwrap();
    let r = req((11.0, 21.0), (11.5, 21.5), when(2030, 1, 2, 8, 0, -5));
    assert_eq!(err_code(plan(&e, r)), "no_route");
}

#[test]
fn point_far_from_every_street_is_outside_coverage() {
    let Some(p) = env_pack() else { return };
    let e = open(&p).unwrap();
    // Brașov pack, a point in the sea off Africa; the time is in the feed (from info).
    let i = e.info().unwrap();
    let ms = (i.valid_from_days as i64 + 1) * 86_400_000 + 8 * 3_600_000;
    let r = req((0.0, 0.0), (45.6427, 25.5887), (ms, 3 * 3600));
    assert_eq!(err_code(plan(&e, r)), "outside_coverage");
}

#[test]
fn same_engine_answers_the_same_twice() {
    let d = scratch("twice");
    let e = open(&tiny_pack_file(&d)).unwrap();
    let a = summary(&plan(&e, tiny_req()).unwrap());
    let b = summary(&plan(&e, tiny_req()).unwrap());
    assert_eq!(a, b);
}

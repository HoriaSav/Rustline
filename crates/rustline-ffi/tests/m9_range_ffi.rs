//! M9 tests-first (Tester, 2026-10-23): range queries and tags through the FFI (plan
//! M9 "range queries work in CLI, FFI and example app"), on the tiny fixture.
//! Run: `RUSTFLAGS="--cfg rustline_m9" cargo test -p rustline-ffi --test m9_range_ffi -- --ignored`
#![allow(unexpected_cfgs)]

mod ffi_common;
mod m9_ffi;

mod m9 {
    use super::ffi_common::*;
    use super::m9_ffi::*;

    #[test]
    fn window_zero_equals_m8_and_window_widens() {
        let d = scratch("m9ffi");
        let e = open(&tiny_pack_file(&d)).unwrap();
        let mut base = tiny_req();
        base.max_results = 20;
        let m8 = plan(&e, base.clone()).unwrap();
        assert_eq!(
            plan(&e, window(base.clone(), 0)).unwrap(),
            m8,
            "window 0 = M8"
        );
        let w = plan(&e, window(base.clone(), 3600)).unwrap();
        assert!(!w.itineraries.is_empty());
        for s in &m8.itineraries {
            assert!(
                w.itineraries
                    .iter()
                    .any(|j| j.departure_ms >= s.departure_ms
                        && j.arrival_ms <= s.arrival_ms
                        && j.transfers <= s.transfers),
                "{s:?} not covered"
            );
        }
        assert!(w.itineraries.len() <= 20);
    }

    #[test]
    fn limits_are_invalid_request() {
        let d = scratch("m9ffilim");
        let e = open(&tiny_pack_file(&d)).unwrap();
        assert_eq!(
            err_code(plan(&e, window(tiny_req(), 7200 + 1))),
            "invalid_request",
            "window over cap (assumed 2 h)"
        );
        let mut r = tiny_req();
        r.max_results = 21;
        assert_eq!(err_code(plan(&e, r)), "invalid_request");
        let mut r = tiny_req();
        r.max_transfers = Some(9);
        assert_eq!(
            err_code(plan(&e, r)),
            "invalid_request",
            "plan M9: over hard max 8 (vs M8 cap; open decision)"
        );
    }

    #[test]
    fn tags_are_known_and_cover_the_set() {
        let d = scratch("m9ffitag");
        let e = open(&tiny_pack_file(&d)).unwrap();
        let mut r = window(tiny_req(), 1800);
        r.max_results = 20;
        let v = plan(&e, r).unwrap();
        let all: Vec<String> = v.itineraries.iter().flat_map(tags).collect();
        for t in &all {
            assert!(
                ["fastest", "fewest_transfers", "leaves_latest"].contains(&t.as_str()),
                "unknown tag {t}"
            );
        }
        for want in ["fastest", "fewest_transfers", "leaves_latest"] {
            assert!(all.iter().any(|t| t == want), "no itinerary tagged {want}");
        }
    }
}

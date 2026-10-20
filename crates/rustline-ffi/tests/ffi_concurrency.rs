//! M6 tests-first (Tester, 2026-10-20): lifecycle edge cases on the Rust side of the
//! FFI: concurrent plans on one shared engine, the same pack opened twice or at once,
//! a pack swap while queries run, dropping an engine another thread still uses, and
//! the single-query bound (≤ 1 s, release only).
//! The `cfg rustline_m6` gate and the "M6 not implemented" ignores were removed in M6.
//! Run: `cargo test --locked -p rustline-ffi --test ffi_concurrency`

mod ffi_common;
use ffi_common::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn requests() -> Vec<FfiPlanRequest> {
    let stops = [TINY_A, (10.005, 20.0), (10.01, 20.005), TINY_D];
    let mut v = Vec::new();
    for (i, a) in stops.iter().enumerate() {
        for b in &stops[i + 1..] {
            for h in [7, 8, 9, 17] {
                v.push(req(*a, *b, when(2030, 1, 2, h, 0, -5)));
            }
        }
    }
    v
}

#[test]
fn concurrent_plans_on_a_shared_engine_match_sequential() {
    let d = scratch("conc");
    let e = Arc::new(open(&tiny_pack_file(&d)).unwrap());
    let want: Vec<_> = requests()
        .into_iter()
        .map(|r| plan(&e, r).map(|x| summary(&x)).map_err(|x| code(&x)))
        .collect();
    std::thread::scope(|s| {
        for t in 0..8 {
            let (e, want) = (Arc::clone(&e), &want);
            s.spawn(move || {
                for round in 0..25 {
                    for (i, r) in requests().into_iter().enumerate() {
                        let got = plan(&e, r).map(|x| summary(&x)).map_err(|x| code(&x));
                        assert_eq!(&got, &want[i], "thread {t} round {round} query {i}");
                    }
                }
            });
        }
    });
}

#[test]
fn same_pack_opened_twice_and_concurrently() {
    let d = scratch("twice");
    let p = tiny_pack_file(&d);
    let a = open(&p).unwrap();
    let b = open(&p).unwrap();
    assert_eq!(
        summary(&plan(&a, tiny_req()).unwrap()),
        summary(&plan(&b, tiny_req()).unwrap())
    );
    let engines: Vec<_> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..6).map(|_| s.spawn(|| open(&p))).collect();
        hs.into_iter()
            .map(|h| h.join().expect("no panic"))
            .collect()
    });
    for e in engines {
        assert!(plan(&e.unwrap(), tiny_req()).is_ok());
    }
}

#[test]
fn pack_swap_mid_query_keeps_old_queries_consistent() {
    // The app holds the engine behind one slot; a swap replaces the slot while
    // queries on the old engine are still running. Old queries finish on the old
    // pack; new ones see the new pack; nothing panics.
    let d = scratch("swap");
    let old_path = tiny_pack_file(&d);
    let old = Arc::new(open(&old_path).unwrap());
    let want_old = summary(&plan(&old, tiny_req()).unwrap());
    let slot = std::sync::RwLock::new(Arc::clone(&old));
    std::thread::scope(|s| {
        let running = Arc::clone(&old);
        let w = &want_old;
        let h = s.spawn(move || {
            for _ in 0..200 {
                assert_eq!(&summary(&plan(&running, tiny_req()).unwrap()), w);
            }
        });
        // Replace the pack on disk (new file, then rename over it) and swap the slot.
        let new_path = put(&d, "tiny.new", &tiny_pack_bytes());
        std::fs::rename(&new_path, &old_path).unwrap();
        *slot.write().unwrap() = Arc::new(open(&old_path).unwrap());
        drop(old);
        h.join().expect("old queries do not panic");
    });
    assert!(plan(&slot.read().unwrap(), tiny_req()).is_ok());
}

#[test]
fn dropping_one_handle_while_another_thread_plans() {
    let d = scratch("drop");
    let e = Arc::new(open(&tiny_pack_file(&d)).unwrap());
    let other = Arc::clone(&e);
    let h = std::thread::spawn(move || {
        for _ in 0..100 {
            plan(&other, tiny_req()).unwrap();
        }
    });
    drop(e);
    h.join().expect("no panic");
}

#[test]
fn a_single_query_is_bounded_to_one_second() {
    let Some(p) = env_pack() else { return };
    let e = open(&p).unwrap();
    let i = e.info().unwrap();
    let day0 = (i.valid_from_days as i64 + 2) * 86_400_000;
    let mut rng = 0x6d36_u64;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % 10_000) as f64 / 10_000.0
    };
    let mut worst = Duration::ZERO;
    for _ in 0..200 {
        // Brașov box, any hour of the day.
        let pt = |a: f64, b: f64| (45.60 + a * 0.08, 25.52 + b * 0.12);
        let r = req(
            pt(next(), next()),
            pt(next(), next()),
            (day0 + (next() * 86_400_000.0) as i64, 3 * 3600),
        );
        let t0 = Instant::now();
        let _ = plan(&e, r);
        worst = worst.max(t0.elapsed());
    }
    println!("worst single query: {worst:?}");
    if cfg!(debug_assertions) {
        println!("skip: the 1 s bound is checked in release only");
    } else {
        assert!(worst <= Duration::from_secs(1), "{worst:?}");
    }
}

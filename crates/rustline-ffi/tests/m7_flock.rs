//! M7 tests-first (Tester, 2026-10-20): R2 flock retried on EINTR (bounded), any other
//! errno is `pack_io`. Uses the debug-only injection hook (process-global, so these
//! tests run one at a time behind a mutex).
//! Debug builds only.
//! Run: `cargo test --locked -p rustline-ffi --test m7_flock`
#![cfg(debug_assertions)]

mod ffi_common;
mod m7_ffi;
use ffi_common::*;
use m7_ffi::*;
use std::sync::Mutex;

static ONE: Mutex<()> = Mutex::new(());

fn setup() -> (std::path::PathBuf, String, String) {
    let d = scratch("m7flock");
    let src = put(
        &d,
        "src.rlp.zst",
        &zstd::encode_all(&tiny_pack_bytes()[..], 3).unwrap(),
    );
    let dest = d.join("brasov.rlp").to_string_lossy().into_owned();
    (d, src, dest)
}

#[test]
fn eintr_a_few_times_is_retried() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let (_d, src, dest) = setup();
    for n in [1, 3, FLOCK_RETRIES - 1] {
        flock_fail(libc::EINTR, n);
        install(&src, &dest).unwrap_or_else(|e| panic!("{n} x EINTR: {e:#}"));
        assert!(open(&dest).is_ok());
    }
}

#[test]
fn endless_eintr_gives_up_with_pack_io() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let (d, src, dest) = setup();
    flock_fail(libc::EINTR, FLOCK_RETRIES * 10);
    let t0 = std::time::Instant::now();
    assert_eq!(err_code(install(&src, &dest)), "pack_io");
    assert!(t0.elapsed() < std::time::Duration::from_secs(5), "bounded");
    assert!(!std::path::Path::new(&dest).exists());
    assert!(names(&d).iter().all(|n| !is_own_tmp("brasov.rlp", n)));
    flock_fail(libc::EINTR, 0); // reset
}

#[test]
fn other_errno_is_pack_io_without_retry() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let (_d, src, dest) = setup();
    for errno in [libc::EBADF, libc::ENOLCK, libc::EWOULDBLOCK] {
        flock_fail(errno, 1);
        assert_eq!(err_code(install(&src, &dest)), "pack_io", "errno {errno}");
        // One failure only: the next install works (no retry consumed the hook).
        install(&src, &dest).expect("hook used once");
    }
}

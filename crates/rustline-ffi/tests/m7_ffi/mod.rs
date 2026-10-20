//! M7 tests-first (Tester, 2026-10-20): the one adapter for the M7 FFI tests
//! (`m7_*.rs`). Builder: change only this module when names differ.
//!
//! Assumed (plan M7 R1-R5):
//! - R2: debug-only hook `rustline_ffi::api::debug_flock_fail(errno: i32, times: u32) -> Result<()>`:
//!   the next `times` flock calls fail with `errno`, then flock runs for real.
//!   Retry bound: `FLOCK_RETRIES` (plan "e.g. 100").
//! - R3: the quiet panic hook prints exactly `PANIC_LINE` once per panic.
//! - R1: own temp files are exactly `{name}.{pid}.{n}.tmp` (digits), regular files.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

pub const PANIC_LINE: &str = "rustline: internal error";
pub const FLOCK_RETRIES: u32 = 100;

#[cfg(debug_assertions)]
pub fn flock_fail(errno: i32, times: u32) {
    rustline_ffi::api::debug_flock_fail(errno, times).expect("hook");
}

pub fn init() {
    rustline_ffi::api::init_rustline().expect("init");
}

/// Is `n` one of install_pack's own temp names for `name`?
pub fn is_own_tmp(name: &str, n: &str) -> bool {
    let Some(rest) = n.strip_prefix(&format!("{name}.")) else {
        return false;
    };
    let Some(mid) = rest.strip_suffix(".tmp") else {
        return false;
    };
    let parts: Vec<&str> = mid.split('.').collect();
    parts.len() == 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Sorted file names in `dir`.
pub fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// Re-runs this test binary for one test with `M7_CHILD=<role>`; returns
/// (exit ok, stdout, stderr).
pub fn child(test: &str, role: &str) -> (bool, String, String) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            test,
            "--nocapture",
            "--include-ignored",
            "--test-threads=1",
        ])
        .env("M7_CHILD", role)
        .env("RUST_BACKTRACE", "0")
        .output()
        .expect("spawn child test");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

pub fn role() -> Option<String> {
    std::env::var("M7_CHILD").ok().filter(|s| !s.is_empty())
}

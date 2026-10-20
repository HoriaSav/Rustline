//! M7 tests-first (Tester, 2026-10-20): R3 the quiet panic hook prints one fixed line,
//! no payload, no file:line (stderr of a child process); R4 the doc of
//! `debug_inject_panic` says test-only and `invalid_request` in release, in the Rust
//! doc and the generated Dart doc.
//! Run: `cargo test --locked -p rustline-ffi --test m7_panic_hook`

mod ffi_common;
mod m7_ffi;
use ffi_common::*;
#[cfg(debug_assertions)]
use m7_ffi::*;

#[cfg(debug_assertions)]
const NAME: &str = "r3_panic_hook_prints_one_fixed_line";

#[cfg(debug_assertions)]
#[test]
fn r3_panic_hook_prints_one_fixed_line() {
    if let Some(r) = role() {
        init();
        let n: usize = r.parse().unwrap();
        for _ in 0..n {
            assert_eq!(err_code(inject_panic()), "internal");
        }
        // And from another thread (FRB worker pool).
        std::thread::spawn(|| err_code(inject_panic()))
            .join()
            .unwrap();
        return;
    }
    for n in [1usize, 3] {
        let (ok, _out, err) = child(NAME, &n.to_string());
        assert!(ok, "child failed: {err}");
        let lines: Vec<&str> = err.lines().filter(|l| l.starts_with("rustline:")).collect();
        assert_eq!(lines, vec![PANIC_LINE; n + 1], "stderr:\n{err}");
        for bad in [
            "injected",
            ".rs:",
            "panicked at",
            "stack backtrace",
            "debug hook",
        ] {
            assert!(!err.contains(bad), "stderr leaks `{bad}`:\n{err}");
        }
    }
}

fn doc_before(src: &str, item: &str) -> String {
    let i = src.find(item).unwrap_or_else(|| panic!("{item} not found"));
    let mut doc: Vec<&str> = src[..i]
        .lines()
        .rev()
        .skip(1)
        .take_while(|l| {
            let t = l.trim();
            t.starts_with("///") || t.starts_with("#[") || t.is_empty()
        })
        .collect();
    doc.reverse();
    doc.join("\n").to_lowercase()
}

#[test]
fn r4_debug_inject_panic_doc_says_test_only_and_invalid_request_in_release() {
    let rs = std::fs::read_to_string(workspace("crates/rustline-ffi/src/api.rs")).unwrap();
    let dart =
        std::fs::read_to_string(workspace("rustline_flutter/lib/src/rust/api.dart")).unwrap();
    for (what, doc) in [
        ("Rust", doc_before(&rs, "pub fn debug_inject_panic")),
        ("Dart", doc_before(&dart, "debugInjectPanic")),
    ] {
        assert!(
            doc.contains("test-only") || doc.contains("tests only") || doc.contains("test only"),
            "{what} doc:\n{doc}"
        );
        assert!(
            doc.contains("invalid_request") && doc.contains("release"),
            "{what} doc:\n{doc}"
        );
    }
}

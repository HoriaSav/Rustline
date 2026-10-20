//! M6 tests-first (Tester, 2026-10-20): a panic never crosses the FFI. An injected
//! debug-only panic comes back as an error with code `internal`, the engine keeps
//! working, and the ffi crate keeps `panic = "unwind"` (plan M6 pitfalls).
//! The `cfg rustline_m6` gate and the "M6 not implemented" ignores were removed in M6.
//! Run: `cargo test --locked -p rustline-ffi --test ffi_panic`

mod ffi_common;
use ffi_common::*;

#[cfg(debug_assertions)]
#[test]
fn injected_panic_is_an_internal_error() {
    let d = scratch("panic");
    let e = open(&tiny_pack_file(&d)).unwrap();
    for _ in 0..3 {
        assert_eq!(err_code(inject_panic()), "internal");
    }
    // Same in another thread (FRB runs calls on a worker pool).
    let h = std::thread::spawn(|| err_code(inject_panic()));
    assert_eq!(h.join().expect("panic stayed inside"), "internal");
    assert!(
        plan(&e, tiny_req()).is_ok(),
        "the engine still works after a panic"
    );
}

#[cfg(debug_assertions)]
#[test]
fn internal_error_message_has_no_backtrace_or_paths() {
    let e = inject_panic().unwrap_err();
    // Builder (M6 review MF2): the message is fixed, no panic payload.
    assert_eq!(
        e.downcast_ref::<FfiError>().unwrap().message,
        "internal error"
    );
    let msg = format!("{e:#}");
    assert!(
        !msg.contains("/home/") && !msg.contains("stack backtrace"),
        "{msg}"
    );
}

#[test]
fn no_profile_sets_panic_abort() {
    for f in ["Cargo.toml", "crates/rustline-ffi/Cargo.toml"] {
        let s = std::fs::read_to_string(workspace(f)).unwrap();
        for line in s.lines() {
            let l = line.split('#').next().unwrap().replace(' ', "");
            assert!(l != "panic=\"abort\"", "{f}: {line}");
        }
    }
}
